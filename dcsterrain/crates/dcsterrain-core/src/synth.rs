//! Reference constants for the synthetic theatre.
//!
//! Its twin is `extractor/test/support/synth_constants.lua`, which holds the
//! same names with the same values. The test at the foot of this file parses
//! that one and fails when the two sets differ, so neither can be edited
//! alone.
//!
//! The synthetic theatre is the thing almost every test is stated against: a
//! closed-form terrain with no DCS in the loop. This file holds only the
//! numbers that define it, never the functions that evaluate them, because two
//! languages have to agree on the numbers and only one of them generates the
//! extract. The generator lands here later.
//!
//! Positions are offsets from the grid origin, in metres: `_NORTH_M` grows
//! with DCS x, `_EAST_M` grows with DCS z. Nothing here is absolute, so
//! raising `DEFAULT_SIZE_KM` adds land to the north-east and moves no feature.
//!
//! Format, so the cross-language test can parse a line:
//!
//! * one `pub const NAME: T = <literal>;` per line, nothing else on it
//! * no trailing comments
//! * primitives only; anything derivable (grid extents, feature positions in
//!   absolute metres, tile counts) is computed by the generator

// Extract format. The layer table and the water codes are ADR-0007's.

pub const FORMAT_VERSION: u32 = 1;
pub const TILE_SIZE: u32 = 256;
pub const CELL_SIZE_M: u32 = 50;

pub const HEIGHT_NODATA: i16 = -32768;
pub const HEIGHT_MIN: i16 = -32767;
pub const HEIGHT_MAX: i16 = 32767;

pub const WATER_LAND: u8 = 0;
pub const WATER_LAKE: u8 = 1;
pub const WATER_SEA: u8 = 2;
pub const WATER_RIVER: u8 = 3;
pub const WATER_UNRECOGNISED: u8 = 254;
pub const WATER_NODATA: u8 = 255;

pub const SURFACE_NODATA: u8 = 0;
pub const SURFACE_LAND: u8 = 1;
pub const SURFACE_SHALLOW_WATER: u8 = 2;
pub const SURFACE_WATER: u8 = 3;
pub const SURFACE_ROAD: u8 = 4;
pub const SURFACE_RUNWAY: u8 = 5;

// The fill triple, which is Caucasus's: what the engine returns outside the
// terrain. A cell matching all three unrounded is written nodata.
//
// The height is worth staring at. It rounds to 5, and so does real 5 m land,
// which is why the test is on the unrounded returns. The synthetic theatre
// contains that collision on purpose: its plane passes through exactly 5 m,
// and those cells are valid land carrying the same encoded height as fill.

pub const FILL_HEIGHT_M: f64 = 5.000005;
pub const FILL_WATER: u8 = 0;
pub const FILL_SEABED_M: f64 = 0.0;

// Theatre identity, as the manifest records it. The three head hashes are the
// SHA-256 of the strings "dcsterrain synth surface5", "dcsterrain synth rn4"
// and "dcsterrain synth scn5"; the digest is the first 8 hex characters of the
// SHA-256 over those three lowercase hex strings concatenated in that order.
// They are distinct, so the digest fails if the three are joined in any other
// order.

pub const THEATRE: &str = "Synth";
pub const EXTRACTOR_VERSION: &str = "0.1.0";
pub const DCS_BUILD: &str = "0.0.0.0";
pub const DCS_BUILD_TIMESTAMP: &str = "00000000-000000";

pub const FINGERPRINT_SURFACE5_PATH: &str = "Mods/terrains/Synth/Surface/Synth.surface5";
pub const FINGERPRINT_SURFACE5_SIZE: u64 = 4194304;
pub const FINGERPRINT_SURFACE5_PAYLOAD_SIZE: u64 = 1048576;
pub const FINGERPRINT_SURFACE5_SHA256: &str =
    "06869a4383338c0a767072ac14fc0f939d238e0101e3e4bb732182fe3c411e60";
