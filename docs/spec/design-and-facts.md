# Design and facts

The background every spec in this directory assumes: what DCS exposes, what
was measured, and the design decisions taken. Specs cite this file rather
than restating it. The project plan is `plan.md`; the raw
measurements are `probe-log-2.9.29.27278.md`.

## Goal

Build one portable SQLite file per theatre per DCS build that answers terrain
siting and routing questions for an external campaign process, with DCS doing
no terrain work at campaign runtime. Every layer in the file comes from DCS's
own API or from DCS's own data files. The file is immutable, self-describing
through a `meta` table, and readable by any stock SQLite client.

The consumers are a Kotlin campaign process and an MCP server. Both go
through one query engine, so the query surface is a small set of named,
parameterised operations with structured results. The repository ships
tools only, never extracted terrain: users extract their own theatres.

## Sources you have

- The `dcs-api-lookup` skill: index and defs for DCS 2.9.29.27278. Use it for
  every symbol before you call it. Anchor lookups on the full dotted path.
  Its `ED's own use` line numbers for `me_mission.lua` sit three lines above
  the install's; read the cited span with a few lines of margin.
- The `dcs-scripting` skill: environment selection, `net.dostring_in` bridge
  rules, `assets/Literal.lua` for cross-state payloads, `assets/probes/` for
  probe shape, `references/coordinates.md` for projections,
  `references/terrain-and-airbases.md` for the airdrome table.
- The DCS install, read-only, build 2.9.29.27278 (`autoupdate.cfg`). Installed
  terrains: Caucasus, Marianas, Marianas WWII, Cold War Germany, Persian Gulf,
  Syria, Afghanistan, Sinai. Kola and Nevada directories exist but hold only
  `radio.lua`.
- The `dcs-api-bridge` MCP tools (`dcs_eval_in`, `dcs_reflect`, `dcs_ping`,
  `dcs_bridge_status`) against a running DCS. `state: "hook"` is the bridge's
  own state; `state: "server"` is the mission-scripting state; `state: "gui"`
  is the Mission Editor.

Never write into the DCS install. Never call `terrain.Init`, `terrain.Create`,
`terrain.Release`, `terrain.InitLight` or `terrain.getCrossParam` from a probe;
the first four manage terrain lifetime and the last logs an error on a bare
call. Read ED's call site for a function before calling it.

**Mission-state `land` and `world` calls only while `dcs_bridge_status`
reports phase `sim`.** A `land.getHeight` reaching the `server` state at the
main menu forces `Terrain.dll createGlobalLand` with no terrain and crashes
DCS with an access violation (crash log 2026-09-02 16:49:11 on this build).
A request left in flight when DCS exits stays in the bridge's transport
directory and is executed by the next DCS process at its first tick, so clear
`%LOCALAPPDATA%\Temp\DCS\dcs-api-eval` before every DCS restart, and check
phase before every `state: "server"` call. Hook-side `terrain.*` calls are
safe at the menu: they return nil.

Hook-state facts **[measured]** on 2.9.29.27278: Lua 5.1 with no
`string.pack`, no `bit` library and no LuaJIT; `io.open` works but file
handles expose only `read`, `write`, `lines`, `flush` and `close` (no
`seek`), so file sizes come from `lfs.attributes(path, "size")`, which
returns 64-bit sizes; a 1 MiB `read` takes 9 ms; a plain Lua loop runs
about 17 million simple iterations per second; `net.dostring_in(state,
src)` returns the chunk's string and a boolean, with the error message
and `false` on a Lua error, and carries a 3 MB return in 18 ms;
`onSimulationFrame` fires at the main menu and in the Mission Editor,
and no callback fires during a mission load (`DcsApiEval.lua`, D36).

## Facts that shape the design

### The hook-side `terrain` module is the primary extraction API

`require("terrain")` in the hook state returns the same C module the Mission
Editor uses. **[measured]** on 2.9.29.27278: at the main menu the module loads
and `GetTerrainConfig("id")` returns nil; with the Mission Editor open on a map
it returns the theatre id and every call below answers, so the editor alone is
enough for hook-side extraction. `probe-log-2.9.29.27278.md` holds
every measurement cited in this section.

Signatures from ED's own call sites **[install]**, all taking flat scalars in
DCS metres, `x` north and `y` meaning DCS **z** (east):

