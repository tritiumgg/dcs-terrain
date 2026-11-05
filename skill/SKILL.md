---
name: dcs-terrain
description: Answer DCS World terrain siting and routing questions with `dcsterrain`, through its MCP tools or its command line, read from the terrain's own files without DCS running. Use for where to put a FARP, SAM site, bunker, ambush or landing zone; what a site can see or what is masked from it; how a convoy gets there and where it can be cut; what stands at a position or an airfield; and how hard a placement makes a mission.
---

# dcs-terrain

`dcsterrain` answers questions about one DCS terrain as JSON. It reads the DCS
install and never writes to it, and it keeps a rebuildable cache in the user's
cache directory. DCS need not be running.

Never put numbers you did not get from the tool into an answer. Work from the
real output, and say which terrain and build it came from.

## Calling it through MCP

Use the MCP tools when you have them: tools named after the operations below,
such as `describe`, `find_sites` and `visible`, from a server called
`dcsterrain`. Use them even if you also have a shell.

- A tool's arguments are the operation's request, the object a batch line puts
  under `"request"`, plus `terrain`. Its answer is the result alone, or a tool
  error holding `{"issues": [...]}`. Where this skill says to look in
  `dcsterrain schema`, read the tool's input schema: it describes every request
  field. This skill and its references describe the answers.
- **Pass `terrain` on every call**, such as `"terrain": "Caucasus"`: the id, as
  `--terrain` takes it. One server answers every terrain of the install. When
  you are unsure of an id, call `list_terrains`: it names the install, the
  build, and each terrain's id and whether it can be read. `describe` with only
  `terrain` gives one terrain's extent and build.
- There is no batch. Where this skill shows a batch, make one call per line.
  A terrain stays open while calls name it, so each is as fast as a batch
  line. A call naming another closes it and opens that one.
- A long `find_sites` or `exposure` reports its progress, and the client can
  cancel it.
- No answer is over 64 KiB. A larger `geojson` is replaced by `geojson_file`, a
  path, and `geojson_bytes`. A still larger answer is only `answer_file` and
  `answer_bytes`. The file is kept for a day. Give the user the path; a `.geojson`
  opens in a map program. If you have a tool that reads files, read only the
  fields you need. Asking again returns another path, not the text.

## Calling it with a shell

Without the MCP tools, run the command line. Run `dcsterrain list-terrains`
first. It names the install, the build and the
`--terrain` id of each terrain. An id is not always its folder: Germany's folder
is `GermanyColdWar`, its id `GermanyCW`. If the command is not on the `PATH`, look
for `dcsterrain/target/release/dcsterrain` in a checkout, and ask the user
for it if it is not there. Without a registry entry, `--install <dir>` names the
install. `--terrain synth` answers from a synthetic terrain with no install,
which is useful to try the shape of a request, not to answer a question.

Send requests as a batch: one JSON object per line on stdin, one answer per line
on stdout, in order. It needs no shell quoting, the terrain opens once, and a
bad line fails alone.

```bash
dcsterrain query --terrain Caucasus < requests.jsonl > answers.jsonl
```

PowerShell has no `<`:

```powershell
Get-Content requests.jsonl | dcsterrain query --terrain Caucasus > answers.jsonl
```

```jsonl
{"id": "kutaisi", "op": "nearest", "request": {"kind": "airdrome", "x": -284900, "z": 683800}}
{"id": "line", "op": "visible", "request": {"a": {"x": -293887, "z": 682059, "alt_agl": 6}, "b": {"x": -293887, "z": 672059, "alt_agl": 100}}}
```

Each answer is `{"line", "id", "op", "result"}`, or `{"line", "id", "op",
"issues"}` for a line that failed; a batch still exits 0, so read every line. A
single call is `dcsterrain query --terrain Caucasus visible '<json>'`: the
result alone on stdout, or `{"issues": [...]}` and exit code 1.

Write answers to a file and pull out the fields you need. `viewshed`,
`radar_floor`, `exposure`, `route` and `coverage` return long rasters, base64
strings or polygons that do not belong in a conversation.

**Look up what you are unsure of** in `dcsterrain schema --op <op>`: the request
and response of one operation, with a description of every field. It is 9 to
34 KB; the whole `dcsterrain schema` is about 150 KB, so filter it. Its
`issue_codes` lists every error. An `issues` entry is `{code, message, path}`,
and `path` names the field at fault, such as `criteria.2.params.deg`.