pub const FINGERPRINT_RN4_PATH: &str = "Mods/terrains/Synth/roads/Synth.rn4";
pub const FINGERPRINT_RN4_SIZE: u64 = 2097152;
pub const FINGERPRINT_RN4_PAYLOAD_SIZE: u64 = 2097152;
pub const FINGERPRINT_RN4_SHA256: &str =
    "261d9210306d14e1734b4503ecb3ce7de0c2d452145f154c34a6f4fe7f527a9b";
pub const FINGERPRINT_SCN5_PATH: &str = "Mods/terrains/Synth/Scenes/Synth.scn5";
pub const FINGERPRINT_SCN5_SIZE: u64 = 3145728;
pub const FINGERPRINT_SCN5_PAYLOAD_SIZE: u64 = 524288;
pub const FINGERPRINT_SCN5_SHA256: &str =
    "12713132dfe4ac49fd903889d6ebedd840b778bb66d6404e99cbc2a428c0c3c7";
pub const FINGERPRINT_DIGEST: &str = "90c9cec8";

// Grid. The authored rectangle is inset a quarter cell from the grid on every
// side, so snapping it outward to a multiple of CELL_SIZE_M reproduces the
// grid exactly, and an extractor that snaps the wrong way is off by a cell in
// every direction. The declared bounds rectangle is the authored one grown by
// BOUNDS_MARGIN_KM, which is the shape every real theatre has: bounds larger
// than the terrain, with fill in between.
//
// DEFAULT_SIZE_KM is 70 for one reason. The fill margin sits inside the grid
// edge, so every tile of tile row or column 0 carries fill and can never be
// all sea; an all-sea tile has to be an interior one, and the interior tile
// nearest the corner reaches 51 200 m out along both axes. A smaller theatre
// either has no all-sea tile at all, and so never exercises omit_sea_tiles end
// to end, or is mostly water. There is no all-fill tile at any size: the
// margin is 2 km and a tile is 12.8 km, so the absent-fill-tile rule is
// unit-tested against a hand-built manifest instead.
//
// Every feature below lies inside a 70 km grid. Raising the size is safe;
// lowering it drops features off the north-east.

pub const DEFAULT_SEED: u32 = 1;
pub const DEFAULT_SIZE_KM: u32 = 70;
pub const ORIGIN_X_M: i32 = -30000;
pub const ORIGIN_Z_M: i32 = -45000;
pub const AUTHORED_INSET_M: f64 = 12.5;
pub const BOUNDS_MARGIN_KM: u32 = 10;
pub const FILL_MARGIN_M: f64 = 2000.0;
pub const OMIT_SEA_TILES: bool = true;

// Terrain. Height is the plane plus both hills, overridden by the lake level
// inside the lake and by sea level beyond the coast. The coastline is the line
// north + east = COAST_M, so the sea is a triangle in the south-west corner.

pub const PLANE_BASE_M: f64 = -30.0;
pub const PLANE_SLOPE: f64 = 0.01;

pub const HILL_A_HEIGHT_M: f64 = 600.0;
pub const HILL_A_NORTH_M: f64 = 46000.0;
pub const HILL_A_EAST_M: f64 = 30000.0;
pub const HILL_B_HEIGHT_M: f64 = 250.0;
pub const HILL_B_NORTH_M: f64 = 46000.0;
pub const HILL_B_EAST_M: f64 = 35000.0;
pub const HILL_SIGMA_M: f64 = 1500.0;

pub const LAKE_NORTH_M: f64 = 13000.0;
pub const LAKE_EAST_M: f64 = 52000.0;
pub const LAKE_RADIUS_M: f64 = 2000.0;
pub const LAKE_LEVEL_M: f64 = 100.0;

pub const RIVER_EAST_M: f64 = 45000.0;
pub const RIVER_WIDTH_M: f64 = 60.0;

pub const COAST_M: f64 = 52000.0;
pub const SEA_LEVEL_M: f64 = 0.0;

