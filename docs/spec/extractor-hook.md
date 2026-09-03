# Spec: extractor hook

A GameGUI hook that sweeps a loaded DCS theatre and writes an extract
directory in the format of `extract-format.md`. Lua 5.1, one file,
runs on the Windows machine with DCS. Background and every measured
figure: `design-and-facts.md`, `probe-log-2.9.29.27278.md`. Write
it with the `dcs-scripting` skill loaded and check every symbol against
`dcs-api-lookup` before calling it.

## Files

- `Scripts/Hooks/DcsTerrainExtract.lua` in Saved Games: the hook. Loads
  on every DCS start, does nothing unless enabled.
- `Config/DcsTerrainExtract.lua` in Saved Games: a Lua table the hook
  `dofile`s at load and re-reads on every start. Absent file means
  disabled.
- Progress log: `Logs/DcsTerrainExtract.log` in Saved Games, one line per
  tile and per phase change, plus `dcs.log` `INFO` lines at phase changes
  only.

Nothing is ever written into the DCS install.

## Config table

```lua
return {
  enabled = true,
  output_dir = "C:/extracts/caucasus-50m",   -- created if absent
  cell_size = 50,                             -- metres; always 50, coarser bases are pack's choice
  tile_size = 256,
  frame_budget_ms = 5,                        -- work per onSimulationFrame
  crop_m = nil,                               -- or {min_x=, min_z=, max_x=, max_z=}
  authored_bounds_m = nil,                    -- nodesMapBorders, if known
  omit_sea_tiles = true,
  road_seed_spacing = 1000,
  road_seed_neighbours = 4,
  passes = { hook = true, mission = true },
  allow_helipads = false,                     -- run the scenery sweep with helipads present
  towns_lua = "C:/Program Files/Eagle Dynamics/DCS World/Mods/terrains/Caucasus/Map/towns.lua",
  nodes_lua = "C:/Program Files/Eagle Dynamics/DCS World/Mods/terrains/Caucasus/MissionGenerator/nodes.lua",
  crs = nil,                                  -- optional {proj4=..., lon_0=..., ...}
  terrain_dir = "Caucasus",                   -- directory under Mods/terrains/
}
```

The hook validates every field at load and logs one line per problem;
any problem disables the run.

## Lifecycle

Register with `DCS.setUserCallbacks`. The hook is a state machine driven
by `onSimulationFrame`, and `onMissionLoadEnd`, `onSimulationStop` and
`onSimulationStart` move it between states:

1. **idle**: waiting for a terrain. Every 60 frames, `require("terrain")`
   then `GetTerrainConfig("id")`; nil means no terrain yet. Non-nil
   means the editor has a map open or a mission is running. On
   2.9.29.27278 `onSimulationFrame` fires at the main menu and in the
   Mission Editor, and no callback fires during a mission load; which
   callbacks fire where is a per-build measurement, so the hook logs the
   frame count reached in idle.
2. **prepare**: read config; when `crop_m` and `authored_bounds_m` are
   both nil, run the pre-sweep: a 5 km lattice over the bounds
   rectangle, and per cell the post-density line (201 `GetHeight` calls
   at 10 m along x) and one `getClosestPointOnRoads` snap (nil where no
   road is reachable; a nil snap is "no road"), sliced under
   the frame budget (about 88 000 cells on Afghanistan; the editor
   streams terrain on first touch, so budget on the order of a
   minute), whose authored-cell bounding rectangle plus 10 km becomes
   `authored_bounds_m` (`extract-format.md`, "manifest.json");
   compute the grid (`extract-format.md`,
   "manifest.json" rules), create directories, write `config.json`, and
   either load an existing `manifest.json` from `output_dir` to resume
   (same theatre, build, grid, else refuse and log) or write a fresh
   one.
3. **hook pass**: the sweeps listed below that need only the `terrain`
   module, in order. Runs whether the terrain came from the editor or a
   mission.
4. **mission pass**: the sweeps that need the mission-scripting state,
   through `net.dostring_in("server", ...)`. Enters only when
   `DCS.getModelTime()` advances (a mission is running) and
   `passes.mission` is true; otherwise the hook writes
   `passes.mission.complete = false` and finishes.
5. **done**: log totals, write the final manifest, stop registering work.
   Stays done until DCS restarts.

Every sweep is a resumable iterator over tiles. The frame callback runs
iterator steps until `frame_budget_ms` is spent, measured with
`os.clock()`, then yields. A tile is written whole: samples accumulate in
a table of strings, concatenated once, written to `<path>.tmp`, renamed,
then one line appended to `tiles.jsonl` (`extract-format.md`,
"manifest.json"). The manifest is rewritten at the end of each sweep
and at every phase change, never per tile. On resume, tiles in the
journal are skipped.

## Hook-pass sweeps

In this order; each one's completion is recorded in the manifest before
the next starts.

