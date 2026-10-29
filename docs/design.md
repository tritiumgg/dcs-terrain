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
  airfield ──┤ ─────────────────────▶                     radio, towns, nodes, CRS
  Lua files ─┘        │                   roads.graph     graph, geometry, water (9–137 MiB)
                      │                   roads.betweenness per edge (< 2 MiB)
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
  them. Measured on all eight terrains, 2.9.30.28738: every triangle of each
  material subset carries its bit. Many carry a second, which the order
  settles: about half of a `Beach` subset is shoreline that also carries a
  water bit, and a sixth of GermanyCW's `Field` subset and a third of the
  Marianas' also carry the forest bit, so read as forest. Forest
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
it with; failing that, the `N` of the `airfieldN_k` beacons nearest it
within 15 km, when every beacon of `N` lies nearest it; failing that, the
`N` of a `Radio.lua` entry whose scene object, placed by the `.scn5`, lies
nearest the box of its runway ends and stands, within 1 km of it, when
every such object of `N` does, through the scene's cached index; a scene
no decoder reads costs those ids, not the tables. No join gives an id
another airdrome or a label holds. The files hold no other source of the
id: the game's list is in a `.crypt` file and follows no order of the
files. On 2.9.30.28738 this leaves 8 of Nevada's 17 airdromes, 2 of
PersianGulf's 30 and 2 of Syria's 80 with a runway with a null id, and
those are named by `name_id` (ADR 0005; `data/airdrome-ids.md`). Its
beacons, radio and display name follow from its id; the display name is
its first beacon's, which on Caucasus is the game's for 18 of 21
airdromes. Its reference point is the mean of its runway ends, within
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
| `tables.json` | Airfields, stands, Lua tables, and the fitted projection each cost a full decode; together they are small | 0.1 MB (MarianaIslands) to 2.4 MB (GermanyCW), read back in 1–10 ms | 47–450 ms, after `scene.nodes`, which it reads to place the towers |
| `roads.graph` | The `.rn4` has no spatial order: a 3.1 s scan on GermanyCW, and the water under 23 M samples | 8.6 MiB (Caucasus) to 137 MiB (GermanyCW) | 0.5 s (Caucasus) to 9.8 s (GermanyCW) |
| `roads.betweenness` | Betweenness is global | 0.04 MiB (Caucasus) to 1.9 MiB (GermanyCW) | 0.4 s (Caucasus) to 29 s (GermanyCW) on two threads |
| `places` | Settlements need every building on the terrain (§6.3) | 6–24 k rows, < 2 MiB | a whole objects-pool walk, 3.8 s (Caucasus) to 53 s (GermanyCW), plus 0.2–8 s grouping |
| `horizon/` | 3.4–4.9 s per 20 × 20 km region on one core, 0.24 s for one cell in a new area (Caucasus) | 8.4 KiB a tile; about 290 MiB (Caucasus) / ~740 MiB (Afghanistan) if every tile were touched | per tile, the first time a query touches it; never by `index` |
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
  is rebuilt. A switch to an older build and back re-stamps or rebuilds
  each time.
- A missing entry, or one whose key or format version does not match, is
  rebuilt before the operation that needs it, and only that entry: `describe`
  needs `mesh.toc` and `tables.json` and reports the other entries' counts as
  null until they exist; `sample` needs `mesh.toc`, plus the entry behind any
  layer it asks for (`roads.graph` for `dist_road`, `scene.nodes` for counts);
  `route` waits for the graph.
- Betweenness and `places` are built at low priority in the background. Until
  they exist, `chokepoints` and `towns` return an error saying they are still
  being built.
- Index builds run at low priority on at most two threads. A query spreads
  its large work (a viewshed's radials, a horizon batch, the tiles under a
  window, the blocks under scattered points or a sightline) over every core,
  below normal CPU priority, and keeps small work on the calling thread;
  results merge in a fixed order, so they do not depend on how the work
  falls. `--threads N` caps both. The tool must not take the CPU from a DCS
  server beside it, and below normal priority it yields to one (ADR 0004).
