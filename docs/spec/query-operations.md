# Spec: query operations

The operations `dcsterrain-core::ops` implements over a packed file.
Each is one Rust function taking a `serde` request struct and returning
a `serde` response struct, one `dcsterrain query <file> <op> <json>`
subcommand, and one MCP tool (`mcp-server.md`). The three surfaces
share the structs, so their schemas cannot drift. Worked campaign
examples of these operations, with live numbers, are in
`design-and-facts.md` under "Worked tasks" and "Choosing a site that
is not the best".

## Common types

```rust
struct Box { min_x: f64, min_z: f64, max_x: f64, max_z: f64 }
enum Region { Box(Box), Circle { x, z, radius }, Polygon { points: Vec<[f64; 2]> },
              AirdromeBand { airdrome_id, min_dist, max_dist } }
struct Point { x: f64, z: f64 }
struct Point3 { x: f64, z: f64, alt: Option<f64>, alt_agl: Option<f64> }  // exactly one of alt (absolute) or alt_agl (above ground) is set; both or neither is an error
struct Observer { x: f64, z: f64, alt_agl: f64 }
struct Footprint { width: f64, depth: f64, orientations_deg: Vec<f64> }  // default [0, 90]
```

Every `Region` is converted to its bounding `Box` plus a membership
predicate before any grid work; cells outside the predicate are
skipped. All lengths are metres, angles degrees in the request and
response structs (radians never cross the API boundary).

Every response carries `theatre`, `dcs_build`, `terrain_digest`,
`cell_size` and `elapsed_ms` so a consumer can always tell what answered.

Errors are a `Vec<Issue { code, message, path }>`; an operation either
returns a full result or an error, never a partial.

## Operations

### `describe`

Request: none. Response: every `meta` key, the `grid` table, row counts
of every vector table, `layers_dropped`, and the validation figures if
stamped.

### `sample`

Request: either `region` with `step: f64` (metres, ≥ cell_size), or
`points: Vec<Point>`; and `layers: Vec<String>`. Response: rows `{x, z,
values: {layer: number|null}}` at lattice points inside the region, or
one row per given point in order, capped at `max_rows` (default 10 000,
error when exceeded rather than truncation). `height` is bilinear;
others nearest.

### `geo`

Request: `to_latlon: Vec<Point>` and/or `to_xz: Vec<{lat, lon}>`.
Response: the converted lists in the same order, using the file's
`crs_proj4`. A pure function over `meta`; no grid reads.

### `score_site`

Request: `x, z`, `footprint`, `criteria: Vec<Criterion>`. Response: for
the best orientation, `{orientation_deg, terms: [{name, value, pass,
score}], hard_fail: bool}`. Each criterion contributes one term;
`pass` is the hard test, `score` is a 0..1 soft value (1 best). See
"Criteria".

### `find_sites`

Request: `region`, `footprint`, `criteria`, `step` (candidate lattice,
default 2 × cell_size), `spacing` (non-maximum suppression radius),
`limit` (default 20, max 200), `weights: {name: f64}` (default 1 each).
Response: candidates `{x, z, orientation_deg, score, terms}` ranked by
weighted sum of term scores after every hard test passes, suppressed at
`spacing`, plus `stats {cells_considered, cells_passed, tiles_skipped}`.
Tile min/max on `height`, `dist_*` and `valid` are used to skip whole
tiles a hard criterion cannot pass (a tile whose height range is under
`span_max` cannot fail `span_max` or `slope_max`).

### `visible`

Request: `a: Point3, b: Point3`, each with `alt` or `alt_agl`.
Response: `{visible: bool, blocking: Point3 | null,
clearance_m: f64}` where clearance is the minimum vertical gap between
the sightline and the terrain (negative when blocked); `blocking`
carries an absolute `alt`.

### `viewshed`

Request: `observer`, `radius`, `target_alt_agl`, `resolution` (default
cell_size, may be coarser). Response: `{origin_x, origin_z, cell,
width, height, bits: base64}` bitmask, plus `visible_fraction`, and
optionally `rings: Vec<Polygon>` when `as_polygons` is true (marching
squares over the mask, simplified to 0.5 cell).

### `coverage`

Request: `site: Observer`, `envelope {radius, alt_bands_agl: Vec<f64>}`,
`asset: Region | null`, `min_masked_range` (default 8 km). Response: per
band `{alt_agl, covered_fraction,
dead_zones: Vec<Polygon>}` and, when `asset` is given, `asset_covered_
fraction` per band, plus `blind_approach_exists: bool` per band (some
radial stays masked from `radius` to inside `min_masked_range`).

### `approach_spectrum`

