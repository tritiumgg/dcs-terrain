//! The extract's `beacons.json`, `radio.json`, `towns.json` and `nodes.json`
//! from a map's Lua files, the projection fitted to its beacons, and the
//! airdrome ids `Radio.lua` labels.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

use serde::Serialize;

use super::{Chunk, Table, Value};
use crate::crs::{self, Pair, Tmerc};
use crate::decode::airfield::write_rows;
use crate::decode::install::Refusal;
use crate::extract::FileEntry;

pub const BEACONS_FILE: &str = "beacons.json";
pub const RADIO_FILE: &str = "radio.json";
pub const TOWNS_FILE: &str = "towns.json";
pub const NODES_FILE: &str = "nodes.json";

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BeaconRow {
    pub beacon_id: Option<String>,
    pub callsign: Option<String>,
    pub display_name: Option<String>,
    /// Null where the file names a type no shared script defines.
    #[serde(rename = "type")]
    pub kind: Option<i64>,
    pub frequency_hz: Option<f64>,
    pub channel: Option<i64>,
    pub direction: Option<f64>,
    pub x: Option<f64>,
    pub alt: Option<f64>,
    pub z: Option<f64>,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub scene_objects: Vec<String>,
}

/// A radio's frequency in each band, by the bands' order in the game:
/// HF, the low VHF band (FM), the high VHF band and UHF.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Frequencies {
    pub hf: Option<f64>,
    pub fm: Option<f64>,
    pub vhf: Option<f64>,
    pub uhf: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RadioRow {
    pub radio_id: Option<String>,
    /// Each language's first name for the station.
    pub callsigns: BTreeMap<String, String>,
    pub roles: Vec<String>,
    pub frequencies_hz: Frequencies,
    pub scene_objects: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TownRow {
    pub name: String,
    pub display_name: Option<String>,
    pub lat: f64,
    pub lon: f64,
    pub x: f64,
    pub z: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Xz {
    pub x: f64,
    pub z: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NodeRow {
    pub id: Option<i64>,
    pub name: Option<String>,
    pub red: Option<Xz>,
    pub blue: Option<Xz>,
}

/// The extract's `crs`: the projection, and how well the beacons fit it.
/// `n`, `rms_m` and `max_m` are null when it comes from the table.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Crs {
    pub proj4: String,
    pub lon_0: i32,
    pub easting: f64,
    pub northing: f64,
    pub k_0: f64,
    pub n: Option<usize>,
    pub rms_m: Option<f64>,
    pub max_m: Option<f64>,
    pub source: &'static str,
}

impl Crs {
    pub fn tmerc(&self) -> Tmerc {
        Tmerc {
            lon_0: self.lon_0,
            easting: self.easting,
            northing: self.northing,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Sample {
    pub x: f64,
    pub z: f64,
    pub lat: f64,
    pub lon: f64,
}

/// What a map's Lua files give the extract.
#[derive(Debug, Clone, PartialEq)]
pub struct LuaTables {
    pub beacons: Vec<BeaconRow>,
    pub radio: Vec<RadioRow>,
    pub towns: Vec<TownRow>,
    pub nodes: Vec<NodeRow>,
    pub crs: Crs,
    pub latlon_samples: Vec<Sample>,
    /// Airdrome id by the label of the `Radio.lua` entry that gives it.
    pub airdrome_ids: BTreeMap<String, i64>,
    /// The shared scripts the files `dofile`, read for their constants.
    pub included: Vec<FileEntry>,
    /// Each name a file read that nothing assigned, as `<path>: <name>`.
    pub unresolved: Vec<String>,
}

impl LuaTables {
    /// The id of the airdrome whose files are named `name_id`.
    /// A label spelled as the file is wins; failing that, the one label that
    /// differs from it only in case, as `Hildesheim` labels `HildesHeim`.
    pub fn airdrome_id(&self, name_id: &str) -> Option<i64> {
        if let Some(&id) = self.airdrome_ids.get(name_id) {
            return Some(id);
        }
        let mut like = self
            .airdrome_ids
            .iter()
            .filter(|(label, _)| label.eq_ignore_ascii_case(name_id));
        match (like.next(), like.next()) {
            (Some((_, &id)), None) => Some(id),
            _ => None,
        }
    }

    /// The beacon ids of airdrome `id`.
    pub fn beacon_ids(&self, id: i64) -> Vec<String> {
        let ids = self.beacons.iter().filter_map(|b| b.beacon_id.as_deref());
        of_airdrome(ids, id)
    }

    /// The radio ids of airdrome `id`.
    pub fn radio_ids(&self, id: i64) -> Vec<String> {
        let ids = self.radio.iter().filter_map(|r| r.radio_id.as_deref());
        of_airdrome(ids, id)
    }

    /// The display name of airdrome `id`'s first beacon.
    pub fn display_name(&self, id: i64) -> Option<&str> {
        self.beacons
            .iter()
            .find(|b| b.beacon_id.as_deref().and_then(airdrome_of) == Some(id))
            .and_then(|b| b.display_name.as_deref())
    }

    /// Writes the four tables into `dir`.
    pub fn write(&self, dir: &Path) -> io::Result<()> {
        write_rows(&dir.join(BEACONS_FILE), &self.beacons)?;
        write_rows(&dir.join(RADIO_FILE), &self.radio)?;
        write_rows(&dir.join(TOWNS_FILE), &self.towns)?;
        write_rows(&dir.join(NODES_FILE), &self.nodes)
    }
}

/// The airdrome an id of the form `airfield<ID>_<n>` belongs to.
fn airdrome_of(id: &str) -> Option<i64> {
    let (n, k) = id.strip_prefix("airfield")?.split_once('_')?;
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit());
    (digits(n) && digits(k)).then(|| n.parse().ok()).flatten()
}

fn of_airdrome<'a>(ids: impl Iterator<Item = &'a str>, id: i64) -> Vec<String> {
    ids.filter(|b| airdrome_of(b) == Some(id))
        .map(str::to_string)
        .collect()
}

/// The map's Lua files by what they hold, matched by name without regard to
/// case.
const SOURCES: [(&str, &str); 4] = [
    ("beacons", "beacons.lua"),
    ("radio", "radio.lua"),
    ("towns", "towns.lua"),
    ("nodes", "nodes.lua"),
];

/// Reads the Lua files among a map's `files`, relative to `install`, for
/// the theatre `theatre`. A file the map lacks gives an empty table. The
/// projection is fitted to the beacons, or taken from the table when fewer
/// than three carry both positions; a map with neither is refused.
pub fn decode(install: &Path, theatre: &str, files: &[FileEntry]) -> Result<LuaTables, Refusal> {
    let mut included: Vec<FileEntry> = Vec::new();
    let mut unresolved = Vec::new();
    let mut chunks: BTreeMap<&str, (String, Chunk)> = BTreeMap::new();
    let mut labels = BTreeMap::new();
    for (what, name) in SOURCES {
        let Some(f) = files.iter().find(|f| {
            f.role == "lua"
                && f.path
                    .rsplit('/')
                    .next()
                    .is_some_and(|n| n.eq_ignore_ascii_case(name))
        }) else {
            continue;
        };
        let text = fs::read(install.join(&f.path))
            .map_err(|e| Refusal(format!("{} cannot be read: {e}", f.path)))?;
        let chunk = super::run_text(install, &f.path, &text).map_err(|e| Refusal(e.0))?;
        for (path, size) in &chunk.included {
            if !included.iter().any(|g| g.path == *path) {
                included.push(FileEntry {
                    role: "lua".into(),
                    path: path.clone(),
                    class: None,
                    version: None,
                    size: *size,
                    payload_size: None,
                });
            }
        }
        unresolved.extend(chunk.unresolved.iter().map(|n| format!("{}: {n}", f.path)));
        if what == "radio" {
            labels = radio_labels(&text);
        }
        chunks.insert(what, (f.path.clone(), chunk));
    }

    let table = |what: &str, global: &str| -> Result<Option<(&str, &Table)>, Refusal> {
        let Some((path, chunk)) = chunks.get(what) else {
            return Ok(None);
        };
        match chunk.table(global) {
            Some(t) => Ok(Some((path.as_str(), t))),
            None => Err(Refusal(format!("{path} assigns no {global} table"))),
        }
    };
    let beacons = match table("beacons", "beacons")? {
        Some((path, t)) => beacons(t).map_err(|e| Refusal(format!("{path}: {e}")))?,
        None => Vec::new(),
    };
    let (radio, airdrome_ids) = match table("radio", "radio")? {
        Some((path, t)) => radio(t, &labels).map_err(|e| Refusal(format!("{path}: {e}")))?,
        None => (Vec::new(), BTreeMap::new()),
    };
    let (crs, latlon_samples) = projection(theatre, &beacons)?;
    let towns = match table("towns", "towns")? {
        Some((path, t)) => towns(t, &crs.tmerc()).map_err(|e| Refusal(format!("{path}: {e}")))?,
        None => Vec::new(),
    };
    let nodes = match table("nodes", "missionNodes")? {
        Some((path, t)) => nodes(t).map_err(|e| Refusal(format!("{path}: {e}")))?,
        None => Vec::new(),
    };
    Ok(LuaTables {
        beacons,
        radio,
        towns,
        nodes,
        crs,
        latlon_samples,
        airdrome_ids,
        included,
        unresolved,
    })
}

/// The entries of a list of tables, or why it is not one. `what` names an
/// entry in the message.
fn entries<'a>(t: &'a Table, what: &str) -> Result<Vec<&'a Table>, String> {
    let list = t
        .list()
        .ok_or_else(|| format!("the {what} table is not a list"))?;
    list.into_iter()
        .zip(1..)
        .map(|(v, i)| match v {
            Value::Table(t) => Ok(t),
            _ => Err(format!("{what} {i} is not a table")),
        })
        .collect()
}

/// A whole number, or why it is not one.
fn whole(v: Option<f64>, what: &str) -> Result<Option<i64>, String> {
    match v {
        None => Ok(None),
        Some(n) if n.fract() == 0.0 && n.abs() < 9e15 => Ok(Some(n as i64)),
        Some(n) => Err(format!("{what} {n} is not a whole number")),
    }
}

/// The strings of a list under `key`; none when it is absent.
fn strings(t: &Table, key: &str) -> Result<Vec<String>, String> {
    let Some(v) = t.get(key) else {
        return Ok(Vec::new());
    };
    let list = match v {
        Value::Table(l) => l.list(),
        _ => None,
    };
    list.ok_or_else(|| format!("{key} is not a list"))?
        .into_iter()
        .map(|v| match v {
            Value::Str(s) => Ok(s.clone()),
            _ => Err(format!("{key} holds something that is not a string")),
        })
        .collect()
}

/// The first `n` numbers of the list under `key`, if it is one.
fn numbers(t: &Table, key: &str, n: usize) -> Option<Vec<f64>> {
    let l = t.table(key)?.list()?;
    let v: Vec<f64> = l
        .iter()
        .take(n)
        .map(|v| match v {
            Value::Num(x) => Some(*x),
            _ => None,
        })
        .collect::<Option<_>>()?;
    (v.len() == n).then_some(v)
}

/// A latitude and longitude on the earth, or why they are not.
fn degrees(lat: f64, lon: f64) -> Result<(), String> {
    if lat.abs() <= 90.0 && lon.abs() <= 180.0 {
        Ok(())
    } else {
        Err(format!(
            "latitude {lat} and longitude {lon} are not on the earth"
        ))
    }
}

fn beacons(t: &Table) -> Result<Vec<BeaconRow>, String> {
    entries(t, "beacon")?
        .into_iter()
        .zip(1..)
        .map(|(b, i)| {
            let at = |e: String| format!("beacon {i}: {e}");
            let pos = numbers(b, "position", 3);
            let geo = b.table("positionGeo");
            let lat = geo.and_then(|g| g.num("latitude"));
            let lon = geo.and_then(|g| g.num("longitude"));
            if let (Some(lat), Some(lon)) = (lat, lon) {
                degrees(lat, lon).map_err(at)?;
            }
            Ok(BeaconRow {
                beacon_id: b.str("beaconId").map(str::to_string),
                callsign: b.str("callsign").map(str::to_string),
                display_name: b.str("display_name").map(str::to_string),
                kind: whole(b.num("type"), "type").map_err(at)?,
                frequency_hz: b.num("frequency"),
                channel: whole(b.num("channel"), "channel").map_err(at)?,
                direction: b.num("direction"),
                x: pos.as_ref().map(|p| p[0]),
                alt: pos.as_ref().map(|p| p[1]),
                z: pos.as_ref().map(|p| p[2]),
                lat,
                lon,
                scene_objects: strings(b, "sceneObjects").map_err(at)?,
            })
        })
        .collect()
}

/// The label of each station `Radio.lua` labels, by its `radioId`: the
/// comment on the line before the line that assigns it, as ED writes
///
/// ```text
///     -- Anapa
///     radioId = 'airfield12_0';
/// ```
///
/// Lua discards comments, so they are read from the text.
fn radio_labels(text: &[u8]) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut previous: Option<String> = None;
    // Lua breaks a line at `\r` as well as `\n`.
    for line in String::from_utf8_lossy(text).split(['\n', '\r']) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(id) = radio_id(line)
            && let Some(label) = previous.as_deref().and_then(|p| p.strip_prefix("--"))
        {
            let label = label.trim();
            // A block comment, or a station commented out, is no label.
            if !label.is_empty() && !label.starts_with('[') && radio_id(label).is_none() {
                out.entry(id.to_string())
                    .or_insert_with(|| label.to_string());
            }
        }
        previous = Some(line.to_string());
    }
    out
}

