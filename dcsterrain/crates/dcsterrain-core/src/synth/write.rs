//! Writes the synthetic theatre as a complete extract directory.
//!
//! The directory is what the file decoders write from a real map, so `pack`
//! and `check-extract` read the two the same way. As a real run does, the
//! writer refuses a directory that is not empty, writes every file under a
//! `.tmp` name and renames it into place, and writes `manifest.json` last: a
//! directory without a manifest is an interrupted run.
//!
//! The output is a function of the theatre alone, byte for byte. Even
//! `extracted_at` is fixed, so two runs can be compared with `diff`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::*;

const EXTRACTED_AT: &str = "2000-01-01T00:00:00Z";

/// The tile layers in manifest order, with dtype, nodata, unit and the file
/// a decoder reads them from.
const LAYERS: [(&str, &str, i64, &str, &str); 5] = [
    ("height", "i16", HEIGHT_NODATA as i64, "m", "surface5"),
    ("water", "u8", WATER_NODATA as i64, "class", "surface5"),
    ("surface", "u8", SURFACE_NODATA as i64, "enum", "surface5"),
    ("trees", "u16", COUNT_NODATA as i64, "count", "scn5"),
    ("lines", "u16", COUNT_NODATA as i64, "count", "scn5"),
];

/// What a write produced, for the caller to report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    pub tiles: usize,
    pub files: usize,
}

/// Writes `theatre` into `dir`, which must be absent or empty.
pub fn write(theatre: &Theatre, dir: &Path) -> io::Result<Written> {
    if dir.exists() && fs::read_dir(dir)?.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} is not empty", dir.display()),
        ));
    }
    fs::create_dir_all(dir)?;
    let mut out = Out {
        dir: dir.to_path_buf(),
        files: 0,
    };

    let tiles = write_tiles(theatre, &mut out)?;
    let tables = [
        ("config", "config.json", config(theatre)),
        ("airdromes", "airdromes.json", airdromes(theatre)),
        ("runways", "runways.json", runways(theatre)),
        ("stands", "stands.json", stands(theatre)),
        ("beacons", "beacons.json", beacons(theatre)),
        ("radio", "radio.json", radio()),
        ("towns", "towns.json", towns(theatre)),
        ("nodes", "nodes.json", nodes(theatre)),
        (
            "scenery_models",
            "scenery_models.json",
            scenery_models(theatre),
        ),
    ];
    let mut names = serde_json::Map::new();
    for (key, name, doc) in tables {
        out.json(name, &doc)?;
        names.insert(key.into(), name.into());
    }
    let lines = [
        ("roads", "roads.jsonl", edges(&theatre.roads())),
        ("railroads", "railroads.jsonl", edges(&theatre.railroads())),
        ("scenery", "scenery.jsonl", scenery(theatre)),
        ("ground", "ground.jsonl", ground(theatre)),
    ];
    for (key, name, docs) in lines {
        out.jsonl(name, &docs)?;
        names.insert(key.into(), name.into());
    }

    let count = tiles.len();
    out.json(
        "manifest.json",
        &manifest(theatre, tiles, Value::Object(names)),
    )?;
    Ok(Written {
        tiles: count,
        files: out.files,
    })
}

struct Out {
    dir: PathBuf,
    files: usize,
}

impl Out {
    fn put(&mut self, rel: &str, bytes: &[u8]) -> io::Result<()> {
        let path = self.dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = path.with_file_name(format!(
            "{}.tmp",
            path.file_name().unwrap().to_string_lossy()
        ));
        fs::write(&tmp, bytes)?;
        fs::rename(&tmp, &path)?;
        self.files += 1;
        Ok(())
    }

    fn json(&mut self, rel: &str, doc: &Value) -> io::Result<()> {
        let mut text = serde_json::to_string_pretty(doc).map_err(io::Error::other)?;
        text.push('\n');
        self.put(rel, text.as_bytes())
    }

