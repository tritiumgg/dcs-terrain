-- Reference constants for the synthetic theatre.
--
-- Its twin is dcsterrain/crates/dcsterrain-core/src/synth.rs, which holds the
-- same names with the same values. A test in that file parses this one and
-- fails when the two sets differ, so neither can be edited alone.
--
-- The synthetic theatre is the thing almost every test is stated against: a
-- closed-form terrain with no DCS in the loop. This file holds only the
-- numbers that define it, never the functions that evaluate them, because two
-- languages have to agree on the numbers and only one of them generates the
-- extract.
--
-- Positions are offsets from the grid origin, in metres: _NORTH_M grows with
-- DCS x, _EAST_M grows with DCS z. Nothing here is absolute, so raising
-- DEFAULT_SIZE_KM adds land to the north-east and moves no feature.
--
-- Format, so the cross-language test can parse a line:
--   * one `M.NAME = <number|string|boolean>` per line, nothing else on it
--   * no trailing comments
--   * primitives only; anything derivable (grid extents, feature positions in
--     absolute metres, tile counts) is computed by the generator

local M = {}

-- Extract format, v2. The tile layout, the water codes and the height,
-- water and surface nodata values are unchanged from v1. The two count
-- layers, `trees` and `lines`, hold instances per cell, clamped one short of
-- their nodata.

M.FORMAT_VERSION = 2
M.TILE_SIZE = 256
M.CELL_SIZE_M = 50

M.HEIGHT_NODATA = -32768
M.HEIGHT_MIN = -32767
M.HEIGHT_MAX = 32767

M.WATER_LAND = 0
M.WATER_LAKE = 1
M.WATER_SEA = 2
M.WATER_RIVER = 3
M.WATER_UNRECOGNISED = 254
M.WATER_NODATA = 255

M.SURFACE_NODATA = 0
M.SURFACE_LAND = 1
M.SURFACE_SHALLOW_WATER = 2
M.SURFACE_WATER = 3
M.SURFACE_ROAD = 4
M.SURFACE_RUNWAY = 5

M.COUNT_NODATA = 65535
M.COUNT_MAX = 65534

-- The fill triple, which is Caucasus's: what the engine returns outside the
-- terrain. A cell matching all three unrounded is written nodata. Only the
-- hook's sweeps test for it; the v2 extract has no fill, because a cell no
-- mesh triangle covers is nodata by construction.
--
-- The height is worth staring at. It rounds to 5, and so does real 5 m land,
-- which is why the test is on the unrounded returns. The synthetic theatre
-- contains that collision on purpose: its plane passes through exactly 5 m,
-- and those cells are valid land carrying the same encoded height as fill.

M.FILL_HEIGHT_M = 5.000005
M.FILL_WATER = 0
M.FILL_SEABED_M = 0.0

-- Theatre identity, as the manifest records it. The data version of a theatre
-- is the DCS build, so the file list is a record and not a version: each
-- file's path, the class name and format version its container header names,
-- its size, and the data base the header carries, which only the surface and
-- scene classes fill. Nothing is hashed and no time is recorded.
-- The surface, roads and scene entries are the terrain fingerprint. A Lua
-- file or the asset file has no header, so no class, version or payload.

M.THEATRE = "Synth"
M.EXTRACTOR_VERSION = "0.1.0"
M.DCS_BUILD = "0.0.0.0"
M.DCS_BUILD_TIMESTAMP = "00000000-000000"