/// The id a line `radioId = 'airfield12_0';` assigns.
fn radio_id(line: &str) -> Option<&str> {
    let rest = line
        .strip_prefix("radioId")?
        .trim_start()
        .strip_prefix('=')?
        .trim_start();
    let q = rest.chars().next().filter(|c| *c == '\'' || *c == '"')?;
    let rest = &rest[1..];
    Some(&rest[..rest.find(q)?])
}

fn radio(
    t: &Table,
    labels: &BTreeMap<String, String>,
) -> Result<(Vec<RadioRow>, BTreeMap<String, i64>), String> {
    let mut rows = Vec::new();
    let mut ids = BTreeMap::new();
    for (r, i) in entries(t, "radio")?.into_iter().zip(1..) {
        let at = |e: String| format!("radio {i}: {e}");
        let radio_id = r.str("radioId").map(str::to_string);
        // Only an airdrome's first station, `airfield<ID>_0`, names it.
        if let (Some(label), Some(id)) = (
            radio_id.as_deref().and_then(|s| labels.get(s)),
            radio_id
                .as_deref()
                .filter(|s| s.ends_with("_0"))
                .and_then(airdrome_of),
        ) {
            ids.entry(label.to_string()).or_insert(id);
        }
        let mut callsigns = BTreeMap::new();
        if let Some(c) = r.table("callsign") {
            for lang in entries(c, "callsign").map_err(at)? {
                for (k, v) in &lang.entries {
                    let (super::Key::Str(k), Value::Table(names)) = (k, v) else {
                        return Err(at("a callsign is not keyed by language".into()));
                    };
                    if let Some(Value::Str(name)) = names.index(1.0) {
                        callsigns.entry(k.clone()).or_insert_with(|| name.clone());
                    }
                }
            }
        }
        let band = |n: f64| -> Option<f64> {
            match r.table("frequency")?.index(n)? {
                Value::Table(f) => match f.index(2.0)? {
                    Value::Num(hz) => Some(*hz),
                    _ => None,
                },
                _ => None,
            }
        };
        rows.push(RadioRow {
            radio_id,
            callsigns,
            roles: strings(r, "role").map_err(at)?,
            frequencies_hz: Frequencies {
                hf: band(0.0),
                fm: band(1.0),
                vhf: band(2.0),
                uhf: band(3.0),
            },
            scene_objects: strings(r, "sceneObjects").map_err(at)?,
        });
    }
    Ok((rows, ids))
}

