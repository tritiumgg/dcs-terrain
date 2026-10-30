//! The synthetic terrain: its reference constants, the closed form they
//! define, and the `Source` that answers from it in memory.
//!
//! The synthetic theatre is the thing almost every test is stated against: a
//! closed-form terrain with no DCS in the loop. This file holds only the
//! numbers that define it, never the functions that evaluate them; anything
//! derivable (grid extents, feature positions in absolute metres, tile
//! counts) is computed by the closed form, beside them in `synth/`.
//!
//! Positions are offsets from the grid origin, in metres: `_NORTH_M` grows
//! with DCS x, `_EAST_M` grows with DCS z. Nothing here is absolute, so
//! raising `DEFAULT_SIZE_KM` adds land to the north-east and moves no feature.

mod memory;
mod tables;
mod theatre;

pub use memory::{LINES_FIRST_ID, Synth, TERRAIN_ID, TREES_FIRST_ID};
pub use tables::{beacon_id, node_name, town_name};
pub use theatre::{Cell, Edge, Object, SynthError, Theatre};

// Extract format, v2. The tile layout, the water codes and the height,
// water and surface nodata values are unchanged from v1. The two count
// layers, `trees` and `lines`, hold instances per cell, clamped one short of
// their nodata.

pub const FORMAT_VERSION: u32 = 2;
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

pub const COUNT_NODATA: u16 = 65535;
pub const COUNT_MAX: u16 = 65534;

// Theatre identity, as the manifest records it. The data version of a theatre
// is the DCS build, so the file list is a record and not a version: each
// file's path, the class name and format version its container header names,
// its size, and the data base the header carries, which only the surface and
// scene classes fill. Nothing is hashed and no time is recorded.
// The surface, roads and scene entries are the terrain fingerprint. A Lua
// file or the asset file has no header, so no class, version or payload.

pub const THEATRE: &str = "Synth";
pub const DCS_BUILD: &str = "0.0.0.0";
pub const DCS_BUILD_TIMESTAMP: &str = "00000000-000000";

pub const FILE_SURFACE5_PATH: &str = "Mods/terrains/Synth/Surface/Synth.surface5";
pub const FILE_SURFACE5_CLASS: &str = "landscape5::Surface5File";
pub const FILE_SURFACE5_VERSION: u32 = 5;
pub const FILE_SURFACE5_SIZE: u64 = 4194304;
pub const FILE_SURFACE5_PAYLOAD_SIZE: u64 = 1048576;
pub const FILE_RN4_PATH: &str = "Mods/terrains/Synth/roads/Synth.rn4";
pub const FILE_RN4_CLASS: &str = "landscape4::lRoadNetwork";
pub const FILE_RN4_VERSION: u32 = 5;
pub const FILE_RN4_SIZE: u64 = 2097152;
pub const FILE_SCN5_PATH: &str = "Mods/terrains/Synth/Scenes/Synth.scn5";
pub const FILE_SCN5_CLASS: &str = "landscape5::Scene5File";
pub const FILE_SCN5_VERSION: u32 = 9;
pub const FILE_SCN5_SIZE: u64 = 3145728;
pub const FILE_SCN5_PAYLOAD_SIZE: u64 = 524288;
pub const FILE_AIRFIELD_RN4_PATH: &str = "Mods/terrains/Synth/AirfieldsTaxiways/SYNTH.rn4";
pub const FILE_AIRFIELD_RN4_SIZE: u64 = 65536;
pub const FILE_AIRFIELD_RN5_PATH: &str = "Mods/terrains/Synth/AirfieldsTaxiways/SYNTH.rn5";
pub const FILE_AIRFIELD_RN5_CLASS: &str = "t5::rn5file";
pub const FILE_AIRFIELD_RN5_VERSION: u32 = 0;
pub const FILE_AIRFIELD_RN5_SIZE: u64 = 16384;
pub const FILE_ASSETS_PATH: &str = "Mods/terrains/Synth/misc/assets.assets";
pub const FILE_ASSETS_SIZE: u64 = 262144;
pub const FILE_BEACONS_PATH: &str = "Mods/terrains/Synth/Beacons.lua";
pub const FILE_BEACONS_SIZE: u64 = 2048;
pub const FILE_RADIO_PATH: &str = "Mods/terrains/Synth/Radio.lua";
pub const FILE_RADIO_SIZE: u64 = 1024;
pub const FILE_TOWNS_PATH: &str = "Mods/terrains/Synth/Map/towns.lua";
pub const FILE_TOWNS_SIZE: u64 = 512;
pub const FILE_NODES_PATH: &str = "Mods/terrains/Synth/MissionGenerator/nodes.lua";
pub const FILE_NODES_SIZE: u64 = 1024;

