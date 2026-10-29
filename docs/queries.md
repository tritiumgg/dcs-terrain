# Queries

The operations `dcsterrain query` answers. Each is one Rust function taking a
`serde` request struct and returning a `serde` response struct; the command
line and `dcsterrain schema` share the structs, so they cannot drift. How the
data behind them is read and derived is in [`design.md`](design.md).

Not frozen: edit it in the pull request that changes an operation.

## 1. Common types

```rust
struct Box { min_x: f64, min_z: f64, max_x: f64, max_z: f64 }
enum Region { Box(Box), Circle { x, z, radius }, Polygon { points: Vec<[f64; 2]> },
              AirdromeBand { airdrome, min_dist, max_dist } }  // airdrome: its id or name_id
struct Point { x: f64, z: f64 }
struct Point3 { x: f64, z: f64, alt: Option<f64>, alt_agl: Option<f64> }  // exactly one is set
struct Observer { x: f64, z: f64, alt_agl: f64 }
struct Footprint { width: f64, depth: f64, orientations_deg: Vec<f64> }   // default [0, 90]
```

- A footprint at an orientation lies `depth` along the bearing it names, from
  north towards east, and `width` across it: at 0°, depth runs north–south.

- Coordinates are DCS x (north) and z (east) in meters.
- Every `Region` becomes its bounding `Box` plus a membership test before any
  grid work; cells outside the test are skipped.
- Lengths are meters and angles degrees in every request and response.
- Every response carries `terrain`, `dcs_build`, `terrain_fingerprint`,
  `cell_size` and `elapsed_ms` (`design.md` §5).
- Errors are a list of `{code, message, path}` issues. An operation returns a
  full result or an error, never a partial one.
- **GeoJSON.** Every operation that returns geometry takes `geojson: true` and
  then also returns a `geojson` field: an RFC 7946 `FeatureCollection` that any
  map program opens. Coordinates are longitude and latitude (WGS84) through the
  terrain's fitted projection, with area outlines simplified to half a cell. §1.1
  lists what each operation's features are.
- Point answers (`sample` at points, observer and target heights, `alt_agl`
  conversions) use point mode, the mesh triangle's own height. Regional answers
  read the 50 m raster.

### 1.1 GeoJSON features

Every feature carries the properties named here and nothing the JSON
response does not already hold. An operation with an observer, site or
endpoints also returns them as `Point` features.

| Operation | Features |
|---|---|
| `sample` | a `Point` per row, its layer values as properties |
| `viewshed` | one `MultiPolygon` of the visible area, holes included, with the observer, radius and target height |
| `radar_floor` | one `MultiPolygon` per band, lowest first, with `alt_agl`; with `points`, a `Point` per point with its floors |
| `coverage` | one `MultiPolygon` per band's dead zones with `alt_agl`; the asset's outline when given |
| `approach_spectrum` | a `Polygon` wedge per blind arc, out to `radius`, with the band and the arc's bearings |
| `unmask_profile` | the route as a `LineString`; a `LineString` per masked segment; the first-seen point |
| `exposure` | the unseen polygons with `alt_agl` |
| `score_site`, `find_sites` | a `Point` per candidate with its rank, score and orientation, and its footprint as a `Polygon` |
| `visible` | the sightline as a `LineString` with `visible` and `clearance_m`, and the blocking point when there is one |
| `route` | the route as a `LineString`, and each leg as one with its `length`, `ascent`, `descent`, `max_grade_pct`, `exposure` and `class` |
| `route_alternatives` | each route as a `LineString` with its stretch and overlap; the edges every alternative shares, as `LineString`s marked `shared` |
| `low_level_route` (deferred) | the path as a `LineString`, with the waypoints' MSL and AGL altitudes as `alt_msl` and `alt_agl` arrays in its properties, and each leg with its height above ground and the sites that see it. Coordinates stay two-dimensional, because RFC 7946 reads a third one as height above the ellipsoid, not MSL |
| `nearest` | a `Point` per result with its kind, name and distance; for roads and rail, the matched edge as a `LineString` |
| `scenery_in` | each object's box as a `Polygon` with its model, class and height; the region or footprint queried |
| `chokepoints` | each edge as a `LineString` with its rank, betweenness, class and `crosses_water` |
| `trafficability` | the line to the nearest road as a `LineString`, and the blocking point when there is one |
| `point_info` | a `Point` per request point with its fields |
| `airfield_info` | each runway as a `LineString` between its thresholds with designator, course, length and width; each stand as a `Point` with its size, and whether it fits when `fits` is given |
| `validate_placements` | a `Point` per placement with its issues; its shape as a `Polygon` (a circle as a 32-sided one) unless it is a point; each offending object's box |
| `towns` | each place's outline as a `Polygon` when outlines are asked for, and a `Point` at its center with its name, size class and form |