## Conventions

- `x` is north, `z` is east, meters. Lengths are meters, angles degrees.
- Bearings run clockwise from north, in grid terms unless a field says `true`.
  There is no magnetic model.
- `alt` is above sea level, `alt_agl` above the ground. A `Point3` sets exactly
  one of them. An `Observer` has `alt_agl`.
- A region is one of:

  ```json
  {"Box": {"min_x": -294887, "min_z": 673859, "max_x": -274887, "max_z": 693859}}
  {"Circle": {"x": -284887, "z": 683859, "radius": 10000}}
  {"Polygon": {"points": [[-290000, 680000], [-280000, 680000], [-285000, 690000]]}}
  {"AirdromeBand": {"airdrome": "Kutaisi", "min_dist": 4000, "max_dist": 15000}}
  ```

- An airdrome is named by its numeric id, its `name_id` or its display name.
  `nearest` with `"kind": "airdrome"`, `"n": 50` from any point lists them with
  their ids, names and positions. Take a position from there unrounded.
- A named town: `towns` with `limit` 1000 and search `name` and `other_names`.
  Latitude and longitude convert with `geo`, which also gives MGRS.
- Terrain only. DCS's own sightline ignores buildings and trees, and so does
  every sightline here. The world is flat to these operations.
- `geojson: true` adds a `geojson` field in longitude and latitude, for a user who
  wants to see the answer in a map program. Every operation takes it but
  `describe` and `geo`.

## Which operation

| The question | Operation |
|---|---|
| Height, slope, cover, distances at points or over a region | `sample` |
| All about one position: nearest town, road, airfield, MGRS | `point_info` |
| Convert x/z, latitude/longitude, MGRS | `geo` |
| The terrain's extent, build, cache state and counts | `describe` |
| Rank positions in a region against constraints | `find_sites` |
| Judge one position against constraints | `score_site` |
| Does this placement sit on water, a slope, a building, a runway | `validate_placements` |
| Can A see B | `visible` |
| What can one observer see at one target height | `viewshed` |
| How low can a target fly unseen around a site | `radar_floor` |
| What does a site cover at each altitude, and of an asset | `coverage` |
| From which directions can a site be approached masked | `approach_spectrum` |
| Where on a given route does a site first see it | `unmask_profile` |
| Where do many sites together leave a gap | `exposure` |
| The route by `network` `roads` or `railroads`, optionally avoiding areas or observers; with `offroad`, each end's way across country to the road planned and counted | `route` |
| Several different routes between two points | `route_alternatives` |
| The roads or bridges that matter most in a region | `chokepoints` |
| Can a ground group reach this point from the road | `trafficability` |
| The nearest road, rail, airdrome, town, node, beacon, scenery, place | `nearest` |
| The objects standing in a region or under a footprint | `scenery_in` |
| Towns by size or form, with outlines | `towns` |
| Runways, navaids, frequencies, stands, what fits where | `airfield_info` |

[references/tasks.md](references/tasks.md) works the common tasks, with requests
that run. Read the one closest to the question before writing a `find_sites`.
[references/metrics.md](references/metrics.md) says what each number means to a
mission designer and how to turn numbers into difficulty. Read it before you
tell a user whether a site is good.

## `find_sites` in brief

Hard tests decide which positions survive; the score only sorts them.

```jsonl
{"id": "lz", "op": "find_sites", "request": {"region": {"Circle": {"x": -284887, "z": 683859, "radius": 10000}}, "footprint": {"width": 60, "depth": 40}, "criteria": [{"name": "slope_max", "params": {"deg": 5}}, {"name": "surface_in", "params": {"classes": ["land"]}}], "spacing": 1000}}
```

- `region`, `footprint`, `criteria` and `spacing` are required. `spacing` is the
  least distance between results; `limit` defaults to 20 and stops at 200. `step`
  defaults to twice the cell size.
- `orientations_deg` defaults to `[0, 90]`. `depth` lies along the bearing, and
  the result's `orientation_deg` is the heading to place with.