    fn jsonl(&mut self, rel: &str, docs: &[Value]) -> io::Result<()> {
        let mut text = String::new();
        for d in docs {
            text.push_str(&serde_json::to_string(d).map_err(io::Error::other)?);
            text.push('\n');
        }
        self.put(rel, text.as_bytes())
    }
}

/// Every tile of every layer the omission rules keep, and the manifest's
/// `tiles` entries for them.
fn write_tiles(t: &Theatre, out: &mut Out) -> io::Result<Vec<Value>> {
    let ts = TILE_SIZE;
    let per_side = t.cells().div_ceil(ts);
    let mut entries = Vec::new();
    for tx in 0..per_side {
        for tz in 0..per_side {
            let cells: Vec<Cell> = (0..ts * ts)
                .map(|i| t.cell(tx * ts + i / ts, tz * ts + i % ts))
                .collect();
            if cells.iter().all(|c| c.water == WATER_NODATA) {
                continue;
            }
            let all_sea = cells.iter().all(|c| c.water == WATER_SEA);
            let any = |f: fn(&Cell) -> u16| cells.iter().any(|c| f(c) != 0 && f(c) != COUNT_NODATA);
            let keep = [
                !(all_sea && OMIT_SEA_TILES),
                true,
                !(all_sea && OMIT_SEA_TILES),
                any(|c| c.trees),
                any(|c| c.lines),
            ];
            for (k, (layer, ..)) in LAYERS.iter().enumerate() {
                if !keep[k] {
                    continue;
                }
                let values: Vec<i64> = cells.iter().map(|c| value(c, k)).collect();
                let nodata = LAYERS[k].2;
                let present = values.iter().filter(|&&v| v != nodata);
                let (min, max) = (present.clone().min(), present.max());
                let rel = format!("tiles/{layer}/{tx}_{tz}.bin");
                out.put(&rel, &encode(&values, LAYERS[k].1))?;
                entries.push(json!({
                    "layer": layer, "tx": tx, "tz": tz, "path": rel, "min": min, "max": max,
                }));
            }
        }
    }
    // Manifest order is by layer, then tile, rather than the order written.
    let rank = |e: &Value| LAYERS.iter().position(|l| e["layer"] == l.0).unwrap();
    entries.sort_by_key(|e| (rank(e), e["tx"].as_u64(), e["tz"].as_u64()));
    Ok(entries)
}

fn value(c: &Cell, layer: usize) -> i64 {
    match layer {
        0 => i64::from(c.height),
        1 => i64::from(c.water),
        2 => i64::from(c.surface),
        3 => i64::from(c.trees),
        _ => i64::from(c.lines),
    }
}

/// Little-endian samples, row-major with columns fastest.
fn encode(values: &[i64], dtype: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(values.len() * 2);
    for &v in values {
        match dtype {
            "u8" => bytes.push(v as u8),
            "i16" => bytes.extend_from_slice(&(v as i16).to_le_bytes()),
            _ => bytes.extend_from_slice(&(v as u16).to_le_bytes()),
        }
    }
    bytes
}

fn point(t: &Theatre, north: f64, east: f64) -> Value {
    let (x, z) = t.abs(north, east);
    json!({"x": x, "z": z})
}

/// The runway's two ends, west then east, at its centre line.
fn runway_ends(t: &Theatre) -> [(f64, f64); 2] {
    let half = RUNWAY_LENGTH_M / 2.0;
    [
        t.abs(RUNWAY_NORTH_M, RUNWAY_EAST_M - half),
        t.abs(RUNWAY_NORTH_M, RUNWAY_EAST_M + half),
    ]
}

fn latlon(x: f64, z: f64) -> (f64, f64) {
    tmerc::to_latlon(x, z)
}

fn beacon_id(k: u32) -> String {
    format!("airfield{AIRDROME_ID}_{k}")
}