`describe` and `geo` return no geometry to draw; `geo` is the conversion
itself.

## 2. Layers

`sample`, `chokepoints` and the criteria address layers by name: `height`,
`water` (0 land, 1 lake, 2 sea, 3 river), `surface` (1 land, 3 water, 4 road,
5 runway), `cover` (1 plain, 2 field, 3 forest, 4 town, 5 beach, 6 runway,
7 water), `valid`, `slope`, `aspect`, `roughness`, `tpi_300`, `tpi_2000`,
`dist_road`, `dist_rail`, `dist_water`, `dist_builtup`, `count_building`,
`count_industrial`, `tree_density`, `share_<cover>` (the share of a cover
class, such as `share_forest`), `enclosure`, and the horizon bins
`horizon_0` (north) to `horizon_31`, clockwise. Definitions are in
`design.md` §6.

The radius layers are measured within fixed radii: `count_building` and
`count_industrial` within 1 km, a neighborhood; `tree_density` and the
`share_<cover>` layers within 300 m, the ground about a site. A distance
searches 5 km. A criterion that names its own radius or maximum uses it.


## 3. Operations from the original set

### `describe`

Request: none. Response: the terrain id, build, fingerprint, extent, each terrain
file with its size, the surface file's class and version (null for the
others, whose headers opening does not read), each cache entry's state
(`current`, `missing` or `stale` with the reason) and size, counts of
airdromes, runways, stands, beacons, towns and nodes from the tables, counts of
road edges and scenery objects once the process has read them (null before,
because reading them costs more than a `describe` may), and the validation
figures if any.

### `sample`

Request: either `region` with `step` (meters, at least the cell size), or
`points: Vec<Point>`; and `layers`. Response: rows `{x, z, values: {layer:
number|null}}` at lattice points inside the region, `step` apart from its
box's minimum corner, or one row per given point in order, capped by the
`max_rows` request field (default 10 000; over the cap is an error); and
`distance_capped`, true when a distance read as its cap. Regional `height` is
bilinear, null where a weighted corner is `nodata`; other layers are nearest.
At points, the ground layers take point mode and the derived layers the cell
holding the point. Points, whether scattered or close together, read the mesh
block by block: each block read once, each point's results merged by rank,
and the rows returned in request order. A region reads the raster's tiles,
each rasterized once from the blocks under it. A derived layer over
scattered points is derived over the box around them all, or around each
tile's points when that box passes about 50 km square.

### `geo`

Request: `to_latlon: Vec<Point>` and/or `to_xz: Vec<{lat, lon}>`. Response:
the converted lists in order, through the terrain's fitted projection;
`to_latlon` gives each position's MGRS reference to the meter, such as
`38T KM 56436 80738`, null outside 80° S to 84° N.

### `score_site`

Request: `x, z`, `footprint`, `criteria`. Response: for the best orientation,
`{orientation_deg, terms: [{name, value, pass, score}], hard_fail}`. Each
criterion gives one term; `pass` is its hard test, `score` a 0..1 soft value,
1 best.

### `find_sites`

Request: `region`, `footprint`, `criteria`, `step` (default 2 × cell size),
`spacing` (non-maximum suppression radius), `limit` (default 20, max 200),
`weights: {name: f64}` (default 1 each). Response: candidates `{x, z,
orientation_deg, score, terms}` ranked by the weighted sum of term scores
after every hard test passes, suppressed at `spacing`, plus `stats
{cells_considered, cells_passed, tiles_skipped, distance_capped}`. A tile whose
zone map of a layer a hard criterion reads cannot pass is skipped whole (§5).
Line-of-sight criteria run last.

