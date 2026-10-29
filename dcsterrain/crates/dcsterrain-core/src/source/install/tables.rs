//! An install terrain's tables: its airdromes, runways, stands, beacons,
//! radio, towns and mission nodes, and the projection fitted to its beacons,
//! decoded once into the cache's `tables.json`.
//!
//! An airdrome is keyed by its `name_id`, the basename of its taxiway file,
//! across every source that names it. Its numeric id is the one `Radio.lua`
//! labels it with or, failing that, one joined by position; its beacons,
//! radio and display name follow from that id.

use std::collections::BTreeSet;
use std::fs::File;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::cache::{Entry, Opened, TerrainCache};
use crate::decode::airfield::{self, Airfields, RunwayRow, StandRow};
use crate::decode::install::{self, Map};
use crate::decode::lua::tables::{
    self as lua, BeaconRow, Crs, LuaTables, NodeRow, RadioRow, TownRow,
};
use crate::decode::scn5::Index;
use crate::extract::FileEntry;
use crate::grid::box_distance;
use crate::source::Refusal;

use super::SCENE_NODES;
use super::airdrome_ids::{BEACON_REACH_M, TOWER_REACH_M, nearest, unanimous};

/// A fitted projection whose worst kept beacon is further off than this
/// refuses the terrain unless the caller accepts it: every measured terrain
/// fits within 0.1 m, so a few meters is a wrong fit, not rounding.
pub const MAX_CRS_RESIDUAL_M: f64 = 2.0;

/// The roles `tables.json` is derived from.
const ROLES: [&str; 4] = ["airfield_rn4", "airfield_rn5", "assets", "lua"];

/// The terrain's tables, as `tables.json` stores them.
pub struct TablesJson;

pub static TABLES_JSON: TablesJson = TablesJson;

impl Entry for TablesJson {
    fn name(&self) -> &str {
        "tables.json"
    }

    /// 3 dropped four airdrome fields and the record of an import, so an
    /// entry of 2 is rebuilt rather than read with them. 4 joins ids by
    /// position where `Radio.lua` labels none.
    fn version(&self) -> u32 {
        4
    }

    fn roles(&self) -> &[&str] {
        &ROLES
    }

    fn build(&self, cache: &TerrainCache, files: &[FileEntry]) -> Result<Vec<u8>, Refusal> {
        let (root, map) = (cache.install(), cache.map());
        // The towers are placed through the scene's cached index: parsing
        // the scene file's head for a few ids would cost hundreds of MiB.
        // The scene is not one of the roles: a scene no decoder reads
        // refuses scenery, not the tables, and costs only the ids its towers
        // give. Nor is the entry rebuilt when the scene alone changes, which
        // moves a tower too little to change the airdrome nearest it.
        let index = || {
            let Some(scene) = map.files.iter().find(|f| f.role == "scene") else {
                return Ok(None);
            };
            if install::classify(root, std::slice::from_ref(scene)).is_err() {
                return Ok(None);
            }
            let opened = cache.get(&SCENE_NODES)?;
            let bytes = (opened.payload()).map_err(|e| unreadable(cache, &opened, &e))?;
            let index = Index::decode(&bytes).map_err(|e| unreadable(cache, &opened, &e))?;
            Ok(Some((index, scene.path.clone())))
        };
        let t = Tables::decode(root, map, files, &index)?;
        let bytes = serde_json::to_vec(&t).expect("the tables serialize");
        // JSON has no NaN or infinity, which it writes as null: a file that
        // gave one is refused now rather than read back as something else.
        if serde_json::from_slice::<Tables>(&bytes).ok().as_ref() != Some(&t) {
            return Err(Refusal(format!(
                "{}'s airfield or Lua files hold a number that is not finite",
                map.theatre
            )));
        }
        Ok(bytes)
    }

    fn json(&self) -> bool {
        true
    }
}

/// The scene's index and the scene file's path, relative to the install;
/// `None` where the terrain has no scene a decoder reads.
pub type SceneIndex = Option<(Index, String)>;

/// A radio station's scene object: the `N` of its `airfieldN_k`, and where
/// the scene places it, x and z; `None` where it does not.
pub type Tower = (i64, Option<[f64; 2]>);

