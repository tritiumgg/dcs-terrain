# ADR 0039: Extract format v2 is written by the file decoders

## Status

Accepted

## Context

**Affects:** `extract-format.md` throughout; `core.md` "Packed file",
"Scenery classification", "Road graph", "Synthetic theatre"; ADR 0007's field
sources, ADR 0009, ADR 0011's `crop` and constants, ADR 0023's fingerprint;
`plan.md` F3, C2, C3, C4, C5a, C5b, C5c, C8, C8a, C9, C15, E1 to E8, X15.

The frozen format is the hand-off between a Lua hook sweeping a running sim
and `pack`:

> The directory the extractor writes and `dcsterrain pack` reads. It is the
> only contract between the Lua side and the Rust side, and the hand-off
> between the Windows machine that runs DCS and any machine that runs
> `dcsterrain`.

Under ADR 0034 the extract is decoded from the map's files by
`dcsterrain extract`, and under ADR 0037 the hook keeps only an optional
tables pass. Everything in v1 that existed because of the sweeps loses its
reason: the tile journal and per-pass progress that let a forty-minute run
resume, the fill triple the sim returns outside its terrain, the pre-sweep's
authored rectangle, the road router's seeds and paths, and the scenery
search. The files also carry what the sim never exposed: a class for every
road edge and its end nodes, the mesh's ground materials, the tree and line
pools, and a box for every model. `docs/data/file-formats.md` describes each
structure as measured on DCS **2.9.30.28738**.

Three facts shape the format.

**The mesh has an edge.** The finest `.surface5` blocks cover a union of
boxes, and a point no finest triangle covers is outside the terrain. That is
what the sim's fill value stood for. On Caucasus the decoded height and water
agree with the live extract on all but 67 of 141.2 M water cells; whether the
mesh edge falls exactly where the live sweep wrote fill is not yet measured.

**The pools are large.** Trees number 12 967 324 on Caucasus and 25 809 719 on
Syria; lines (wire, power poles, fences, walls) 1 069 315 and 28 832 057.
At about 80 bytes a JSON line that is 1 to 2 GB a map for trees alone. What
the queries need from them is density and cover, not identity.

**A decode takes about a minute.** Rasterizing Caucasus's height and water
takes about 30 s. An interrupted run costs less to repeat than a journal costs
to write, read back and validate.

## Decision

`format_version` is 2. The extract directory is:

```
<extract>/
  manifest.json
  config.json  airdromes.json  runways.json  stands.json
  beacons.json  radio.json  towns.json  nodes.json
  roads.jsonl  railroads.jsonl  scenery.jsonl  scenery_models.json
  ground.jsonl
  tiles/<layer>/<tx>_<tz>.bin     for height, water, surface, trees, lines
```

**Completeness.** `extract` refuses an output directory that is not empty,
writes every file as `<name>.tmp` renamed into place, and writes
`manifest.json` last. A directory with a manifest is complete; one without is
an interrupted run, which `check-extract` and `pack` refuse and the user
reruns. There is no `tiles.jsonl`, no `passes`, and no resume.

**Coverage.** A cell is *outside the mesh* when no finest-mesh triangle covers
its centre, and it is `nodata` in every layer. This replaces the fill test.

**Tile layers.** Grid geometry, tile addressing and the binary layout are
v1's. `cell_size` is 50 and `tile_size` 256. At each cell centre the
*winning triangle* is the covering finest-mesh triangle chosen by the
precedence rule of `docs/data/file-formats.md`: a road strip first, then the
higher block index, then the higher triangle index.