- `horizon/` is a directory of one file per tile, named `<tx>-<tz>`, each
  opening with the entry header and keyed on the surface file alone, so a
  changed road or scene file leaves it. A tile is checked as an entry is,
  against one stat of the surface file per process. It is written under a
  temporary name in the terrain's directory and renamed into `horizon/`.
  After an update a tile is re-stamped when it is next read, the surface
  file's header read once for all. `cache list` reports the directory's
  total size; `clear` deletes it with the rest.
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

Computed over a rectangle of 50 m cells from one window padded by the
layer's reach, never stored. A radius takes the cells whose centers lie
within it of the cell's center, and an object or instance counts in the
cell that holds it. The times are for a 20 × 20 km region around an
airfield on each of the eight terrains, one thread, on 2.9.30.28738, with
the region's tiles held and the scenery index and road graph built:

| Layer | Definition | Per 20 × 20 km region |
|---|---|---|
| `valid` | 1 where height is not `nodata` and the cell is not sea | 0.3 ms |
| `slope`, `aspect`, `roughness` | Horn 3 × 3 gradient in degrees; downslope azimuth from north, clockwise, undefined under 0.5°; max − min height in 3 × 3; undefined where any of the nine has no height | 1.9–4.8 ms |
| `tpi_300`, `tpi_2000` | height − mean of the known heights within 300 m, 2 km, at 50 m | 4.0–4.4 ms; 17–19 ms |
| `dist_road`, `dist_rail`, `dist_water`, `dist_builtup`, distance to a `cover` class | Euclidean distance (Felzenszwalb–Huttenlocher) between cell centers to a cell a road or rail edge passes through, any water, a built-up cell (3 or more buildings within 100 m), or a cell of the class | 1.6–57 ms; road and rail scan every edge of the graph, built-up reads the buildings within 5.1 km |
| building and industrial counts | objects of the class within a radius | 5.6–28 ms at 1 km |
| tree and line density | instances of the trees or lines pool per hectare within a radius | 2.7–224 ms at 300 m, the most on the Marianas' forests |
| `cover` fractions | share of the covered cells within a radius that are of a class | 3.9–4.0 ms at 300 m |
| radar floor, exposure | one viewshed sweep per site (`queries.md`) | 89–111 ms per 20 km site cold; `exposure` 35–160 ms a site (`data/exposure.md`) |

The first read of a region's scenery costs more: up to 0.56 s for
Afghanistan's buildings within 5 km of Herat and 0.91 s for the trees
around Agana.

A distance layer searches out to a cap: the criterion's own maximum where it
has one, else 5 km. A cell with nothing within the cap reads as the cap, and a
response that used a capped distance says so.

A radius or cap may be any length. Past the farthest a cell of the region lies
from a tile of the grid it takes in nothing more, so the window is padded only
that far; a density still divides by the area asked for. This assumes no road
or scenery lies off the grid's tiles. What is bounded is the work: the padded
window at 25 million cells, about 250 km square and 1 GB while derived, and a
radius layer at 2 × 10⁹ disk rows summed, one per row of each cell's disk, a
few seconds. A layer over either is refused, naming the size, so the asker
shrinks the region or the radius.

### 6.2 Horizon

Per 200 m cell (4 × 4 grid cells), the maximum elevation angle to the
terrain in each of 32 azimuth bins, the first due north and the rest
clockwise, for an observer 2 m above the ground at the cell's center, out to
40 km. Each bin is one ray along its center line, sampled every 25 m,
bilinear between the four cell centers around the sample; a sample with any
of the four outside the mesh is passed over. An angle is coded in quarter
degrees from −10° (0) to 53.5° (254), and 255 marks a cell whose center is
outside the mesh. A max pyramid over the heights skips any square that cannot
raise the angle found so far, which leaves the answer exactly what a march of
every sample gives; a brute-force march on a small grid checks this.