### `visible`

Request: `a: Point3, b: Point3`. Response: `{visible, blocking: Point3 | null,
clearance_m}`, where clearance is the least vertical gap anywhere between
the ends, so it reaches each end's own height above its ground, and is
below −0.1 mm exactly when the line is blocked; `blocking` is where the ground
first rises above the line, with an absolute `alt`. The ends stand on the
mesh triangle's own height, and between them the line is judged exactly
over every mesh triangle under it, on a flat world: where triangles overlap
the one a point would report is the ground, and a stretch no triangle covers
does not block. The ground blocks when it rises more than 0.1 mm above the line, so a
line ending on the ground is not blocked by rounding (ADR 0003). An end off
the mesh is the issue `off_terrain`. DCS's own sightline ignores buildings
and trees, and so does this.

### `viewshed`

Request: `observer`, `radius`, `target_alt_agl`, `resolution` (default the
cell size, never finer). Response: `{origin_x, origin_z, cell, width, height,
bits: base64}`, `visible_fraction`, and `rings: Vec<Polygon>` when
`as_polygons` is true. The mask is aligned to multiples of its cell from the
grid's origin, so at the cell size its cells are the grid's; `bits` is row
major, rows running north, most significant bit first. A cell is set when its
center lies within `radius` and a target `target_alt_agl` above its raster
ground is seen; `visible_fraction` is the share of the cells within `radius`
that have ground, the cell holding the observer always among them. `rings`
are `[x, z]`, exteriors counterclockwise on a map and holes clockwise.

Radials are spaced to give one per mask cell on the outer ring, each stepped
every half cell and keeping the steepest angle to the ground so far. A cell
is judged against the radial nearest it in bearing, by the test a raster
walk makes, so the share matches a half-cell raster sightline to every cell;
`visible` judges over the mesh's triangles instead, and differs from it
mostly on a target on the ground (ADR 0003). The rings run along
cell edges, holes included and cells meeting only at a corner apart, then are
simplified to half a mask cell; `geojson` draws the same polygons whether or
not `as_polygons` is set. A mask, or the window of heights under it, of more
than 4 Mi cells is the issue `too_large`: about 50 km at 50 m.

### `coverage`

Request: `site: Observer`, `envelope {radius, alt_bands_agl}`, `asset: Region
| null`, `min_masked_range` (default 8 km), `resolution` (default the cell
size). Response per band, in the order asked: `{alt_agl, covered_fraction,
dead_zones: Vec<Polygon>, asset_covered_fraction, blind_approach_exists}`.
Computed from the radar floor: a cell is covered at a band when its floor is
at or below it, and `covered_fraction` is the share of the cells within the
radius with ground that are. The dead zones are the rest, as `viewshed`'s
rings are, holes told by winding. `asset_covered_fraction` is over the
asset's cells within the radius, 0 when it has none there.
`blind_approach_exists` is true when some radial, one per cell on the outer
ring, is blind as `approach_spectrum` judges one.

### `approach_spectrum`

Request: `site: Observer`, `radius`, `alt_bands_agl`, `azimuth_step_deg`
(default 5), `min_masked_range` (default 8 km), `sector: {from_deg, to_deg} |
null`. Response per band: `radials: Vec<{bearing_deg, unmask_dist,
masked_fraction, blind}>` — the range at which a target at that altitude
coming in from `radius` is first visible (null if never), the masked share of
the radial, and whether it stays masked in to `min_masked_range` — plus
`blind_arc_fraction`, `widest_blind_arc_deg`, `blind_arcs: Vec<{from_deg,
to_deg}>`, `unmask_dist_min`, `unmask_dist_median` and `unmask_dist_max`;
with `sector`, the same summaries restricted to it as `sector_summary`.
Computed from the radar floor along each radial, where the original design
marched each radial at half-cell steps; the answer is the same geometry.
Each radial is marched in from `radius` every half cell, a target seen where
the floor of the cell under it is at or below its band. A radial is blind
when it is never seen or first seen inside `min_masked_range`; one with no
ground under it is not; so with `radius` inside `min_masked_range` every
radial with ground is blind. The step, 0.1° to 90°, is rounded so the
radials divide the circle evenly, the first due north. A blind arc is a run
of blind radials, half a step either side, and may close over north; a
sector runs clockwise from
`from_deg` to `to_deg`, the whole circle when they are equal. `resolution`
(default the cell size) is the floor's.