M.FILE_SURFACE5_PATH = "Mods/terrains/Synth/Surface/Synth.surface5"
M.FILE_SURFACE5_CLASS = "landscape5::Surface5File"
M.FILE_SURFACE5_VERSION = 5
M.FILE_SURFACE5_SIZE = 4194304
M.FILE_SURFACE5_PAYLOAD_SIZE = 1048576
M.FILE_RN4_PATH = "Mods/terrains/Synth/roads/Synth.rn4"
M.FILE_RN4_CLASS = "landscape4::lRoadNetwork"
M.FILE_RN4_VERSION = 5
M.FILE_RN4_SIZE = 2097152
M.FILE_SCN5_PATH = "Mods/terrains/Synth/Scenes/Synth.scn5"
M.FILE_SCN5_CLASS = "landscape5::Scene5File"
M.FILE_SCN5_VERSION = 9
M.FILE_SCN5_SIZE = 3145728
M.FILE_SCN5_PAYLOAD_SIZE = 524288
M.FILE_AIRFIELD_RN4_PATH = "Mods/terrains/Synth/AirfieldsTaxiways/SYNTH.rn4"
M.FILE_AIRFIELD_RN4_SIZE = 65536
M.FILE_AIRFIELD_RN5_PATH = "Mods/terrains/Synth/AirfieldsTaxiways/SYNTH.rn5"
M.FILE_AIRFIELD_RN5_CLASS = "t5::rn5file"
M.FILE_AIRFIELD_RN5_VERSION = 0
M.FILE_AIRFIELD_RN5_SIZE = 16384
M.FILE_ASSETS_PATH = "Mods/terrains/Synth/misc/assets.assets"
M.FILE_ASSETS_SIZE = 262144
M.FILE_BEACONS_PATH = "Mods/terrains/Synth/Beacons.lua"
M.FILE_BEACONS_SIZE = 2048
M.FILE_RADIO_PATH = "Mods/terrains/Synth/Radio.lua"
M.FILE_RADIO_SIZE = 1024
M.FILE_TOWNS_PATH = "Mods/terrains/Synth/Map/towns.lua"
M.FILE_TOWNS_SIZE = 512
M.FILE_NODES_PATH = "Mods/terrains/Synth/MissionGenerator/nodes.lua"
M.FILE_NODES_SIZE = 1024

-- Grid. The authored rectangle is inset a quarter cell from the grid on every
-- side, so snapping it outward to a multiple of CELL_SIZE_M reproduces the
-- grid exactly, and an extractor that snaps the wrong way is off by a cell in
-- every direction. The declared bounds rectangle is the authored one grown by
-- BOUNDS_MARGIN_KM, which is the shape every real theatre has: bounds larger
-- than the terrain.
--
-- The mesh covers the grid less MESH_MARGIN_M on every side. The finest
-- blocks' boxes reach the authored rectangle, but no triangle covers the
-- margin, so its cells are outside the mesh and nodata in every layer.
--
-- DEFAULT_SIZE_KM is 70 for one reason. The margin sits inside the grid edge,
-- so every tile of tile row or column 0 holds cells outside the mesh and can
-- never be all sea; an all-sea tile has to be an interior one, and the
-- interior tile nearest the corner reaches 51 200 m out along both axes. A
-- smaller theatre either has no all-sea tile at all, and so never exercises
-- omit_sea_tiles end to end, or is mostly water. There is no tile wholly
-- outside the mesh at any size: the margin is 2 km and a tile is 12.8 km, so
-- that absent-tile rule is unit-tested against a hand-built manifest instead.
--
-- Every feature below lies inside a 70 km grid. Raising the size is safe;
-- lowering it drops features off the north-east.

M.DEFAULT_SEED = 1
M.DEFAULT_SIZE_KM = 70
M.ORIGIN_X_M = -30000
M.ORIGIN_Z_M = -45000
M.AUTHORED_INSET_M = 12.5
M.BOUNDS_MARGIN_KM = 10
M.MESH_MARGIN_M = 2000.0
M.OMIT_SEA_TILES = true

-- Terrain. Height is the plane plus both hills, overridden by the lake level
-- inside the lake and by sea level beyond the coast. The coastline is the line
-- north + east = COAST_M, so the sea is a triangle in the south-west corner.
-- A point on that line is land, and a diagonal of cell centres lies on it.

M.PLANE_BASE_M = -30.0
M.PLANE_SLOPE = 0.01

M.HILL_A_HEIGHT_M = 600.0
M.HILL_A_NORTH_M = 46000.0
M.HILL_A_EAST_M = 30000.0
M.HILL_B_HEIGHT_M = 250.0
M.HILL_B_NORTH_M = 46000.0
M.HILL_B_EAST_M = 35000.0
M.HILL_SIGMA_M = 1500.0