// Grid. The authored rectangle is inset a quarter cell from the grid on every
// side, so snapping it outward to a multiple of CELL_SIZE_M reproduces the
// grid exactly, and a reader that snaps the wrong way is off by a cell in
// every direction. The declared bounds rectangle is the authored one grown by
// BOUNDS_MARGIN_KM, which is the shape every real theatre has: bounds larger
// than the terrain.
//
// The mesh covers the grid less MESH_MARGIN_M on every side. The finest
// blocks' boxes reach the authored rectangle, but no triangle covers the
// margin, so its cells are outside the mesh and nodata in every layer.
//
// DEFAULT_SIZE_KM is 70 for one reason. The margin sits inside the grid edge,
// so every tile of tile row or column 0 holds cells outside the mesh and can
// never be all sea; an all-sea tile has to be an interior one, and the
// interior tile nearest the corner reaches 51 200 m out along both axes. A
// smaller theatre either has no all-sea tile at all, and so never exercises
// omit_sea_tiles end to end, or is mostly water. There is no tile wholly
// outside the mesh at any size: the margin is 2 km and a tile is 12.8 km, so
// that absent-tile rule is unit-tested against a hand-built manifest instead.
//
// Every feature below lies inside a 70 km grid. Raising the size is safe;
// lowering it drops features off the north-east.

pub const DEFAULT_SEED: u32 = 1;
pub const DEFAULT_SIZE_KM: u32 = 70;
pub const ORIGIN_X_M: i32 = -30000;
pub const ORIGIN_Z_M: i32 = -45000;
pub const AUTHORED_INSET_M: f64 = 12.5;
pub const BOUNDS_MARGIN_KM: u32 = 10;
pub const MESH_MARGIN_M: f64 = 2000.0;
pub const OMIT_SEA_TILES: bool = true;

// Terrain. Height is the plane plus both hills, overridden by the lake level
// inside the lake and by sea level beyond the coast. The coastline is the line
// north + east = COAST_M, so the sea is a triangle in the south-west corner.
// A point on that line is land, and a diagonal of cell centres lies on it.

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
/// The lake is a bowl: this deep at its center, shoaling to nothing at its
/// shore.
pub const LAKE_DEPTH_M: f64 = 12.0;

pub const RIVER_EAST_M: f64 = 45000.0;
pub const RIVER_WIDTH_M: f64 = 60.0;
pub const RIVER_DEPTH_M: f64 = 3.0;

pub const COAST_M: f64 = 52000.0;
pub const SEA_LEVEL_M: f64 = 0.0;
/// The sea deepens by this many meters per meter out from the coast.
pub const SEA_SHELF_SLOPE: f64 = 0.01;