A horizon tile holds the 16 × 16 horizon cells of one grid tile, 8 KiB.
Tiles are computed the first time a query touches them and cached in
`horizon/`; up to 8 × 8 tiles share one window of heights, the tiles and
40 km around them, about 9 MiB as 16-bit heights plus 12 MiB of pyramid. One
call computes at most 64 tiles, about a 25 km square, from at most four
windows, and refuses more before computing anything, naming the count. `enclosure` is the mean of the bins in
degrees, undefined where the horizon cell's center is outside the mesh. On
Caucasus on 2.9.30.28738 a 20 × 20 km region around an airfield (54 tiles)
first costs 3.4–4.9 s on one core, the low ground the slowest because little
is skipped. Reading the heights in range cold takes 0.17 s of that. From the
cache it costs 7–10 ms, and one cell in a new area costs 0.24 s.

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
source: the override file and its reason, or what the rule says of the model.
The class, in order: a per-terrain override (`classes/<terrain id>.toml` in
the core crate, built into the binary, else `classes/common.toml`; a key
matches a model in any case); `effect` for a model named `smoke#…` — 17 143
objects 200–600 m tall on Caucasus, smoke columns not structures; `wall` when
`min(w, d) < 1 m` and `max(w, d) ≥ 4 m`; `misc` under 4 m²; `industrial` by a
name pattern list; else `building`. An override names a class and, when it
wants one, a family.

A `building` or `industrial` model also has the settlement family of §6.3 (a
model of another class has one only when its override names one):
house, apartment, shop, civic or industrial, which make places, and
outbuilding, farm or military, which do not; a model whose name says it is a
vehicle, bridge, mast, light, sign or the like has none, and a building no
rule names is `unknown`. The family rules are names, first match wins, from
the settlements research. A family of house-like models whose names do not
say which (`german_town_*`, `syria_house*` and the like) is split by its box:
under 30 m² an outbuilding, a narrow box under 100 m² and 14 m or taller a
house, 14 m or taller (or over 400 m² and over 10 m) an
apartment block, else a house, an urban name tipping a mid-size box to
apartments. A wreck (`…crash`) takes the family of what it was.

The classes are made in memory the first time a terrain's scenery is asked
for, never cached: 0.8 ms (MarianaIslandsWWII, 304 models) to 27 ms (Syria,
15 289) on 2.9.30.28738. Every height, clearing, placement, count and place
answer reads scenery through the one path that drops `effect` objects. The
catalog lists every model with its class, family, source, box and how many
objects place it, most placed first, the tallest beside it, and the override
keys that name no model; counting reads the objects pool's ids and one model
word per object, 54 ms (Caucasus) to 3.4 s (GermanyCW) with the files in the
OS cache, 0.6 s on Caucasus without. `dcsterrain check` prints it, so a new
terrain's top models can be reviewed in minutes.
Only Caucasus has overrides; on the other terrains a bridge or a parked
vehicle large enough is a `building` until its terrain's file is written,
though its name already keeps it out of every place.

### 6.5 Road graph

Built from the `.rn4` by junction id (§3.2): roads and rails are two networks
that share no node, and a node where exactly two edges of one class meet and
run on within 1° is contracted. Each edge has its polyline, class, length
(the polyline and the gaps from its ends to its nodes, so a straight line
between nodes never exceeds a path) and `crosses_water`: any 25 m sample
under a water triangle that is not a road strip, even where a bridge's road
strip wins the point. These are `roads.graph`, keyed on the road and surface
files; the water is read a surface block at a time, each block under the
roads prepared once with its water triangles alone, so a sample tests the
few near it. Class names include tunnels (`tunnel_hose`,
`roki_tunnel_road`).

Betweenness is its own entry, `roads.betweenness`, built from the graph so a
route never waits for it: Brandes over each network's largest component from
at most 2 000 sources spread through it, on at most two threads at
background priority on Windows, normalized 0..1 by the network's largest.

The road file is read once, front to back, through a 16 MiB window, never
whole and never mapped: on 2.9.30.28738, OS cache warm, the scan takes
0.20 s and 62 MiB at peak on Caucasus against 0.23 s and 240 MiB reading it
whole, 3.5 s and 0.65 GiB on GermanyCW against 3.9 s and 3.3 GiB, the
difference what the whole file holds. On Caucasus both water rules, the
water under a road strip and the winning triangle's, mark the same 799 of
8 764 road edges. GermanyCW's 1.52 M road stretches contract to 465 k
edges, and its road and rail graph has 357 k nodes.

