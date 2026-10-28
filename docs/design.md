# Design

What `dcsterrain` is, how it reads a DCS install, and what it keeps beside it.
The operations it answers are in [`queries.md`](queries.md); the byte layouts
of the files it reads are in [`data/file-formats.md`](data/file-formats.md).

This document is not frozen. Edit it when the design changes, in the pull
request that changes the code. A decision that overturns something here and
needs its reasoning kept goes in [`decisions/`](decisions/README.md) as an ADR,
and this document is updated to match.

Measured figures are from DCS **2.9.30.28738** on one machine (Ryzen 7 7800X3D,
32 GB, NVMe, Windows 11) unless marked otherwise. The research behind them is
on the branches `claude/query-install-directly-a8d685` (`research/no-pack.md`),
`claude/research-new-queries-2f4ba7` (`research/new-queries.md`,
`research/settlements.md`) and `claude/shrink-storage-28d662`.

## 1. What it is

A command-line tool that answers terrain siting and routing questions about a
DCS World terrain — where a FARP fits, what a SAM site sees, how a convoy gets
from A to B unseen — by reading the terrain's own files from the DCS install,
read-only, in place. DCS does no terrain work at campaign runtime, and need
not be running.

Beside the install it keeps one small **cache directory per terrain and build**
holding only what cannot be computed per query. Deleting the cache loses
nothing; the next query rebuilds what it needs.

The consumers are campaign tools, scripts, and an AI assistant with a shell.
There is one interface, `dcsterrain query`, JSON in and JSON out. An assistant
learns it from a skill shipped in this repository.

The repository ships tools only. Users run them against their own install.

## 2. Architecture

```
DCS install (read-only)                 cache/<terrain>/           (rebuildable)
  .surface5 ─┐                            mesh.toc        block offsets (0.05–1.5 MB)
  .scn5   ───┤                            scene.nodes     node offsets, models, pools (0.35–25 MiB)
  .rn4    ───┤   decoders                 tables.json     airdromes, runways, stands, beacons,
  airfield ──┤ ─────────────────────▶                     radio, towns, nodes, CRS, classes
  Lua files ─┘        │                   roads.graph     graph, geometry, betweenness (9–200 MiB)
                      │                   places          settlements (< 2 MiB)
                      │                   horizon/        one file per tile, filled lazily
                      ▼                   validation.json the last live validation figures
              Source (trait)  ◀──────── Synth (in memory, closed form)
                      │
         50 m tiles on demand, point mode, scenery boxes, road graph
                      │
              query engine (operations, criteria, derived windows)
                      │
              dcsterrain query (JSON in, JSON out)
                      │
        campaign tools, scripts, an assistant with a shell (via the skill)
```

The engine reads terrain through a `Source` trait with two implementations:

- **`Install`**: the decoders over a DCS install, plus the cache.
- **`Synth`**: a closed-form synthetic terrain held in memory (§9), the terrain
  named `synth`. Every operation is tested against it.

Operations, criteria, line of sight and derived windows are written once
against the trait. `grid::Window` and `grid::sample` read tiles and do not care
where a tile came from.

## 3. Reading the install

### 3.1 Finding a terrain

- The install root on Windows comes from the registry value
  `HKCU\Software\Eagle Dynamics\DCS World\Path`; elsewhere it is given with
  `--install`. No path is recorded in this repository.
- A terrain is a directory under `Mods/terrains` with an `entry.lua`; a directory
  without one is passed over. Its name is the **id** that `entry.lua`
  declares, not the folder name: Germany's folder is `GermanyColdWar` and its
  id `GermanyCW`. `entry.lua` is read as text and never run: of every
  assignment to an `id` key the shallowest wins, and `update_id` is not an
  id. Every per-terrain key — `--terrain`, the
  cache directory, per-terrain class overrides, the projection table — uses the
  id. An unknown name is refused with the list of ids the install holds.
- Each terrain file is the one file in its directory whose name ends in its
  extension, matched without regard to case: `.surface5` under `Surface`,
  `.rn4` under `roads`, `.scn5` under `Scenes`. None, or more than one, refuses
  the terrain.
- `.crypt` files are never opened. Nothing is ever written into the install.

