# Spec: `dcsterrain` core and `pack`

The Rust workspace, the packed SQLite file, the derived layers, and the
`pack` and `check` subcommands. Query operations are
`query-operations.md`; the MCP server is `mcp-server.md`. Both
are thin layers over the core crate defined here. Background:
`design-and-facts.md`.

## Workspace

```
dcsterrain/
  Cargo.toml                 (workspace)
  crates/
    dcsterrain-core/         library: extract reader, packer, file reader, operations
    dcsterrain-cli/          binary `dcsterrain`: pack, check-extract, check, query, serve
    dcsterrain-mcp/          library: MCP tool definitions over core (see mcp-server.md)
  tests/                     workspace integration tests (synthetic only)
```

Dependencies: `rusqlite` with `bundled` and `rtree`-capable build (the
bundled SQLite has R-tree on); `serde`, `serde_json`; `rayon`; `clap`
for the CLI; `thiserror`; `memmap2` optional for tile reads. No native
libraries beyond the bundled SQLite. Builds on Windows, macOS and Linux
with stable Rust; CI builds all three.

Public surface of `dcsterrain-core`:

```rust
pub mod extract;   // read + validate an extract directory (extract-format.md)
pub mod pack;      // build a packed file from an extract
pub mod file;      // open a packed file; tile reads; table reads
pub mod grid;      // tile addressing, windows, bilinear sample
pub mod derive;    // slope, aspect, tpi, roughness, distance transforms, SAT, horizon
pub mod graph;     // road graph build, A*, betweenness
pub mod los;       // line of sight, viewshed, coverage
pub mod ops;       // the operations of query-operations.md, as functions
pub mod synth;     // synthetic theatre generator (cfg(any(test, feature = "synth")))
pub mod types;     // serde types shared by ops, CLI and MCP
```

## Packed file

One SQLite file. Opened by the reader with `query_only=1`,
`mmap_size=1 GiB`, no journal. Written by `pack` with `page_size=8192`,
`journal_mode=OFF`, `synchronous=OFF`, one transaction per grid, then
`VACUUM`.

```sql
CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT);
-- format_version, theatre_id, dcs_build, dcs_build_timestamp,
-- terrain_fingerprint (the manifest object as JSON), terrain_digest,
-- extracted_at, packed_at, dcsterrain_version, crs_proj4, bounds_sw_x, bounds_sw_z, bounds_ne_x,
-- bounds_ne_z, grid_origin_x, grid_origin_z, grid_width, grid_height,
-- cell_size, tile_size, sea_enabled, bullseye_blue_x, bullseye_blue_z,
-- bullseye_red_x, bullseye_red_z, has_mission_pass, layers_dropped,
-- omit_sea_tiles, fill_height, fill_water, fill_seabed,
-- validation_height_rmse, validation_los_agreement,
-- validation_route_ratio, validation_at

CREATE TABLE grid(name TEXT PRIMARY KEY, cell_size REAL, origin_x REAL,
  origin_z REAL, width INT, height INT, dtype TEXT, tile_size INT,
  nodata TEXT, unit TEXT, derived_from TEXT, params TEXT);

CREATE TABLE grid_tile(name TEXT, tx INT, tz INT, data BLOB,
  min_val REAL, max_val REAL, PRIMARY KEY(name, tx, tz)) WITHOUT ROWID;

CREATE TABLE scenery(id INTEGER PRIMARY KEY, model TEXT, class TEXT,
  x REAL, z REAL, alt REAL, rotation REAL, obb_w REAL, obb_d REAL, radius REAL);
CREATE VIRTUAL TABLE scenery_idx USING rtree(id, minx, maxx, minz, maxz);
CREATE TABLE scenery_class(model TEXT PRIMARY KEY, class TEXT, source TEXT);

CREATE TABLE road_node(id INTEGER PRIMARY KEY, x REAL, z REAL, network TEXT);
CREATE TABLE road_edge(id INTEGER PRIMARY KEY, a INT, b INT, network TEXT,
  length REAL, geometry BLOB, betweenness REAL, crosses_water INT);
CREATE VIRTUAL TABLE road_edge_idx USING rtree(id, minx, maxx, minz, maxz);
CREATE INDEX road_edge_a ON road_edge(a); CREATE INDEX road_edge_b ON road_edge(b);

CREATE TABLE airdrome(id INT PRIMARY KEY, name_id TEXT, code TEXT, name TEXT,
  x REAL, z REAL, lat REAL, lon REAL, civilian INT, abandoned INT, class TEXT);
CREATE TABLE runway(airdrome INT, name TEXT, x1 REAL, z1 REAL, x2 REAL,
  z2 REAL, course REAL, length REAL);
CREATE TABLE stand(airdrome INT, name TEXT, x REAL, z REAL, crossroad_index INT,
  for_helicopters INT, for_airplanes INT, shelter INT, width REAL, length REAL,
  height REAL);
CREATE TABLE beacon(id TEXT PRIMARY KEY, type INT, callsign TEXT, name TEXT,
  frequency_hz INT, channel INT, x REAL, z REAL, alt REAL, lat REAL, lon REAL);
CREATE TABLE radio(id TEXT PRIMARY KEY, callsign TEXT, roles TEXT,
  hf INT, fm INT, vhf INT, uhf INT);
CREATE TABLE poi(id INTEGER PRIMARY KEY, kind TEXT, name TEXT, x REAL, z REAL);
CREATE VIRTUAL TABLE poi_idx USING rtree(id, minx, maxx, minz, maxz);
```