## 7. The command line

- `dcsterrain query [--install <dir>] --terrain <Terrain> <op> <json>` answers one
  request. `--install` defaults to the registry value on Windows. A terrain
  whose projection misses a beacon by more than 2 m is refused unless
  `--allow-crs-residual` is given (§3.5).
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
- `dcsterrain check [--install <dir>] --terrain <Terrain> [--top <n>] [--json]`
  reports each terrain file's class and version and why no decoder reads it
  when none does, each cache entry's state as found and the horizon tiles
  computed so far, the tables with the shared scripts the Lua files ran and
  the names they left unresolved, the airdromes left without an id and the
  projection's fit,
  the class catalog's totals, its `n` models placed most (25 by default) and
  its 10 tallest structures, and the last validation figures. It reads the
  tables and the scenery as a query would, building `tables.json` and the
  scene index if the cache lacks them, and never the road graph. A file no
  decoder reads refuses only what reads its role, so the rest is still
  reported. It exits 1 naming each problem: a file no decoder reads, a part
  that cannot be read, a fit over 2 m. 0.4 s on
  Caucasus with an empty cache on 2.9.30.28738.

A skill in this repository teaches an assistant with a shell which operation
answers which question, by example, with `dcsterrain schema` as the reference
and the metric meanings it needs to turn numbers into difficulty. There is no
MCP server: the warm state a server would hold lives in the cache. A thin MCP
wrapper over the same operations can be added if a client without a shell
needs one.

## 8. Targets

NVMe, cold OS cache unless marked; on one thread unless a figure says it is
from the query benchmark, which spreads large work over every core of an
8-core machine (ADR 0004, `data/query-bench.md`). Each is a benchmark, not a
correctness rule: a miss is a decision about the target.