Request: `site: Observer`, `radius` (engagement or search radius),
`alt_bands_agl: Vec<f64>`, `azimuth_step_deg` (default 5),
`min_masked_range` (default 8 km), `sector: {from_deg, to_deg} | null`.
For every azimuth and band, march the radial from `radius` inward at
half-cell steps and record where the site first has line of sight to a
target at that altitude. Response per band: `radials: Vec<{bearing_deg,
unmask_dist, masked_fraction, blind}>` where `unmask_dist` is the range
of the first visible step (null when the radial is masked over its whole
length), `masked_fraction` is the fraction of the radial's steps from
`radius` to the site that are masked, and `blind` is true when the
radial stays masked from `radius` in to `min_masked_range`; and
summaries `blind_arc_fraction`, `widest_blind_arc_deg`, `blind_arcs:
Vec<{from_deg, to_deg}>`, `unmask_dist_min`, `unmask_dist_median`,
`unmask_dist_max`. When `sector` is given the summaries are also
returned restricted to it as `sector_summary`. `coverage` is this
operation integrated over area; `unmask_profile` is it along one chosen
route. This one is for a player who may come from any direction.

### `unmask_profile`

Request: `site: Observer`, `route: Vec<Point3>` (each with `alt` or
`alt_agl`), `step` (default 100 m). Response: `{first_seen: Point3 | null,
first_seen_dist_from_site, first_seen_dist_along_route, masked_segments:
Vec<{from, to}>, visible_fraction}`.

### `route`

Request: `network: "roads" | "railroads"`, `from: Point`, `to: Point`,
`avoid: Vec<Region>`, `exposure_observers: Vec<Observer>`,
`exposure_weight` (default 1), `max_detour_factor` (default 3).
Response: `{waypoints: Vec<Point>, length, ascent, descent, max_grade_pct,
legs: Vec<{from, to, length, ascent, descent, max_grade_pct, exposure}>,
snapped_from, snapped_to, straight_line}`. `length` is the planar length
of the road geometry between the snapped endpoints, summed over edges
(the `road_edge.length` values); `ascent` and `descent` are cumulative
from `height` sampled every 25 m along the geometry; `max_grade_pct`
is the steepest 100 m window; `straight_line` is the endpoint distance,
so `length / straight_line` is the detour factor. Loads the subgraph in the
expanded box of the endpoints (25 %, grown until connected or
`max_detour_factor` exceeded), removes edges intersecting `avoid`,
weights `length × (1 + exposure_weight × exposure)` where exposure is
the fraction of 25 m samples visible to any observer, runs A* with
Euclidean heuristic, and simplifies the polyline to ≤ 1 point per
500 m preserving every graph node.

### `route_alternatives`

Request: everything `route` takes, plus `k` (default 3, max 10),
`max_stretch` (default 1.5: no alternative longer than 1.5 × the
shortest), `max_overlap` (default 0.5: no two alternatives share more
than half their length), `penalty` (default 1.6). Response:
`routes: Vec<Route>` where each is a full `route` response plus
`stretch`, `overlap_with: Vec<f64>` (shared length fraction against
each other returned route), `shared_edges_with_all: Vec<edge id>` (the
edges every alternative uses, which are the true chokepoints of the
pair of endpoints), and `chokepoints: Vec<edge id>` (edges on this
route with `betweenness` in the top decile).

Algorithm: run `route` for the shortest; then repeat up to 4 × `k`
times: multiply the weight of every edge on the routes found so far by
`penalty`, run A* again, accept the result if its stretch and its
overlap against every accepted route are within the limits, stop when
`k` are accepted or the candidates repeat. This is the penalty method;
it produces routes that differ where the network allows and coincide
only where it does not (a single bridge), and it reports those forced
coincidences as `shared_edges_with_all`. Yen's k-shortest is not used
because its alternatives differ by one edge and look identical to a
player.

### `nearest`

Request: `kind: "road" | "rail" | "airdrome" | "town" | "node" |
"beacon" | "scenery"`, `x, z`, `n` (default 1, max 50), `class` filter
for scenery (`town` and `node` read `poi` by `kind`; `node` matches both
`node_red` and `node_blue` and returns the `poi.kind`). Response: `Vec<{kind, id, name, x, z, distance}>`. Road and
rail return the closest point on the nearest edge and its edge id.

### `scenery_in`

Request: `region` or `footprint_at {x, z, footprint, orientation_deg}`.
Response: `{objects: Vec<{id, model, class, x, z, rotation_deg, obb_w,
obb_d}>, counts_by_class}` with exact OBB overlap for the footprint
form; capped at 5 000 objects, error above.

### `chokepoints`