`poi` holds `towns.json` as `kind = 'town'` and `nodes.json` as two
rows per node, `kind = 'node_red'` and `kind = 'node_blue'`, with the
node's `name`.
`geometry` is the edge polyline as little-endian `f32` pairs `(x, z)`.
`meta.layers_dropped` lists grids omitted by pack config. Every grid's
`params` is a JSON string of the parameters that produced it (window
sizes, bin count, classes), so a reader can tell what it is looking at.

### Versioning

A packed file describes one theatre at one terrain data version under
one DCS core build, and says so in `meta`: `dcs_build`,
`dcs_build_timestamp`, `terrain_fingerprint`, `terrain_digest`, all
copied from the manifest. `pack` names the output
`<theatre>-<dcs_build>-<terrain_digest>-<cell_size>m.sqlite` when no
name is given, and refuses to overwrite a file whose `meta` differs
unless `--force`. `describe` returns all four keys, every operation's
response carries `dcs_build` and `terrain_digest`, and the reader
exposes `Theatre::matches_install(install_dir)` which recomputes the
fingerprint from a local DCS install and reports which of the three
files differ. A terrain patch that changes nothing in those files (a
texture-only update) leaves the digest unchanged and the file valid.

## Grids

Base resolution is the pack config `cell_size`: 50 (the extract's) or
100, produced by resampling; the default is 50 when the authored
rectangle is under 500 000 km² and 100 above. Coarser grids are integer
multiples of the base and share the origin. All tiles use the file's `tile_size`,
which is the pack config `tile_size` (default 64) and not the extract's:
`pack` re-tiles every grid. The tile is the unit a window read fetches,
and 64 keeps a 10 km sightline to a handful of 8 KiB blobs. A blob
never fits in one page: `grid_tile` is `WITHOUT ROWID`, an index
b-tree whose in-page payload limit at `page_size = 8192` is about
2 030 bytes (SQLite file format, "Index B-Tree Leaf Or Interior
Cell"), so every tile spills to overflow pages, which `VACUUM` lays
out contiguously.
`nodata` is per grid. Tile blobs are the same raw layout as the extract
(row-major, little-endian). Absent tiles follow the extract's rule
(`extract-format.md`, "Grid geometry"): a tile with no `water`
tile is fill, and the reader returns `nodata` for every grid and 0 for
`valid`; a tile whose `water` tile is entirely 2 and that has no
`height` tile is sea, and the reader returns 0 for `height`, 3 for
`surface`, 0 for `valid`, and `nodata` for every other grid. `water`
tiles are stored wherever the extract has them.

| Grid | dtype | Res | Unit | Definition |
|---|---|---|---|---|
| `height` | i16 | base | m | from extract |
| `water` | u8 | base | class | from extract |
| `surface` | u8 | base | enum | from extract, mission pass only |
| `valid` | u8 | base | bool | 1 where `height` is not nodata, `water` is not 2, and the cell is inside `authored_bounds_m` if known; 0 elsewhere. Fill cells arrive from the extractor as nodata in every hook layer (`extract-format.md`, "Tile binary layout"), so `pack` applies no fill test of its own; `meta.fill_*` records the triple for provenance |
| `tpi_2000` | i8 | 4 × base | m | height − mean height in 2 km radius, clamped ±127 |
| `dist_road` | u16 | 2 × base | m | Euclidean distance transform to rasterised `roads` edges, clamped 65535 |
| `dist_rail` | u16 | 2 × base | m | same for `railroads` |
| `dist_water` | u16 | 2 × base | m | to any `water ≠ 0` cell |
| `dist_builtup` | u16 | 2 × base | m | to any 2 × base cell whose `sat_building` window of 100 m radius holds ≥ 3 objects |
| `sat_building`, `sat_industrial` | i32 | 2 × base | count | summed-area table over the count of objects of the class whose centre lies in each 2 × base cell, row-major inclusive prefix sums |
| `horizon` | u8 × bins | 4 × base | elevation | max elevation angle to the terrain in each azimuth bin, encoded `(angle_deg + 10) × 4` clamped 0..255 (−10° to 53.75° at 0.25°); bins = 32 by default, `params` records it; observer 2 m AGL; search radius 40 km |
| `enclosure` | u8 | 4 × base | deg × 4 | mean over `horizon` bins, same encoding |

Four layers are not stored and are computed from the `height` window
at query time, addressed by the same names in `sample`, `chokepoints`
and the criteria: `slope` (Horn 3 × 3 gradient, degrees), `aspect`
(downslope azimuth from north, clockwise; undefined where slope <
0.5°), `roughness` (max−min of `height` in 3 × 3), `tpi_300` (height −
mean height in a 300 m radius). The `grid` table lists them with
`derived_from = 'height'`, no tiles, and `params` holding the window;
their cost is a few cells of `height` per sample and is not measurable
beside the tile read.

Distance transforms use Felzenszwalb–Huttenlocher (exact, linear). The
horizon uses a per-cell azimuth sweep over the height tiles with a step
of half a cell and early exit when the remaining range cannot raise the
angle; at 32 bins and 40 km this is the slowest derived layer, so it
runs last, tiles in parallel, and is skippable with `--drop horizon`.

Pack config (`--config pack.toml` or flags): `cell_size` (50 or 100;
default by the 500 000 km² rule above; recorded in `meta.cell_size`),
`drop = [...]` grids to omit,
`tile_size` (default 64), `horizon_bins`, `horizon_range_m`,
`builtup_threshold`, `omit_sea_tiles`
(default true: a tile whose `water` is entirely sea is not stored for any
grid except `water`). A `cell_size` of 100 resamples the extract by
nearest-neighbour for `water`/`surface` and mean for `height`. The
extract is always 50 m, so the choice is remade at pack time without
another DCS run.

## Scenery classification

Model names are per theatre: the Caucasus set is Soviet-era Russian
(`SKLAD_NEW` warehouse, `KOTELNAYA_A_NEW` boiler house, `TR_BUDKA_NEW`
transformer booth, `HOME1UG_A` apartment block, `HIM_BAK_A_NEW` chemical
tank, `BLK_LIGHT_POLE`, `concrete_wall_01`), and Syria, Sinai or Germany
ship their own sets. No hand table can be complete across theatres, so
classification is data-driven with a hand table as an override:

1. The extract's `scenery_models.json` (see `extract-format.md`) is
   the per-theatre catalogue: for each distinct model, its count,
   `displayName` and `category` from `getDesc()`, `life`, the `type`
   bits from `getObjectsAtMapPoint`, and the median footprint `w × d`
   and `radius` (the footprint call returns no box height).
2. `pack` classifies each model by rules over the catalogue, in order:
   a hand override in `classes/<theatre>.toml` or `classes/common.toml`
   (shipped in the crate, model → class with a `reason`, optional); else
   `misc` when no instance returned a footprint (parked vehicles and
   radars placed as scenery do this: 26 of 263 Caucasus models); else
   `wall` when `min(w, d) < 1 m` and `max(w, d) ≥ 4 m`; else `misc` when
   `w × d < 4 m²` (poles, wire, signs, pipes); else `industrial` when
   the model name or `displayName` matches a small regex list
   (`SKLAD|ANGAR|HIM_BAK|BAK|CEH|KOTELNAYA|TANK|SILO|HANGAR|WAREHOUSE|
   FACTORY|PLANT|DEPOT|REFINERY`, extended per theatre); else
   `building`. `scenery_class.source` records `override`,
   `rule:nofootprint`, `rule:wall`, `rule:misc`, `rule:industrial` or
   `rule:building`. On the live Caucasus catalogue the rules alone give
   misc 34.8 %, building 57.8 %, industrial 7.4 % of objects;
   `classes/Caucasus.toml` (drafted as `classes-Caucasus.toml` beside
   these documents) moves bridges, harbour works, sports fields and
   vehicles to `misc`, tanks, garages, barns, towers and plant to
   `industrial`, and the fortress wall towers to `wall`.
3. The pack report prints the catalogue with the assigned class and
   source, sorted by count, so a person can review the top hundred
   models of a new theatre in a minute and add overrides. Nothing is
   silently `misc` any more; every assignment has a stated reason.

Classes stay the same four everywhere: `building`, `industrial`, `wall`,
`misc`. `alt` is the object's own `y`. On Caucasus no model reaches
the `wall` rule, because the objects it is for (`concrete_wall_01`,
`wire`, hook `type` 131072) never arrive from `world.searchObjects`;
the rule stays for theatres that may expose them.

## Road graph

From `roads.jsonl` and `railroads.jsonl`:

1. Collect every path polyline. Snap vertices to a 1 m lattice; identical
   snapped vertices are one node.
2. Consecutive vertices form edges; identical edges are merged.
3. Split an edge where another path's vertex lies on it within 1 m.
4. Nodes with degree 2 whose two edges are collinear within 1° are
   contracted, concatenating geometry, so intersections and bends are
   the nodes.
5. `length` is the polyline length; `crosses_water` is 1 when any 25 m
   sample along the geometry has `water ≠ 0`.
6. Betweenness centrality by Brandes on the largest component, with
   `--betweenness-samples N` (default 2000 source nodes) for large
   graphs; stored per edge, normalised to 0..1.

Seeds and `nopath` lines are not stored; the pack report gives their
counts and the fraction of seeds whose snap point lies on the final
graph, which is the coverage check.

## Projection

`config.json` carries `latlon_samples`. `pack` fits a transverse
Mercator (`k_0 = 0.9996`, WGS84) from them by least squares over
`lon_0` and the two offsets, compares to the table in the design doc when
the theatre is listed, and stores the fitted `crs_proj4` in `meta` with
the residual. A residual over 2 m fails the pack unless `--allow-crs-residual`.

## Reader

`file::Theatre::open(path)` reads `meta` and `grid` once, then serves
tile blobs on demand through a small LRU (default 64 tiles) so a
regional query touches its tiles once and nothing stays resident after
the call. `grid::Window` reads an arbitrary rectangle across tiles into
a contiguous buffer; `grid::sample(x, z)` is bilinear on `height` and
nearest on everything else. The R-tree tables are queried with plain
SQL. No application-defined SQL functions are registered.

## Line of sight

`los::visible(a, b)` samples the height along the segment every half
cell with bilinear interpolation and compares to the straight line
between the two altitudes; the world is flat (design doc). It returns
the first blocking sample. `los::viewshed` runs radials at an angular
step that gives one radial per cell on the outer ring; `los::coverage`
is viewshed at several target altitudes; `los::spectrum` keeps the
per-radial unmask distance instead of integrating it, and is what
`approach_spectrum` returns. The design doc records 300 of
300 agreement with `terrain.isVisible` for this method at 10 m sampling;
the half-cell rule at 50 m is 25 m and is the parameter the validation
sortie measures.

## Synthetic theatre

`synth::Theatre::new(seed, size_km, cell_size)` returns closed-form
functions and can write a complete extract directory. Terrain: a plane
rising 1 % to the north, two Gaussian hills (600 m and 250 m, sigma 1.5
km) at fixed positions, a circular lake of 2 km radius at a fixed centre
(`water = 1`, height 100 m flat), a straight river 60 m wide (`water =
3`), sea (`water = 2`, height 0) in the south-west corner
beyond a straight coastline. One road along the plane from west to east
crossing the river on a 200 m bridge, a second road north over the col
between the hills, a railway parallel to the first road 3 km south; a
runway 2.5 km long; 40 scenery objects with footprints in two clusters
(a "town" of `building` and an "industrial" pair) plus a line of `misc`
poles along the road; a 2 km margin inside the grid edge that is fill:
the Lua fake returns the exact triple (`GetHeight` 5.000005, `land`,
seabed 0) there so the hook's fill test writes nodata, and the Rust
generator writes nodata directly, so `valid` has a closed-form
region. Every quantity the derived layers compute has a
closed-form expectation: slope of the plane, horizon angle at a point at
distance `d` from a hill of height `h` and sigma, distance to the road
line, betweenness maximal at the bridge, the FARP scan's accepted region.
The Lua harness fake implements the same functions from the same
constants; the constants live in one place in each language and a test
compares the two extract directories byte for byte.

## Subcommands in this spec

- `dcsterrain check-extract <dir>`: the validation in
  `extract-format.md`; exit 0 on pass, 1 with a report on failure.
- `dcsterrain pack <dir> <out.sqlite> [--config pack.toml] [--drop g,...]
  [--cell-size N] [--allow-partial] [--allow-crs-residual]
  [--betweenness-samples N] [--threads N]`: builds the file, prints a
  report (grids, sizes, timings, unknown scenery models, road coverage,
  CRS residual) as text or `--json`.
- `dcsterrain check <file> [--install <dir>]`: invariants on a packed file: `meta`
  complete; every `grid` row has tiles or is listed in `layers_dropped`;
  tile blob sizes match; `min_val`/`max_val` match a sampled 5 % of
  tiles; every `road_edge` references existing nodes; R-tree entries
  equal row counts; `poi`, `airdrome`, `beacon` inside bounds. With
  `--install <dir>` it also runs `Theatre::matches_install` and reports
  which of the three fingerprint files differ. Exit 0 or
  1 with a report.
- `dcsterrain stamp <file> key=value ...`: writes `meta` keys, used by
  the validation sortie to record agreement figures.
- Logging: every subcommand writes warnings and progress to stderr,
  more with `--verbose`; stdout carries only the report or the `--json`
  document. Only `serve` takes `--log <path>`, because it is the one
  long-running process.
- `dcsterrain schema [--op name]`: prints the JSON schema of every
  operation's request and response structs (`schemars`), consumed by
  the Kotlin client's code generation.
- `dcsterrain synth <dir> [--seed N] [--size-km N] [--cell N]`: writes
  the synthetic theatre as an extract directory (feature `synth`, on by
  default in the CLI), so users and the Kotlin build can produce a test
  file without DCS.

## Testing

Unit tests per module against the synthetic theatre and closed-form
expectations, plus boundary tests: window reads straddling tile edges
and the grid edge; nodata propagation through every derived layer;
distance transform against brute force on a 64 × 64 grid; SAT window
sums against brute force; horizon against a brute-force ray march on a
small grid; graph build on hand-drawn polylines with a T-junction, a
loop and a duplicated segment; betweenness on a known small graph; A*
against Dijkstra. Integration tests in `tests/` run `pack` and `check`
on the synthetic extract in a temp dir and assert the report. The
opt-in group under `DCSTERRAIN_EXTRACT` packs a real extract and checks
only invariants, never values. No test reads a committed data file
larger than a few kilobytes.

## Acceptance

1. `pack` on the synthetic extract produces a file `check` accepts, and
   every derived layer matches its closed-form expectation within the
   stated tolerance (slope 0.1°, distances one cell, horizon 0.25°).
2. `pack` on a real Caucasus extract at 50 m completes on a desktop in
   under 30 minutes with `horizon`, under 10 without, and `check`
   passes.
3. The Lua-harness extract and the Rust synthetic extract are identical.