// Roads, railway and runway. The east-west road crosses the north-south
// road at (ROAD_EW_NORTH_M, ROAD_NS_EAST_M), west of the river, and the east
// road at (ROAD_EW_NORTH_M, ROAD_EAST_EAST_M), east of it; only the east-west
// road meets the river, on the bridge. The north-south road runs over the col
// between the two hills. The railway is not in the road graph and crosses the
// river without a bridge. A loop road leaves the north-south road at
// ROAD_LOOP_NORTH_M, runs east to ROAD_LOOP_EAST_M and south to the
// east-west road, west of the river, so two roads join the west of the
// terrain to the bridge: a route has an alternative, and both cross it.
//
// Once nodes of degree two are contracted, the bridge lies on the one edge
// between the loop's south corner and the east road's junction, with seven
// nodes west of it and four east: every one of their 28 pairs crosses it,
// and no other edge carries as many, so it carries the most betweenness.
//
// Every road, the railway and the runway's centre line run along cell
// centres, which sit a half cell off every multiple of CELL_SIZE_M. A strip
// ten metres wide covers a cell centre only when it runs through one, so this
// is what puts exactly one row or column of ROAD and RUNWAY cells in the
// surface layer rather than none.
//
// Each network is cut into edges where it meets another or changes, as the
// map's road file is: the east-west road at both junctions and at both ends
// of the bridge, which is its own edge, and where the loop meets it, each
// north-south road at its junction, the north-south road where the loop
// leaves it, the loop at its corner, and the railway not at all. Node ids
// count up from ROAD_FIRST_NODE_ID and each edge carries a vertex every
// ROAD_VERTEX_SPACING_M. The classes are Caucasus's spellings.

pub const ROAD_EW_NORTH_M: f64 = 55025.0;
pub const ROAD_NS_EAST_M: f64 = 32525.0;
pub const ROAD_WIDTH_M: f64 = 10.0;
pub const RAIL_SOUTH_OFFSET_M: f64 = 3000.0;
pub const BRIDGE_LENGTH_M: f64 = 200.0;
pub const ROAD_SEED_SPACING_M: f64 = 1000.0;
pub const ROAD_SEED_NEIGHBOURS: u32 = 4;
pub const ROAD_EW_WEST_EAST_M: f64 = 5000.0;
pub const ROAD_EW_EAST_EAST_M: f64 = 65000.0;
pub const ROAD_NS_SOUTH_NORTH_M: f64 = 40000.0;
pub const ROAD_NS_NORTH_NORTH_M: f64 = 65000.0;
pub const ROAD_EAST_EAST_M: f64 = 55025.0;
pub const ROAD_EAST_SOUTH_NORTH_M: f64 = 50000.0;
pub const ROAD_EAST_NORTH_NORTH_M: f64 = 60000.0;
pub const ROAD_LOOP_NORTH_M: f64 = 57025.0;
pub const ROAD_LOOP_EAST_M: f64 = 40525.0;
pub const ROAD_EW_CLASS: &str = "road";
pub const ROAD_NS_CLASS: &str = "rdb";
pub const RAIL_CLASS: &str = "rw";
pub const ROAD_FIRST_NODE_ID: u32 = 1;
pub const ROAD_VERTEX_SPACING_M: f64 = 500.0;

// The runway is centred on (RUNWAY_NORTH_M, RUNWAY_EAST_M).
pub const RUNWAY_NORTH_M: f64 = 60025.0;
pub const RUNWAY_EAST_M: f64 = 20000.0;
pub const RUNWAY_LENGTH_M: f64 = 2500.0;
pub const RUNWAY_WIDTH_M: f64 = 50.0;
pub const RUNWAY_COURSE_RAD: f64 = std::f64::consts::FRAC_PI_2;

// The only randomness in the theatre: where the town's buildings sit inside
// their spread. Terrain, roads and tables are closed-form and ignore the seed.
// The generator is Park and Miller's minimal standard, whose products stay
// under 2^53 and so are exact in a double.

pub const PRNG_MULTIPLIER: u32 = 16807;
pub const PRNG_MODULUS: u32 = 2147483647;

// Scenery: SCENERY_TOWN_COUNT houses, an industrial pair, and a line of poles
// beside the east-west road, SCENERY_COUNT objects in all. The footprints are
// chosen to land on three different classification rules: the poles on the
// area-under-4-square-metre rule, the warehouse on the industrial name regex,
// the houses on nothing, which is `building`. Two more objects place a model
// whose box is all zero, which is a model with no footprint.
//
// A model's box is centred on its origin, OBB_W across its local x and OBB_D
// across its local z, and rises from 0 to its HEIGHT. Model names are written
// in lowercase, as the scene file spells them.

pub const SCENERY_COUNT: u32 = 42;
pub const SCENERY_FIRST_ID: u64 = 70000001;