// Roads, railway and runway. The two roads cross at
// (ROAD_EW_NORTH_M, ROAD_NS_EAST_M); only the east-west one meets the river,
// on the bridge, so the bridge is the single cut edge of the road graph and
// carries maximal betweenness. The north-south road runs over the col between
// the two hills. The railway is not in the road graph and crosses the river
// without a bridge.

pub const ROAD_EW_NORTH_M: f64 = 55000.0;
pub const ROAD_NS_EAST_M: f64 = 32500.0;
pub const ROAD_WIDTH_M: f64 = 10.0;
pub const RAIL_SOUTH_OFFSET_M: f64 = 3000.0;
pub const BRIDGE_LENGTH_M: f64 = 200.0;
pub const ROAD_SEED_SPACING_M: f64 = 1000.0;
pub const ROAD_SEED_NEIGHBOURS: u32 = 4;

pub const RUNWAY_NORTH_M: f64 = 60000.0;
pub const RUNWAY_EAST_M: f64 = 20000.0;
pub const RUNWAY_LENGTH_M: f64 = 2500.0;
pub const RUNWAY_WIDTH_M: f64 = 50.0;
// Spelled out rather than taken from `std::f64::consts`, because the Lua twin
// has to carry the same digits and the cross-language test compares literals.
#[allow(clippy::approx_constant)]
pub const RUNWAY_COURSE_RAD: f64 = 1.5707963267948966;

// The only randomness in the theatre: where the town's buildings sit inside
// their spread. Terrain, roads and tables are closed-form and ignore the seed.
// The generator is Park and Miller's minimal standard, whose products stay
// under 2^53 and so are exact in a Lua 5.1 double.

pub const PRNG_MULTIPLIER: u32 = 16807;
pub const PRNG_MODULUS: u32 = 2147483647;

// Scenery: SCENERY_TOWN_COUNT houses, an industrial pair, and a line of poles
// beside the east-west road, SCENERY_COUNT objects in all. The footprints are
// chosen to land on three different classification rules: the poles on the
// area-under-4-square-metre rule, the warehouse on the industrial name regex,
// the houses on nothing, which is `building`.

pub const SCENERY_COUNT: u32 = 40;
pub const SCENERY_FIRST_ID: u64 = 70000001;

pub const SCENERY_TOWN_COUNT: u32 = 24;
pub const SCENERY_TOWN_NORTH_M: f64 = 56000.0;
pub const SCENERY_TOWN_EAST_M: f64 = 30000.0;
pub const SCENERY_TOWN_SPREAD_M: f64 = 800.0;
pub const SCENERY_TOWN_MODEL: &str = "HOME1UG_A";
pub const SCENERY_TOWN_OBB_W_M: f64 = 24.0;
pub const SCENERY_TOWN_OBB_D_M: f64 = 12.0;
pub const SCENERY_TOWN_MODEL_RADIUS_M: f64 = 15.0;

pub const SCENERY_INDUSTRIAL_COUNT: u32 = 2;
pub const SCENERY_INDUSTRIAL_NORTH_M: f64 = 54000.0;
pub const SCENERY_INDUSTRIAL_EAST_M: f64 = 40000.0;
pub const SCENERY_INDUSTRIAL_SPACING_M: f64 = 120.0;
pub const SCENERY_INDUSTRIAL_MODEL_A: &str = "SKLAD_NEW";
pub const SCENERY_INDUSTRIAL_A_OBB_W_M: f64 = 41.4;
pub const SCENERY_INDUSTRIAL_A_OBB_D_M: f64 = 43.3;
pub const SCENERY_INDUSTRIAL_A_RADIUS_M: f64 = 30.1;
pub const SCENERY_INDUSTRIAL_MODEL_B: &str = "HIM_BAK_A_NEW";
pub const SCENERY_INDUSTRIAL_B_OBB_W_M: f64 = 20.0;
pub const SCENERY_INDUSTRIAL_B_OBB_D_M: f64 = 20.0;
pub const SCENERY_INDUSTRIAL_B_RADIUS_M: f64 = 14.0;