### 3.2 Decoders

Every ED terrain file starts with a container header naming a class and a format
version. Each decoder declares the (class, version) pairs it reads, and the
engine picks the decoder from the pair. Opening a terrain reads the surface
file's header only; every other role is classified the first time an entry or
operation needs it: the airfield, assets and Lua files when `tables.json` is
keyed or built, the scene file for scenery, the road file for the graph
([ADR 0001](decisions/0001-read-file-headers-per-role-when-first-needed.md)).
**A pair no decoder declares refuses everything that reads its role**, naming
file, class and version, and nothing else: an unknown taxiway file refuses
`tables.json` and the operations behind it, not heights or sightlines, and
`list-terrains` and `check` report it. An unknown surface pair refuses the
terrain. Other terrains keep answering.

After decoding, each decoder checks its own output against the ground and the
terrain's extent — road vertices, scenery, stands and runway ends lie on the ground
and inside the mesh, and counts are non-zero — when the cache entry is built.
A failed check refuses the terrain the same way.

| File | Yields | Settled rules |
|---|---|---|
| `.surface5` (`landscape5::Surface5File`) | Finest-mesh triangles: height, water, ground type | §3.3 |
| `.rn4` | Road and rail polylines with class names and node ids | A node id names a **junction**, not a point: an edge end's id joins it to other ends with the same id. An id of `0xFFFFFFFF`, or one whose ends over the terrain's roads and rails do not all lie within 10 km of their mean, joins nothing. The router's own points lie within 2 m of a decoded road edge and 3 m of a rail edge at p90 |
| `.scn5` | Scenery: objects, trees and lines pools, each over its own quadtree, and the model table | §3.3. An instance id splits at bit 13 when every node holds at most 8 192 instances, at bit 15 when the largest holds at most 32 768; a larger node refuses the file. At least 99 % of 1 000 sampled ids per pool, split that way, must land within 0.5 m of the pool's own box for that id |
| `.rn5`, `assets.assets` | Airfields: runways, taxiways, stands | A named node that is not a runway end is a stand only when its class has a stand record in `assets.assets`. A node with no class at all is a stand with null class fields; no installed terrain has one |
| `Beacons.lua`, `Radio.lua`, `Map/towns.lua`, `MissionGenerator/nodes.lua` | Beacons, radio, towns, mission nodes | §3.4 |

### 3.3 The ground

- **Point mode.** A point's height, water, surface and cover are the winning
  finest-mesh triangle's own. This is the ground `land.getHeight` returns.
- **Precedence where triangles overlap:** a road strip (`TRITYPE & 8`) wins,
  then a water triangle (`TRITYPE & 7`), then the higher block index, then the
  higher triangle index. Without the road rule the Roki tunnel area is 729 m
  off: a road strip runs under a mountain triangle and the game reports the
  road. Without the water rule, lake triangles over land read as land.
- **Water class:** `TRITYPE & 4` river (3), else `& 1` sea (2), else `& 2`
  lake (1), else land (0).
- **Surface:** 1 land, 3 water, 4 road (a road-strip triangle), 5 runway
  (inside the `Runway` material subset, the one subset the decoder reads); 2 is
  never written.
- **Ground cover** comes from the higher `TRITYPE` bits, first match wins:
  water; `runway` (32); `town` (64); `forest` (1024); `field` (128); `beach`
  (256); else `plain`. Road and rail strips keep the bit of the ground under
  them. Measured on Caucasus, 2.9.30.28738 (new-queries research): each bit
  matches its material subset on 94–99 % of the subset's triangles. The order
  settles the few mixed triangles by the subset they fall in. Forest
  concealment reads bit 1024, never the trees pool: only 9.8 % of instance
  trees stand on forest ground, because DCS draws its forests at runtime from
  the ground type. Cover reads no material subset.
- **Coverage.** A point is `nodata` only where no finest-mesh triangle covers
  it. Flat land at 5 m is land at 5 m; no fill value is tested. The sea is
  covered by sea triangles, so a sightline across a bay finds no holes.
- **Regional operations** read 64-cell tiles at 50 m holding height, water,
  surface and cover, rasterized on first use from the triangles under them
  with the same precedence, at each cell's center, and kept in a tile LRU.
  Height is `floor(h + 0.5)` of the f32 interpolated height. There is no
  coarser base.