pub const SCENERY_TOWN_COUNT: u32 = 24;
pub const SCENERY_TOWN_NORTH_M: f64 = 56000.0;
pub const SCENERY_TOWN_EAST_M: f64 = 30000.0;
pub const SCENERY_TOWN_SPREAD_M: f64 = 800.0;
pub const SCENERY_TOWN_MODEL: &str = "HOME1UG_A";
pub const SCENERY_TOWN_OBB_W_M: f64 = 24.0;
pub const SCENERY_TOWN_OBB_D_M: f64 = 12.0;
pub const SCENERY_TOWN_MODEL_RADIUS_M: f64 = 15.0;
pub const SCENERY_TOWN_HEIGHT_M: f64 = 8.0;

pub const SCENERY_INDUSTRIAL_COUNT: u32 = 2;
pub const SCENERY_INDUSTRIAL_NORTH_M: f64 = 54000.0;
pub const SCENERY_INDUSTRIAL_EAST_M: f64 = 40000.0;
pub const SCENERY_INDUSTRIAL_SPACING_M: f64 = 120.0;
pub const SCENERY_INDUSTRIAL_MODEL_A: &str = "SKLAD_NEW";
pub const SCENERY_INDUSTRIAL_A_OBB_W_M: f64 = 41.4;
pub const SCENERY_INDUSTRIAL_A_OBB_D_M: f64 = 43.3;
pub const SCENERY_INDUSTRIAL_A_RADIUS_M: f64 = 30.1;
pub const SCENERY_INDUSTRIAL_A_HEIGHT_M: f64 = 12.0;
pub const SCENERY_INDUSTRIAL_MODEL_B: &str = "HIM_BAK_A_NEW";
pub const SCENERY_INDUSTRIAL_B_OBB_W_M: f64 = 20.0;
pub const SCENERY_INDUSTRIAL_B_OBB_D_M: f64 = 20.0;
pub const SCENERY_INDUSTRIAL_B_RADIUS_M: f64 = 14.0;
pub const SCENERY_INDUSTRIAL_B_HEIGHT_M: f64 = 15.0;

pub const SCENERY_POLE_COUNT: u32 = 14;
pub const SCENERY_POLE_MODEL: &str = "BLK_LIGHT_POLE";
pub const SCENERY_POLE_FIRST_EAST_M: f64 = 30000.0;
pub const SCENERY_POLE_SPACING_M: f64 = 1000.0;
pub const SCENERY_POLE_NORTH_OFFSET_M: f64 = 25.0;
pub const SCENERY_POLE_OBB_W_M: f64 = 0.4;
pub const SCENERY_POLE_OBB_D_M: f64 = 0.4;
pub const SCENERY_POLE_RADIUS_M: f64 = 0.3;
pub const SCENERY_POLE_HEIGHT_M: f64 = 10.0;

pub const SCENERY_NOBOX_COUNT: u32 = 2;
pub const SCENERY_NOBOX_MODEL: &str = "SYNTH_NOBOX";
pub const SCENERY_NOBOX_NORTH_M: f64 = 50025.0;
pub const SCENERY_NOBOX_EAST_M: f64 = 20025.0;
pub const SCENERY_NOBOX_SPACING_M: f64 = 100.0;

// Vector tables. One airfield with one runway, so runways.json joins to
// airdromes.json on a single row; stands run north of the runway from its west
// end; the two beacons sit on the runway edges and differ in whether they
// carry a channel, which is the null case beacons.json has to encode. The
// radio frequencies are Anapa's, so the four-way hf/fm/vhf/uhf mapping has a
// real example behind it. Towns and nodes are lines, spaced far enough apart
// that a nearest-neighbour query has an unambiguous answer.

pub const AIRDROME_ID: u32 = 1;
pub const AIRDROME_NAME_ID: &str = "SYNTH";
pub const AIRDROME_DISPLAY_NAME: &str = "Synth Field";
pub const RUNWAY_EDGE1_NAME: &str = "09";
pub const RUNWAY_EDGE2_NAME: &str = "27";