fn config(t: &Theatre) -> Value {
    let size = t.size_m();
    let (min_x, min_z) = t.abs(AUTHORED_INSET_M, AUTHORED_INSET_M);
    let (max_x, max_z) = t.abs(size - AUTHORED_INSET_M, size - AUTHORED_INSET_M);
    let grow = f64::from(BOUNDS_MARGIN_KM) * 1000.0;
    let km = |v: f64| v / 1000.0;
    let (cx, cz) = t.abs(size / 2.0, size / 2.0);
    let samples: Vec<Value> = runway_ends(t)
        .iter()
        .map(|&(x, z)| {
            let (lat, lon) = latlon(x, z);
            json!({"x": x, "z": z, "lat": lat, "lon": lon})
        })
        .collect();
    json!({
        "id": THEATRE,
        "crs": {
            "proj4": CRS_PROJ4,
            "lon_0": CRS_LON_0_DEG,
            "easting": -CRS_EASTING_OFFSET_M,
            "northing": -CRS_NORTHING_OFFSET_M,
            "k_0": CRS_K_0,
            "n": null, "rms_m": null, "max_m": null,
            "source": CRS_SOURCE,
        },
        "latlon_samples": samples,
        "bounds_km": {
            "sw": [km(min_x - grow), km(min_z - grow)],
            "ne": [km(max_x + grow), km(max_z + grow)],
        },
        "default_bullseye": {
            "blue": point(t, BULLSEYE_BLUE_NORTH_M, BULLSEYE_BLUE_EAST_M),
            "red": point(t, BULLSEYE_RED_NORTH_M, BULLSEYE_RED_EAST_M),
        },
        "sea_enabled": SEA_ENABLED,
        "default_camera_km": [km(cx), CAMERA_ALT_KM, km(cz)],
        "summer_time_delta": SUMMER_TIME_DELTA,
        "shape": SHAPE,
    })
}

fn airdromes(t: &Theatre) -> Value {
    let (x, z) = t.abs(RUNWAY_NORTH_M, RUNWAY_EAST_M);
    let (lat, lon) = latlon(x, z);
    json!([{
        "id": AIRDROME_ID,
        "name_id": AIRDROME_NAME_ID,
        "code": AIRDROME_CODE,
        "display_name": AIRDROME_DISPLAY_NAME,
        "names": {"en": AIRDROME_DISPLAY_NAME},
        "x": x, "z": z, "lat": lat, "lon": lon,
        "civilian": false,
        "abandoned": false,
        "class": "airfield",
        "runway_names": [runway_name()],
        "beacon_ids": (0..BEACON_COUNT).map(beacon_id).collect::<Vec<_>>(),
        "radio_ids": (0..RADIO_COUNT).map(beacon_id).collect::<Vec<_>>(),
        "roadnet": FILE_AIRFIELD_RN4_PATH,
        "roadnet5": FILE_AIRFIELD_RN5_PATH,
        "towers": [], "warehouses": [], "fueldepots": [], "shelters": [],
    }])
}

fn runway_name() -> String {
    format!("{RUNWAY_EDGE1_NAME}-{RUNWAY_EDGE2_NAME}")
}

fn runways(t: &Theatre) -> Value {
    let [(x1, z1), (x2, z2)] = runway_ends(t);
    json!([{
        "airdrome_id": AIRDROME_ID,
        "airdrome_name_id": AIRDROME_NAME_ID,
        "name": runway_name(),
        "edge1_name": RUNWAY_EDGE1_NAME, "edge1_x": x1, "edge1_z": z1,
        "edge2_name": RUNWAY_EDGE2_NAME, "edge2_x": x2, "edge2_z": z2,
        "course": RUNWAY_COURSE_RAD,
    }])
}