/// What gave an airdrome its id: `Radio.lua`'s label, the beacons nearest
/// it, or a station's scene object within its extent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IdBy {
    Label,
    Beacons,
    Tower,
}

/// One airdrome. Its `display_name` is the name of its first beacon.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AirdromeRow {
    pub id: Option<i64>,
    /// What gave the id; null with none.
    pub id_by: Option<IdBy>,
    pub name_id: String,
    pub display_name: Option<String>,
    /// The mean of its runway ends: within 3 mm of the game's reference
    /// point on Caucasus, whose airdromes have one runway each. Null with
    /// no runway.
    pub x: Option<f64>,
    pub z: Option<f64>,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    /// Each runway's two end names, in runway order.
    pub runway_names: Vec<String>,
    pub beacon_ids: Vec<String>,
    pub radio_ids: Vec<String>,
    /// Its taxiway files, relative to the install and written as the game
    /// writes them, from `./`.
    pub roadnet: String,
    pub roadnet5: Option<String>,
}

/// Everything `tables.json` holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tables {
    /// First, so it is the first field written, and opening a terrain reads
    /// it without the rows.
    pub crs: Crs,
    pub airdromes: Vec<AirdromeRow>,
    pub runways: Vec<RunwayRow>,
    pub stands: Vec<StandRow>,
    pub beacons: Vec<BeaconRow>,
    pub radio: Vec<RadioRow>,
    pub towns: Vec<TownRow>,
    pub nodes: Vec<NodeRow>,
    /// The shared scripts the Lua files ran for their constants.
    pub lua_included: Vec<String>,
    /// Each name a Lua file read that nothing assigned, as `<path>: <name>`.
    pub lua_unresolved: Vec<String>,
}

/// How the payload opens: the projection is its first field.
const CRS_FIELD: &[u8] = b"{\"crs\":";

/// How much of the payload is read for the projection, which takes a few
/// hundred bytes.
const CRS_HEAD: u64 = 4096;

/// The projection from the first bytes of a payload, `None` when they stop
/// inside it. The value is parsed and what follows it is not looked at.
fn crs_from(head: &[u8]) -> Option<Crs> {
    let rest = head.strip_prefix(CRS_FIELD)?;
    Crs::deserialize(&mut serde_json::Deserializer::from_slice(rest)).ok()
}

fn unreadable(cache: &TerrainCache, opened: &Opened, e: &dyn std::fmt::Display) -> Refusal {
    Refusal(format!(
        "{} cannot be read ({e}); `dcsterrain cache clear --terrain {}` rebuilds it",
        opened.path.display(),
        cache.map().theatre
    ))
}

/// Refuses a projection fitted with a residual over `MAX_CRS_RESIDUAL_M`
/// unless `allow` accepts it.
pub fn check_crs(crs: &Crs, terrain: &str, allow: bool) -> Result<(), Refusal> {
    match crs.max_m {
        Some(m) if m > MAX_CRS_RESIDUAL_M && !allow => Err(Refusal(format!(
            "{terrain}: the projection fitted to {} beacons misses one by {m:.2} m, over \
             the {MAX_CRS_RESIDUAL_M} m allowed; --allow-crs-residual accepts it",
            crs.n.unwrap_or(0)
        ))),
        _ => Ok(()),
    }
}

/// The basename of a path relative to the install, without its extension.
fn stem(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.').map_or(name, |(s, _)| s)
}

impl Tables {
    /// The tables from the cache, built there first if missing or stale.
    pub fn open(cache: &TerrainCache) -> Result<Tables, Refusal> {
        Tables::read(cache, &cache.get(&TABLES_JSON)?)
    }

    /// The tables from `opened`, the cache's `tables.json`.
    pub fn read(cache: &TerrainCache, opened: &Opened) -> Result<Tables, Refusal> {
        let bytes = opened
            .payload()
            .map_err(|e| unreadable(cache, opened, &e))?;
        serde_json::from_slice(&bytes).map_err(|e| unreadable(cache, opened, &e))
    }