pub const SCENERY_POLE_COUNT: u32 = 14;
pub const SCENERY_POLE_MODEL: &str = "BLK_LIGHT_POLE";
pub const SCENERY_POLE_FIRST_EAST_M: f64 = 30000.0;
pub const SCENERY_POLE_SPACING_M: f64 = 1000.0;
pub const SCENERY_POLE_NORTH_OFFSET_M: f64 = 25.0;
pub const SCENERY_POLE_OBB_W_M: f64 = 0.4;
pub const SCENERY_POLE_OBB_D_M: f64 = 0.4;
pub const SCENERY_POLE_RADIUS_M: f64 = 0.3;

// Vector tables. One airfield with one runway, so runways.json joins to
// airdromes.json on a single row; stands run north of the runway from its west
// end; the two beacons sit on the runway edges and differ in whether they
// carry a channel, which is the null case beacons.json has to encode. The
// radio frequencies are Anapa's, so the four-way hf/fm/vhf/uhf mapping has a
// real example behind it. Towns and nodes are lines, spaced far enough apart
// that a nearest-neighbour query has an unambiguous answer.

pub const AIRDROME_ID: u32 = 1;
pub const AIRDROME_NAME_ID: &str = "SYNTH";
pub const AIRDROME_CODE: &str = "XSYN";
pub const AIRDROME_DISPLAY_NAME: &str = "Synth Field";
pub const RUNWAY_EDGE1_NAME: &str = "09";
pub const RUNWAY_EDGE2_NAME: &str = "27";

pub const STAND_COUNT: u32 = 4;
pub const STAND_NORTH_OFFSET_M: f64 = 300.0;
pub const STAND_SPACING_M: f64 = 100.0;

pub const BEACON_COUNT: u32 = 2;
pub const BEACON_1_FREQUENCY_HZ: u64 = 750000;
pub const BEACON_1_CHANNEL: u32 = 10;
pub const BEACON_2_FREQUENCY_HZ: u64 = 108100000;

pub const RADIO_COUNT: u32 = 1;
pub const RADIO_HF_HZ: u64 = 3750000;
pub const RADIO_FM_HZ: u64 = 38400000;
pub const RADIO_VHF_HZ: u64 = 121000000;
pub const RADIO_UHF_HZ: u64 = 250000000;

pub const TOWNS_COUNT: u32 = 3;
pub const TOWNS_FIRST_NORTH_M: f64 = 30000.0;
pub const TOWNS_FIRST_EAST_M: f64 = 60000.0;
pub const TOWNS_SPACING_M: f64 = 12000.0;

pub const NODES_COUNT: u32 = 2;
pub const NODES_FIRST_NORTH_M: f64 = 20000.0;
pub const NODES_FIRST_EAST_M: f64 = 40000.0;
pub const NODES_SPACING_M: f64 = 15000.0;
pub const NODES_SIDE_OFFSET_M: f64 = 2000.0;

// Projection. Caucasus's parameters, because the fit must reproduce the published
// Caucasus row within a metre, and the samples it fits are generated from these.

pub const CRS_LON_0_DEG: i32 = 33;
pub const CRS_K_0: f64 = 0.9996;
pub const CRS_EASTING_OFFSET_M: f64 = 99517.0;
pub const CRS_NORTHING_OFFSET_M: f64 = 4998115.0;
pub const CRS_PROJ4: &str =
    "+proj=tmerc +lat_0=0 +lon_0=33 +k_0=0.9996 +x_0=-99517 +y_0=-4998115 +datum=WGS84 +units=m";
pub const LATLON_SAMPLE_ROWS: u32 = 4;
pub const LATLON_SAMPLE_COLS: u32 = 5;

// The rest of config.json. The shape is FLAT because every measured theatre
// returns FLAT, which is what lets line of sight run over the heightfield
// alone.