pub const STAND_COUNT: u32 = 4;
pub const STAND_NORTH_OFFSET_M: f64 = 300.0;
pub const STAND_SPACING_M: f64 = 100.0;
pub const STAND_CLASS: &str = "stand_40x40m";
pub const STAND_HEADING_RAD: f64 = 0.0;
pub const STAND_WIDTH_M: f64 = 40.0;
pub const STAND_LENGTH_M: f64 = 40.0;

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

// Trees, lines and ground. The forest is a square with a square hole, every
// edge on a cell edge, and every cell inside it and outside the hole holds
// TREES_PER_CELL trees, so a cell's count has a closed form. The forest
// polygon is the same square and hole, which is the case that tests a hole's
// ring orientation. The power line runs north along a column of cell centres
// with a pole every LINE_POLE_SPACING_M, which puts one pole in every fourth
// cell of that column. The built-up polygon is a rectangle round the town.

pub const FOREST_SOUTH_M: f64 = 40000.0;
pub const FOREST_NORTH_M: f64 = 44000.0;
pub const FOREST_WEST_M: f64 = 55000.0;
pub const FOREST_EAST_M: f64 = 59000.0;
pub const FOREST_HOLE_SOUTH_M: f64 = 41500.0;
pub const FOREST_HOLE_NORTH_M: f64 = 42500.0;
pub const FOREST_HOLE_WEST_M: f64 = 56500.0;
pub const FOREST_HOLE_EAST_M: f64 = 57500.0;
pub const TREES_PER_CELL: u32 = 3;
pub const TREE_MODEL: &str = "pine_1";
pub const TREE_OBB_W_M: f64 = 6.0;
pub const TREE_OBB_D_M: f64 = 6.0;
pub const TREE_HEIGHT_M: f64 = 15.0;

pub const LINE_EAST_M: f64 = 50025.0;
pub const LINE_SOUTH_M: f64 = 20000.0;
pub const LINE_NORTH_M: f64 = 60000.0;
pub const LINE_POLE_SPACING_M: f64 = 200.0;
pub const LINE_MODEL: &str = "pole_power";
pub const LINE_OBB_W_M: f64 = 1.0;
pub const LINE_OBB_D_M: f64 = 1.0;
pub const LINE_HEIGHT_M: f64 = 12.0;

pub const BUILT_UP_SOUTH_M: f64 = 55100.0;
pub const BUILT_UP_NORTH_M: f64 = 56900.0;
pub const BUILT_UP_WEST_M: f64 = 29100.0;
pub const BUILT_UP_EAST_M: f64 = 30900.0;

// Ground cover. The town patch is the built-up rectangle and the forest patch
// the forest square less its hole; the field and the beach are rectangles of
// their own. The beach runs into the sea, whose cells stay water. Every edge
// is on a cell edge, and each patch is half-open on its north and east.

pub const FIELD_SOUTH_M: f64 = 20000.0;
pub const FIELD_NORTH_M: f64 = 23000.0;
pub const FIELD_WEST_M: f64 = 60000.0;
pub const FIELD_EAST_M: f64 = 63000.0;
pub const BEACH_SOUTH_M: f64 = 30000.0;
pub const BEACH_NORTH_M: f64 = 32000.0;
pub const BEACH_WEST_M: f64 = 20000.0;
pub const BEACH_EAST_M: f64 = 24000.0;

// Projection. Caucasus's parameters, because a fit must reproduce the published
// Caucasus row within a metre. The samples are the beacons', one per beacon;
// with fewer than three the tool takes the parameters from its table
// rather than fitting them, so the synthetic crs says so.

pub const CRS_LON_0_DEG: i32 = 33;
pub const CRS_K_0: f64 = 0.9996;
pub const CRS_EASTING_OFFSET_M: f64 = 99517.0;
pub const CRS_NORTHING_OFFSET_M: f64 = 4998115.0;
pub const CRS_PROJ4: &str =
    "+proj=tmerc +lat_0=0 +lon_0=33 +k_0=0.9996 +x_0=-99517 +y_0=-4998115 +datum=WGS84 +units=m";
pub const CRS_SOURCE: &str = "table";

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