| Layer | dtype | Value | nodata | Decoder |
|---|---|---|---|---|
| `height` | i16 | metres, `floor(h + 0.5)` of the winning triangle's interpolated height: water surface over water | −32768 | E2 |
| `water` | u8 | 0 land, 1 lake, 2 sea, 3 river, from the winning triangle's type bits | 255 | E2 |
| `surface` | u8 | the first that holds: 5 RUNWAY where a triangle of a `Runway` material subset covers the centre (a subset triangle's corners are `Pbase[baseIndex[TRI[i]]]`); 4 ROAD where the winning triangle is a road strip; 3 WATER where `water` ≠ 0; 1 LAND. 2 SHALLOW_WATER is not written | 0 | E2 |
| `trees` | u16 | count of tree-pool instances whose position falls in the cell, clamped to 65534 | 65535 | E4 |
| `lines` | u16 | count of line-pool instances, likewise | 65535 | E4 |

**Absent tiles.** A tile is omitted in three cases, and a reader rebuilds it
from the `water` tile at the same `(tx, tz)`:

| `water` tile | Layer absent | Reads as |
|---|---|---|
| absent: every cell outside the mesh | every layer | `nodata` in every layer |
| present and entirely 2 | `height`, `surface` | `height` 0, `surface` 3 |
| present | `trees` or `lines`, because every covered cell is 0 | 0 where `water` is not `nodata`, `nodata` where it is |

A count tile is written whenever any of its covered cells is non-zero, sea
tiles included.

**manifest.json** keys:

| Key | Value | Written by | Read by |
|---|---|---|---|
| `format_version` | 2 | E8 | C3 |
| `extractor_version` | the `dcsterrain` version | E8 | C5a |
| `theatre` | `id` from the map's `entry.lua` | E1 | C3, C5a, C12c |
| `dcs_build`, `dcs_build_timestamp` | `version`, `timestamp` of `autoupdate.cfg` at `--install` | E1 | C5a, C12c |
| `files` | `[{role, path, class, version, size, payload_size}]`, every file read, `path` relative to the install; `role` one of `surface`, `roads`, `scene`, `airfield_rn4`, `airfield_rn5`, `assets`, `lua`; `class` and `version` from the container header, null for Lua and `assets.assets`; `payload_size` null where the header has none | E1 | C3, C5a, C12b |
| `extracted_at` | UTC timestamp | E8 | C5a |
| `authored_bounds_m` | `{min_x, min_z, max_x, max_z}`, the union of the finest blocks' boxes | E2 | C5a, C6 |
| `authored_bounds_source` | `"surface5"` | E2 | C5a |
| `grid` | v1's object; origin and extents from `authored_bounds_m` snapped outward to 50 m | E2 | C3, C4 |
| `omit_sea_tiles` | true | E8 | C4 |
| `layers` | per layer `{dtype, nodata, unit, source}`; `source`, which replaces v1's `pass`, is the decoder's file (`"surface5"`, `"scn5"`) | E8 | C3, C5b |
| `tiles` | v1's list, `[{layer, tx, tz, path, min, max}]` | E2, E4 | C3, C4 |
| `tables` | file name per table, as v1 plus `ground` | E8 | C3 |
| `hook` | null, or `{dcs_build, dcs_build_timestamp, extracted_at}` of the hook output merged in | E8 | C5a |
| `timing_ms` | per decoder | E8 | C13 |
| `notes` | strings | E8 | C3 |

`files` replaces `terrain_fingerprint`; the fingerprint of ADR 0023 is its
`surface`, `roads` and `scene` entries, one each. `crop_m` and `bounds_km`
leave the manifest: there is no crop, and the bounds are a hook field in
`config.json`.

**Hook fields.** The hook's tables pass (ADR 0037) writes a directory holding
`config.json` and `airdromes.json` in the shape below, and a `manifest.json`
with `format_version`, `theatre`, `dcs_build`, `dcs_build_timestamp` and
`extracted_at` only. `extract --hook <dir>` merges it, and refuses one whose
`theatre`, `dcs_build` or `dcs_build_timestamp` differs. Without it, every
field marked *hook* below is null.

**config.json**:

| Key | Source | Read by |
|---|---|---|
| `id` | `entry.lua` `id`, the same read that gives the manifest's `theatre` (E1) | C5a |
| `crs`, `latlon_samples` | the beacon fit (E6, ADR 0040) | C10 |
| `bounds_km` | hook: `SW_bound`, `NE_bound` as v1 | C5a |
| `default_bullseye`, `sea_enabled`, `default_camera_km`, `summer_time_delta`, `shape` | hook, as v1 | C5a |

`presweep` and `fill` are not written: coverage is the mesh, so no fill triple
is tested or recorded.

**meta.** `pack` (C5a) fills the packed file's `meta` from the extract:

| `meta` key | From |
|---|---|
| `theatre_id`, `dcs_build`, `dcs_build_timestamp`, `extracted_at` | the manifest keys of those names (`theatre` for `theatre_id`) |
| `terrain_fingerprint` | the manifest's `files` entries with role `surface`, `roads` and `scene`, as JSON |
| `bounds_sw_x`, `bounds_sw_z`, `bounds_ne_x`, `bounds_ne_z` | `config.bounds_km` in metres (ADR 0008), else NULL |
| `sea_enabled`, `bullseye_*` | `config.sea_enabled`, `config.default_bullseye`, else NULL |
| `has_hook` | 1 when the manifest's `hook` is not null, else 0; replaces `has_mission_pass` |
| `crs_proj4` | `config.crs.proj4` (C10) |
| grid, authored-rectangle and `omit_sea_tiles` keys | as v1 and ADR 0008, from the manifest |

`fill_height`, `fill_water`, `fill_seabed` and `has_mission_pass` are not
written.

**airdromes.json**: v1's keys, one entry per airfield `.rn4` in
`AirfieldsTaxiways/`, plus every airdrome the hook lists. E8 assembles it from
what E5 and E6 return and from the hook file. Read by C5c.

| Key | Source |
|---|---|
| `name_id` | the airfield file's basename; the join key, never null |
| `id` | `Radio.lua`'s `airfield<ID>_0` for the block whose comment names `name_id` (E6); else hook; else null |
| `code`, `names`, `civilian`, `abandoned`, `class`, `towers`, `warehouses`, `fueldepots`, `shelters` | hook |
| `display_name` | hook; else the `Beacons.lua` `display_name` of the airdrome's beacons (E6); else `name_id` |
| `x`, `z` | hook `reference_point`; else the midpoint of the airdrome's first runway (E5); else the mean of its stands; else null |
| `lat`, `lon` | hook `reference_point_geo`; else `x, z` through `crs` (ADR 0040) |
| `runway_names` | the `name` of each of its `runways.json` rows (E5) |
| `beacon_ids`, `radio_ids` | `beaconId` and `radioId` values of `Beacons.lua` and `Radio.lua` whose `airfield<ID>_` prefix is the airdrome's `id`; `[]` when `id` is null (E6) |
| `roadnet`, `roadnet5` | the `.rn4` and `.rn5` paths relative to the install (E5) |

**runways.json**: v1's keys plus `airdrome_name_id`, from the paired runway
ends of the airfield `.rn4` (E5). `airdrome_id` is the airdrome's `id`, null
when that is null. `name` is `edge1_name`, a hyphen and `edge2_name` as in
ADR 0007. Read by C5c.

**stands.json**: v1's keys plus `airdrome_name_id`, `stand_class` and
`heading`, from the airfield `.rn4` and `.rn5` (E5). `airdrome_id` is as in
`runways.json`. `params` come from the stand class's record in
`misc/assets.assets`; `SHELTER` is null, since no file carries it. `flag` is
64 when `FOR_HELICOPTERS`, else 32. Read by C5c, which has no column yet for
`stand_class` or `heading`.

How the packed `airdrome`, `runway` and `stand` tables key an airdrome whose
`id` is null is C5c's decision, recorded as an ADR when it is built.

**beacons.json**, **radio.json**, **nodes.json**: v1's keys, read from
`Beacons.lua`, `Radio.lua` and `MissionGenerator/nodes.lua` as data, never
executed (E6). A beacon's `channel` is null where `Beacons.lua` gives none;
whether the file carries channels at all is not yet measured. Read by C5c.

**towns.json**: v1's keys; `lat`, `lon` from `Map/towns.lua`, `x`, `z`
through `crs` (E6, ADR 0040). Read by C5c.

**roads.jsonl**, **railroads.jsonl**: one edge of the map's `.rn4` per line,
kind 2 in roads and kind 1 in railroads; kind 0 (airfield lines, whose `y` is
not a height) is not written (E3). Read by C9 and C15.

```
{"id": 812, "class": "road", "a": 10411, "b": 10412, "points": [[x, z, alt], ...]}
```

`id` is the edge's ordinal in the decoder's scan of the file, stable for a
given file, `class` the name from the file's class table, `a` and `b` the
u32 values at 12 and 8 bytes before the point count, read as the edge's node
ids, and `points` its f64 vertices. Edges
that share a node id meet at that node, so `pack` builds the graph from `a`
and `b` and places each node at the mean of the endpoints that name it. Seeds,
paths and `nopath` lines are gone.

**scenery.jsonl**: one object of the `.scn5` object pool or its non-instanced
group per line (E4). Read by C8 and C8b.

```
{"id": 70741507, "model": "sklad_new", "x": ..., "alt": ..., "z": ..., "rotation": ...}
```

`id` is the instance id (`node << S | index`), `model` the model name as the
file spells it, lowercase, `x, alt, z` its position, and `rotation` the
heading of its local X axis in radians. Its footprint is its model's box:
`pack` (C8) writes the packed `scenery` row's `obb_w` and `obb_d` as the box's
x and z extents, `radius` as half the diagonal of those two, and `rotation`
as given. A model with an all-zero box has no footprint, and those three are
NULL.

**scenery_models.json**: one row per model the three pools place (E4), read
by C8 and C15:
`{model, counts: {objects, lines, trees}, box: {min_x, min_y, min_z, max_x,
max_y, max_z}}`, the box model-local from the file's model table, all zero for
a model with no box. `display_name`, `category`, `life`, `type_bits` and the
footprint medians are gone.

**ground.jsonl**: one polygon per line, the union of the finest mesh's
material subset triangles for each ground class (E7). Read by C15.

```
{"class": "forest", "rings": [[[x, z], ...], [[x, z], ...]]}
```

`class` is `forest` for materials whose name begins `Forest` and `built_up`
for those beginning `Town`; other materials are not written. The first ring is
the outer boundary and the rest are holes. An outer ring has a positive
signed area `Σ (x_i z_{i+1} − x_{i+1} z_i)`, and a hole a negative one. Rings
are closed by their first point, which is not repeated. How the triangles are
merged into rings is E7's.

**Packed form of the new layers.** The packed columns and tables for road
class and kind, the count layers and their SATs, the ground polygons and the
model boxes are C15's, recorded as an ADR when it is built.

### Alternatives considered

**Write every tree and line.** One line per instance with position and model,
as the scenery objects are. It would let a later query ask for single trees.
It lost on size, 1 to 2 GB of text a map for trees alone, when every reader
the plan names would count them into cells first.

**Keep the journal and per-pass resume.** Nothing in a decode takes long
enough to resume: repeating a run costs about a minute, while the journal
costs a second write path, a union rule in `check-extract` and the
journal-ahead-of-manifest failure case.

**Forest and built-up ground as a code grid.** One u8 tile layer, a material
code per cell, read by machinery that already exists. The maintainer chose
polygons: the mesh's materials are areas with exact edges, a 50 m grid blurs
them, and a polygon keeps a forest edge where a line-of-sight or concealment
query needs it.

**Keep the road seeds and paths.** The router's output is what `pack`'s graph
rules were written for. It lost because the file holds the network itself,
with the router's lane offset removed and the node ids that make snapping
unnecessary.

## Consequences

Every field of the extract has a decoder that writes it and a C task that
reads it, as the tables above say. Three are carried for a reader that does
not exist yet: a stand's `stand_class` and `heading`, and the airdrome's hook
lists. The extract is smaller: no journal, no seeds, and two count layers in
place of 40 M tree and line records.

Without the hook an extract has null in six `config.json` fields and nine
airdrome keys. `meta`'s bullseye, bounds and sea keys are then NULL, and on
Nevada half the airdromes have a null `id`, so `runways.json` and
`stands.json` join on `airdrome_name_id`, and those airdromes' `beacon_ids`
and `radio_ids` are empty. C5c decides the packed key; the packed schema is not
settled by this record.

E2's opt-in comparison checks that the cells outside the mesh are the cells
the live Caucasus extract wrote as fill; a difference reopens the coverage
rule.

The `surface` layer is derived by a rule no ground truth has scored, and
SHALLOW_WATER is never written. E9's live crop scores it; a disagreement
reopens the `surface` row of this record.

Scenery classification loses `display_name`, `category`, `life` and the
footprint medians. Its rules run on the model box: no footprint is a zero box,
`wall` and `misc` read the box's x and z extents, and the industrial regex
matches the model name alone. Model names are lowercase in the file and
uppercase in the live catalogue, so `classes/Caucasus.toml` is matched without
regard to case. The scenery object id is structural and unchecked against a
live `getName()`; a campaign script naming a scenery object by id depends on
E9 confirming it.

Building the road graph from node ids rests on two things not yet confirmed:
that the two u32 before the zero word are the edge's node ids, and that edges
sharing an id meet at one point. On the airfield files endpoints sit 4 to 18 m
from their node. E3 checks both, by the endpoints of edges sharing an id, and
C9 reports the spread of each node's endpoints; a large one reopens the graph
rule.

Done tests that change: C3 drops "journal ahead of manifest" and adds a
directory with no manifest; C4 adds the absent-tile table above; C8 and C8a
classify from boxes; C9 builds from `a` and `b`; C15 reads `ground.jsonl` and
the count layers. X15's hook writes the reduced manifest above. F2's
constants gain the synthetic theatre's road classes, tree and line counts and
ground polygons, which C2 writes.

Frozen text that now reads false: `extract-format.md` from its first
paragraph on, except grid geometry and the tile binary layout; `core.md`'s
road-graph steps 1 to 3, its catalogue source, and the `meta` fill and
mission-pass keys; ADR 0009's null rectangle,
which v2 never writes; ADR 0011's crop and road-seed constants.