| Call | Site | Returns |
|---|---|---|
| `Terrain.GetHeight(x, z)` | `install:MissionEditor/modules/me_utilities.lua:1288-1290` | height in metres |
| `Terrain.isVisible(x1, alt1, z1, x2, alt2, z2)` | `install:MissionEditor/modules/me_utilities.lua:1292-1294` | boolean LOS |
| `Terrain.getClosestPointOnRoads(typeRoad, x, z)` | `install:MissionEditor/modules/me_mission.lua:6488` | `x, z` |
| `Terrain.findPathOnRoads(typeRoad, x1, z1, x2, z2)` | `install:MissionEditor/modules/me_mission.lua:6467`, consumed at `install:MissionEditor/modules/me_exportToMiz.lua:1560-1567` | array of `{x=, y=}` polyline points, or nil |
| `Terrain.FindNearestPoint(x, z, 40000.0)` | `install:MissionEditor/modules/me_mission.lua:6490` | `x, z` (legacy road-only fallback) |
| `Terrain.getObjectsAtMapPoint(x, z)` | `install:MissionEditor/modules/me_contextMenu.lua:118` | array of scenery objects |
| `Terrain.GetTerrainConfig(key)` | `install:MissionEditor/modules/me_terrainDATA.lua:19-34` | per key, see below |

`typeRoad` is `'roads'` or `'railroads'` **[install]**
`me_exportToMiz.lua:1556-1558`.

Measured behaviour and cost **[measured]** on Caucasus:

- `GetHeight(x, z)`: 0.0018 ms/call random, 0.00045 ms/call sequential. A
  50 m sweep of the Caucasus authored area (148 M cells) is about two minutes.
  Older measurements of 0.03 ms/call come from scattered mission-state
  `land.getHeight` calls; sequential access is what the sweep does, so plan
  on the sequential figure.
- `GetSurfaceType(x, z)` returns a **string**: `land`, `sea`, `lake`, `river`.
  No road or runway class. 0.113 ms random, 0.00065 ms sequential: sweep
  row-major or it costs 170× more.
- `GetSurfaceHeightWithSeabed(x, z)` returns `(height, 0)` on land and
  `(0, depth)` on sea with depth positive. It is the third channel of the
  fill test; no layer stores the depth.
- `isVisible(x1, alt1, z1, x2, alt2, z2)` is terrain-only: a 41 × 43 m
  warehouse between two points at 0.5 m AGL does not block it. It agrees with
  an offline straight-line check over `GetHeight` at 10 m on 300 of 300
  random pairs, 0.5 to 15 km, in hills.