fn stands(t: &Theatre) -> Value {
    let west = RUNWAY_EAST_M - RUNWAY_LENGTH_M / 2.0;
    let list: Vec<Value> = (0..STAND_COUNT)
        .map(|i| {
            let (x, z) = t.abs(
                RUNWAY_NORTH_M + STAND_NORTH_OFFSET_M,
                west + f64::from(i) * STAND_SPACING_M,
            );
            json!({
                "airdrome_id": AIRDROME_ID,
                "airdrome_name_id": AIRDROME_NAME_ID,
                "crossroad_index": i,
                "name": format!("{}", i + 1),
                "flag": 32,
                "x": x, "z": z,
                "stand_class": STAND_CLASS,
                "heading": STAND_HEADING_RAD,
                "params": {
                    "SHELTER": null,
                    "FOR_HELICOPTERS": false,
                    "FOR_AIRPLANES": true,
                    "WIDTH": STAND_WIDTH_M,
                    "LENGTH": STAND_LENGTH_M,
                    "HEIGHT": "",
                },
            })
        })
        .collect();
    Value::Array(list)
}

/// One beacon on each runway end; only the first carries a channel.
fn beacons(t: &Theatre) -> Value {
    let freq = [BEACON_1_FREQUENCY_HZ, BEACON_2_FREQUENCY_HZ];
    let list: Vec<Value> = runway_ends(t)
        .iter()
        .zip(0u32..)
        .map(|(&(x, z), k)| {
            let (lat, lon) = latlon(x, z);
            let alt = t.ground(x - f64::from(ORIGIN_X_M), z - f64::from(ORIGIN_Z_M));
            json!({
                "beacon_id": beacon_id(k),
                "callsign": format!("SY{}", k + 1),
                "display_name": AIRDROME_DISPLAY_NAME,
                "type": k + 1,
                "frequency_hz": freq[k as usize],
                "channel": if k == 0 { json!(BEACON_1_CHANNEL) } else { Value::Null },
                "direction": RUNWAY_COURSE_RAD.to_degrees(),
                "x": x, "alt": alt, "z": z, "lat": lat, "lon": lon,
                "scene_objects": [],
            })
        })
        .collect();
    Value::Array(list)
}

fn radio() -> Value {
    json!([{
        "radio_id": beacon_id(0),
        "callsigns": {"en": AIRDROME_DISPLAY_NAME},
        "roles": ["ground", "tower", "approach"],
        "frequencies_hz": {
            "hf": RADIO_HF_HZ, "fm": RADIO_FM_HZ, "vhf": RADIO_VHF_HZ, "uhf": RADIO_UHF_HZ,
        },
        "scene_objects": [],
    }])
}

fn towns(t: &Theatre) -> Value {
    let list: Vec<Value> = (0..TOWNS_COUNT)
        .map(|k| {
            let north = TOWNS_FIRST_NORTH_M + f64::from(k) * TOWNS_SPACING_M;
            // A map lists its towns by latitude and longitude, and the
            // extractor projects them; so do this, from the position the
            // constants give, and the town lands within a millimetre of it.
            let (x0, z0) = t.abs(north, TOWNS_FIRST_EAST_M);
            let (lat, lon) = latlon(x0, z0);
            let (x, z) = tmerc::to_xz(lat, lon);
            json!({
                "name": format!("TOWN{}", k + 1),
                "display_name": format!("Town {}", k + 1),
                "lat": lat, "lon": lon, "x": x, "z": z,
            })
        })
        .collect();
    Value::Array(list)
}

fn nodes(t: &Theatre) -> Value {
    let list: Vec<Value> = (0..NODES_COUNT)
        .map(|k| {
            let north = NODES_FIRST_NORTH_M + f64::from(k) * NODES_SPACING_M;
            let east = NODES_FIRST_EAST_M;
            json!({
                "id": k + 1,
                "name": format!("Node {}", k + 1),
                "red": point(t, north, east - NODES_SIDE_OFFSET_M),
                "blue": point(t, north, east + NODES_SIDE_OFFSET_M),
            })
        })
        .collect();
    Value::Array(list)
}