- A candidate is `{x, z, orientation_deg, score, terms}`. Each term is `{name,
  value, pass, score}`. **The terms are the answer.** Give them, not the score,
  which is only their sum and means nothing between two requests.
- A band's soft score is 1 at its `min` and 0 at its `max`, so a band pulls
  candidates to `min`. To use one only as a fence, set its weight to 0 in
  `weights`. `valid` is always a term, and always a hard test.
- `stats` says how many positions passed. Report it: 3 and 30 000 are different
  answers.

| Criterion | Params | Passes when |
|---|---|---|
| `slope_max` | `deg` | the steepest footprint cell is at most `deg` |
| `span_max` | `m` | the height range over the footprint's four corners and center is at most `m`; ground between them is not read, so add `slope_max` for a large footprint |
| `surface_in` | `classes`: `land`, `lake`, `sea`, `river` | every footprint cell is in them |
| `surface_enum_in` | `values`: 1 land, 3 water, 4 road, 5 runway | the same on `surface` |
| `cover_in` | `classes`: `plain`, `field`, `forest`, `town`, `beach`, `runway`, `water` | every footprint cell is in them |
| `cover_fraction_min` | `class, radius, min` | the share of `class` within `radius` is at least `min` |
| `dist_cover_band` | `class, min, max` | the distance to `class` is in the band |
| `dist_road_band`, `dist_rail_band` | `min, max` | the distance is in the band |
| `dist_water_min` | `m` | water is at least `m` away |
| `dist_airdrome_band` | `min, max`, optional `airdrome` | the nearest airdrome, or that one, is in the band |
| `scenery_max` | `class, radius, max` | at most `max` objects of `class` within `radius` |
| `scenery_in_footprint_max` | `max` | at most `max` objects overlap the footprint |
| `clearing_max` | `radius` (150), `max` | at most `max` objects within `radius` |
| `enclosure_min` | `deg` | the mean horizon angle is at least `deg` |
| `masked_from` | `bearing_deg, half_width_deg, min_elev_deg` | the horizon in that sector is at least `min_elev_deg` high |
| `not_visible_from` | `observers, max_fraction, own_alt_agl` (2) | at most that share of observers see the site |
| `visible_to` | `targets, min_fraction, own_alt_agl` (2) | the site sees at least that share of targets |
| `tpi_class_in` | `classes`: `ridge`, `valley`, `upper`, `lower`, `mid`, `flat`; `threshold_m` (10) | the site's landform is one of them |

Scenery classes are `building`, `industrial`, `wall` and `misc`, which is what
`scenery_max` takes and `clearing_max` counts. `effect` objects are volumes in the
air: no criterion counts them, and `scenery_in` returns them only when `classes`
names them.

Mind these:

- **Observers and targets are fixed points.** `not_visible_from` cannot express
  "a ring around each candidate". For that, take the candidates and run a second
  pass of `visible` in a batch, as the FARP task does. Without a shell, make that
  pass one `score_site` per candidate with its ring as the observers, as the FARP
  task also shows.
- **On a plain the ranking ties.** `span_max` reads the 50 m raster's heights,
  rounded to the meter, and a road distance runs between cell centers, so many
  candidates score alike and ties go to the lesser x, then the lesser z. Tied
  candidates are ordered by position, not merit: ask for more of them and
  separate them with a second pass.
- **A position between lattice points is not judged.** Lower `step`, down to the
  cell size, to look closer.
- **Line-of-sight criteria are the expensive ones**: their cost is sightlines
  times mesh triangles. Keep observers few, the region small, and hard tests
  ahead of them. The denser a terrain's mesh, the more they cost.
- **`too_large`** at about 256 Ki sites: lattice points times orientations. That
  is about 36 km square at the default step and two orientations. Raise `step`,
  or split the region.

## Other limits and failures