M.LAKE_NORTH_M = 13000.0
M.LAKE_EAST_M = 52000.0
M.LAKE_RADIUS_M = 2000.0
M.LAKE_LEVEL_M = 100.0

M.RIVER_EAST_M = 45000.0
M.RIVER_WIDTH_M = 60.0

M.COAST_M = 52000.0
M.SEA_LEVEL_M = 0.0

-- Roads, railway and runway. The two roads cross at
-- (ROAD_EW_NORTH_M, ROAD_NS_EAST_M); only the east-west one meets the river,
-- on the bridge. The north-south road runs over the col between the two
-- hills. The railway is not in the road graph and crosses the river without
-- a bridge.
--
-- The road network is a tree, so every edge is a cut edge and the bridge
-- does not carry the most betweenness on its own: the edge from the junction
-- to the bridge carries more, and once nodes of degree two are contracted the
-- four arms from the junction tie.
--
-- Every road, the railway and the runway's centre line run along cell
-- centres, which sit a half cell off every multiple of CELL_SIZE_M. A strip
-- ten metres wide covers a cell centre only when it runs through one, so this
-- is what puts exactly one row or column of ROAD and RUNWAY cells in the
-- surface layer rather than none.
--
-- Each network is cut into edges where it meets another or changes, as the
-- map's road file is: the east-west road at the junction and at both ends of
-- the bridge, which is its own edge, the north-south road at the junction,
-- and the railway not at all. Node ids count up from ROAD_FIRST_NODE_ID and
-- each edge carries a vertex every ROAD_VERTEX_SPACING_M. The classes are
-- Caucasus's spellings.

M.ROAD_EW_NORTH_M = 55025.0
M.ROAD_NS_EAST_M = 32525.0
M.ROAD_WIDTH_M = 10.0
M.RAIL_SOUTH_OFFSET_M = 3000.0
M.BRIDGE_LENGTH_M = 200.0
M.ROAD_SEED_SPACING_M = 1000.0
M.ROAD_SEED_NEIGHBOURS = 4
M.ROAD_EW_WEST_EAST_M = 5000.0
M.ROAD_EW_EAST_EAST_M = 65000.0
M.ROAD_NS_SOUTH_NORTH_M = 40000.0
M.ROAD_NS_NORTH_NORTH_M = 65000.0
M.ROAD_EW_CLASS = "road"
M.ROAD_NS_CLASS = "rdb"
M.RAIL_CLASS = "rw"
M.ROAD_FIRST_NODE_ID = 1
M.ROAD_VERTEX_SPACING_M = 500.0

-- The runway is centred on (RUNWAY_NORTH_M, RUNWAY_EAST_M).
M.RUNWAY_NORTH_M = 60025.0
M.RUNWAY_EAST_M = 20000.0
M.RUNWAY_LENGTH_M = 2500.0
M.RUNWAY_WIDTH_M = 50.0
M.RUNWAY_COURSE_RAD = 1.5707963267948966

-- The only randomness in the theatre: where the town's buildings sit inside
-- their spread. Terrain, roads and tables are closed-form and ignore the seed.
-- The generator is Park and Miller's minimal standard, whose products stay
-- under 2^53 and so are exact in a Lua 5.1 double.

M.PRNG_MULTIPLIER = 16807
M.PRNG_MODULUS = 2147483647

-- Scenery: SCENERY_TOWN_COUNT houses, an industrial pair, and a line of poles
-- beside the east-west road, SCENERY_COUNT objects in all. The footprints are
-- chosen to land on three different classification rules: the poles on the
-- area-under-4-square-metre rule, the warehouse on the industrial name regex,
-- the houses on nothing, which is `building`. Two more objects place a model
-- whose box is all zero, which is a model with no footprint.
--
-- A model's box is centred on its origin, OBB_W across its local x and OBB_D
-- across its local z, and rises from 0 to its HEIGHT. Model names are written
-- in lowercase, as the scene file spells them.

M.SCENERY_COUNT = 42
M.SCENERY_FIRST_ID = 70000001