fn edges(list: &[Edge]) -> Vec<Value> {
    list.iter()
        .map(|e| json!({"id": e.id, "class": e.class, "a": e.a, "b": e.b, "points": e.points}))
        .collect()
}

fn scenery(t: &Theatre) -> Vec<Value> {
    t.scenery()
        .iter()
        .map(|o| {
            json!({
                "id": o.id, "model": o.model.to_lowercase(),
                "x": o.x, "alt": o.alt, "z": o.z, "rotation": o.rotation,
            })
        })
        .collect()
}

/// One row per model any pool places, sorted by name.
fn scenery_models(t: &Theatre) -> Value {
    let objects = t.scenery();
    let placed = |model: &str| objects.iter().filter(|o| o.model == model).count();
    let models = [
        (
            SCENERY_TOWN_MODEL,
            SCENERY_TOWN_OBB_W_M,
            SCENERY_TOWN_OBB_D_M,
            SCENERY_TOWN_HEIGHT_M,
        ),
        (
            SCENERY_INDUSTRIAL_MODEL_A,
            SCENERY_INDUSTRIAL_A_OBB_W_M,
            SCENERY_INDUSTRIAL_A_OBB_D_M,
            SCENERY_INDUSTRIAL_A_HEIGHT_M,
        ),
        (
            SCENERY_INDUSTRIAL_MODEL_B,
            SCENERY_INDUSTRIAL_B_OBB_W_M,
            SCENERY_INDUSTRIAL_B_OBB_D_M,
            SCENERY_INDUSTRIAL_B_HEIGHT_M,
        ),
        (
            SCENERY_POLE_MODEL,
            SCENERY_POLE_OBB_W_M,
            SCENERY_POLE_OBB_D_M,
            SCENERY_POLE_HEIGHT_M,
        ),
        (SCENERY_NOBOX_MODEL, 0.0, 0.0, 0.0),
        (TREE_MODEL, TREE_OBB_W_M, TREE_OBB_D_M, TREE_HEIGHT_M),
        (LINE_MODEL, LINE_OBB_W_M, LINE_OBB_D_M, LINE_HEIGHT_M),
    ];
    let mut rows: Vec<Value> = models
        .iter()
        .map(|&(model, w, d, h)| {
            let trees = if model == TREE_MODEL {
                t.tree_count()
            } else {
                0
            };
            let lines = if model == LINE_MODEL {
                t.pole_count()
            } else {
                0
            };
            json!({
                "model": model.to_lowercase(),
                "counts": {"objects": placed(model), "lines": lines, "trees": trees},
                "box": {
                    "min_x": -w / 2.0, "min_y": 0.0, "min_z": -d / 2.0,
                    "max_x": w / 2.0, "max_y": h, "max_z": d / 2.0,
                },
            })
        })
        .collect();
    rows.sort_by(|a, b| a["model"].as_str().cmp(&b["model"].as_str()));
    Value::Array(rows)
}

/// An axis-aligned ring, positive signed area when `outer`.
fn ring(t: &Theatre, south: f64, north: f64, west: f64, east: f64, outer: bool) -> Vec<[f64; 2]> {
    let corners = [(south, west), (north, west), (north, east), (south, east)];
    let mut pts: Vec<[f64; 2]> = corners
        .iter()
        .map(|&(n, e)| {
            let (x, z) = t.abs(n, e);
            [x, z]
        })
        .collect();
    if !outer {
        pts.reverse();
    }
    pts
}