### `unmask_profile`

Request: `site: Observer`, `route: Vec<Point3>` of two points or more,
`step` (default 100 m).
Response: `{first_seen: Point3 | null, first_seen_dist_from_site,
first_seen_dist_along_route, masked_segments: Vec<{from, to}>,
visible_fraction}`. The route is sampled every `step` from its first point,
and at its last. A sample between two heights above ground takes their
blend above the ground under it; otherwise the blend of the two ends'
altitudes. Each is judged against its point floor over the mesh's
triangles, so it is seen exactly when `visible` sees it. `first_seen` has an
absolute `alt`; a masked segment runs from its first masked sample's
distance along the route to its last's; a sample off the mesh between
heights above ground is not judged.

### `route`

Request: `network: "roads" | "railroads"`, `from`, `to`, `avoid:
Vec<Region>`, `exposure_observers: Vec<Observer>`, `exposure_weight` (default
1), `max_detour_factor` (default 3). Response: `{waypoints, length, ascent,
descent, max_grade_pct, legs: [{from, to, edge_id, class, length, ascent,
descent, max_grade_pct, exposure}], snapped_from, snapped_to, straight_line}`.

Each end snaps to the nearest point of its network, part way along an edge.
`length` is the planar length of the road geometry between the snapped ends,
the edges' own lengths; a leg is one edge, or the part of it at an end;
ascent and descent are summed from the mesh height every 25 m along each leg;
`max_grade_pct` is the steepest 100 m window, or the whole of a shorter leg;
`length / straight_line` is the detour factor, `straight_line` running between
the ends as given.

The search is A* over the network's nodes and the two snapped ends. An edge
costs `length × (1 + exposure_weight × exposure)`, exposure being the share
of its 25 m samples any observer sees a vehicle 2 m above the raster ground
at, judged as `viewshed` judges. No edge costs less than its length, so the
straight line to the far end is a consistent estimate and the route is the
least-cost one, exactly. Only nodes whose distances to the two snapped ends
sum to at most `max_detour_factor` times the straight line between them, or
that line and 5 km when that is more, are searched: no route longer passes
outside that ellipse. An edge any point of whose path lies in an `avoid`
region is not used, and of the edges the ends snapped to, the part between
the end and a node is not used when it reaches into one. With no route the
answer is the issue `no_route`. The waypoints are every graph node and,
between them, a point of the road's geometry each time 500 m have passed.

### `route_alternatives`

Request: everything `route` takes, plus `k` (default 3, max 10),
`max_stretch` (default 1.5), `max_overlap` (default 0.5), `penalty` (default
1.6, more than 1). Response: `routes`, shortest first, each a full `route`
response plus `stretch`, `overlap_with` (the share of its length each other
route also runs along, in order), `shared_edges_with_all` (edges some stretch
of which every route runs along: the true chokepoints of the pair) and
`chokepoints` (its edges in the top tenth of their network's betweenness;
null while betweenness is still being built).

The penalty method: route the shortest, then, up to 4 × `k` searches in all,
multiply the cost of every edge of the last route found by `penalty` and
route again. A route is kept when it is new, at most `max_stretch` times the
shortest's length, and shares at most `max_overlap` of its length with each
route kept, and they of theirs with it; the search stops at `k` routes. Yen's
k-shortest is not used: its alternatives differ by one edge and look the same
to a player. The method promises nothing of the alternatives it misses.

### `nearest`