    /// The projection alone from `opened`, the cache's `tables.json`, read
    /// from the payload's first bytes, since it is the first field: a call
    /// that only checks the projection then never parses the megabytes of
    /// rows after it.
    pub fn read_crs(cache: &TerrainCache, opened: &Opened) -> Result<Crs, Refusal> {
        let head = opened
            .head(CRS_HEAD)
            .map_err(|e| unreadable(cache, opened, &e))?;
        match crs_from(&head) {
            Some(crs) => Ok(crs),
            // A projection longer than the head is read with the rest.
            None => Ok(Tables::read(cache, opened)?.crs),
        }
    }

    /// Decodes the airfield and Lua files among `files`, relative to `root`,
    /// with `index` giving the scene's index when a station names an object.
    pub fn decode(
        root: &Path,
        map: &Map,
        files: &[FileEntry],
        index: &dyn Fn() -> Result<SceneIndex, Refusal>,
    ) -> Result<Tables, Refusal> {
        let air = airfield::decode(root, files)?;
        let lua = lua::decode(root, &map.theatre, files)?;
        let towers = Tables::towers(root, &lua.radio, index)?;
        Ok(Tables::assemble(files, air, lua, &towers))
    }

    /// `towers` are each `Radio.lua` station's scene objects, by the `N` of
    /// its `airfieldN_k`, where the scene places them.
    fn assemble(files: &[FileEntry], air: Airfields, lua: LuaTables, towers: &[Tower]) -> Tables {
        let role = |r: &'static str| files.iter().filter(move |f| f.role == r);
        let tmerc = lua.crs.tmerc();
        let mut airdromes: Vec<AirdromeRow> = role("airfield_rn4")
            .map(|f| {
                let name_id = stem(&f.path);
                let rn5 =
                    role("airfield_rn5").find(|g| stem(&g.path).eq_ignore_ascii_case(name_id));
                let runways: Vec<&RunwayRow> = air
                    .runways
                    .iter()
                    .filter(|r| r.airdrome_name_id == name_id)
                    .collect();
                let (x, z) = reference_point(&runways);
                let (lat, lon) = match (x, z) {
                    (Some(x), Some(z)) => {
                        let (lat, lon) = tmerc.to_latlon(x, z);
                        (Some(lat), Some(lon))
                    }
                    _ => (None, None),
                };
                AirdromeRow {
                    id: None,
                    id_by: None,
                    name_id: name_id.to_string(),
                    display_name: None,
                    x,
                    z,
                    lat,
                    lon,
                    runway_names: runways
                        .iter()
                        .flat_map(|r| [r.edge1_name.clone(), r.edge2_name.clone()])
                        .collect(),
                    beacon_ids: Vec::new(),
                    radio_ids: Vec::new(),
                    roadnet: format!("./{}", f.path),
                    roadnet5: rn5.map(|g| format!("./{}", g.path)),
                }
            })
            .collect();
        for a in &mut airdromes {
            a.id = lua.airdrome_id(&a.name_id);
            a.id_by = a.id.map(|_| IdBy::Label);
        }
        let mut t = Tables {
            airdromes,
            runways: air.runways,
            stands: air.stands,
            beacons: lua.beacons,
            radio: lua.radio,
            towns: lua.towns,
            nodes: lua.nodes,
            crs: lua.crs,
            lua_included: lua.included.into_iter().map(|f| f.path).collect(),
            lua_unresolved: lua.unresolved,
        };
        // Where the label gives none, an id joined by position: never one a
        // label gives, even a label that names no taxiway file.
        let mut taken: BTreeSet<i64> = t.airdromes.iter().filter_map(|a| a.id).collect();
        taken.extend(lua.airdrome_ids.values());
        let joined = t.joined(towers, &taken);
        for (a, j) in t.airdromes.iter_mut().zip(joined) {
            if let (None, Some((id, by))) = (a.id, j) {
                (a.id, a.id_by) = (Some(id), Some(by));
            }
        }
        t.follow_ids();
        t
    }