fn ground(t: &Theatre) -> Vec<Value> {
    let forest = [
        ring(
            t,
            FOREST_SOUTH_M,
            FOREST_NORTH_M,
            FOREST_WEST_M,
            FOREST_EAST_M,
            true,
        ),
        ring(
            t,
            FOREST_HOLE_SOUTH_M,
            FOREST_HOLE_NORTH_M,
            FOREST_HOLE_WEST_M,
            FOREST_HOLE_EAST_M,
            false,
        ),
    ];
    let built_up = [ring(
        t,
        BUILT_UP_SOUTH_M,
        BUILT_UP_NORTH_M,
        BUILT_UP_WEST_M,
        BUILT_UP_EAST_M,
        true,
    )];
    vec![
        json!({"class": "forest", "rings": forest}),
        json!({"class": "built_up", "rings": built_up}),
    ]
}

fn files() -> Value {
    let header =
        |role: &str, path: &str, class: &str, version: u32, size: u64, payload: Option<u64>| {
            json!({
                "role": role, "path": path, "class": class, "version": version,
                "size": size, "payload_size": payload,
            })
        };
    let plain = |role: &str, path: &str, size: u64| {
        json!({
            "role": role, "path": path, "class": null, "version": null,
            "size": size, "payload_size": null,
        })
    };
    json!([
        header(
            "surface",
            FILE_SURFACE5_PATH,
            FILE_SURFACE5_CLASS,
            FILE_SURFACE5_VERSION,
            FILE_SURFACE5_SIZE,
            Some(FILE_SURFACE5_PAYLOAD_SIZE)
        ),
        header(
            "roads",
            FILE_RN4_PATH,
            FILE_RN4_CLASS,
            FILE_RN4_VERSION,
            FILE_RN4_SIZE,
            None
        ),
        header(
            "scene",
            FILE_SCN5_PATH,
            FILE_SCN5_CLASS,
            FILE_SCN5_VERSION,
            FILE_SCN5_SIZE,
            Some(FILE_SCN5_PAYLOAD_SIZE)
        ),
        header(
            "airfield_rn4",
            FILE_AIRFIELD_RN4_PATH,
            FILE_RN4_CLASS,
            FILE_RN4_VERSION,
            FILE_AIRFIELD_RN4_SIZE,
            None
        ),
        header(
            "airfield_rn5",
            FILE_AIRFIELD_RN5_PATH,
            FILE_AIRFIELD_RN5_CLASS,
            FILE_AIRFIELD_RN5_VERSION,
            FILE_AIRFIELD_RN5_SIZE,
            None
        ),
        plain("assets", FILE_ASSETS_PATH, FILE_ASSETS_SIZE),
        plain("lua", FILE_BEACONS_PATH, FILE_BEACONS_SIZE),
        plain("lua", FILE_RADIO_PATH, FILE_RADIO_SIZE),
        plain("lua", FILE_TOWNS_PATH, FILE_TOWNS_SIZE),
        plain("lua", FILE_NODES_PATH, FILE_NODES_SIZE),
    ])
}