/// Towns by name, each put into DCS metres by `crs`.
fn towns(t: &Table, crs: &Tmerc) -> Result<Vec<TownRow>, String> {
    let mut rows = Vec::new();
    for (k, v) in &t.entries {
        let (super::Key::Str(name), Value::Table(town)) = (k, v) else {
            return Err("a town is not a table keyed by its name".into());
        };
        let (Some(lat), Some(lon)) = (town.num("latitude"), town.num("longitude")) else {
            return Err(format!("town {name} has no latitude and longitude"));
        };
        degrees(lat, lon).map_err(|e| format!("town {name}: {e}"))?;
        let (x, z) = crs.to_xz(lat, lon);
        rows.push(TownRow {
            name: name.clone(),
            display_name: town.str("display_name").map(str::to_string),
            lat,
            lon,
            x,
            z,
        });
    }
    rows.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(rows)
}

fn nodes(t: &Table) -> Result<Vec<NodeRow>, String> {
    let xz = |n: &Table, key| numbers(n, key, 2).map(|p| Xz { x: p[0], z: p[1] });
    entries(t, "node")?
        .into_iter()
        .zip(1..)
        .map(|(n, i)| {
            Ok(NodeRow {
                id: whole(n.num("id"), "id").map_err(|e| format!("node {i}: {e}"))?,
                name: n.str("name").map(str::to_string),
                red: xz(n, "redPos"),
                blue: xz(n, "bluePos"),
            })
        })
        .collect()
}