pub const SEA_ENABLED: bool = true;
pub const SHAPE: &str = "FLAT";
pub const SUMMER_TIME_DELTA: i32 = 4;
pub const BULLSEYE_BLUE_NORTH_M: f64 = 10000.0;
pub const BULLSEYE_BLUE_EAST_M: f64 = 60000.0;
pub const BULLSEYE_RED_NORTH_M: f64 = 60000.0;
pub const BULLSEYE_RED_EAST_M: f64 = 20000.0;
pub const CAMERA_ALT_KM: f64 = 5.0;

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    /// A constant's value, compared across the two languages by what it means
    /// rather than by how it is spelled: `50` and `50.0` are the same number,
    /// and only strings are compared verbatim.
    #[derive(Debug, PartialEq)]
    enum Value {
        Num(f64),
        Str(String),
        Bool(bool),
    }

    /// Parses `<literal>` from either language. Both files are written to a
    /// format that keeps this honest: one constant per line, no trailing
    /// comment, primitives only.
    fn parse_value(raw: &str) -> Value {
        let raw = raw.trim();
        if let Some(inner) = raw.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
            assert!(
                !inner.contains('\\') && !inner.contains('"'),
                "escapes are not handled; keep constant strings plain: {raw}"
            );
            return Value::Str(inner.to_string());
        }
        match raw {
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            _ => Value::Num(
                raw.parse::<f64>()
                    .unwrap_or_else(|_| panic!("not a primitive literal: {raw}")),
            ),
        }
    }

    fn rust_constants() -> BTreeMap<String, Value> {
        // Reading this file's own source keeps the parser and the constants in
        // one place; a constant added below is picked up with no second list
        // to maintain.
        let src = include_str!("synth.rs");
        // A Windows checkout carries CRLF, and the join below matches on the
        // newline, so normalise first. Nothing else here needs it: lines()
        // drops the carriage return by itself.
        let src = src.replace("\r\n", "\n");
        let mut out = BTreeMap::new();
        // `pub const NAME: T = value;` may wrap after the `=` when rustfmt
        // decides the line is too long, so joining continuations comes first.
        let joined = src.replace("=\n", "= ");
        for line in joined.lines() {
            let line = line.trim();
            let Some(rest) = line.strip_prefix("pub const ") else {
                continue;
            };
            let (name, rest) = rest.split_once(':').expect("const without a type");
            let (_, value) = rest.split_once('=').expect("const without a value");
            let value = value.trim().strip_suffix(';').expect("const without a ;");
            out.insert(name.trim().to_string(), parse_value(value));
        }
        out
    }

    fn lua_constants() -> BTreeMap<String, Value> {
        let path: PathBuf = [
            env!("CARGO_MANIFEST_DIR"),
            "..",
            "..",
            "..",
            "extractor",
            "test",
            "support",
            "synth_constants.lua",
        ]
        .iter()
        .collect();
        let src = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        let mut out = BTreeMap::new();
        for line in src.lines() {
            let Some(rest) = line.strip_prefix("M.") else {
                continue;
            };
            let (name, value) = rest.split_once(" = ").expect("assignment without a value");
            out.insert(name.trim().to_string(), parse_value(value));
        }
        out
    }

    #[test]
    fn the_two_constant_files_agree() {
        let rust = rust_constants();
        let lua = lua_constants();

        assert!(rust.len() > 100, "the Rust parser found {}", rust.len());

        let missing_in_lua: Vec<_> = rust.keys().filter(|k| !lua.contains_key(*k)).collect();
        let missing_in_rust: Vec<_> = lua.keys().filter(|k| !rust.contains_key(*k)).collect();
        assert!(
            missing_in_lua.is_empty() && missing_in_rust.is_empty(),
            "absent from the Lua file: {missing_in_lua:?}; absent from this one: {missing_in_rust:?}"
        );

        let differing: Vec<_> = rust
            .iter()
            .filter(|(name, value)| lua[*name] != **value)
            .map(|(name, value)| format!("{name}: rust {value:?}, lua {:?}", lua[name]))
            .collect();
        assert!(differing.is_empty(), "values differ: {differing:?}");
    }
}