fn manifest(t: &Theatre, tiles: Vec<Value>, tables: Value) -> Value {
    let size = t.size_m();
    let (min_x, min_z) = t.abs(AUTHORED_INSET_M, AUTHORED_INSET_M);
    let (max_x, max_z) = t.abs(size - AUTHORED_INSET_M, size - AUTHORED_INSET_M);
    let layers: serde_json::Map<String, Value> = LAYERS
        .iter()
        .map(|&(name, dtype, nodata, unit, source)| {
            let spec = json!({"dtype": dtype, "nodata": nodata, "unit": unit, "source": source});
            (name.to_string(), spec)
        })
        .collect();
    json!({
        "format_version": FORMAT_VERSION,
        "extractor_version": EXTRACTOR_VERSION,
        "theatre": THEATRE,
        "dcs_build": DCS_BUILD,
        "dcs_build_timestamp": DCS_BUILD_TIMESTAMP,
        "files": files(),
        "extracted_at": EXTRACTED_AT,
        "authored_bounds_m": {"min_x": min_x, "min_z": min_z, "max_x": max_x, "max_z": max_z},
        "authored_bounds_source": "surface5",
        "grid": {
            "cell_size": t.cell_size,
            "origin_x": ORIGIN_X_M,
            "origin_z": ORIGIN_Z_M,
            "height": t.cells(),
            "width": t.cells(),
            "tile_size": TILE_SIZE,
        },
        "omit_sea_tiles": OMIT_SEA_TILES,
        "layers": layers,
        "tiles": tiles,
        "tables": tables,
        "hook": {
            "dcs_build": DCS_BUILD,
            "dcs_build_timestamp": DCS_BUILD_TIMESTAMP,
            "extracted_at": EXTRACTED_AT,
        },
        "timing_ms": {},
        "notes": [],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A fresh directory under the system temp, removed when dropped.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            static N: AtomicU32 = AtomicU32::new(0);
            let name = format!(
                "dcsterrain-synth-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            );
            Scratch(std::env::temp_dir().join(name))
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn theatre() -> Theatre {
        Theatre::new(DEFAULT_SEED, DEFAULT_SIZE_KM, CELL_SIZE_M).unwrap()
    }

    fn read_json(dir: &Path, name: &str) -> Value {
        serde_json::from_str(&fs::read_to_string(dir.join(name)).unwrap()).unwrap()
    }

    #[test]
    fn writes_a_complete_v2_directory() {
        let s = Scratch::new();
        write(&theatre(), &s.0).unwrap();
        let m = read_json(&s.0, "manifest.json");
        assert_eq!(m["format_version"], 2);
        assert_eq!(m["grid"]["height"], 1400);

        // Every table the manifest names exists, and nothing is left as .tmp.
        for (_, name) in m["tables"].as_object().unwrap() {
            assert!(s.0.join(name.as_str().unwrap()).is_file(), "{name}");
        }
        let mut stray = Vec::new();
        let mut stack = vec![s.0.clone()];
        while let Some(d) = stack.pop() {
            for e in fs::read_dir(d).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|x| x == "tmp") {
                    stray.push(p);
                }
            }
        }
        assert!(stray.is_empty(), "{stray:?}");

        // Each listed tile is exactly a tile of its dtype.
        for tile in m["tiles"].as_array().unwrap() {
            let len = fs::metadata(s.0.join(tile["path"].as_str().unwrap()))
                .unwrap()
                .len();
            let bytes = if tile["layer"] == "water" || tile["layer"] == "surface" {
                1
            } else {
                2
            };
            assert_eq!(len, 256 * 256 * bytes, "{tile}");
        }
    }

    #[test]
    fn omits_sea_height_and_surface_and_empty_counts() {
        let s = Scratch::new();
        write(&theatre(), &s.0).unwrap();
        let tiles = read_json(&s.0, "manifest.json")["tiles"].clone();
        let has = |layer: &str, tx: u32, tz: u32| {
            tiles
                .as_array()
                .unwrap()
                .iter()
                .any(|t| t["layer"] == layer && t["tx"] == tx && t["tz"] == tz)
        };
        // Tile (1, 1) spans 12.8 to 25.6 km on both axes, all of it below
        // the coast line north + east = 52 km.
        assert!(has("water", 1, 1) && !has("height", 1, 1) && !has("surface", 1, 1));
        assert!(
            has("height", 0, 0),
            "an edge tile holds margin, so it is not all sea"
        );
        // The forest is in tile (3, 4) and the power line in column 3.
        assert!(has("trees", 3, 4) && !has("trees", 0, 0));
        assert!(has("lines", 1, 3) && !has("lines", 1, 2));
        let trees: Vec<_> = tiles
            .as_array()
            .unwrap()
            .iter()
            .filter(|t| t["layer"] == "trees")
            .collect();
        assert_eq!(trees.len(), 1);
        assert_eq!(
            (trees[0]["min"].as_i64(), trees[0]["max"].as_i64()),
            (Some(0), Some(3))
        );
    }

    #[test]
    fn a_tile_reads_back_as_the_closed_form() {
        let s = Scratch::new();
        let t = theatre();
        write(&t, &s.0).unwrap();
        let bytes = fs::read(s.0.join("tiles/surface/4_0.bin")).unwrap();
        // Row 1100 of the grid is the east-west road; tile row 4 starts at
        // grid row 1024.
        for (row, col) in [(1100, 120), (1101, 120), (1150, 30), (1200, 2)] {
            let i = ((row - 1024) * 256 + col) as usize;
            assert_eq!(bytes[i], t.cell(row, col).surface, "({row}, {col})");
        }
        let heights = fs::read(s.0.join("tiles/height/1_5.bin")).unwrap();
        let (row, col) = (300u32, 1300u32);
        let i = (((row - 256) * 256 + (col - 1280)) * 2) as usize;
        let h = i16::from_le_bytes([heights[i], heights[i + 1]]);
        assert_eq!(h, t.cell(row, col).height);
    }

    #[test]
    fn rings_turn_the_stated_way() {
        let area = |ring: &Value| {
            let p = ring.as_array().unwrap();
            (0..p.len())
                .map(|i| {
                    let (a, b) = (&p[i], &p[(i + 1) % p.len()]);
                    a[0].as_f64().unwrap() * b[1].as_f64().unwrap()
                        - b[0].as_f64().unwrap() * a[1].as_f64().unwrap()
                })
                .sum::<f64>()
        };
        let forest = &ground(&theatre())[0];
        assert!(area(&forest["rings"][0]) > 0.0);
        assert!(area(&forest["rings"][1]) < 0.0);
    }

    #[test]
    fn the_models_cover_every_pool() {
        let rows = scenery_models(&theatre());
        let by = |m: &str| {
            rows.as_array()
                .unwrap()
                .iter()
                .find(|r| r["model"] == m)
                .unwrap()
                .clone()
        };
        assert_eq!(by("pine_1")["counts"]["trees"], theatre().tree_count());
        assert_eq!(by("pole_power")["counts"]["lines"], 201);
        assert_eq!(by("synth_nobox")["box"]["max_x"], 0.0);
        assert_eq!(
            by("sklad_new")["box"]["max_x"],
            SCENERY_INDUSTRIAL_A_OBB_W_M / 2.0
        );
        let objects: u64 = rows
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["counts"]["objects"].as_u64().unwrap())
            .sum();
        assert_eq!(objects, u64::from(SCENERY_COUNT));
    }

    #[test]
    fn samples_project_onto_their_own_position() {
        let samples = config(&theatre())["latlon_samples"].clone();
        assert_eq!(samples.as_array().unwrap().len(), BEACON_COUNT as usize);
        for s in samples.as_array().unwrap() {
            let (x, z) = tmerc::to_xz(s["lat"].as_f64().unwrap(), s["lon"].as_f64().unwrap());
            assert!((x - s["x"].as_f64().unwrap()).abs() < 1e-3);
            assert!((z - s["z"].as_f64().unwrap()).abs() < 1e-3);
        }
    }

    #[test]
    fn two_runs_write_the_same_bytes() {
        let (a, b) = (Scratch::new(), Scratch::new());
        write(&theatre(), &a.0).unwrap();
        write(&theatre(), &b.0).unwrap();
        for name in ["manifest.json", "scenery.jsonl", "tiles/height/2_2.bin"] {
            assert_eq!(
                fs::read(a.0.join(name)).unwrap(),
                fs::read(b.0.join(name)).unwrap(),
                "{name}"
            );
        }
    }

    #[test]
    fn refuses_a_directory_that_is_not_empty() {
        let s = Scratch::new();
        fs::create_dir_all(&s.0).unwrap();
        fs::write(s.0.join("keep.txt"), "mine").unwrap();
        let e = write(&theatre(), &s.0).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read_to_string(s.0.join("keep.txt")).unwrap(), "mine");
    }
}