    /// Fills what follows from each airdrome's id: its beacons, radio and
    /// display name, and the id on its runways and stands. Then sorts the
    /// airdromes by id, those without one last, by name.
    fn follow_ids(&mut self) {
        let of = |s: &Option<String>, id| s.as_deref().and_then(lua::airdrome_of) == Some(id);
        for a in &mut self.airdromes {
            let Some(id) = a.id else { continue };
            let mine = self.beacons.iter().filter(|b| of(&b.beacon_id, id));
            a.beacon_ids = mine.clone().filter_map(|b| b.beacon_id.clone()).collect();
            a.display_name = mine.clone().next().and_then(|b| b.display_name.clone());
            a.radio_ids = (self.radio.iter())
                .filter(|r| of(&r.radio_id, id))
                .filter_map(|r| r.radio_id.clone())
                .collect();
        }
        let id_of = |name_id: &str| {
            self.airdromes
                .iter()
                .find(|a| a.name_id == name_id)
                .and_then(|a| a.id)
        };
        let ids: Vec<Option<i64>> = self
            .runways
            .iter()
            .map(|r| id_of(&r.airdrome_name_id))
            .collect();
        for (r, id) in self.runways.iter_mut().zip(ids) {
            r.airdrome_id = id;
        }
        let ids: Vec<Option<i64>> = self
            .stands
            .iter()
            .map(|s| id_of(&s.airdrome_name_id))
            .collect();
        for (s, id) in self.stands.iter_mut().zip(ids) {
            s.airdrome_id = id;
        }
        self.airdromes.sort_by(|a, b| {
            (a.id.is_none(), a.id, &a.name_id).cmp(&(b.id.is_none(), b.id, &b.name_id))
        });
    }

    /// Refuses a projection fitted with a residual over
    /// `MAX_CRS_RESIDUAL_M` unless `allow` accepts it.
    pub fn check_crs(&self, terrain: &str, allow: bool) -> Result<(), Refusal> {
        check_crs(&self.crs, terrain, allow)
    }

    /// The id each airdrome takes by position, and what gave it: the `N` of
    /// the beacons nearest it, then of a tower, one of `towers`, nearest its
    /// extent. No id in `taken`, or that the beacons give another airdrome,
    /// is given. A tower the scene does not place votes for none.
    ///
    /// With `taken` empty, it joins the airdromes `Radio.lua` labels as well,
    /// which is how the joins are checked against the labels.
    pub fn joined(&self, towers: &[Tower], taken: &BTreeSet<i64>) -> Vec<Option<(i64, IdBy)>> {
        let n = self.airdromes.len();
        let mut taken = taken.clone();
        let sites: Vec<Option<[f64; 2]>> = (self.airdromes.iter())
            .map(|a| Some([a.x?, a.z?]))
            .collect();
        let beacons = self.beacons.iter().filter_map(|b| {
            let id = lua::airdrome_of(b.beacon_id.as_deref()?)?;
            let p = [b.x?, b.z?];
            let d = sites
                .iter()
                .map(|s| s.map(|s| (s[0] - p[0]).hypot(s[1] - p[1])));
            Some((id, nearest(d, BEACON_REACH_M)))
        });
        let by_beacons = unanimous(n, beacons, &taken);
        taken.extend(by_beacons.iter().flatten());
        let extents: Vec<Option<[f64; 4]>> = (self.airdromes.iter())
            .map(|a| extent(&a.name_id, &self.runways, &self.stands))
            .collect();
        let towers = towers.iter().map(|&(id, p)| {
            let site = p.and_then(|p| {
                let d = extents.iter().map(|e| e.map(|e| box_distance(e, p)));
                nearest(d, TOWER_REACH_M)
            });
            (id, site)
        });
        let by_tower = unanimous(n, towers, &taken);
        (by_beacons.into_iter().zip(by_tower))
            .map(|(b, t)| {
                (b.map(|id| (id, IdBy::Beacons))).or_else(|| t.map(|id| (id, IdBy::Tower)))
            })
            .collect()
    }