- **Positional reads.** The engine reads only the block arrays a query touches,
  prepares each block (vertices to world coordinates, its triangles and its
  runway triangles binned, so a point over an airfield tests a few) into a
  small LRU, and never memory-maps a whole terrain file: on Windows that costs
  about 2 MiB of commit per GiB mapped (123 MiB for Germany's `.surface5`).
- **Scenery is read the same way.** `scene.nodes` keeps where each
  instancer node's arrays lie. A box descends each pool's own tree, tests
  the boxes each meeting node owns, and reads each hit's position, model
  and axis at its offset, through 4 MiB of 16 KiB chunks. Objects also take
  each group record whose model box, turned by its heading, meets the box.
  Measured on 2.9.30.28738, 2 km boxes over all three pools: a whole-file
  map of GermanyCW's `.scn5` added 15.5 MiB of commit, the chunks 4.6 MiB;
  warm p50 0.056 ms mapped against 0.38 ms read, cold p50 4.5 ms against
  7.8 ms. On Caucasus, cold p50 1.6 ms, p95 8.3 ms.

Measured against the live Caucasus extract (2.9.29.27468), whole terrain: heights
exact on 99.9485 % of 141.19 M cells and within 1 m on 99.99859 %, worst 29 m
in lake clusters; water class on all but 4 cells. Raster height against the
mesh at 100 000 random points per terrain (no-pack research): p50 0.17 m, p99
2–3 m overall and 4–8 m on steep ground, worst 159 m where surfaces overlap.
That gap is why points use point mode.

### 3.4 The Lua files

Each runs in a sandboxed Lua 5.1 built into the binary, in a fresh environment
holding the basic library without `pcall`, `load*`, `getfenv`, `setfenv`, `io`,
`os` or `debug`, and `string` without `find`, `match`, `gmatch`, `gsub` or
`dump`. Only source is loaded. `dofile` runs a file of the install, found
without regard to case and never outside it. `require("i_18n")` returns a
`translate` that returns its argument; any other module is an empty table and
is reported, as is a read of an unassigned global. A run stops after
200 000 000 instructions or 512 MB; values come back as nil, booleans, numbers,
strings and tables, at most 1 000 000 of them, nested at most 64 deep.
`Radio.lua`'s labels are read from the comment on the line before each
`radioId`.

An airdrome is keyed by its `name_id`, the basename of its `.rn4`, across
every source that names it. Its id is the one `Radio.lua`'s comment labels
it with, and its beacons, radio and display name follow from that id; the
display name is its first beacon's, which on Caucasus is the game's for 18 of
21 airdromes. Its reference point is the mean of its runway ends, within
3 mm of the game's on Caucasus, whose airdromes have one runway each.
`tables.json` does not carry the game's names by language or the scene
objects it ties to an airdrome: towers, warehouses, fuel depots, shelters.

### 3.5 The projection

A transverse Mercator (`k_0 = 0.9996`, WGS84), fitted per terrain from every
beacon that has both `position` and `positionGeo`. Each whole-degree central
meridian within 12° of the beacons is scored by the RMS of the residuals about
the median offsets; for the best, pairs 5 m or more off are dropped and the
offsets are the means of the rest, rounded to the millimeter. There is no fit
with fewer than 3 distinct pairs before or after that cut, or with longitudes
spanning more than 30°; the terrain then takes its parameters from the built-in
table (`crs.rs`), and a terrain in neither is refused. Fitted when `tables.json`
is built. A fit that misses a beacon it kept by more than 2 m refuses the
terrain unless `--allow-crs-residual` is given; every installed terrain fits
within 0.071 m on 2.9.30.28738.

## 4. The cache

### 4.1 Where and what

`<cache>/<terrain>/`, where `<cache>` is `%LOCALAPPDATA%\dcs-terrain\cache`
on Windows, `$XDG_CACHE_HOME/dcs-terrain` (else `~/.cache/dcs-terrain`) on
Linux, `~/Library/Caches/dcs-terrain` on macOS, or `--cache <dir>`. Never in
the install, never in the repository. One directory per terrain holds the
entries of whichever build was installed when each was last checked; each
records its build in its key, and survives an update that leaves its files
alone ([ADR 0002](decisions/0002-keep-one-cache-directory-per-terrain.md)).
Whether an entry there is current is decided from metadata (§4.2), never by
opening the install files it was derived from, except once after an update.

| Entry | Why it cannot be per query | Size | Build |
|---|---|---|---|
| `mesh.toc` | The `.surface5` table of contents can only be pattern-scanned: 21 ms (MarianasWWII) to 0.7 s (GermanyCW) per open | 0.05–1.5 MB, every block's arrays and its runway subsets, the one material read | < 1 s |
| `scene.nodes` | Parsing the `.scn5` head costs 2 s and 559 MiB on Germany | 0.35 MiB (MarianaIslands) to 24.5 MiB (GermanyCW): each node's array offsets, the model table, the group and the pools | 0.05–1.8 s |
| `tables.json` | Airfields, stands, Lua tables, the fitted projection, scenery classes and any imported hook tables each cost a full decode; together they are small | 0.1 MB (MarianaIslands) to 2.4 MB (GermanyCW), read back in 1–10 ms | 54–560 ms |
| `roads.graph` | The `.rn4` has no spatial order: 5.5 s and 3.3 GB to scan on Germany. Betweenness is global | 9.3 MiB (Caucasus) to 200 MiB (Germany) | 7 ms–5 s; betweenness 1 s (Caucasus) to ~250 s CPU (Germany) |
| `places` | Settlements need every building on the terrain (§6.3) | 6–24 k rows, < 2 MiB | a whole objects-pool walk, 3.8 s (Caucasus) to 53 s (GermanyCW), plus 0.2–8 s grouping |
| `horizon/` | 0.1–0.2 ms per site, 0.9–4 s per 20 × 20 km region on one core | up to 282 MiB (Caucasus) / ~720 MiB (Afghanistan) if every tile were touched | per tile, the first time a query touches it |
| `validation.json` | Written by the live validation sortie, not computed | KiB | — |

### 4.2 How

- Every entry but the two JSON files is a raw little-endian file starting with
  a header: an 8-byte magic, the entry's u32 format version, its key as
  length-prefixed text, and the u64 length of the payload that follows. The key
  covers only the install files of the roles the entry reads (§5), so a
  changed road file leaves `mesh.toc` alone. It is memory-mapped read-only;
  mapping a 200 MiB entry costs about 0.4 MiB of commit.
- `tables.json` cannot open with that header and stay JSON, so it is one
  object whose first three members are the version, the key and the payload,
  and a check reads it only as far as the payload.
- A key is checked from metadata alone: a stat of each file of a single-file
  role, one directory listing with size and mtime for a directory role, so a
  file added to `AirfieldsTaxiways` is seen, and the build. The class, version
  and payload size are read from the headers when the entry is built and
  recorded in its key. Checking an entry over GermanyCW's 454 airfield files
  costs 0.5 ms; reading their headers cost 40 ms. A header that changes while
  its file's size and mtime stay the same goes unseen until the build changes;
  no ED patch is known to do that.
- An entry is written under a temporary name and renamed into place, so two
  processes building it at once leave one valid copy. On NTFS with current
  Rust the tool's own handles, open or mapped, do not block a rename over an
  entry; another process's handle opened without delete sharing, such as an
  antivirus or backup tool's, and a FAT, exFAT or network volume still can. A
  rename that fails still succeeds when the file already there carries the
  same key.
- **After an update.** An entry whose key names another build has the
  headers of its install files read again, and when its key matches in
  everything but the build it is re-stamped: the same payload is written
  under the key with the installed build, as a build is written. Otherwise it
  is rebuilt. `tables.json` is always rebuilt, because its hook import is
  bound to one build. A switch to an older build and back re-stamps or
  rebuilds each time.
- A missing entry, or one whose key or format version does not match, is
  rebuilt before the operation that needs it, and only that entry: `describe`
  needs `mesh.toc` and `tables.json` and reports the other entries' counts as
  null until they exist; `sample` needs `mesh.toc`, plus the entry behind any
  layer it asks for (`roads.graph` for `dist_road`, `scene.nodes` for counts);
  `route` waits for the graph.
- Betweenness and `places` are built at low priority in the background. Until
  they exist, `chokepoints` and `towns` return an error saying they are still
  being built.
- Index builds run at low priority on at most two threads; queries on one.
  `--threads` overrides both. The tool must not take the CPU from a DCS server
  beside it.
- **No SQLite.** The road graph beat packed road tables (2.5 ms vs 3.7 ms cold
  on Caucasus, 13 ms vs 47 ms on Germany), the scenery pool carries its own
  loose quadtree, and nothing else is tabular enough to need a database.
- `dcsterrain index` builds a terrain's entries ahead of time; it is optional.
  `dcsterrain cache list` and `clear` manage the directory; `list` names the
  build each terrain's entries were made on. `clear` asks before deleting
  `validation.json`, which reports the build its figures were measured on,
  and `--yes` deletes it without asking. Nothing is deleted without a
  command, `clear` deletes nothing under a directory with no `CACHEDIR.TAG`,
  and it follows no link. The cache tags its root only
  when the root is new or empty, and never makes one inside the install, so a
  mistyped `--cache` cannot empty somebody's folder. `query`, `list-terrains`,
  `index` and `cache` take `--cache`.

### 4.3 The hook's tables

Only the running sim knows some airdrome fields: `code`, `civilian`,
`abandoned`, `class`, the game's `display_name`, and the `id` where
`Radio.lua` labels none (every airdrome of Nevada and PersianGulf, and about
half of GermanyCW's and Syria's, on 2.9.30.28738). The optional GameGUI hook
in `extractor/` writes them to an `airdromes.json` beside its
`manifest.json`. `dcsterrain index --install <dir> --terrain <Terrain> --hook <dir>`
merges that file into `tables.json`, joining on `name_id` and filling exactly
those fields; an id other than `Radio.lua`'s is refused. A hook directory
whose manifest names another terrain, `dcs_build` or build timestamp is
refused, naming each difference, before anything is built. The merged file
records the hook directory; when `tables.json` is rebuilt the merge is redone
from it if it still exists and still matches, and otherwise the fields go
back to null and `check` says the import was dropped.

## 5. Versions and the response stamp

- **The core build is the version**: `version` and `timestamp` from
  `autoupdate.cfg` at the install root, read when a terrain is opened, never from
  memory. ED ships terrain only inside DCS updates, so the build is the
  version; the terrain files are fingerprinted as a record of what answered.
- **The cache key** for an entry is the build, plus for each source file its
  path, size and modification time, which a check compares (§4.2), and the
  class, version and payload size (the u64 at bytes 8–15 of the header) read
  when the entry was built. A build that differs while the rest matches
  re-stamps the entry rather than rebuilding it (§4.2). A repair that
  rewrites identical files costs a rebuild of seconds.
- **A batch re-checks** the build and the keys before each request (a few
  `stat`s) and reopens a terrain whose files changed. Terrain files are opened with
  delete sharing so a patch can replace them under a long batch.
- **Every response carries** `terrain` (the terrain id), `dcs_build`,
  `terrain_fingerprint` — the decimal sizes of `.surface5`, `.rn4` and `.scn5`
  joined by `-`, which needs no hash — `cell_size` (50) and `elapsed_ms`.
- **Validation figures** (height RMSE, LOS agreement, route ratio,
  `validated_at`) come from `validation.json` and are returned by `describe`.

## 6. Derived data

### 6.1 Per query

Computed from a window padded by the layer's reach, never stored:

| Layer | Definition | Per 20 × 20 km region, one thread |
|---|---|---|
| `valid` | 1 where height is not `nodata` and the cell is not sea | — |
| `slope`, `aspect`, `roughness` | Horn 3 × 3 gradient in degrees; downslope azimuth from north, clockwise, undefined under 0.5°; max − min height in 3 × 3 | 2.4–7.4 ms |
| `tpi_300`, `tpi_2000` | height − mean height within 300 m, 2 km | 1.6–9.5 ms; 1.8–3.3 ms |
| `dist_road`, `dist_rail`, `dist_water`, `dist_builtup` | Euclidean distance (Felzenszwalb–Huttenlocher) to road or rail edges, any water, or any 100 m cell holding 3 or more buildings within 100 m | 0.8–3.2 ms each |
| building and industrial counts | objects of the class within a radius, from a bucketed point list | 0.2–0.5 ms |
| tree and line density | instances per area from the trees and lines pools | as counts |
| `cover` fractions and distances | share of a class within a radius; distance to a class | 1–3 ms |
| radar floor, exposure | one viewshed sweep per site (`queries.md`) | 89–111 ms per 20 km site cold |

A distance layer searches out to a cap: the criterion's own maximum where it
has one, else 5 km. A cell with nothing within the cap reads as the cap, and a
response that used a capped distance says so.

### 6.2 Horizon

Per 200 m cell, the maximum elevation angle to the terrain in each of 32
azimuth bins, for an observer 2 m above ground, out to 40 km. Computed per tile
the first time a query touches it and cached in `horizon/`; `enclosure` is the
mean over the bins. A first siting query in a new area pays 0.9–4 s on one core
for a 20 × 20 km region, plus reading the surrounding window cold (not
measured).

### 6.3 Places

A **place** is a set of settlement buildings — houses, apartments, shops,
civic and industrial buildings — each within 200 m of another (the UK
statistics office's built-up-area rule). Garages, sheds, kiosks, farm and
military buildings and effect volumes are not grouped. Each place of 3 or more
buildings has a stable id, center, box, building count, residential floor area
(footprint × storeys), apartment share, a size class and a form:

| Size class | Rule |
|---|---|
| hamlet | 3–49 buildings |
| village | 50–199 |
| large village | 200+, apartment share under 5 %, residential floor under 1 M m² |
| town | 200+, apartment share 5 % or more, or residential floor 1 M m² or more |
| city | residential floor 3 M m² or more and apartment share 10 % or more |
| large city | residential floor 20 M m² or more |

Form is `agglomeration` when a place holds several `towns.lua` names,
`dispersed` at 1 000 or more buildings with an apartment share under 8 % and
low floor density, else `compact`. A `towns.lua` point names the place with the
most buildings within 1 km of it; a place with several names keeps its largest
member's. Outlines are made on request: the place's buildings buffered by
100 m, rasterized at 50 m, traced and simplified.

Why this way, measured on Caucasus against 47 towns' census populations: the
200 m rule ranked best of six methods (rank correlation 0.73); the mesh's town
ground merges whole rural districts and cannot be an outline; and building
counts do not track people (Tbilisi has 9 351 buildings for 1.1 M people), so
classes are in the terrain's own units, which is how a place looks in the sim.

### 6.4 Scenery classes

Rules over the `.scn5` model table, per terrain, each assignment recording its
source. In order: a per-terrain override (`classes/<terrain id>.toml`, or
`classes/common.toml`); `effect` for a model named `smoke#…` — 17 143 boxes
100–300 m tall on Caucasus, smoke columns not structures; `wall` when
`min(w, d) < 1 m` and `max(w, d) ≥ 4 m`; `misc` under 4 m²; `industrial` by a
name pattern list; else `building`. The settlement families of §6.3 refine
`building`. No height, clearing, placement, count or place answer includes an
`effect` object. `dcsterrain check` prints the catalog sorted by count, with
the tallest models, so a new terrain's top models can be reviewed in minutes.

### 6.5 Road graph

Built from the `.rn4` by junction id (§3.2), with degree-2 nodes whose edges
are collinear within 1° contracted. Each edge has its polyline, class, length,
`crosses_water` (any 25 m sample on water) and betweenness (Brandes on the
largest component, sampled from 2 000 sources on large graphs, normalized
0..1). Class names include tunnels (`tunnel_hose`, `roki_tunnel_road`).

## 7. The command line

- `dcsterrain query [--install <dir>] --terrain <Terrain> <op> <json>` answers one
  request. `--install` defaults to the registry value on Windows.
- With no `<json>`, `query` reads JSON lines from stdin, one request per line,
  each naming its `op`, and writes one response line per request. A batch
  opens the terrain once and keeps its block and tile caches across requests.
- Every operation that returns geometry takes `geojson: true` and adds a
  GeoJSON `FeatureCollection` in longitude and latitude, for drawing the
  answer in any map program (`queries.md` §1.1).
- `sample` and `scenery_in` take a `max_rows` request field (defaults 10 000
  and 5 000); over the cap is an error, never a truncation. Errors are a list of `{code, message, path}` issues; an
  operation returns a full result or an error.
- `--terrain synth` answers from the synthetic terrain with no install, with
  `--seed`, `--size-km` and `--cell`.
- `dcsterrain list-terrains [--install <dir>] [--json]` lists every installed
  terrain: its id, its folder, whether a decoder reads each of its terrain
  files, and what its cache holds and for which build. It reads only
  each `entry.lua` and the file headers, needs no cache, and is how a user
  finds the id `--terrain` takes.
- `dcsterrain schema` prints every operation's request and response schema.
- `dcsterrain index`, `dcsterrain cache`, as §4.
- `dcsterrain check [--install <dir>] --terrain <Terrain>` reports each terrain file's
  class, version and decoder, the cache state, the class catalog and the last
  validation figures, and exits 1 naming any file no decoder reads.

A skill in this repository teaches an assistant with a shell which operation
answers which question, by example, with `dcsterrain schema` as the reference
and the metric meanings it needs to turn numbers into difficulty. There is no
MCP server: the warm state a server would hold lives in the cache. A thin MCP
wrapper over the same operations can be added if a client without a shell
needs one.

## 8. Targets

Single thread, NVMe, cold OS cache unless marked. Each is a benchmark, not a
correctness rule: a miss is a decision about the target.

| Operation | Measured | Target |
|---|---|---|
| open / `describe` | open 2.6 / 4.2 / 5.4 ms (Caucasus / GermanyCW / Afghanistan) and 4.9 ms on Syria from the cache, OS cache warm, on 2.9.30.28738; on GermanyCW finding the files and reading the surface header take 2 ms, and reading and decoding `mesh.toc` 1 ms | 20 ms |
| `sample` 10 k lattice | 10–12 ms; over the block with the most runway triangles, OS cache warm, blocks held: 4.3 / 13.0 / 11.9 ms (Caucasus / GermanyCW / Afghanistan) against 2.8 / 5.1 / 3.0 ms over a block of like size with no runway, the gap the airfield's denser ground, its runway test 0.2–1.1 ms | 100 ms |
| `sample` 10 k scattered over a whole terrain | 8.5 / 25.9 / 2.7 s at the 32 MiB block budget, 0.7 / 13.5 / 1.2 s sorted by tile Z-order, 0.7 / 2.9 / 1.3 s with each block prepared once (Caucasus / GermanyCW / Afghanistan) | 3 s on GermanyCW, answered block by block: each block prepared once and the results merged by rank |
| `visible` 10–40 km | 5.4–12 ms cold, 0.01–0.05 ms warm | 1 ms within a batch, 20 ms cold |
| one `query` call end to end, OS cache warm | not measured | 50 ms for `describe`, `sample`, `visible` |
| `viewshed`, `radar_floor` 20 km | 89–111 ms | 300 ms |
| `exposure`, 10 sites of 20 km | ~1 s estimated | 3 s |
| `find_sites` 20 × 20 km, geometric | 15–24 ms | 500 ms |
| `find_sites` with one LOS criterion, 36 observers | not measured | 10 s |
| `find_sites` with a horizon criterion, first touch | 0.9–4 s plus a cold window | 5 s, then 500 ms |
| `route` 50 km | 2.5 / 13 ms | 500 ms |
| `low_level_route` 100 km, 10 sites | not prototyped, 1–3 s estimated | 5 s |
| `validate_placements`, 1 000 placements | ~2 s cold on Germany estimated | 3 s |
| `point_info`, `airfield_info` | µs to a few ms | 20 ms cold |
| cache build, all but betweenness and places | < 2 s; roads ≈ 5 s on Germany | 10 s |
| private memory per query process | 15–45 MiB | 64 MiB |

Accuracy:

| Answer | Target | Basis |
|---|---|---|
| Point height | within 0.5 m of live `terrain.GetHeight` at 200 points | the mesh is the ground DCS uses |
| Water class | 100 % at cell centers | measured |
| `visible` vs `terrain.isVisible` | ≥ 99 % of 300 pairs | live validation |
| Route length vs `findPathOnRoads` | within 5 % | live validation; not yet compared |

## 9. Testing

- **The synthetic terrain first.** `Synth` is closed form: a plane rising 1 %
  to the north; two Gaussian hills (600 m and 250 m, sigma 1.5 km); a circular
  lake of 2 km radius, flat at 100 m; a straight river 60 m wide; sea beyond a
  straight coastline in the south-west; one road west to east crossing the
  river on a 200 m bridge, a second over the col between the hills, a railway
  3 km south of the first; a 2.5 km runway; a town cluster and an industrial
  pair of scenery objects and a line of poles; a patch of each cover class; a
  2 km margin of `nodata`. Every derived quantity has a closed-form
  expectation. Its constants live in one place.
- **Decoders** have offline tests on byte buffers the test builds, and an
  opt-in group gated on `DCSTERRAIN_INSTALL` that reads a real install. When
  `DCSTERRAIN_EXTRACT` names the live Caucasus extract (written by the old hook
  on 2.9.29.27468), the opt-in group also compares against it through the
  extract reader, which survives as a test oracle only.
- **Live validation** (task V1) needs a running DCS on the terrain: point heights,
  `visible` against `terrain.isVisible`, route lengths against
  `findPathOnRoads`, written to `validation.json`.
- No fixture cut from an ED file is ever committed.

## 10. Risks and open questions

Each names what settles it.

1. **DCS's share mode on its terrain files.** If DCS opens them denying
   readers, the tool cannot run beside a live server. *Settled:* it does not;
   with Caucasus loaded on 2.9.30.28738 every terrain file opens for read,
   even sharing read only (`data/live-gate.md`).
2. **OS file-cache sharing with DCS.** Decides a benefit, not viability.
   *Settled:* DCS reads through the cache and maps `.surface5` and `.scn5`
   read-only, but keeps only 1.5 % of each resident, all standby; the road
   network is fully resident (`data/live-gate.md`).
3. **A new ED format version** stops that terrain's queries until a decoder
   ships; a silent layout change gives wrong answers live, with no last-good
   copy. *Mitigation:* the class-and-version refusal, the decoders' checks at
   cache build, and live validation after each DCS update.
4. **`visible` steps the 50 m raster**, 4–8 m off at p99 on steep ground,
   where grazing sightlines are decided; `radar_floor` inherits it. *Settles
   it:* N11, an exact sightline over the mesh.
5. **Horizon cold cost** and the cache's growth under real use are not
   measured. *Settles it:* N9.
6. **One machine, one disk.** A hard disk or network share could be several
   times slower cold. *Settles it:* the opt-in benchmark on a user's disk.
7. **macOS and Linux need the terrain's files**, 7 GB (Caucasus) to 72 GB
   (Germany). Development there runs on `Synth`. *Settles it:* a user asking;
   an export to one file is then its own design.
8. **CPU beside DCS.** Betweenness is minutes of CPU on Germany, the places
   walk about one. *Mitigation:* low priority, two threads, once per build.
9. **Road graph fidelity.** Built from junction ids; route lengths not yet
   compared with `findPathOnRoads`. *Settles it:* V1.
10. **Per-call cost.** Every `query` call is a fresh process. *Settles it:*
    the end-to-end benchmark; batches are the fallback.
11. **Cover bits and place sizes are measured on one terrain.** On Caucasus the
    town subsets cover 46 468 km² but bit 64 only 27 110 km², unexplained;
    a forest edge follows coarse triangles (13 per km² p50). The size classes
    were fitted on 47 Caucasus towns; some building families (`korpus_*`,
    `airfields_building_*`) are uncertain. *Settles it:* the opt-in checks per
    terrain in N8 and Q15, and the per-terrain class review in N7.
12. **Low-level routes ignore obstacles.** Wires, pylons and masts are not
    modeled, so `low_level_route` clears terrain only and says so. *Settles
    it:* a user asking for obstacles.
13. **Assistants without a shell** cannot use the tool. *Settles it:* a user
    who needs one; an MCP wrapper is then its own task.
