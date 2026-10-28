//! An install terrain's tables: its airdromes, runways, stands, beacons,
//! radio, towns and mission nodes, and the projection fitted to its beacons,
//! decoded once into the cache's `tables.json`.
//!
//! An airdrome is keyed by its `name_id`, the basename of its taxiway file,
//! across every source that names it. Its numeric id is the one `Radio.lua`
//! labels it with; its beacons, radio and display name follow from that id.
//! The few fields only a running sim knows are null here until the hook's
//! tables are imported.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::cache::{Entry, TerrainCache};
use crate::decode::airfield::{self, Airfields, RunwayRow, StandRow};
use crate::decode::install::Map;
use crate::decode::lua::tables::{
    self as lua, BeaconRow, Crs, LuaTables, NodeRow, RadioRow, TownRow,
};
use crate::extract::FileEntry;
use crate::source::Refusal;

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

    fn version(&self) -> u32 {
        1
    }

    fn roles(&self) -> &[&str] {
        &ROLES
    }

    fn build(
        &self,
        root: &Path,
        map: &Map,
        files: &[FileEntry],
        _: &Path,
    ) -> Result<Vec<u8>, Refusal> {
        let t = Tables::decode(root, map, files)?;
        Ok(serde_json::to_vec(&t).expect("the tables serialize"))
    }

    fn json(&self) -> bool {
        true
    }
}

/// One airdrome. `code`, `civilian`, `abandoned` and `class` come only from
/// the hook, and `id` from it where `Radio.lua` labels none.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AirdromeRow {
    pub id: Option<i64>,
    pub name_id: String,
    pub display_name: Option<String>,
    pub code: Option<String>,
    pub civilian: Option<bool>,
    pub abandoned: Option<bool>,
    pub class: Option<String>,
    /// The mean of its runway ends, which is the game's reference point;
    /// null with no runway.
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
    pub airdromes: Vec<AirdromeRow>,
    pub runways: Vec<RunwayRow>,
    pub stands: Vec<StandRow>,
    pub beacons: Vec<BeaconRow>,
    pub radio: Vec<RadioRow>,
    pub towns: Vec<TownRow>,
    pub nodes: Vec<NodeRow>,
    pub crs: Crs,
    /// The shared scripts the Lua files ran for their constants.
    pub lua_included: Vec<String>,
    /// Each name a Lua file read that nothing assigned, as `<path>: <name>`.
    pub lua_unresolved: Vec<String>,
}

/// The basename of a path relative to the install, without its extension.
fn stem(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.').map_or(name, |(s, _)| s)
}

impl Tables {
    /// The tables from the cache, built there first if missing or stale.
    pub fn open(cache: &TerrainCache) -> Result<Tables, Refusal> {
        let opened = cache.get(&TABLES_JSON)?;
        let unreadable = |e: &dyn std::fmt::Display| {
            Refusal(format!(
                "{} cannot be read ({e}); `dcsterrain cache clear --terrain {}` rebuilds it",
                opened.path.display(),
                cache.map().theatre
            ))
        };
        let bytes = opened.payload().map_err(|e| unreadable(&e))?;
        serde_json::from_slice(&bytes).map_err(|e| unreadable(&e))
    }

    /// Decodes the airfield and Lua files among `files`, relative to `root`.
    pub fn decode(root: &Path, map: &Map, files: &[FileEntry]) -> Result<Tables, Refusal> {
        let air = airfield::decode(root, files)?;
        let lua = lua::decode(root, &map.theatre, files)?;
        Ok(Tables::assemble(files, air, lua))
    }

    fn assemble(files: &[FileEntry], air: Airfields, lua: LuaTables) -> Tables {
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
                    name_id: name_id.to_string(),
                    display_name: None,
                    code: None,
                    civilian: None,
                    abandoned: None,
                    class: None,
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
        match self.crs.max_m {
            Some(m) if m > MAX_CRS_RESIDUAL_M && !allow => Err(Refusal(format!(
                "{terrain}: the projection fitted to {} beacons misses one by {m:.2} m, over \
                 the {MAX_CRS_RESIDUAL_M} m allowed; --allow-crs-residual accepts it",
                self.crs.n.unwrap_or(0)
            ))),
            _ => Ok(()),
        }
    }
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
        assert_eq!(
            (&a.code, a.civilian, a.abandoned, &a.class),
            (&None, None, None, &None)
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
    fn is_read_back_from_the_cache_as_built() {
        let root = install();
        let c = Scratch::new();
        let terrain = terrain(&root.0, &c.0);
        let built = Tables::open(&terrain).unwrap();
        assert!(!terrain.get(&TABLES_JSON).unwrap().built);
        assert_eq!(Tables::open(&terrain).unwrap(), built);
        let path = c.0.join("Test/1.2.3.4/tables.json");
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
    fn a_taxiway_file_no_decoder_reads_refuses_the_tables_by_name() {
        let root = install();
        let bad = container::build("t5::rn5file", 99, 0);
        put(&root.0, &format!("{MAP}/AirfieldsTaxiways/X.rn5"), &bad);
        let c = Scratch::new();
        let e = Tables::open(&terrain(&root.0, &c.0)).unwrap_err();
        assert!(e.0.contains("X.rn5") && e.0.contains("version 99"), "{e}");
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