    /// Each scene object a `Radio.lua` station names, `t:<id>`, by the `N`
    /// of the station's `airfieldN_k`, where the terrain's scene places it.
    /// `index` gives the scene's index, asked for only when a station names
    /// an object; with none, there are no towers. An object the scene does
    /// not place, or not named as `t:<id>`, has no position.
    pub fn towers(
        root: &Path,
        radio: &[RadioRow],
        index: &dyn Fn() -> Result<SceneIndex, Refusal>,
    ) -> Result<Vec<Tower>, Refusal> {
        let named: Vec<(i64, Option<u64>)> = radio
            .iter()
            .filter_map(|row| Some((lua::airdrome_of(row.radio_id.as_deref()?)?, row)))
            .flat_map(|(n, row)| {
                let ids = row.scene_objects.iter();
                ids.map(move |o| (n, o.strip_prefix("t:").and_then(|s| s.parse().ok())))
            })
            .collect();
        if named.is_empty() {
            return Ok(Vec::new());
        }
        let Some((index, path)) = index()? else {
            return Ok(Vec::new());
        };
        let refuse = |e: &dyn std::fmt::Display| Refusal(format!("{path}: {e}"));
        let file = File::open(root.join(&path)).map_err(|e| refuse(&e))?;
        let mut out = Vec::new();
        for (n, id) in named {
            let at = match id {
                Some(id) => index.position(&file, id).map_err(|e| refuse(&e))?,
                None => None,
            };
            out.push((n, at.map(|[x, _, z]| [x, z])));
        }
        Ok(out)
    }
}

/// The box around an airdrome's runway ends and stands, `[min_x, min_z,
/// max_x, max_z]`; `None` with neither.
fn extent(name_id: &str, runways: &[RunwayRow], stands: &[StandRow]) -> Option<[f64; 4]> {
    let ends = (runways.iter())
        .filter(|r| r.airdrome_name_id == name_id)
        .flat_map(|r| [[r.edge1_x, r.edge1_z], [r.edge2_x, r.edge2_z]]);
    let stands = (stands.iter())
        .filter(|s| s.airdrome_name_id == name_id)
        .map(|s| [s.x, s.z]);
    ends.chain(stands).fold(None, |b, [x, z]| {
        let [x0, z0, x1, z1] = b.unwrap_or([x, z, x, z]);
        Some([x0.min(x), z0.min(z), x1.max(x), z1.max(z)])
    })
}