- `getObjectsAtMapPoint(x, z)` is a point-in-footprint test (nil unless the
  point is inside an object's box), 0.004 ms/call. A hit carries `id`
  (a string of digits), `model` (lower-case), `type` (65536, 65537 or 131072 seen),
  `radius`, `rotation`, and four positional arrays: `sizeOBB[1], [2]`
  (`w, d`), `center[1], [2]` (`x, z`), `boxMin[1], [2]`, `boxMax[1], [2]`.
  No field is keyed `x`, `z`, `w` or `d`. Use it to fetch a footprint
  for an object you already know about, not to discover objects.
- `getClosestPointOnRoads` 0.26 ms/call random; `findPathOnRoads` 16 ms for a
  23 km, 161-vertex path, endpoints snapped to the road.

`GetTerrainConfig` keys ED uses: `id`, `Airdromes`, `defaultcamera`,
`SW_bound`, `NE_bound`, `defaultDateTime`, `standDescriptionVersion`,
`SummerTimeDelta`, `defaultBullseye`, `seaEnabled`. All answer from the hook
**[measured]**. `SW_bound` and `NE_bound` are `{x_km, 0, z_km}`; ED reads
`[1]` as x and `[3]` as z and multiplies by 1000 **[install]**
`me_map_window.lua:908-921`. Caucasus gives SW (−600, −560) km, NE (380,
1130) km: that is the raster rectangle, 980 × 1690 km, not the authored hull.
The authored hull is `nodesMapBorders` in `entry.lua` where present (Caucasus
in `entry.lua`; the other seven ship it in
`MissionGenerator/nodesMap.lua`, where it is the node-map image extent
and not the hull: Afghanistan's equals its bounds rectangle and Cold War
Germany's exceeds it **[install]**). Only the Caucasus value bounds the
authored area (x −418.6..26.4 km, z 113.7..943.2 km); elsewhere the
extractor's pre-sweep finds the hull (Geometry facts).

`Airdromes` is a table keyed by the numeric airdrome id (Kutaisi 25,
Batumi 22; the same number `Airbase:getID()` returns in the mission
state). Each entry carries `id` (a string name, `"KUTAISI"`), `code`
(ICAO), `display_name`, `names`,
`reference_point` (`{x, y}`, `y` is z), `reference_point_geo`
(`{lat, lon}`), `runways`, `runwayName`, `beacons` and `radio` (beaconId
lists), `towers`, `warehouses`, `fueldepots`, `shelters`
(`"externalId:NNN"` strings), `roadnet` and `roadnet5` (paths to the
airfield `.rn4`/`.rn5`),
`civilian`, `abandoned`, `class`. Prefer `reference_point` over
`Airbase:getPoint()`, which is wrong for some FOBs. The extract records
`abandoned`; consumers skip entries with `abandoned == true` as ED's own
editor does.

### The mission-scripting `land` and `world` API is secondary

Use it for two things only: `land.getSurfaceType` (the integer enum with
`ROAD = 4` and `RUNWAY = 5`, which the hook string lacks) and
`world.searchObjects` with `Object.Category.SCENERY`. Reach it with
`net.dostring_in("server", ...)` from the hook, or `dcs_eval_in` with
`state: "server"`. `land.*` takes Vec2/Vec3 tables, so the `y`-means-z trap
applies there and nowhere in the terrain module.

`world.searchObjects(Object.Category.SCENERY, {id = world.VolumeType.SPHERE,
params = {point = vec3, radius = r}}, function(obj, acc) ... return true end,
acc)` enumerates every scenery object in the volume **[measured]**: 50 196
objects in a 20 km sphere around Kutaisi in 134 ms; **919 641 objects, the
whole Caucasus, in a 600 km sphere in 1.34 s**. `obj:getTypeName()` is the
uppercase model name, `obj:getPoint()` the position, `obj:getDesc()` has
`life`, `category`, `typeName`, `displayName`. A scenery object has no
`getID`; `obj:getName()` returns its numeric id as a Lua number
(70741507), and `getObjectsAtMapPoint` returns the same id as a Lua
string (`"70741507"`) **[measured]**, so the two APIs join on the
digits and a comparison must `tostring` both sides. The mission state
does not return every object the hook sees: objects whose hook `type`
is 131072 (`wire`, `powertranspole_rail_01`, `kran-stroi`,
`kran_bash`, and `concrete_wall_01` by its absence from the
whole-theatre catalogue) are invisible to `world.searchObjects` in
every category, while every `type` 65536 and 65537 object is found
(74 of 74 in 9 km² of Kutaisi) **[measured]**. Wires, walls and cranes
are therefore absent from the dataset; they do not block DCS's own
line of sight and a heliport clears them, so nothing in the design
depends on them, and the `wall` class exists for theatres that may
expose them.

`land.getSurfaceType({x=, y=z})` returns the integer enum with `ROAD = 4` and
`RUNWAY = 5` **[measured]**, 0.00075 ms/call sequential, 0.078 ms scattered.
`land.getHeight` sequential costs the same as the hook call, 0.00045 ms.

**Trees are not reachable.** Wooded hillsides return zero scenery objects by
both methods; `land.getIP` cast downward returns exactly `getHeight` in woods,
in a city, and at a warehouse's own origin; `land.isVisible` and
`terrain.isVisible` ignore buildings. No API in `scripting`, `hook`, `gui`,
`export` or `config` names vegetation. Vegetation is out of the dataset. Land
cover for siting comes from terrain morphology (TPI, slope, roughness) and
from building density.

`land.profile(vec3, vec3)` returns irregularly spaced points (median 60 m,
range 0.2 to 290 m) whose heights differ from `getHeight` by up to 1.9 m,
from a coarser mesh, 5 ms per 10 km. It is not the height posts and is not
used.

### The map files

Every terrain binary shares one container **[install]**: bytes 0-3 version
`2`, bytes 4-7 header size `0x30`, bytes 8-15 payload size, and at offset
`0x20` a length-prefixed class name. Classes seen: `landscape5::Surface5File`
(`Surface/<Map>.surface5`), `landscape5::Scene5File` (`Scenes/<Map>.scn5`,
which lists scenery model names in plain text near the head),
`landscape4::lRoadNetwork` (both `roads/<Map>.rn4` and
`AirfieldsTaxiways/<Airfield>.rn4`), `landscape4::lRoutesFile`
(`roads/<Map>.routes`). The map-wide road network is
`Mods/terrains/<Map>/roads/<Map>.rn4` (Caucasus 203 MB, Syria 2.3 GB). It is
the same class as the airfield files that `getStandList` and `getRunwayList`
parse on demand, so a known-plaintext mutation oracle exists for both: hand
the engine a copied file, read the exact coordinates it returns, find those
floats in the bytes, flip one, feed it back, watch which output moves.

Plain Lua beside the binaries: `MissionGenerator/nodes.lua` (named regions with
positions), `Map/towns.lua` (town names with lat/lon), `notInstances.lua`
(scenery instance ids the map suppresses, ED's own authored scenery-removal
mechanism), `beacons.lua`, `radio.lua`, `entry.lua`. `terrain.cfg.lua` is
encrypted (`.pak.crypt`).

### Geometry facts

- `getTerrainShpare()` returns `"FLAT"` **[measured]**: no earth curvature, so
  an offline line-of-sight over the extracted heightfield reproduces DCS's own
  model up to interpolation detail.
- DCS x is north, z is east, metres. Every installed theatre is a
  transverse Mercator on WGS84 with `k_0 = 0.9996` and an integer
  `lon_0`, exact to 0.000 m on 20 points per map **[measured on
  2.9.29.27278; probe log, "Projection, bounds and fill"]**:

  | Theatre (`id`) | `lon_0` | easting offset | northing offset |
  |---|---|---|---|
  | Afghanistan | 63 | 300150 | 3759657 |
  | Caucasus | 33 | 99517 | 4998115 |
  | Cold War Germany (`GermanyCW`) | 21 | −35427.62 | 6061633.128 |
  | Marianas (`MarianaIslands`) | 147 | −238418 | 1491840 |
  | Marianas WWII (`MarianaIslandsWWII`) | 147 | −238418 | 1491840 |
  | Persian Gulf (`PersianGulf`) | 57 | −75756 | 2894933 |
  | Sinai (`SinaiMap`) | 33 | −169222 | 3325313 |
  | Syria | 39 | −282801 | 3879866 |

  Negate both offsets into `+x_0` and `+y_0` and PROJ's forward direction
  emits `(z_dcs, x_dcs)`:
  `+proj=tmerc +lat_0=0 +lon_0=63 +k_0=0.9996 +x_0=-300150 +y_0=-3759657
  +datum=WGS84 +units=m`. Kola and Nevada are not installed and not
  measured; `pack` fits any unlisted theatre from the extract's
  `latlon_samples` and verifies a listed one the same way.
- Every theatre's `SW_bound`/`NE_bound` rectangle, `defaultBullseye` and
  airdrome count are in the same probe-log table. `defaultBullseye` is
  `{0, 0}` on Persian Gulf and Marianas, so it is not a reliable campaign
  reference on every map.
- Queries outside the terrain return a per-map fill constant, not nil
  **[measured on 2.9.29.27278 from the hook, 500 km outside the bounds
  rectangle]**: height 5.000005 with surface `land` on Afghanistan, Caucasus
  and Cold War Germany; 10.00001 `land` on Persian Gulf; 0 `sea` with seabed
  100 on Syria, Sinai, Marianas and Marianas WWII. A point is fill when
  `GetHeight`, `GetSurfaceType` and the seabed return all equal the
  map's constants. Fill lies outside the bounds rectangle, not inside
  it **[measured, 1 or 2 km lattices over the whole rectangle]**:
  exact fill at 0 of 2 210 192 points on Afghanistan, 0 of 535 500 on
  Persian Gulf, 3 of 640 000 on Cold War Germany (real land at
  5.000005 m: 3 258 further points lie within 0.5 m). On Caucasus
  `nodesMapBorders` is smaller than the rectangle and contains a fill
  margin along its edges (11 843 of 368 905 points, 500 near-fill).
  On the sea-fill maps the seabed channel separates fill from
  in-bounds sea: in-bounds sea reads its real depth (63 to 96 m off
  Batumi, 2 571 m off Cyprus, 8 944 m in the Mariana Trench), the void
  reads exactly 100.000, and the test is exact equality of all three
  channels, not a depth threshold; on Syria, Sinai and Marianas the
  exact triple occurs at 0 of 189 000, 210 000 and 520 000 lattice
  points inside the bounds **[measured]**. The extractor tests each cell
  against the exact triple before rounding (after `floor(h + 0.5)` the
  fill is 5, like real 5 m land) and writes fill cells as `nodata`;
  `config.json.fill` records the triple; `pack` derives `valid` from
  `nodata`.
- The height raster is far larger than the authored map (Afghanistan: 1837 ×
  1413 km of posts around an authored hull of 552 × 798 km) and post spacing
  is 30 to 110 m inside the authored area, 250 to 1500 m outside
  **[measured on 2.9.28]**. On 2.9.29.27278 **[measured]** the second
  difference of `GetHeight` along a 4 km line sampled at 5 m is
  non-zero at almost every sample inside the authored area (Berlin 262
  of 799, Leipzig 410), so the interior mesh is finer than 5 m or the
  interpolation is not linear between sparse posts; outside the hull
  the breakpoints sit 250 to 1 085 m apart or vanish. The 50 m extract
  undersamples the interior either way, and the line-of-sight
  agreement figure is the measure that matters. The raster outside
  the hull is real, coarse terrain, so
  neither fill nor `nodesMapBorders` finds the hull; post density and
  roads do **[measured]**. Along a 2 km line sampled at 10 m, the
  count of samples where the second difference of height is non-zero
  is 100 to 170 inside the authored area (Kabul, Kandahar, Herat,
  Jalalabad, Berlin, Hamburg, Frankfurt, Leipzig, Bremen, Kerman) and
  0 to 35 outside it (Mazar-i-Sharif, Islamabad, the Turkmen and Tajik
  corners, Amsterdam, Prague, Muscat, Doha), with a road within 150 m
  inside and 50 to 800 km away outside. Flat authored ground reads
  few breakpoints (Dubai 0, road 403 m; Bandar Abbas 58), so the rule
  is: a 5 km cell is authored when breakpoints ≥ 60 or a road lies
  within 5 km. Detailed terrain without roads exists (Zahedan, Quetta:
  120 to 160 breakpoints, roads 37 to 80 km away) and counts as
  authored.

## Design decisions

Take these as decided unless a probe contradicts them.

**One base grid, derived layers offline.** Sweep DCS for the minimum: height,
hook surface string, mission-state surface enum; the seabed return is
read only for the fill test. Fixed 50 m
posts in the authored rectangle (Caucasus: 445 × 830 km → 8900 × 16600 =
148 M cells, 296 MB int16; Afghanistan authored 552 × 798 km → 176 M cells),
found by the pre-sweep (post density or road proximity on a 5 km
lattice) where the theatre ships no authored `nodesMapBorders`; tiles
that are entirely fill or entirely sea are not written.
At measured sequential cost that is about 2 minutes for height and 2 for
surface per theatre. Everything else — TPI, distance transforms, horizon,
summed-area tables, coarser pyramids — is computed in the pack step from
the extracted grid, and the cheap window layers (slope, aspect,
roughness, `tpi_300`) are computed at query time from the `height`
window and never stored. The extract is always 50 m; the packed base
is a pack decision, 50 m for theatres under 500 000 km² of authored
area and 100 m above, recorded in `meta`. Do not multi-resolution the
DCS sweep; multi-resolution the derived products.

**No visibility work inside DCS.** No `getIP` ray sweeps and no horizon
layer sampled through the API; the horizon is computed offline from the
heightfield. The world is flat and piecewise-linear, and the offline straight-line check
matched `terrain.isVisible` on 300 of 300 pairs, so the file's visibility
layers reproduce DCS's own. Compute the horizon at the resolution the siting
questions need (grid resolution or 2×), with 32 or 64 bins as a tunable, and
treat it as one derived product among several rather than the core
abstraction. Point-to-point visibility and radial viewsheds read height tiles
directly.

**Scenery from `world.searchObjects`, footprints from `getObjectsAtMapPoint`.**
Tile the bounds rectangle with 20 km spheres (or boxes) in the mission state,
collect `id`, type name and position for every object, then in the hook call
`getObjectsAtMapPoint(x, z)` at each object's own position to get its oriented
box. Store objects as rows with an R-tree and summed-area tables of
building and industrial counts at 100 m.
`BLK_LIGHT_POLE` is 30 % of all objects; class it as `misc` so it does not
dominate density. Vegetation is not in the data and the schema has no tree
class.

**Roads from the path oracle, reverse engineering bounded to one session.**
Build the road graph by seeding with `getClosestPointOnRoads` from a coarse
grid (1 km) plus every airdrome and town, then calling `findPathOnRoads`
between each seed and its k nearest seeds, unioning the polylines, and snapping
vertices within 1 m. Repeat with `'railroads'`. This yields exact road geometry
from the engine's own router. Spend at most one bounded session on `<Map>.rn4`
using the known-plaintext oracle; stop if the node table is not located.

**SQLite, stock, no application-defined functions.** Tiles as BLOBs of raw
int16/uint8 in row-major order, 64 × 64 cells (int16 8 KiB) so a window
read fetches few blobs; `page_size = 8192`. A blob spills to overflow
pages whatever its size (`grid_tile` is a `WITHOUT ROWID` index b-tree
with an in-page payload limit near 2 KiB), and `VACUUM` lays the
overflow chain out contiguously. The extract uses a
larger tile (256) for fewer files; `pack` re-tiles. `min_val`/`max_val` per tile.
Irregular data in normal tables with R-tree indexes. Open read-only with
`query_only`, `mmap_size` set. Registering host functions into the connection
binds the file to one host; keep the file pure data and put predicates in the
`dcsterrain` core crate, behind both the CLI and the MCP server.

**Region bound.** Canonical form is an axis-aligned box in DCS metres
`{minX, minZ, maxX, maxZ}`. Circle, polygon and airfield distance band are
convenience inputs `dcsterrain` converts to a box plus a post-filter predicate.
Every query takes the box.

**Query results.** Return a ranked, capped list of candidates with each
criterion's score kept as a separate field, and non-maximum suppression at a
caller-given spacing. Never return a scored grid over MCP. `dcsterrain query` may
expose the grid for the campaign's own filtering.

**Routing.** Store nodes and edges in SQLite with an R-tree on edges and an
index on node id. A route query loads the subgraph inside the endpoints' box
expanded by 25 %, runs A* in memory, and discards it. This is a bounded,
per-query cache, which satisfies "no eager theatre load". Contraction
hierarchies are not worth it at road-network sizes DCS ships.

**Immutable file plus campaign overlay.** The file describes the map as
shipped. The campaign keeps its own overlay of placed and destroyed objects and
consults both.

**Scenery clearing is measured** (probe log, "Scenery clearing by spawned
statics"). A heliport-category static (`FARP`, `FARP_SINGLE_01`) removes
every scenery object within about 150 m at spawn time, permanently; a
building static removes nothing. So for a FARP, buildings inside 150 m are
never a veto: they are the "expected clearing" cost, and the campaign
records the cleared objects in its overlay when it places the FARP. For a
building static, scenery inside the footprint is a hard veto (the objects
will interpenetrate). The scenery pass of an extract runs in a mission with
no heliports placed, since the terrain module and the mission share one
scene and a placed FARP would leave a hole in the extract.

Two Mission Editor trigger actions also clear scenery, reachable from a
hook through `net.dostring_in("mission", ...)` **[install]**
`MissionEditor/modules/me_trigrules.lua:2972-3005`:
`a_scenery_destruction_zone(zone, destruction_level 0..100)` and
`a_remove_scene_objects(zone, objects_mask)` with mask `0` ALL, `1`
TREES ONLY, `2` OBJECTS ONLY. Both take a trigger-zone name, so the
mission must define the zone. The `TREES ONLY` mask is the engine
admitting a tree layer it exposes no query for; it settles nothing about
reading vegetation but is the tool for a campaign that wants to flatten
a site before placing a building static. Neither is measured here.

**Two version stamps, not one.** The DCS core build (`autoupdate.cfg`)
and the terrain module's data are versioned separately by ED: a terrain
can be rebuilt under an unchanged core build, and that is what moves
heights, roads and scenery. The install carries no readable terrain
version string (`entry.lua` `version` is empty, `manifest.bin` is
opaque), so the terrain version is a fingerprint of the three data files
that matter (`.surface5`, `.rn4`, `.scn5`: size, container payload size,
SHA-256 of the first MiB). Every extract, packed file, query response and
file name carries both stamps; the consumer compares them to the install
it drives.

**Concealment is terrain masking plus building density.** With vegetation
unreachable, the two concealment channels are exposure (from the
heightfield) and built-up density (from scenery counts). Both are stored
separately and never fused in the file.

## Size budget

Estimates, uncompressed, from the grid list in `core.md`. Per 50 m cell: base
layers (height i16, surface u8, water u8, valid u8) 5 bytes; `tpi_2000`
at 200 m; distances at 100 m (four u16) 2 bytes per 50 m cell;
summed-area tables at 100 m (two i32) 2 bytes; horizon 32 bins at 200 m
2 bytes. About 11 bytes per 50 m cell, 400 000 cells per 1000 km²:
**about 4.5 MB per 1000 km² full, 2 MB base-only, at 50 m**; a quarter
of that at 100 m. The extract is 4 bytes per cell (height, water,
surface) plus JSON. Vector tables are
small beside the grids: 920 000 Caucasus scenery rows with an R-tree are
about 100 MB, road graphs tens of megabytes.

| Authored land area (estimate) | Theatres | 50 m full | 50 m base | 100 m full |
|---|---|---|---|---|
| under 5 000 km² land, sea tiles omitted | Marianas, Marianas WWII | < 100 MB | < 30 MB | < 30 MB |
| 350 000 to 450 000 km² | Caucasus (369 000 measured), Afghanistan (440 000 measured) | 1.6 to 2.0 GB | 0.7 to 0.9 GB | 0.4 to 0.5 GB |
| 600 000 to 1 000 000 km² | Cold War Germany, Persian Gulf, Syria, Sinai | 2.7 to 4.5 GB at 50 m; packed at 100 m by default: 0.7 to 1.1 GB | 1.2 to 2 GB | 0.7 to 1.1 GB |

Land area here is the authored area; sea and fill tiles inside it are
not stored, which is what makes Marianas and Persian Gulf small. The
extract directory holds the three base layers, so it is about 0.6 GB
for Caucasus raw; zipped, heights compress two to three times, so a
hand-off is 250 to 400 MB.

Two further levers exist if a file is still too large, neither taken:
coarser summed-area tables and horizon (200 m and 400 m, halving their
2 bytes each), and compressed tile blobs, which conflict with the goal
that any stock SQLite client reads the file.

## Worked tasks

Approach each as a campaign designer who has to settle placement with code.
Every task states the inputs, the computation against the file, and what goes
back to DCS.

### FARP for a helicopter forward base

Run live on Caucasus; numbers in the probe log.

Inputs: box = 20 km around Kutaisi airfield; footprint 120 × 80 m; height
span across the footprint ≤ 2.5 m at the best of two orientations; surface
`land`; 200 m to 3 km from a road; ≥ 3 km from the airfield; scenery objects
within 100 m = 0 (veto) and within 500 m as a cost; ground exposure from a
36-observer ring (12 bearings × 3, 6, 10 km at 2 m AGL) as a cost; spacing
2 km; limit 8.

Computation against the file: read `slope` tile min/max to skip tiles whose
minimum exceeds the limit; for surviving cells at 50 m, read the 4 × 4
`height` window and take max−min over the footprint corners at 0° and 90°
(add 45° and 135° when the grid is 25 m); `dist_road_band` and `dist_airdrome_band`
predicates (the `dist_road` grid, the `airdrome` table); `scenery_idx` box query then exact OBB overlap
for the veto; `sat_building` window for the 500 m cost; exposure by 36
`visible()` calls. Score = span/2.5 + road/3000 + 0.002 × scenery500 +
exposure; keep each term. Non-maximum suppress at 2 km.

Live result: 40 401 cells swept in 4.9 s from the hook, 26 085 passed the
geometric filters (the Rioni plain is flat). Of the top eight, candidate 2
at (−275487, 684259) has zero scenery within 500 m and exposure 0.42;
candidate 4 at (−283387, 679459) matches on exposure with a village 500 m
off. Candidate 3 has 14 objects inside 100 m and is rejected; 7 is exposed
to 78 % of observers.

Back to DCS: FARP static at `(x, z)` with heading = chosen orientation.
Record the placement in the campaign overlay. Report the building count the
footprint covers as "expected clearing".

### SAM site covering an airfield against low-level ingress

Run live on Caucasus; numbers in the probe log.

Inputs: asset = Kutaisi airfield; protection set = airfield centre plus 24
points on a 5 km ring at 100 m AGL; candidates 4 to 15 km from the asset,
surface `land`, road within 500 m; masking requirement = the site is not
visible from a 100 m AGL approach on the western arc at 10 km; spacing 5 km;
limit 5.

Computation against the file: `dist_road_band` and `dist_airdrome_band` predicates;
per candidate 25 `visible()` calls from a 6 m mast to the protection set and
7 to the western arc; rank by coverage then masking. `coverage()` on the
chosen site samples 72 radials × 20 ranges × 4 altitudes for the dead-zone
rings.

Live result: 1 979 candidates × 32 LOS in 0.9 s. Every candidate on the
plain covers 100 % of the ring, so masking decides: site 1 at
(−293887, 682059), 9.2 km south of the field, is masked from all seven
western approach points by a 108 m rise 2 km west of it and a 369 m ridge at
6 km on bearing 225. An ingress at 100 m AGL from 60 km due west is first
seen by site 1 at 7.8 km from the airfield, which is the unmasking-point
query answered on the same data.

Inverse: `coverage()` on the final site returns the dead-zone rings for the
briefing and for the campaign's own threat model.

### Concealed command bunker

Inputs: box = a province; ground observer set = all `dist_road ≤ 30` cells
at 2 m AGL within 3 km; air observer band = 300 m to 1500 m AGL on a 1 km
lattice within 15 km; maximum exposure fraction 0.15 for ground and 0.35 for
air; plausibility: `dist_road ≤ 2 km`, `dist_builtup ≤ 5 km`, slope ≤ 8°.

Computation: apply plausibility predicates from grids; for each candidate,
ground exposure = fraction of road cells within 3 km with `visible()` true;
air exposure = fraction of lattice points with `visible()` true. Keep two
scores separate: terrain masking (from exposure) and built-up cover
(building count in 150 m from `sat_building`, since a bunker in a town block
is hard to pick out and DCS has no vegetation to hide in). Rank on exposure;
report built-up cover as a second column so the designer chooses a valley
floor versus a town.

Inverse: `score_site()` on a hand-chosen position returns the same two
numbers.

### Convoy from depot to front

Inputs: from and to points; network `roads`; avoid = SAM coverage boxes;
exposure observers = enemy OPs; vehicle capability = wheeled (roads only).

Computation: `route()` loads the road subgraph in the expanded box, removes
edges inside avoid boxes, weights each edge by length × (1 + exposure
fraction along its geometry from `visible()` at 25 m samples), runs A*, and
emits the edge polylines as waypoints simplified to ≤ 1 per 500 m. Return
per-leg exposure so the campaign can time the move.

Back to DCS: waypoints with `action = "On Road"` per point, so DCS's own
router only fills the gaps between the file's vertices.

### Alternative routes for a recurring convoy

Inputs: the same endpoints, `k = 3`, `max_stretch = 1.3`, `max_overlap =
0.4`.

Computation: `route_alternatives()` by the penalty method: after each
accepted route, multiply the weight of its edges by 1.6 and search
again; accept a candidate when its stretch and its overlap with every
accepted route are within the limits; stop at `k` or when candidates
repeat. Return the set with each route's exposure and chokepoints, and
the edges all of them share.

Back to DCS: the campaign keeps the set and draws one per departure
(`using-the-data.md`, "A recurring convoy that should not be
predictable"). Only the drawn route's waypoints go into the mission.

### Ambush against that route

Inputs: the route; box = 2 km buffer; observer = the convoy at 2 m AGL every
200 m along the route; requirement: site sees ≥ 400 m of road, is not visible
from the route's approach side beyond 1.5 km, `dist_road` 100 to 600 m, slope
≤ 15°.

Computation: candidates at 100 m in the buffer; count visible route samples
within 1.5 km (`visible()`); compute masking from the approach direction by
`horizon` bin lookup along the route's incoming bearing; rank by visible road
length × masking. Return top 5 with the sector of road each covers.

### Unmasking point on an ingress

Inputs: a flight's route as a polyline with altitudes; a SAM site with radar
height.

Computation: sample the route at 100 m; `visible(site, sample)` in order;
the first true sample is the unmasking point. Return distance from the site,
and the list of masked segments if the route dips back below the horizon.

### Spawn visibility for fog of war

Inputs: a group's spawn point; the player's likely position and altitude.

Computation: one `visible()` call plus the building count within 100 m from
`sat_building`. Return boolean and the count. No DCS call.

### Choke points on a road network

Computation at pack time: betweenness centrality over `road_edge`. At query
time `chokepoints()` inside the box, which ranks edges by `betweenness`
and reports `crosses_water` (bridges) and the mean `tpi_300` along the
edge (defiles when strongly negative). Return the top edges with their
midpoints.

### Recovery landing sites along a route

Inputs: route polyline; helicopter footprint 30 × 30 m; slope ≤ 5°; surface
LAND; scenery objects within 60 m = 0; within 3 km of the route.

Computation: `find_sites()` in the route buffer with those criteria, spacing
5 km. Return a chain of sites.

### Trafficability of a point for a ground group

Computation: `nearest("road", x, z, 1)` distance, `slope` along the
straight line to the road sampled at 25 m, `surface` not WATER anywhere on
it. Return reachable/unreachable with the blocking sample.

## Choosing a site that is not the best

The file ranks; the campaign chooses. The best SAM site by coverage and
masking is often the least fun one, because "fun" is a property of the
player's approach to the site, not of the site. So every `find_sites` result
carries its full metric vector and the consumer selects from a band rather
than taking rank 1. The rules for that live in the campaign, but the file
must supply the metrics they need, and the ones below are cheap to add to
the SAM and FARP operations.

Player-facing metrics for a SAM candidate, all from `visible()` and the road
graph. When the mission prescribes a route they are read along it; when the
player may come from any bearing, which is the normal campaign case, they
are read around the compass with `approach_spectrum` and summarised as the
fraction of bearings with a masked way in, the widest such arc, and the
spread of unmask distances, with a second summary over the arc facing the
player's base:

- **Unmask distance per corridor**: where a flight at the corridor's
  altitude first comes into the site's line of sight. Large means the player
  sees it coming; small means a pop-up threat.
- **Blind approach exists**: whether any corridor at the player's altitude
  stays masked until inside the player's weapon standoff. This is the
  "solvable" check: a site with no blind approach at any altitude the mission
  allows is impossible without SEAD, and the campaign should either reject
  it or pair it with a SEAD flight.
- **Dead-zone fraction** of the engagement circle at low level, from
  `coverage()`. High means there is somewhere to hide inside the ring.
- **Terrain around the site**: mean and max relief within 5 km, from the
  `height` tiles. Rugged terrain gives the player options; a site on a plain
  gives none.
- **Egress cover**: nearest masked cell from the site's edge, so a player can
  break line of sight after a shot.

A difficulty band is a range on these. One workable mapping, to be tuned
against real sorties: *easy* = unmask distance ≥ 15 km on the primary
corridor and a blind approach at 500 m AGL; *medium* = unmask 6 to 15 km and
a blind approach at ≤ 200 m AGL; *hard* = unmask 2 to 6 km, blind approach
only at ≤ 100 m AGL; *unfair* = no blind approach at any allowed altitude,
or unmask under 2 km on every corridor. Player ability sets which band the
campaign draws from, and a per-player rating that moves up after a clean kill
and down after a loss keeps it adaptive. Draw from the band at random rather
than taking its top entry, or the player learns the generator.

The same shape applies to a FARP (how far from the front, how exposed to a
ground search) and to a concealed target (exposure fraction is directly the
difficulty of finding it). In each case the file's job is to return the
metric vector for every candidate above the hard constraints, and never to
collapse it into one score inside the file.

Helicopters need the same operations at different altitudes: a masked
approach is judged at 15 to 60 m AGL, and the decisive metric is the count
of pop-up positions inside weapon range that are hidden from the threat at
5 m AGL and see the target at 30 m AGL (`find_sites` with
`not_visible_from` and `visible_to` at those `own_alt_agl` values), plus the
share of them the threat cannot see at pop-up height and the distance to
egress cover. `using-the-data.md` defines every metric, gives the
difficulty bands for fixed-wing, helicopter, search, convoy and FARP tasks,
and works the helicopter case on the live Caucasus SAM example.

## Coordinate rules

DCS x is north, z is east, metres. The terrain module takes `(x, z)` scalars.
`land.*` takes Vec2 `{x, y}` where `y` is DCS z, or Vec3 `{x, y, z}` where `y`
is altitude. Convert once at the bridge boundary and never pass a table to a
terrain-module call. Geo conversion for the file's `meta` comes from
`terrain.convertMetersToLatLon(x, z)`; verify the projection table above
against 20 sample points before stamping `crs_proj4`.