Request: `kind: "road" | "rail" | "airdrome" | "town" | "node" | "beacon" |
"scenery" | "place"`, `x, z`, `n` (default 1, max 50), `class` for scenery.
Response: `Vec<{kind, id, name, class, x, z, distance}>`, nearest first. Road
and rail return the closest point on each nearest edge, the edge's index in
the graph and its class. A node gives its red and blue sides as two results,
of kind `node_red` and `node_blue`. Scenery returns each object's model as its
name and its class; without `class` it finds every class but `effect`. An
airdrome returns as `id` its numeric id in digits, or its `name_id` where the
files give none, and as `name` its display name, or its `name_id`. Wherever a
request names an airdrome, a number is an id, and a string is a `name_id` or,
failing any, an id in digits, so an `id` from here can be passed back as is.

### `scenery_in`

Request: `region`, or `footprint_at {x, z, footprint, orientation_deg}`;
`classes`, a filter. Response: `{objects: [{id, model, class, x, z, alt,
rotation_deg, obb_w, obb_d, height}], counts_by_class}` in id order: in a
region, each object whose position lies in it; for a footprint, each whose
turned model box meets it, edges touching included, among the objects within
500 m of its box. Capped by `max_rows` (default 5 000; over is an error).
`effect` objects are returned only when `classes` names them.

### `chokepoints`

Request: `region`, `network`, `limit` (default 20, max 500). Response:
`edges`, the edges any point of whose path lies in the region, ranked by
betweenness, ties in edge order, each with its `edge_id`, `betweenness`,
`class`, `crosses_water`, `tpi_300_mean` (the mean `tpi_300` of the cells
under it every 25 m, null where none has ground) and `midpoint`, halfway
along it. Until betweenness is built, which `index` does, the answer is the
issue `not_ready`; a query never builds it, since that takes minutes on the
largest terrains.

### `trafficability`

Request: `x, z`, `max_slope_deg` (more than 0, at most 60), `network`.
Response: `{reachable, nearest_road, distance, cost, blocking: Point | null,
reason}`, judging the straight line to the nearest point of the network every
25 m by the off-road cost of the cell under it, short of the road itself,
which may be a bridge. Water, a cell with no ground, or a slope past
`max_slope_deg` blocks, and `blocking` is the first sample that does. A road
more than 20 km away is the issue `too_far`. `cost` is the line's length weighted by the cost per meter
of the cell under each sample, null when it is blocked.

The cost per meter is 1 on open flat ground, divided by a factor for cover
(plain and runway 1, field 0.8, beach 0.6, town 0.5, forest 0.25) and one for
slope (1 − 0.75 × (slope / `max_slope_deg`)², a quarter at the limit): a
coarse cross-country movement map of this tool's own, not measured against
the game's ground units.

## 4. Operations added

### `point_info`

Request: `points: Vec<Point>`. Response per point: height (point mode), water
class and depth, cover, slope and aspect, the nearest town with bearing and
distance ("6 km NE of Zugdidi"), the place it stands in if any, the nearest
road with its class and distance, the nearest airfield, lat/lon and MGRS.

### `airfield_info`

Request: an airdrome id or `name_id`, and optionally `fits {width, length, height,
helicopter}`. Response: runways (designator, true course, length, width,
and each threshold's position and height), elevation, navaids (TACAN channel,
ILS, VOR and NDB frequencies), radio frequencies, and stands, each with its
name, position, width, length, height, helicopter use and shelter, plus counts
by size; with `fits`, each stand says whether an airframe of that size fits. Courses are
true; there is no magnetic model. The caller supplies the airframe's size.

### `radar_floor`

Request: `site: Observer`, `radius`, `resolution` (default the cell size),
optionally `points`, and `bands_agl` (default 30, 100, 300, 1 000 m) for the
GeoJSON. Response: a raster of the lowest altitude, AGL and MSL, at which a
target is visible from the site, or that value at each point. Each GeoJSON
band is the area where a target at that height above ground is seen; they nest,
so a map can color them as contours.
The viewshed sweep keeps the running maximum elevation angle along each
radial; the floor at a cell is that angle times range, plus the site height,
minus the ground. One sweep, the cost of a viewshed. Flat world.