**config**: `GetTerrainConfig` for every key listed in the design doc,
`getTerrainShpare`, 20 `convertMetersToLatLon` samples on a 4 × 5
lattice inside the grid, and the `fill` triple: `GetHeight`,
`GetSurfaceType` and `GetSurfaceHeightWithSeabed` at three points
500 km outside the bounds rectangle. Write `config.json`. In
**prepare**, before this, read `autoupdate.cfg` at `lfs.currentdir()`
for `dcs_build` and `dcs_build_timestamp`. Then compute
`terrain_fingerprint` per `extract-format.md` from the theatre
directory under `Mods/terrains/` (the directory name comes from config
as `terrain_dir`; it is not always the `id`, e.g. `SinaiMap` lives in
`Sinai`): file size from `lfs.attributes`, header and head from one
`read` of 1 MiB (9 ms), then SHA-256 in pure Lua as a sliced iterator
under the frame budget, because the hook state runs about 17 million
simple Lua operations per second and the three hashes take several
seconds in total. Both go into the manifest; a resume refuses when
either differs from the existing manifest.

**airdromes**: iterate `GetTerrainConfig("Airdromes")` with `pairs`;
the table key is the numeric `id` and the entry's `id` field is
`name_id`; write `airdromes.json`. For each entry with a `roadnet`, call
`getRunwayList(roadnet)` and
`getStandList(roadnet, {"SHELTER","FOR_HELICOPTERS","FOR_AIRPLANES",
"WIDTH","LENGTH","HEIGHT"})`; write `runways.json` and `stands.json`
(`getRunwayHeading(roadnet)` returns the first runway's `course` and is
not called).
Then `getBeacons()` and `getRadio()` to `beacons.json` and `radio.json`.
Then `towns_lua` and `nodes_lua` through a sandbox: `loadfile` the
file, `setfenv` the chunk to a fresh table whose `require` returns
`{translate = function(s) return s end}` (the files call
`require("i_18n")` and then `gettext.translate`) and whose `_` is the
identity, run it, and read the globals it sets (`towns`,
`missionNodes`) from that table; write `towns.json`
and `nodes.json`. One frame per table is fine; these are small.

**water, height**: two separate sweeps over the tile grid in
tile order `tx` outer, `tz` inner, and within a tile row-major. Sequential
access is what makes `GetSurfaceType` cheap (0.00065 ms versus 0.113 ms
scattered), so never interleave layers within a tile. `water` calls
`GetSurfaceType(x, z)` and maps the string; `height` calls
`GetHeight(x, z)`. Encode per `extract-format.md`. Each sweep applies
the fill test of `extract-format.md` ("Tile binary layout") on
the unrounded value: it compares its own call's return with the fill
triple first and makes the other two calls (`GetHeight` or
`GetSurfaceType`, and `GetSurfaceHeightWithSeabed`) only on a match, so
the extra cost falls on fill cells alone; a fill cell is encoded
`nodata`. The `water` sweep runs first and builds the skip set: a tile
whose every cell is fill is not written and joins the set; when
`omit_sea_tiles` is set, a tile that is entirely `2` is still written
for `water` and joins the set. The `height` and `surface` sweeps skip
the set, and the roads sweep places no seeds in it. Order: `water`,
`height`.

**roads, railroads**: seeds = lattice at `road_seed_spacing` over the
grid, plus airdrome reference points and towns; for each seed call
`getClosestPointOnRoads(kind, x, z)` and write the `seed` line. Then for
each seed find its `road_seed_neighbours` nearest seeds by snap point
(a simple grid bucket lookup), and for each unordered pair not yet
requested call `findPathOnRoads(kind, x1, z1, x2, z2)` with the snap
points and write the `path` or `nopath` line. `findPathOnRoads` costs
about 16 ms per 23 km path, so make path calls under the frame budget
like every other step: one call is a step, and a step may exceed the
budget by one call. A path between snaps about 1 km apart costs 0.61 ms
mean (200 calls near Kutaisi: 166 under 1 ms, 8 above 5 ms, maximum
18 ms), so about eight calls fit a 5 ms budget. Caucasus at 1 km seeds
is roughly 370 000 seeds and 700 000 paths before merging: about
7 minutes of call time, about 25 minutes at 60 frames per second.
Seeds whose snap points lie within 100 m of another seed's are merged
before pairing (126 of 441 near Kutaisi, where 309 of 441 snapped more
than 500 m), and no seed is placed in a skip-set tile.

## Mission-pass sweeps

Both run in the `server` state through `net.dostring_in`. Follow the
`dcs-scripting` `references/states.md` rules: escape injected source with
`%q`, treat every return as an untrusted string, namespace any global the
chunk leaves behind under `DcsTerrainExtract_`, and declare and verify
the length of every returned payload. The chunk does the work and
returns one string; the hook decodes it. `net.dostring_in` returns two
values, the chunk's string and a boolean; on a Lua error the string is
the error message and the boolean is false. A 3 MB return costs 18 ms.
Never send a chunk before `DCS.getModelTime()` has advanced. A
mission-pass step is one chunk and exceeds `frame_budget_ms` by design;
the budget applies between steps.