| Operation | Measured | Target |
|---|---|---|
| open / `describe` | open 2.6 / 4.2 / 5.4 ms (Caucasus / GermanyCW / Afghanistan) and 4.9 ms on Syria from the cache, OS cache warm, on 2.9.30.28738; on GermanyCW finding the files and reading the surface header take 2 ms, and reading and decoding `mesh.toc` 1 ms; query benchmark, OS cache warm, open 1.9 / 3.5 ms (Caucasus / GermanyCW) | 20 ms |
| `sample` 10 k lattice | 10–12 ms; over the block with the most runway triangles, OS cache warm, blocks held: 4.3 / 13.0 / 11.9 ms (Caucasus / GermanyCW / Afghanistan) against 2.8 / 5.1 / 3.0 ms over a block of like size with no runway, the gap the airfield's denser ground, its runway test 0.2–1.1 ms | 100 ms |
| `sample` 10 k scattered over a whole terrain | 8.5 / 25.9 / 2.7 s at the 32 MiB block budget, 0.7 / 13.5 / 1.2 s sorted by tile Z-order, 0.7 / 2.9 / 1.3 s with each block prepared once (Caucasus / GermanyCW / Afghanistan); block by block, a block holding 32 points or fewer tested triangle by triangle rather than binned, 0.33 / 1.4–1.6 s (Caucasus / GermanyCW, OS cache warm, 2.9.30.28738), and 10 k points over 2 km on MarianaIslands 74 ms; query benchmark 0.075 / 0.18 s warm, 1.0 / 1.7 s cold (Caucasus / GermanyCW) | 3 s on GermanyCW |
| `visible` 10–40 km | over the mesh's triangles, 1–4 ms over 8–38 km on Caucasus in a fresh process, OS cache warm; over 2000 lines of 1–40 km, p50 0.60 / 0.37 ms and p99 9.3 / 30 ms (Caucasus / GermanyCW), 2.9.30.28738; query benchmark over 1–40 km, p50 0.35 / 0.30 ms warm within a batch, 4.3 / 6.5 ms cold in a fresh process | 1 ms within a batch, 20 ms cold |
| one `query` call end to end, OS cache warm | 42–48 ms for `visible` on Caucasus, 2.9.30.28738; query benchmark 8.5 / 10.6 ms for `visible`, 11.3 / 19.1 ms for `describe` (Caucasus / GermanyCW) | 50 ms for `describe`, `sample`, `visible` |
| `viewshed`, `radar_floor` 20 km | 89–111 ms; `radar_floor` 20 km 91–303 ms median per terrain over eight terrains at 5 threads, the mesh out to 1 km and GeoJSON bands included, MarianaIslandsWWII 3 ms over the target (`data/radar-floor.md`); `viewshed` 120 ms at 20 km and 644 ms at 50 km on Caucasus, OS cache warm, on 2.9.30.28738; query benchmark `viewshed` 20 km 15 / 12 ms warm, 23 / 14 ms cold (Caucasus / GermanyCW) | 300 ms |
| `find_sites` 20 × 20 km, geometric | 15–24 ms; a FARP's ten criteria every 100 m, 57–205 ms over the 8 terrains of 2.9.30.28738, tiles read, 5 threads (`data/find-sites.md`) | 500 ms |
| `exposure`, 10 sites of 20 km | 7 sites of 20 km 0.24–1.2 s warm, 3.4–9.3 Gcycles, over eight terrains at 5 threads, on 2.9.30.28738 (`data/exposure.md`) | 3 s |
| `find_sites` with one LOS criterion, 36 observers | not measured | 10 s |
| `find_sites` with a horizon criterion, first touch | 3.4–4.9 s for the horizon of a 20 × 20 km region, 7–10 ms once cached (Caucasus, 2.9.30.28738) | 5 s, then 500 ms |
| `route` 50 km | 2.5 / 13 ms; 100 pairs of 2–80 km on each of 8 terrains, the search 0.05–1.6 ms p50 and the whole operation 4.9–43 ms p50, 90 ms p95 at most, on 2.9.30.28738 (`data/route-search.md`) | 500 ms |
| `validate_placements`, 1 000 placements | ~2 s cold on Germany estimated | 3 s |
| `point_info`, `airfield_info` | µs to a few ms | 20 ms cold |
| cache build, all but betweenness and places | < 2 s; roads 0.5 / 9.8 s (Caucasus / GermanyCW, 2.9.30.28738), 3.1 s of it the GermanyCW scan and 4.6 s its water | 10 s |
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
  river on a 200 m bridge, a second over the col between the hills and a
  third east of the river, both crossing the first, and a loop west of the
  river from the second to the first, so a route to the bridge has an
  alternative and the bridge lies on the one edge between two junctions and
  carries the most betweenness; a
  railway 3 km south of the first; a 2.5 km runway; a town cluster and an industrial
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
4. **The radial sweeps step the 50 m raster**, rounded to the meter.
   *Settled for `visible`:* it walks the mesh's triangles (ADR 0003); the
   raster agreed on 99.8 % of lines to a target 2 m up, but hid 3.5–4.3 % of
   lines to a target on the ground (`data/sightline-exact.md`). *Settled
   for `viewshed` and `radar_floor`:* their sweep crosses the mesh out to
   1 km and steps the raster beyond (ADR 0006); the floor's p99 error at
   20 km fell from up to 330 m to up to 41 m; the mesh all the way, within
   9 m, cost up to 1.2 s a sweep (`data/radar-floor.md`).
5. **The horizon cache's growth under real use** is not measured; every
   tile of Caucasus would take about 290 MiB. The cold cost is measured
   (§6.2). *Settles it:* the skill's use, watched through `cache list`.
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
    terrain in N8 and Q15, and each terrain's catalog reviewed into its
    override file; only Caucasus has one.
12. **Low-level routes are deferred.** `exposure` maps where the sites
    cannot see; `low_level_route` is not built. Wires, pylons and masts are
    not modeled, and the specified request ignores speed. *Settles it:* a
    user asking for routes; `plan.md` says what the first version takes.
13. **Assistants without a shell** cannot use the tool. *Settles it:* a user
    who needs one; an MCP wrapper is then its own task.