The raster, `{origin_x, origin_z, cell, width, height, floor_agl,
floor_msl}`, is laid out as `viewshed`'s mask, each layer one little-endian
`f32` per cell, base64, NaN beyond the radius or off the ground; a
`viewshed` at any target height is, but for rounding, the cells whose
`floor_agl` is at or below it. `bands` gives each band's `seen_fraction`,
lowest first, the bands sorted and repeats dropped. The GeoJSON bands run
along cell edges unsimplified, so each lies inside the next as its cells
do, to the centimeter positions keep. With `points` (at most 10 000) the
response is instead one row `{x, z, ground, floor_agl, floor_msl}` per
point, each judged over the
mesh's triangles as `visible` judges a sightline, so it is the least
altitude `visible` sees, to within half its 0.1 mm tolerance; the floors
are null beyond `radius`, off the mesh, or where the site stands under its
own ground. The raster steps the 50 m
raster and so departs from the exact floor most where the ground near the
site is steep: its error at range is the near ground's rounding times the
ratio of the ranges (`data/radar-floor.md`).

### `exposure`

Request: `sites: Vec<{observer, radius, weight}>`, `region`, and a
`target_alt_agl` or a band. Response per cell: the lowest floor over all sites
and how many sites see the target altitude; optionally polygons of "unseen at
or below h". Geometry only: no clutter, Doppler notch or minimum elevation.

### `validate_placements`

Request: `placements: Vec<{id, x, z, heading, shape, on, max_slope_deg?,
min_depth?}>`. `x, z` is the placement's position and label point; `on` is
`land` (default) or `water`; `shape` is one of:

```rust
enum Shape { Point,                                // the position alone
             Rect { width: f64, depth: f64 },      // centered, turned to `heading`
             Circle { radius: f64 },               // centered
             Polygon { points: Vec<[f64; 2]> } }   // absolute DCS x, z
```

`heading` turns only a `Rect`; a circle and a polygon ignore it.

The tool knows nothing about units: the caller gives the size and what the
placement needs.

Response per placement: the steepest slope and the shallowest water under the
shape, and a list of issues, each naming the offending object. The checks run
over every cell and object the shape touches:

- **Surface:** for `on: land`, any water under the shape; for `on: water`,
  any land, or water shallower than `min_depth` where it is given.
- **Slope:** the steepest slope over `max_slope_deg`, where it is given. A
  point is judged at its own cell.
- **Overlap:** the shape against each building, wall and wire box, each
  instance tree's radius, and each runway and taxiway, by exact intersection;
  `effect` objects never count.
- **Removed objects:** the shape inside an area where the terrain removes
  scenery.

Procedural forest is not a collision; a placement in forest is reported from
`cover`.

### `low_level_route`

**Deferred, not built.** `plan.md` says why and what a later version takes.

Request: `from`, `to`, `agl_band {min, max}`, `sites` as `exposure` takes,
`max_climb_gradient`, `corridor_width`, `resolution` (default 200 m).
Response: waypoints with AGL and MSL altitude, and per leg the sites that see
it and for how long. An A* over an exposure cost raster in the corridor. It
clears terrain only, not wires or masts, and says so in its response. DCS's AI
does not hold an AGL between waypoints without terrain following.

### `towns`

Request: `region` or none for the whole terrain, filters on size class, form and
minimum building count, `sort` (building count, floor area, apartment share),
`limit`, `outlines: bool`. Response: places (`design.md` §6.3) with id, name
and other names, center, box, size class, form, building count, residential
floor area, apartment share and industrial count, and an outline when asked.
Errors with "still being built" until the `places` entry exists.

## 5. Criteria

A criterion is `{name, params}` from this closed set. Each has a hard test and
a soft score. `valid` is always applied.