M.SCENERY_TOWN_COUNT = 24
M.SCENERY_TOWN_NORTH_M = 56000.0
M.SCENERY_TOWN_EAST_M = 30000.0
M.SCENERY_TOWN_SPREAD_M = 800.0
M.SCENERY_TOWN_MODEL = "HOME1UG_A"
M.SCENERY_TOWN_OBB_W_M = 24.0
M.SCENERY_TOWN_OBB_D_M = 12.0
M.SCENERY_TOWN_MODEL_RADIUS_M = 15.0
M.SCENERY_TOWN_HEIGHT_M = 8.0

M.SCENERY_INDUSTRIAL_COUNT = 2
M.SCENERY_INDUSTRIAL_NORTH_M = 54000.0
M.SCENERY_INDUSTRIAL_EAST_M = 40000.0
M.SCENERY_INDUSTRIAL_SPACING_M = 120.0
M.SCENERY_INDUSTRIAL_MODEL_A = "SKLAD_NEW"
M.SCENERY_INDUSTRIAL_A_OBB_W_M = 41.4
M.SCENERY_INDUSTRIAL_A_OBB_D_M = 43.3
M.SCENERY_INDUSTRIAL_A_RADIUS_M = 30.1
M.SCENERY_INDUSTRIAL_A_HEIGHT_M = 12.0
M.SCENERY_INDUSTRIAL_MODEL_B = "HIM_BAK_A_NEW"
M.SCENERY_INDUSTRIAL_B_OBB_W_M = 20.0
M.SCENERY_INDUSTRIAL_B_OBB_D_M = 20.0
M.SCENERY_INDUSTRIAL_B_RADIUS_M = 14.0
M.SCENERY_INDUSTRIAL_B_HEIGHT_M = 15.0

M.SCENERY_POLE_COUNT = 14
M.SCENERY_POLE_MODEL = "BLK_LIGHT_POLE"
M.SCENERY_POLE_FIRST_EAST_M = 30000.0
M.SCENERY_POLE_SPACING_M = 1000.0
M.SCENERY_POLE_NORTH_OFFSET_M = 25.0
M.SCENERY_POLE_OBB_W_M = 0.4
M.SCENERY_POLE_OBB_D_M = 0.4
M.SCENERY_POLE_RADIUS_M = 0.3
M.SCENERY_POLE_HEIGHT_M = 10.0

M.SCENERY_NOBOX_COUNT = 2
M.SCENERY_NOBOX_MODEL = "SYNTH_NOBOX"
M.SCENERY_NOBOX_NORTH_M = 50025.0
M.SCENERY_NOBOX_EAST_M = 20025.0
M.SCENERY_NOBOX_SPACING_M = 100.0

-- Vector tables. One airfield with one runway, so runways.json joins to
-- airdromes.json on a single row; stands run north of the runway from its west
-- end; the two beacons sit on the runway edges and differ in whether they
-- carry a channel, which is the null case beacons.json has to encode. The
-- radio frequencies are Anapa's, so the four-way hf/fm/vhf/uhf mapping has a
-- real example behind it. Towns and nodes are lines, spaced far enough apart
-- that a nearest-neighbour query has an unambiguous answer.

M.AIRDROME_ID = 1
M.AIRDROME_NAME_ID = "SYNTH"
M.AIRDROME_CODE = "XSYN"
M.AIRDROME_DISPLAY_NAME = "Synth Field"
M.RUNWAY_EDGE1_NAME = "09"
M.RUNWAY_EDGE2_NAME = "27"

M.STAND_COUNT = 4
M.STAND_NORTH_OFFSET_M = 300.0
M.STAND_SPACING_M = 100.0
M.STAND_CLASS = "stand_40x40m"
M.STAND_HEADING_RAD = 0.0
M.STAND_WIDTH_M = 40.0
M.STAND_LENGTH_M = 40.0

M.BEACON_COUNT = 2
M.BEACON_1_FREQUENCY_HZ = 750000
M.BEACON_1_CHANNEL = 10
M.BEACON_2_FREQUENCY_HZ = 108100000

M.RADIO_COUNT = 1
M.RADIO_HF_HZ = 3750000
M.RADIO_FM_HZ = 38400000
M.RADIO_VHF_HZ = 121000000
M.RADIO_UHF_HZ = 250000000