/// The fitted projection and the pairs it kept, or the table's row and
/// whatever pairs there are. The samples are the kept pairs, so an outlier
/// the fit set aside does not fail the residual check of the very fit that
/// set it aside.
fn projection(theatre: &str, beacons: &[BeaconRow]) -> Result<(Crs, Vec<Sample>), Refusal> {
    let pairs: Vec<Pair> = beacons
        .iter()
        .filter_map(|b| {
            Some(Pair {
                x: b.x?,
                z: b.z?,
                lat: b.lat?,
                lon: b.lon?,
            })
        })
        .collect();
    let sample = |p: &Pair| Sample {
        x: p.x,
        z: p.z,
        lat: p.lat,
        lon: p.lon,
    };
    let crs = |t: Tmerc, n, rms_m, max_m, source| Crs {
        proj4: t.proj4(),
        lon_0: t.lon_0,
        easting: t.easting,
        northing: t.northing,
        k_0: crs::K_0,
        n,
        rms_m,
        max_m,
        source,
    };
    if let Some(f) = crs::fit(&pairs) {
        let c = crs(
            f.tmerc,
            Some(f.kept.len()),
            Some(f.rms_m),
            Some(f.max_m),
            "beacons",
        );
        return Ok((c, f.kept.iter().map(sample).collect()));
    }
    match crs::table(theatre) {
        Some(t) => Ok((
            crs(t, None, None, None, "table"),
            pairs.iter().map(sample).collect(),
        )),
        None => Err(Refusal(format!(
            "{theatre}: {} beacons carry both positions, too few to fit a projection, \
             and the theatre has no row in the projection table",
            pairs.len()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratch::Scratch;
    use serde_json::Value as Json;

    const MAP: &str = "Mods/terrains/Synth";
    const CAUCASUS: Tmerc = Tmerc {
        lon_0: 33,
        easting: 99517.0,
        northing: 4998115.0,
    };

    fn put(root: &Path, rel: &str, text: &str) {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
    }

    fn lua(rel: &str) -> FileEntry {
        FileEntry {
            role: "lua".into(),
            path: format!("{MAP}/{rel}"),
            class: None,
            version: None,
            size: 0,
            payload_size: None,
        }
    }

    const HEAD: &str = "dofile('Scripts/World/Radio/BeaconTypes.lua')\n\
        local gettext = require(\"i_18n\")\nlocal       _ = gettext.translate\n";

    /// A beacon at `(x, z)` with its degrees to six places, as ED writes them.
    fn beacon(id: &str, kind: &str, x: f64, z: f64, extra: &str) -> String {
        let (lat, lon) = CAUCASUS.to_latlon(x, z);
        format!(
            "\t{{\n\t\tdisplay_name = _('Anapa-Vityazevo');\n\t\tbeaconId = '{id}';\n\
             \t\ttype = {kind};\n\t\tcallsign = 'AP';\n{extra}\
             \t\tposition = {{ {x:.6}, 12.340623, {z:.6} }};\n\t\tdirection = 41.500201;\n\
             \t\tpositionGeo = {{ latitude = {lat:.6}, longitude = {lon:.6} }};\n\
             \t\tsceneObjects = {{'t:75145216'}};\n\t}};\n"
        )
    }

    /// An install whose map has every Lua file, with `n` positioned beacons.
    fn install(n: usize) -> (Scratch, Vec<FileEntry>) {
        let s = Scratch::new();
        let r = &s.0;
        put(
            r,
            "Scripts/World/Radio/BeaconTypes.lua",
            "package.path = package.path..';x'\nBEACON_TYPE_HOMER = 8\n\
             BEACON_TYPE_TACAN = 4\nlocal function f() end\n",
        );
        put(
            r,
            "Scripts/World/Radio/FrequencyBands.lua",
            "HF = 0\nVHF_LOW = 1\nVHF_HI = 2\nUHF = 3\n",
        );
        put(
            r,
            "Scripts/World/Radio/ModulationTypes.lua",
            "MODULATIONTYPE_AM = 0\n",
        );
        let at = [
            (-1321.8, 246748.5),
            (-356584.8, 618472.4),
            (-300000.0, 900000.0),
            (-50000.0, 400000.0),
        ];
        let mut b = format!("{HEAD}beaconsTableFormat = 2\nbeacons = {{\n");
        b += &beacon(
            "airfield12_0",
            "BEACON_TYPE_HOMER",
            at[0].0,
            at[0].1,
            "\t\tfrequency = 443000.000000;\n",
        );
        for (i, &(x, z)) in at.iter().enumerate().skip(1).take(n.saturating_sub(1)) {
            let id = format!("airfield22_{}", i - 1);
            b += &beacon(&id, "BEACON_TYPE_TACAN", x, z, "\t\tchannel = 16;\n");
        }
        b += "\t{ beaconId = 'world_1'; type = BEACON_TYPE_AIRPORT_TACAN; callsign = 'X' };\n}\n";
        put(r, &format!("{MAP}/Beacons.lua"), &b);
        put(
            r,
            &format!("{MAP}/Radio.lua"),
            "dofile('Scripts/World/Radio/ModulationTypes.lua')\n\
             dofile('Scripts/World/Radio/FrequencyBands.lua')\n\
             local gettext = require(\"i_18n\")\nlocal       _ = gettext.translate\n\
             radio = {\n\t{\n\t\t-- Anapa\n\t\tradioId = 'airfield12_0';\n\
             \t\trole = {\"ground\", \"tower\", \"approach\"};\n\
             \t\tcallsign = {{[\"nato\"] = {_(\"Batumi\"), \"Batumi\"}}, {[\"ussr\"] = {_(\"Druzhinnik\"), \"x\"}}};\n\
             \t\tfrequency = {[HF] = {MODULATIONTYPE_AM, 3750000.000000}, [UHF] = {MODULATIONTYPE_AM, 250000000.000000}, \
             [VHF_HI] = {MODULATIONTYPE_AM, 121000000.000000}, [VHF_LOW] = {MODULATIONTYPE_AM, 38400000.000000}};\n\
             \t\tsceneObjects = {'t:35192994'};\n\t};\n\
             \t{\n\t\t-- Batumi\n\t\tradioId = 'Batumi1';\n\t\trole = {};\n\t\tfrequency = {};\n\t};\n\
             \t{\n\t\tradioId = 'airfield22_0';\n\t\tfrequency = {[VHF_HI] = {MODULATIONTYPE_AM, 131000000.0}};\n\t};\n}\n",
        );
        let (lat, lon) = (45.053964, 39.000128);
        put(
            r,
            &format!("{MAP}/map/towns.lua"),
            &format!(
                "local gettext = require(\"i_18n\")\nlocal       _ = gettext.translate\n\ntowns = {{\n\
                 [\"KRASNODAR\"] = {{ latitude = {lat}, longitude = {lon}, display_name = _(\"KRASNODAR\")}},\n\
                 [\"ABASHA\"] = {{ latitude = 42.221963, longitude = 42.211652, display_name = _(\"ABASHA\")}},\n}}\n"
            ),
        );
        put(
            r,
            &format!("{MAP}/MissionGenerator/nodes.lua"),
            "missionNodes = \n{\n    [2] = \n    {\n        bluePos =  {-266428.57142857, 613828.57142857},\n\
             \x20       id = 2,\n        redPos =  {-248685.71428572, 630828.57142857},\n\
             \x20       name = \"2 SW-Rivers-1\",\n    }, -- end of [2]\n    [1] = \n    {\n\
             \x20       id = 1,\n        name = \"1-SW-Gali-1\",\n        redPos =  {-235357.1, 638342.9},\n\
             \x20       bluePos =  {-241543.1, 618581.4},\n    }, -- end of [1]\n}\n",
        );
        let files = vec![
            lua("Beacons.lua"),
            lua("Radio.lua"),
            lua("map/towns.lua"),
            lua("MissionGenerator/nodes.lua"),
        ];
        (s, files)
    }

    #[test]
    fn reads_each_beacon_with_its_type_from_the_shared_script() {
        let (s, files) = install(4);
        let t = decode(&s.0, "Caucasus", &files).unwrap();
        assert_eq!(t.beacons.len(), 5);
        let b = &t.beacons[0];
        assert_eq!(b.beacon_id.as_deref(), Some("airfield12_0"));
        assert_eq!(b.display_name.as_deref(), Some("Anapa-Vityazevo"));
        assert_eq!(
            (b.kind, b.frequency_hz, b.channel),
            (Some(8), Some(443000.0), None)
        );
        assert_eq!(
            (b.x, b.alt, b.z),
            (Some(-1321.8), Some(12.340623), Some(246748.5))
        );
        assert_eq!(b.scene_objects, ["t:75145216"]);
        assert_eq!(
            (t.beacons[1].kind, t.beacons[1].channel),
            (Some(4), Some(16))
        );
        // A type no script defines is no type, as in the game.
        let w = &t.beacons[4];
        assert_eq!((w.kind, w.x, w.lat), (None, None, None));
        assert_eq!(
            t.unresolved,
            [format!("{MAP}/Beacons.lua: BEACON_TYPE_AIRPORT_TACAN")]
        );
        let paths: Vec<&str> = t.included.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "Scripts/World/Radio/BeaconTypes.lua",
                "Scripts/World/Radio/ModulationTypes.lua",
                "Scripts/World/Radio/FrequencyBands.lua",
            ]
        );
    }

    #[test]
    fn reads_radio_bands_callsigns_and_the_airdrome_ids_it_labels() {
        let (s, files) = install(4);
        let t = decode(&s.0, "Caucasus", &files).unwrap();
        let r = &t.radio[0];
        assert_eq!(r.radio_id.as_deref(), Some("airfield12_0"));
        assert_eq!(r.roles, ["ground", "tower", "approach"]);
        let names: Vec<(&str, &str)> = r
            .callsigns
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        assert_eq!(names, [("nato", "Batumi"), ("ussr", "Druzhinnik")]);
        assert_eq!(
            r.frequencies_hz,
            Frequencies {
                hf: Some(3.75e6),
                fm: Some(38.4e6),
                vhf: Some(121e6),
                uhf: Some(250e6),
            }
        );
        assert_eq!(t.radio[1].frequencies_hz, Frequencies::default());
        assert_eq!(t.radio[2].frequencies_hz.vhf, Some(131e6));
        // Batumi's label sits on a station that is not `airfield<ID>_0`, and
        // the airdrome 22 station has no label.
        assert_eq!(t.airdrome_ids, [("Anapa".to_string(), 12)].into());
        assert_eq!(t.airdrome_id("Anapa"), Some(12));
        assert_eq!(t.airdrome_id("ANAPA"), Some(12));
        assert_eq!(t.airdrome_id("Batumi"), None);
        assert_eq!(t.radio_ids(22), ["airfield22_0"]);
        assert_eq!(
            t.beacon_ids(22),
            ["airfield22_0", "airfield22_1", "airfield22_2"]
        );
        assert_eq!(t.display_name(12), Some("Anapa-Vityazevo"));
        assert_eq!(t.beacon_ids(2), Vec::<String>::new());
    }

    #[test]
    fn fits_the_projection_and_puts_towns_through_it() {
        let (s, files) = install(4);
        let t = decode(&s.0, "Nowhere", &files).unwrap();
        let c = &t.crs;
        assert_eq!((c.source, c.lon_0, c.n), ("beacons", 33, Some(4)));
        assert!((c.easting - CAUCASUS.easting).abs() < 0.05, "{c:?}");
        assert!((c.northing - CAUCASUS.northing).abs() < 0.05, "{c:?}");
        assert!(c.max_m.unwrap() < 0.1);
        assert_eq!(c.proj4, c.tmerc().proj4());
        assert_eq!(t.latlon_samples.len(), 4);
        assert_eq!(
            (t.latlon_samples[0].x, t.latlon_samples[0].z),
            (-1321.8, 246748.5)
        );
        let names: Vec<&str> = t.towns.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["ABASHA", "KRASNODAR"]);
        let k = &t.towns[1];
        let (x, z) = c.tmerc().to_xz(45.053964, 39.000128);
        assert_eq!((k.x, k.z), (x, z));
        assert_eq!(k.display_name.as_deref(), Some("KRASNODAR"));
    }

    #[test]
    fn with_too_few_beacons_takes_the_tables_row_or_refuses() {
        let (s, files) = install(2);
        let t = decode(&s.0, "Caucasus", &files).unwrap();
        let c = &t.crs;
        assert_eq!(
            (c.source, c.n, c.rms_m, c.max_m),
            ("table", None, None, None)
        );
        assert_eq!(c.tmerc(), CAUCASUS);
        assert_eq!(t.latlon_samples.len(), 2);
        let e = decode(&s.0, "Kola", &files).unwrap_err();
        assert!(e.0.contains("Kola: 2 beacons carry both positions"), "{e}");
    }

    #[test]
    fn reads_nodes_in_key_order() {
        let (s, files) = install(4);
        let t = decode(&s.0, "Caucasus", &files).unwrap();
        assert_eq!(
            t.nodes[0],
            NodeRow {
                id: Some(1),
                name: Some("1-SW-Gali-1".into()),
                red: Some(Xz {
                    x: -235357.1,
                    z: 638342.9
                }),
                blue: Some(Xz {
                    x: -241543.1,
                    z: 618581.4
                }),
            }
        );
        assert_eq!(t.nodes[1].id, Some(2));
    }

    #[test]
    fn a_map_without_its_lua_files_has_empty_tables() {
        let (s, _) = install(4);
        let t = decode(&s.0, "Caucasus", &[]).unwrap();
        assert!(t.beacons.is_empty() && t.radio.is_empty() && t.towns.is_empty());
        assert_eq!(t.crs.source, "table");
        assert!(t.included.is_empty());
    }

    #[test]
    fn refuses_a_file_that_does_not_run_naming_it() {
        let (s, files) = install(4);
        put(&s.0, &format!("{MAP}/Radio.lua"), "radio = {}\nradio = {\n");
        let e = decode(&s.0, "Caucasus", &files).unwrap_err();
        assert!(e.0.contains(&format!("{MAP}/Radio.lua:3:")), "{e}");
        put(&s.0, &format!("{MAP}/Radio.lua"), "radios = {}\n");
        let e = decode(&s.0, "Caucasus", &files).unwrap_err();
        assert_eq!(e.0, format!("{MAP}/Radio.lua assigns no radio table"));
        put(
            &s.0,
            &format!("{MAP}/Radio.lua"),
            "dofile('Scripts/Missing.lua')\nradio = {}\n",
        );
        let e = decode(&s.0, "Caucasus", &files).unwrap_err();
        assert!(e.0.contains("dofile names Scripts/Missing.lua"), "{e}");
    }

    #[test]
    fn a_label_is_the_comment_on_the_line_before_its_station() {
        let text = "{\r\t\t-- Anapa\r\t\tradioId = 'airfield12_0';\r}\n\
                    {\n-- radioId = 'old';\nradioId = \"airfield13_0\";\n}\n\
                    {\n--[[ block ]]\nradioId = 'airfield14_0';\n}\n\
                    {\n\t-- Batumi\n\n\tradioId = 'airfield22_0';\n}\n";
        let labels = radio_labels(text.as_bytes());
        let got: Vec<(&str, &str)> = labels
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        assert_eq!(got, [("airfield12_0", "Anapa"), ("airfield22_0", "Batumi")]);
    }

    #[test]
    fn refuses_degrees_off_the_earth() {
        let (s, files) = install(4);
        let path = s.0.join(format!("{MAP}/Beacons.lua"));
        let text = fs::read_to_string(&path).unwrap();
        // The digits after the replaced "37." stay, so "1e999" reads as 1e999396435.
        for lon in ["1937.", "1e999", "-1e999"] {
            let bad = text.replacen("longitude = 37.", &format!("longitude = {lon}"), 1);
            fs::write(&path, bad).unwrap();
            let e = decode(&s.0, "Caucasus", &files).unwrap_err();
            assert!(e.0.contains("beacon 1: latitude 45."), "{lon}: {e}");
            assert!(e.0.contains("not on the earth"), "{lon}: {e}");
        }
        fs::write(&path, &text).unwrap();
        let towns = s.0.join(format!("{MAP}/map/towns.lua"));
        let text = fs::read_to_string(&towns).unwrap();
        fs::write(
            &towns,
            text.replace("latitude = 42.221963", "latitude = -91"),
        )
        .unwrap();
        let e = decode(&s.0, "Caucasus", &files).unwrap_err();
        assert!(e.0.contains("town ABASHA: latitude -91"), "{e}");
    }

    #[test]
    fn writes_the_four_tables() {
        let (s, files) = install(4);
        let t = decode(&s.0, "Caucasus", &files).unwrap();
        let out = s.0.join("out");
        fs::create_dir_all(&out).unwrap();
        t.write(&out).unwrap();
        let read = |name: &str| -> Json {
            serde_json::from_str(&fs::read_to_string(out.join(name)).unwrap()).unwrap()
        };
        let b = read(BEACONS_FILE);
        assert_eq!(b[0]["type"], 8);
        assert_eq!(b[4]["type"], Json::Null);
        assert_eq!(read(RADIO_FILE)[0]["frequencies_hz"]["fm"], 38.4e6);
        assert_eq!(read(TOWNS_FILE)[0]["name"], "ABASHA");
        assert_eq!(read(NODES_FILE)[1]["red"]["x"], -248685.71428572);
    }
}