| Issue code | What to do |
|---|---|
| `not_ready` | `chokepoints` and `towns` need `dcsterrain index --terrain <id>`, which takes minutes on the largest terrains. Ask before running it |
| `too_many_rows` | Raise `max_rows` on `sample` or `scenery_in`, or narrow the request. An answer is never cut short |
| `too_large` | Coarsen `step` or `resolution`, or split the region |
| `off_terrain` | A point lies off the mesh. Drop it and say so; a ring near an edge hits this |
| `no_route` | Nothing joins the ends: an `avoid` cut the network, the ends are on separate networks, a rail route would have to reverse at a switch, or the route would pass `max_detour_factor` (3) times the straight line, which you can raise. When the message says an end snaps to a piece of road that joins none the other's reaches, as every road route from `airport:Kobuleti` does on Caucasus, set `offroad`. With `offroad`, a `no_route` at `from` or `to` says what blocks that end's way to the road within `max_offroad` |
| `too_far` | `trafficability`: the nearest road is past `max_distance`, 20 km at most |
| `unknown_airdrome` | List the airdromes with `nearest` |
| `terrain_refused`, `refused` | The terrain cannot be read: an unknown file format, or a projection that misses a beacon by over 2 m. Report the message. `--allow-crs-residual` accepts the projection, but only do that if the user agrees |

`sample` sets `distance_capped` when a `dist_*` layer read its 5 km cap.

## Provenance

Every result carries `terrain`, `dcs_build`, `terrain_fingerprint`, `cell_size`
and `elapsed_ms`.

- State the terrain and `dcs_build` with any answer. ED changes terrain only in
  updates, so the build dates the answer.
- `terrain_fingerprint` is the terrain files' sizes. Two answers with the same
  build and fingerprint came from the same files. Keep it with any placement a
  campaign stores.
- `cell_size` is the raster's, 50 m on every terrain so far. `sample` over a region,
  the criteria other than sightlines, the horizon and a `route`'s `exposure` read
  that raster, and `viewshed` and the `radar_floor` raster read it beyond 1 km.
  `sample` at points, `visible`, `point_info`, `unmask_profile`, `radar_floor` at
  points and the criteria `not_visible_from` and `visible_to` read the mesh itself.
  Answers from the two can differ by a meter or more; for a final check on one
  position, use the mesh.
- `describe` gives the state of each cache entry and the validation figures. A
  null validation means no live comparison with DCS has been recorded for this
  install, so sightlines and routes are unconfirmed there.

## What a call costs

Warm, every core, in a held process, as a geometric mean over the 8 terrains on
DCS 2.9.31.29434 and a Ryzen 7 7800X3D. These are typical requests; the size of a
request moves them by orders of magnitude.

| Time | Operations |
|---|---|
| under 1 ms | `scenery_in`, `nearest`, `geo`, `towns`, `visible`, `score_site`, `unmask_profile`, `point_info` |
| 1 to 10 ms | `route`, `sample`, `describe`, `airfield_info`, `route_alternatives`, `trafficability`, `viewshed` |
| 10 to 60 ms | `chokepoints`, `approach_spectrum`, `coverage`, `find_sites`, `radar_floor` |
| 200 to 330 ms | `exposure`, `validate_placements` |

`trafficability` whose nearest road lies far off searches up to 20 km of
cells round the point, about 150 ms. A `route` or `route_alternatives` with
`offroad` searches 5 km round each end, 30 to 100 ms.

The first request on a terrain, or on a stretch of it, reads files and costs
more. A horizon criterion (`masked_from`, `enclosure_min`) or an `enclosure` or
`horizon_*` layer computes its region's horizon once and caches it. `--threads
<n>` caps the cores; the work already runs below normal priority, so a DCS
server beside it keeps the CPU. The engine's work stays within 64 MiB of
commit, and the worst case measured is 61.1 MiB. A large answer adds its own:
`sample` of all 57 layers at 10 000 rows with GeoJSON peaks near 300 MiB, so
ask for fewer rows or layers where memory is short. A process building a cache
entry is exempt: `dcsterrain index` from empty peaks at about 1.5 GiB on
GermanyCW.

## Layers for `sample`

`height`, `water` (0 land, 1 lake, 2 sea, 3 river), `surface` (1 land, 3 water,
4 road, 5 runway), `cover` (1 plain, 2 field, 3 forest, 4 town, 5 beach,
6 runway, 7 water), `valid`, `slope`, `aspect`, `roughness`, `tpi_300`,
`tpi_2000`, `dist_road`, `dist_rail`, `dist_water`, `dist_builtup`,
`count_building`, `count_industrial` (within 1 km), `tree_density`,
`share_<cover>` such as `share_forest` (within 300 m), `enclosure`, and
`horizon_0` (north) to `horizon_31`, clockwise.