**surface**: per tile, one chunk that loops the tile's cells calling
`land.getSurfaceType({x = x, y = z})` and returns the `tile_size²` bytes
as a string. A full 256 × 256 tile costs 147 ms measured; use
quarter-tiles (64 rows) so a step is about 40 ms. Tiles in the
skip set are not swept. Caucasus is about 2 250 tiles, about
six minutes of call time.

**scenery**: one chunk per sphere of radius 15 km centred on a 20 km
lattice covering the grid (a lattice of spacing `s` needs radius
≥ `s/√2` for full coverage), returning one line per object:
`getName()` (the numeric id, a Lua number; the hook's `id` for the
same object is a string, so join on `tostring`), `getTypeName()`,
`getPoint()`, and
for the first instance of each type name also `getDesc().displayName`,
`getDesc().category` and `getLife()`. A 20 km sphere at Kutaisi
returns 3.2 MB in 525 ms. The
hook de-duplicates by `id` across spheres, accumulates the per-model
catalogue for `scenery_models.json` (counts, OBB and radius medians from
the footprint calls, `type` bits), then for each new object
calls `terrain.getObjectsAtMapPoint(x, z)` in its own state and attaches
the `obb` and `radius` when an entry with the same `id` comes back.
Whole-theatre counts are around a million objects on Caucasus; write
`scenery.jsonl` in append mode as spheres complete. The mission must
contain no heliport statics or FARPs: a placed heliport removes every
scenery object within about 150 m at spawn (probe log), and the extract
would carry the hole. The hook checks `world.getAirbases()` for
`Airbase.Category.HELIPAD` entries whose `getID()` is not an `id` in
`airdromes.json` and
logs a warning naming them and skips the scenery sweep unless
`allow_helipads = true` is set in config (map-shipped helipads on some
theatres may appear here too, which is why it is a switch).

## Encoders

Two small encoders live in the hook and are unit-tested in the stub
harness before anything else is written:

- `i16le(v)`, `u8(v)`: per `extract-format.md`, with clamping.
- `json(value)`: strings escaped per RFC 8259 (quotes, backslash,
  control characters as `\u00XX`), numbers as `%.17g` with `inf`/`nan`
  refused, arrays for tables with consecutive integer keys from 1, objects
  otherwise with keys sorted, `null` for a sentinel `JSON_NULL` value.
  Adapt `dcs-scripting/assets/Literal.lua`'s traversal; do not reuse its
  output syntax.

## Failure handling

Every DCS call is wrapped in `pcall`. A failure logs the call, arguments
and message, marks the current tile `failed` in the manifest `notes`, and
moves on; the run does not stop. A tile with any failed sample is
written with `nodata` in those cells and listed in `notes`. If the
terrain id changes mid-run (the user opened another map), the hook logs,
writes the manifest, and returns to idle.

## Performance targets

Caucasus authored area at 50 m, on the measured machine: config and
tables under a minute; the pre-sweep about a minute; `water` and `height`
about two minutes each; roads about 25 minutes at 1 km seeds (see the
roads sweep); `surface` about six minutes; `scenery` a few minutes
(about 1 000 spheres; a dense 15 km sphere is 225 to 355 ms, an empty
one 4 ms, about 7 µs per object serialised). The hook must keep the editor or the sim responsive
throughout, which is what `frame_budget_ms` is for; the target is the
extraction finishing while the user does something else, not raw speed.

## Testing

Run under `extractor/test/StubHarness.lua`, a copy of
`dcs-scripting/scripts/StubHarness.lua` (1 197 lines; it stubs
`net.dostring_in` and passes `require` through, so the fake `terrain`
module installs through `package.loaded`) vendored into the repository
so CI does not depend on a skill directory, with a fake `terrain`
module and a fake `net.dostring_in` backed by the same closed-form
terrain the Rust synthetic generator uses (`core.md`,
"Synthetic theatre"). The harness run must produce an extract directory
that `dcsterrain check-extract` accepts and that is sample-for-sample
identical to the Rust generator's output for the same parameters. Unit
tests: the two encoders on boundary values; grid computation from each
bounds source and from the pre-sweep; resume from a manifest with half
the tiles; the fill and sea skip set; the road pair de-duplication; the mission-pass length check
rejecting a truncated payload.

## Acceptance

1. Harness run produces a valid extract identical to the synthetic
   reference.
2. On the real Caucasus with a `crop_m` of 10 × 10 km around Kutaisi,
   both passes complete in under two minutes and `dcsterrain check-extract`
   passes.
3. Spot check: ten cells' `height` samples equal `floor(GetHeight + 0.5)`
   read live through the bridge; the Kutaisi reference point
   (−284887, 683859) reads 45.010 m live and 45 in the extract.
4. Full Caucasus run completes both passes with the sim responsive, and
   the manifest `timing_ms` is recorded in the probe log.