/// The mean of the runway ends; `None` with no runway.
fn reference_point(runways: &[&RunwayRow]) -> (Option<f64>, Option<f64>) {
    if runways.is_empty() {
        return (None, None);
    }
    let n = 2.0 * runways.len() as f64;
    let x: f64 = runways.iter().map(|r| r.edge1_x + r.edge2_x).sum();
    let z: f64 = runways.iter().map(|r| r.edge1_z + r.edge2_z).sum();
    (Some(x / n), Some(z / n))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::cache::Cache;
    use crate::crs::Tmerc;
    use crate::decode::airfield::build::{self as runway, file};
    use crate::decode::assets;
    use crate::decode::container;
    use crate::decode::install;
    use crate::decode::rn5;
    use crate::scratch::Scratch;

    const MAP: &str = "Mods/terrains/Test";
    const TM: Tmerc = Tmerc {
        lon_0: 33,
        easting: 99517.0,
        northing: 4998115.0,
    };

    fn put(root: &Path, rel: &str, bytes: &[u8]) {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, bytes).unwrap();
    }

    /// An airfield of one runway, `22` at `a` to `04` at `b`, and two stands.
    fn airfield(a: [f64; 2], b: [f64; 2]) -> Vec<u8> {
        let node = |name, [x, z]: [f64; 2], links: &[u32]| runway::Node {
            name: Some(name),
            at: [x, 20.0, z],
            heading: 0.25,
            links: links.iter().map(|&j| (j, 100.0)).collect(),
        };
        file(
            &["vpp"],
            &[
                node("1#22", a, &[]),
                node("A1", [a[0] - 400.0, a[1] + 600.0], &[3]),
                node("A2", [a[0] - 700.0, a[1] + 500.0], &[3]),
                node("04", b, &[1, 2]),
            ],
        )
    }

    /// A beacon at `(x, z)`, its degrees to six places.
    fn beacon(id: &str, x: f64, z: f64) -> String {
        let (lat, lon) = TM.to_latlon(x, z);
        format!(
            "{{ display_name = 'Anapa-Vityazevo'; beaconId = '{id}'; type = 8; \
             position = {{ {x}, 12.0, {z} }}; \
             positionGeo = {{ latitude = {lat:.6}, longitude = {lon:.6} }} }},\n"
        )
    }

    /// An install holding `Test`: Anapa, which `Radio.lua` labels airdrome
    /// 12, with its stand classes; Nowhere, which nothing labels; and the
    /// beacons the projection is fitted to.
    fn install() -> Scratch {
        let s = Scratch::new();
        let r = &s.0;
        put(
            r,
            "autoupdate.cfg",
            br#"{"version": "1.2.3.4", "timestamp": "20000101-000000"}"#,
        );
        put(
            r,
            &format!("{MAP}/entry.lua"),
            b"theatre = { ['id'] = \"Test\" }\n",
        );
        let surface = container::build("landscape5::Surface5File", 5, 0);
        put(r, &format!("{MAP}/Surface/Test.surface5"), &surface);
        let rn4 = container::build("landscape4::lRoadNetwork", 5, 0);
        put(r, &format!("{MAP}/roads/Test.rn4"), &rn4);
        let scn5 = container::build("landscape5::Scene5File", 9, 0);
        put(r, &format!("{MAP}/Scenes/Test.scn5"), &scn5);
        let taxi = format!("{MAP}/AirfieldsTaxiways");
        put(
            r,
            &format!("{taxi}/Anapa.rn4"),
            &airfield([-4427.5, 244_000.0], [-6396.5, 242_258.0]),
        );
        put(
            r,
            &format!("{taxi}/ANAPA.rn5"),
            &rn5::build::file(&[(0, 1, &[], -4827.5, 244_600.0)]),
        );
        put(
            r,
            &format!("{taxi}/Nowhere.rn4"),
            &airfield([-1000.0, 1000.0], [-1000.0, 3000.0]),
        );
        let mut a = vec![0; 8];
        assets::build::record(
            &mut a,
            "stand_end_24m",
            &[("height", "8")],
            &[[-11.0, -13.0], [11.0, 13.0]],
        );
        put(r, &format!("{MAP}/misc/assets.assets"), &a);
        let mut b = String::from("beacons = {\n");
        b += &beacon("airfield12_0", -1321.8, 246748.5);
        b += &beacon("airfield12_1", -356584.8, 618472.4);
        b += &beacon("world_1", -300000.0, 900000.0);
        b += "}\n";
        put(r, &format!("{MAP}/Beacons.lua"), b.as_bytes());
        let radio = "radio = {\n\t{\n\t\t-- Anapa\n\t\tradioId = \"airfield12_0\";\n\t};\n}\n";
        put(r, &format!("{MAP}/Radio.lua"), radio.as_bytes());
        s
    }

    fn terrain(root: &Path, cache: &Path) -> TerrainCache {
        let map = install::open(root, "Test").unwrap();
        Cache::new(cache).terrain(root, &map)
    }

    #[test]
    fn keys_every_table_by_the_airdrome_and_fills_what_its_id_gives() {
        let root = install();
        let c = Scratch::new();
        let t = Tables::open(&terrain(&root.0, &c.0)).unwrap();
        let names: Vec<(&str, Option<i64>)> = t
            .airdromes
            .iter()
            .map(|a| (a.name_id.as_str(), a.id))
            .collect();
        assert_eq!(names, [("Anapa", Some(12)), ("Nowhere", None)]);

        let a = &t.airdromes[0];
        let (x, z) = ((-4427.5 - 6396.5) / 2.0, (244_000.0 + 242_258.0) / 2.0);
        assert_eq!((a.x, a.z), (Some(x), Some(z)));
        let (lat, lon) = t.crs.tmerc().to_latlon(x, z);
        assert_eq!((a.lat, a.lon), (Some(lat), Some(lon)));
        assert_eq!(a.runway_names, ["22", "04"]);
        assert_eq!(a.beacon_ids, ["airfield12_0", "airfield12_1"]);
        assert_eq!(a.radio_ids, ["airfield12_0"]);
        assert_eq!(a.display_name.as_deref(), Some("Anapa-Vityazevo"));
        assert_eq!(a.roadnet, format!("./{MAP}/AirfieldsTaxiways/Anapa.rn4"));
        assert_eq!(
            a.roadnet5,
            Some(format!("./{MAP}/AirfieldsTaxiways/ANAPA.rn5"))
        );

        let n = &t.airdromes[1];
        assert!(n.beacon_ids.is_empty() && n.radio_ids.is_empty());
        assert_eq!((&n.display_name, &n.roadnet5), (&None, &None));

        let runway_ids: Vec<(&str, Option<i64>)> = t
            .runways
            .iter()
            .map(|r| (r.airdrome_name_id.as_str(), r.airdrome_id))
            .collect();
        assert_eq!(runway_ids, [("Anapa", Some(12)), ("Nowhere", None)]);
        let classed: Vec<_> = t
            .stands
            .iter()
            .map(|s| (s.airdrome_id, s.stand_class.as_deref()))
            .collect();
        assert_eq!(
            classed,
            [
                (Some(12), Some("stand_end_24m")),
                (Some(12), None),
                (None, None),
                (None, None)
            ]
        );

        assert_eq!((t.crs.source.as_str(), t.crs.lon_0), ("beacons", 33));
        assert_eq!(t.beacons.len(), 3);
        assert!(t.lua_unresolved.is_empty());
    }

    #[test]
    fn an_airdrome_no_label_names_takes_the_id_its_beacons_then_tower_give() {
        let root = install();
        // A beacon of airdrome 7 beside Nowhere, which no label names.
        let path = root.0.join(format!("{MAP}/Beacons.lua"));
        let text = fs::read_to_string(&path).unwrap();
        let text = text.replacen(
            "}\n",
            &format!("{}}}\n", beacon("airfield7_0", -1000.0, 2400.0)),
            1,
        );
        fs::write(&path, text).unwrap();
        let c = Scratch::new();
        let t = Tables::open(&terrain(&root.0, &c.0)).unwrap();
        let ids: Vec<_> = (t.airdromes.iter())
            .map(|a| (a.name_id.as_str(), a.id, a.id_by))
            .collect();
        assert_eq!(
            ids,
            [
                ("Nowhere", Some(7), Some(IdBy::Beacons)),
                ("Anapa", Some(12), Some(IdBy::Label))
            ]
        );
        // What follows from the id follows from a joined one too.
        assert_eq!(t.airdromes[0].beacon_ids, ["airfield7_0"]);

        // Held out, Anapa takes no id from its beacons: one lies 450 km off,
        // so they disagree, and that costs the id rather than gives a wrong one.
        let blind = t.joined(&[], &BTreeSet::new());
        assert_eq!(blind, [Some((7, IdBy::Beacons)), None]);
        // A tower within reach of Nowhere's runway names it, where the
        // beacons give it none; one further out names nothing.
        let held = [7, 12].into();
        let near = [(9, Some([-1500.0, 2000.0]))];
        assert_eq!(t.joined(&near, &held), [Some((9, IdBy::Tower)), None]);
        let far = [(9, Some([-3000.0, 2000.0]))];
        assert_eq!(t.joined(&far, &held), [None, None]);
        // A tower of 9 the scene does not place spoils the other's vote.
        let unplaced = [(9, Some([-1500.0, 2000.0])), (9, None)];
        assert_eq!(t.joined(&unplaced, &held), [None, None]);
        // An id taken already, by a label even of no airdrome, is not given.
        let labeled = [7, 12, 9].into();
        assert_eq!(t.joined(&near, &labeled), [None, None]);
    }

    #[test]
    fn a_scene_no_decoder_reads_costs_the_towers_not_the_tables() {
        let root = install();
        let radio = "radio = {\n\t{\n\t\t-- Anapa\n\t\tradioId = \"airfield12_0\";\n\t};\n\
                     \t{\n\t\tradioId = \"airfield7_0\";\n\t\tsceneObjects = {'t:5'};\n\t};\n}\n";
        put(&root.0, &format!("{MAP}/Radio.lua"), radio.as_bytes());
        let scene = container::build("landscape5::Scene5File", 99, 0);
        put(&root.0, &format!("{MAP}/Scenes/Test.scn5"), &scene);
        let c = Scratch::new();
        let t = Tables::open(&terrain(&root.0, &c.0)).unwrap();
        let ids: Vec<_> = (t.airdromes.iter())
            .map(|a| (a.name_id.as_str(), a.id))
            .collect();
        assert_eq!(ids, [("Anapa", Some(12)), ("Nowhere", None)]);
    }

    #[test]
    fn the_projection_is_read_alone_as_the_whole_reads_it() {
        let root = install();
        let c = Scratch::new();
        let terrain = terrain(&root.0, &c.0);
        let opened = terrain.get(&TABLES_JSON).unwrap();
        assert!(opened.payload().unwrap().starts_with(CRS_FIELD));
        let whole = Tables::read(&terrain, &opened).unwrap();
        assert_eq!(Tables::read_crs(&terrain, &opened).unwrap(), whole.crs);
        // A head that stops inside the projection gives none, and the whole
        // payload is read instead.
        let bytes = opened.payload().unwrap();
        let n = CRS_FIELD.len() + serde_json::to_vec(&whole.crs).unwrap().len();
        assert!((n as u64) < CRS_HEAD, "{n}");
        assert_eq!(crs_from(&bytes[..n]), Some(whole.crs.clone()));
        assert_eq!(crs_from(&bytes[..n - 1]), None);
    }

    #[test]
    fn is_read_back_from_the_cache_as_built() {
        let root = install();
        let c = Scratch::new();
        let terrain = terrain(&root.0, &c.0);
        let built = Tables::open(&terrain).unwrap();
        assert!(!terrain.get(&TABLES_JSON).unwrap().built);
        assert_eq!(Tables::open(&terrain).unwrap(), built);
        let path = c.0.join("Test/tables.json");
        let doc: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert_eq!(doc["payload"]["airdromes"][0]["name_id"], "Anapa");

        // A new taxiway file is seen from the listing, and rebuilds it.
        put(
            &root.0,
            &format!("{MAP}/AirfieldsTaxiways/Batumi.rn4"),
            &airfield([0.0, 0.0], [0.0, 2000.0]),
        );
        assert_eq!(Tables::open(&terrain).unwrap().airdromes.len(), 3);
    }

    #[test]
    fn an_entry_of_version_2_is_rebuilt_without_its_dropped_fields() {
        let root = install();
        let c = Scratch::new();
        let terrain = terrain(&root.0, &c.0);
        let built = Tables::open(&terrain).unwrap();
        // An entry as version 2 wrote it, with a field 3 dropped on a row.
        let path = c.0.join("Test/tables.json");
        let text = fs::read_to_string(&path).unwrap();
        let old = text.replacen("\"version\":4", "\"version\":2", 1).replacen(
            "\"name_id\":",
            "\"code\":\"URKA\",\"name_id\":",
            1,
        );
        assert_ne!(old, text);
        fs::write(&path, old).unwrap();
        assert!(terrain.get(&TABLES_JSON).unwrap().built);
        assert_eq!(Tables::open(&terrain).unwrap(), built);
        let doc: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(doc["version"], 4);
        assert!(doc["payload"]["airdromes"][0].get("code").is_none());
    }

    #[test]
    fn a_taxiway_file_no_decoder_reads_refuses_the_tables_by_name() {
        let root = install();
        let bad = container::build("t5::rn5file", 99, 0);
        put(&root.0, &format!("{MAP}/AirfieldsTaxiways/X.rn5"), &bad);
        let c = Scratch::new();
        let e = Tables::open(&terrain(&root.0, &c.0)).unwrap_err();
        assert!(e.0.contains("X.rn5") && e.0.contains("version 99"), "{e}");
    }

    #[test]
    fn refuses_a_number_json_cannot_hold() {
        let root = install();
        let path = root.0.join(format!("{MAP}/Beacons.lua"));
        let text = fs::read_to_string(&path).unwrap();
        fs::write(
            &path,
            text.replacen("type = 8;", "type = 8; direction = 0/0;", 1),
        )
        .unwrap();
        let c = Scratch::new();
        let e = Tables::open(&terrain(&root.0, &c.0)).unwrap_err();
        assert!(e.0.contains("not finite"), "{e}");
    }

    #[test]
    fn a_fit_off_by_more_than_two_meters_refuses_unless_allowed() {
        let root = install();
        let c = Scratch::new();
        let mut t = Tables::open(&terrain(&root.0, &c.0)).unwrap();
        assert!(t.check_crs("Test", false).is_ok());
        t.crs.max_m = Some(2.5);
        let e = t.check_crs("Test", false).unwrap_err();
        assert!(
            e.0.contains("2.50 m") && e.0.contains("--allow-crs-residual"),
            "{e}"
        );
        assert!(t.check_crs("Test", true).is_ok());
        // A projection from the table has no residual to check.
        t.crs.max_m = None;
        assert!(t.check_crs("Test", false).is_ok());
    }
}