M.TOWNS_COUNT = 3
M.TOWNS_FIRST_NORTH_M = 30000.0
M.TOWNS_FIRST_EAST_M = 60000.0
M.TOWNS_SPACING_M = 12000.0

M.NODES_COUNT = 2
M.NODES_FIRST_NORTH_M = 20000.0
M.NODES_FIRST_EAST_M = 40000.0
M.NODES_SPACING_M = 15000.0
M.NODES_SIDE_OFFSET_M = 2000.0

-- Trees, lines and ground. The forest is a square with a square hole, every
-- edge on a cell edge, and every cell inside it and outside the hole holds
-- TREES_PER_CELL trees, so a cell's count has a closed form. The forest
-- polygon is the same square and hole, which is the case that tests a hole's
-- ring orientation. The power line runs north along a column of cell centres
-- with a pole every LINE_POLE_SPACING_M, which puts one pole in every fourth
-- cell of that column. The built-up polygon is a rectangle round the town.

M.FOREST_SOUTH_M = 40000.0
M.FOREST_NORTH_M = 44000.0
M.FOREST_WEST_M = 55000.0
M.FOREST_EAST_M = 59000.0
M.FOREST_HOLE_SOUTH_M = 41500.0
M.FOREST_HOLE_NORTH_M = 42500.0
M.FOREST_HOLE_WEST_M = 56500.0
M.FOREST_HOLE_EAST_M = 57500.0
M.TREES_PER_CELL = 3
M.TREE_MODEL = "pine_1"
M.TREE_OBB_W_M = 6.0
M.TREE_OBB_D_M = 6.0
M.TREE_HEIGHT_M = 15.0

M.LINE_EAST_M = 50025.0
M.LINE_SOUTH_M = 20000.0
M.LINE_NORTH_M = 60000.0
M.LINE_POLE_SPACING_M = 200.0
M.LINE_MODEL = "pole_power"
M.LINE_OBB_W_M = 1.0
M.LINE_OBB_D_M = 1.0
M.LINE_HEIGHT_M = 12.0

M.BUILT_UP_SOUTH_M = 55100.0
M.BUILT_UP_NORTH_M = 56900.0
M.BUILT_UP_WEST_M = 29100.0
M.BUILT_UP_EAST_M = 30900.0

-- Ground cover. The town patch is the built-up rectangle and the forest patch
-- the forest square less its hole; the field and the beach are rectangles of
-- their own. The beach runs into the sea, whose cells stay water. Every edge
-- is on a cell edge, and each patch is half-open on its north and east.

M.FIELD_SOUTH_M = 20000.0
M.FIELD_NORTH_M = 23000.0
M.FIELD_WEST_M = 60000.0
M.FIELD_EAST_M = 63000.0
M.BEACH_SOUTH_M = 30000.0
M.BEACH_NORTH_M = 32000.0
M.BEACH_WEST_M = 20000.0
M.BEACH_EAST_M = 24000.0

-- Projection. Caucasus's parameters, because a fit must reproduce the published
-- Caucasus row within a metre. The samples are the beacons', one per beacon;
-- with fewer than three the extractor takes the parameters from its table
-- rather than fitting them, so the synthetic crs says so.

M.CRS_LON_0_DEG = 33
M.CRS_K_0 = 0.9996
M.CRS_EASTING_OFFSET_M = 99517.0
M.CRS_NORTHING_OFFSET_M = 4998115.0
M.CRS_PROJ4 = "+proj=tmerc +lat_0=0 +lon_0=33 +k_0=0.9996 +x_0=-99517 +y_0=-4998115 +datum=WGS84 +units=m"
M.CRS_SOURCE = "table"

-- The rest of config.json. The shape is FLAT because every measured theatre
-- returns FLAT, which is what lets line of sight run over the heightfield
-- alone.

M.SEA_ENABLED = true
M.SHAPE = "FLAT"
M.SUMMER_TIME_DELTA = 4
M.BULLSEYE_BLUE_NORTH_M = 10000.0
M.BULLSEYE_BLUE_EAST_M = 60000.0
M.BULLSEYE_RED_NORTH_M = 60000.0
M.BULLSEYE_RED_EAST_M = 20000.0
M.CAMERA_ALT_KM = 5.0

return M