Request: `region`, `network`, `limit`. Response: edges ranked by
`betweenness`, with `crosses_water` and the mean `tpi_300` along the
geometry, and midpoint.

### `trafficability`

Request: `x, z`, `max_slope_deg`, `network`. Response: `{reachable:
bool, nearest_road: Point, distance, blocking: Point | null, reason}`
by sampling the straight line to the nearest road at 25 m for slope and
water.

## Criteria

A `Criterion` is `{name, params}` from this closed set. Each has a hard
test and a soft score.

| Name | Params | Hard | Soft score |
|---|---|---|---|
| `slope_max` | `deg` | max slope over footprint ≤ deg | 1 − max/deg |
| `span_max` | `m` | max−min height over footprint corners and centre ≤ m | 1 − span/m |
| `surface_in` | `classes: ["land"]` | every footprint cell's `water` in set | 1 |
| `surface_enum_in` | `values: [1]` | same on `surface` (mission pass) | 1 |
| `dist_road_band` | `min, max` | `dist_road` in band | 1 at min, 0 at max |
| `dist_rail_band` | `min, max` | same | same |
| `dist_water_min` | `m` | `dist_water` ≥ m | `min(1, dist_water / (2 × m))` |
| `dist_airdrome_band` | `min, max, airdrome_id` (optional) | to the nearest airdrome, or to `airdrome_id` when given | 1 at min, 0 at max |
| `scenery_max` | `class, radius, max` | count in radius ≤ max (SAT for building/industrial, R-tree otherwise) | 1 − count/max |
| `scenery_in_footprint_max` | `max` | OBB overlap count ≤ max | 1 − count/max |
| `clearing_max` | `radius` (default 150), `max` | objects within radius ≤ max; the count a heliport static would remove at spawn (probe log) | 1 − count/max |
| `enclosure_min` | `deg` | `enclosure` ≥ deg | saturating |
| `masked_from` | `bearing_deg, half_width_deg, min_elev_deg` | every horizon bin in the sector has elevation ≥ min_elev (a ridge that way) | mean margin |
| `not_visible_from` | `observers, max_fraction, own_alt_agl` (default 2) | fraction of observers with LOS to the candidate at `own_alt_agl` ≤ max | 1 − fraction |
| `visible_to` | `targets, min_fraction, own_alt_agl` (default 2) | fraction of targets with LOS from the candidate at `own_alt_agl` ≥ min | fraction |
| `tpi_class_in` | `classes`, `threshold_m` (default 10) | class from `tpi_300` (local, `l`), `tpi_2000` (regional, `r`) and `slope` with `t = threshold_m`: `ridge` when `l > t`; `valley` when `l < −t`; else `upper` when `r > t`; `lower` when `r < −t`; else `mid` when slope > 5°; else `flat`. Class in `classes` | 1 |
| `valid` | none | `valid == 1` | 1 |

`valid` is always applied. `clearing_max` is `scenery_max` over every
class with radius 150 by default, kept as a named criterion because
`clearing` is the word the metrics document and the campaign use. LOS criteria are evaluated last because they
are the expensive ones; the others prune first.

## Performance targets

On a packed Caucasus at 50 m, desktop, cold cache: `describe` under
10 ms; `sample` of 10 000 rows under 100 ms; `visible` under 1 ms;
`viewshed` 20 km radius under 300 ms; `find_sites` over a 20 × 20 km
region with geometric criteria only under 500 ms, with one LOS
criterion of 36 observers under 10 s; `route` 50 km under 500 ms. Each
is a benchmark on the synthetic theatre scaled to a 100 × 100 km grid,
asserted as an upper bound in CI at 3× the target to allow for slow
runners. The targets are benchmarks, not correctness rules; a miss is
a decision about the target or the LOS step, not a failing operation.

## Testing

Every operation has tests on the synthetic theatre with closed-form
expectations (design doc, `core.md` "Synthetic
theatre"): `visible` across the hill and around it; `viewshed` fraction
against brute force; `coverage` dead zone behind the hill;
`unmask_profile` first-seen distance from the hill geometry; `route`
choosing the bridge and avoiding a box; `find_sites` accepting only the
plane and rejecting the lake; `masked_from` true east of the big hill
and false west of it; `chokepoints` ranking the bridge first;
`scenery_in` counts against the generator's object list. Serialisation
round-trip tests on every request and response struct. No real data.

## Acceptance

1. All operations pass their synthetic tests.
2. `find_sites` and `coverage` reproduce the live FARP and SAM examples
   in the design doc on a real Caucasus extract (developer-local, opt-in
   test): the same eight FARP candidates within one cell, and site 1 of
   the SAM example masked from all seven western points.
3. Benchmarks within target.