| Name | Params | Hard | Soft score |
|---|---|---|---|
| `slope_max` | `deg` | max slope over footprint ≤ deg | 1 − max/deg |
| `span_max` | `m` | max − min height over footprint corners and center ≤ m | 1 − span/m |
| `surface_in` | `classes: ["land"]` | every footprint cell's `water` in set | the share of cells in set |
| `surface_enum_in` | `values` | same on `surface` | the share |
| `cover_in` | `classes` | every footprint cell's `cover` in set | the share |
| `cover_fraction_min` | `class, radius, min` | share of `class` within radius ≥ min | the share |
| `dist_cover_band` | `class, min, max` | distance to `class` in band | 1 at min, 0 at max |
| `dist_road_band`, `dist_rail_band` | `min, max` | distance in band | 1 at min, 0 at max |
| `dist_water_min` | `m` | `dist_water` ≥ m, searched to 2 × m and 5 km at least | min(1, dist_water / (2 × m)) |
| `dist_airdrome_band` | `min, max, airdrome?` | to the nearest airdrome, or the one given by id or `name_id` | 1 at min, 0 at max |
| `scenery_max` | `class, radius, max` | count within radius ≤ max | 1 − count/max |
| `scenery_in_footprint_max` | `max` | OBB overlap count ≤ max | 1 − count/max |
| `clearing_max` | `radius` (default 150), `max` | objects within radius ≤ max: what a heliport static removes at spawn | 1 − count/max |
| `enclosure_min` | `deg` | `enclosure` ≥ deg | min(1, enclosure / (2 × deg)) |
| `masked_from` | `bearing_deg, half_width_deg, min_elev_deg` | every horizon bin in the sector ≥ min_elev | the bins' mean margin over min_elev, over 10°, at most 1 |
| `not_visible_from` | `observers, max_fraction, own_alt_agl` (default 2) | share of observers seeing the candidate ≤ max | 1 − share |
| `visible_to` | `targets, min_fraction, own_alt_agl` (default 2) | share of targets the candidate sees ≥ min | the share |
| `tpi_class_in` | `classes, threshold_m` (default 10) | with `t = threshold_m`: `ridge` when `tpi_300 > t`; `valley` when `< −t`; else `upper` when `tpi_2000 > t`; `lower` when `< −t`; else `mid` when slope > 5°; else `flat` | 1 |
| `valid` | none | every footprint cell's `valid == 1` | the share |

A criterion searches each distance past where its answer could change, so
none reads a cap: a band one cell past its maximum, so a site with nothing
nearer fails it, and `dist_water_min` to twice its minimum, 5 km at the
least, past which it scores 1. A footprint's sides are at most 10 km.

A site is a candidate position at one orientation. The footprint's cells are
those whose centers lie in it, and always the cell holding the site. A
criterion with no footprint in it reads the layer at the site's cell; a
count within a radius counts structures whose positions lie within it of the
site; `masked_from`'s sector is the bins whose center lines lie within
`half_width_deg` of the bearing, or the one bin holding it.

Evaluation is cheapest first. The layers read at the site's cell are derived
over the region once, and each grid tile's least and greatest value of each,
or the codes it holds, skips a tile where no cell can pass, before its
horizon is computed or its scenery read; the skip is exact, because each such
test reads the site's own cell or a footprint that includes it. The horizon
criteria follow on the tiles left and skip again; then the rest, in order of
cost over the share of sites each fails; then line of sight.

## 6. Testing

Every operation has tests on `Synth` with closed-form expectations: `visible`
across the hill and around it; `viewshed` fraction against brute force;
`radar_floor` behind the hill against its geometry, and `coverage`'s dead zone
from it; `unmask_profile` first-seen distance from the hill; `route` choosing
the bridge and avoiding a box, A* equal to Dijkstra and to Bellman–Ford, and
`route_alternatives` finding the two roads to the bridge; `find_sites` accepting only the plane and
rejecting the lake; `masked_from` true east of the big hill and false west;
`chokepoints` ranking the bridge first; `scenery_in` counts against the
generator's objects; `cover_in` on each cover patch; `towns` against
brute-force linkage; one `validate_placements` case per issue kind and per shape. Every
request and response struct round-trips through JSON.

The opt-in group replays two live examples on a real Caucasus install:
`find_sites` gives the same eight FARP candidates within one cell, and site 1
of the SAM example is masked from all seven western points.
