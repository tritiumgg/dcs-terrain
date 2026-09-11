# extractor

The DCS extractor hook and its offline Lua tests. Lua 5.1, because that is what
the DCS hook state runs: no `string.pack`, no `bit`, no LuaJIT, and file handles
without `seek`.

- `DcsTerrainExtract.lua` — the hook. A user copies it to
  `Scripts/Hooks/` in their Saved Games folder, with a config table at
  `Config/DcsTerrainExtract.lua` holding `enabled = true`. It does nothing at
  all until that is there. It is meant to be run with a map open in the Mission
  Editor: a mission is not needed, and a FARP or heliport placed in one clears
  the scenery around it. Configuration is a window rather than the file — the
  file is what the window reads at start and writes at Start (ADR 0011), and
  the three fields it holds are in that record. The window is drawn with the
  Mission Editor's own skins and opens centered. It carries a box for the
  output directory; a tick for the crop that, on, shows three boxes for the
  center and the radius and a button that picks the center off the map in
  whole meters; one line that shows whatever was said last — what to do, what
  is wrong, what a press came to, which phase the run is in; and one button,
  Start or Stop, whichever the run can take. A progress bar appears under
  them once there is progress. The directory has to be an absolute path on a
  drive that exists, and is created if absent (ADR 0020); a crop is refused
  under a 1 km radius or where its box reaches outside the map (ADR 0019).
  Start re-points the run at
  whatever the boxes now say (ADR 0017), and the window is raised above DCS's
  own chrome so it stays on screen (ADR 0018). Prepare reads the DCS build
  from the install, finds the theatre's directory under `Mods/terrains` by
  the id it declares, and records its three data files by path, size and
  payload size, nothing hashed, because the build is the terrain's version
  (ADR 0022, ADR 0023); an install that does not hold them stops the run with
  the reason on the window. No rectangle is ever read from a theatre file:
  a run without a crop measures the built area itself, the same way on every
  theatre (ADR 0026), with a 5 km lattice over the map where a cell counts
  when a 2 km line of heights crosses enough posts or a road lies within 5
  km, and the grid is that rectangle grown by 10 km; a crop run measures
  nothing and records no rectangle (ADR 0009). The grid is planned from the
  crop or the measurement, and the output directory is opened, or resumed
  when it already holds an extract of the same theatre and build, in which
  case the rectangle is the manifest's and is not measured again. The hook
  pass then writes `config.json`, with the theatre's own facts, twenty
  lat/lon samples and the fill triple, and the seven tables, one file a step.
  Every table is shaped the same way on every theatre: a value that is not
  what was measured elsewhere is written null with a line in the log, a table
  the theatre cannot give is written empty, and the run never stops for
  either. Then the `water` and `height` tiles, one tile a step, each cell
  tested against the fill triple before it is rounded: a tile that is fill
  throughout is not written, a tile that is sea throughout is written for
  `water` only, and a resumed run counts its journalled tiles as done and
  reads them back to find the sea ones. *The road and scenery sweeps are not
  built yet, so a run reaches `done` with those two layers and no roads.*
  While a run is sweeping, the line names the sweep, which of how many it is
  and its count, refreshed every second, and the bar is that sweep's own count;
  nothing claims to know how far the whole run is (ADR 0025).
  A run writes `Logs/DcsTerrainExtract.log` in the same Saved Games folder, one
  line per tile, per phase change and per finished sweep, a heartbeat every ten
  seconds naming the sweep, which of how many, its count and elapsed, and
  one line of totals at done (ADR 0024), appended across runs and never rotated;
  `dcs.log` gets a phase change at `INFO` and anything to act on at `WARNING`.
- `test/` — offline tests, run with a plain `lua5.1` interpreter. Every `.lua`
  file directly under it is a test; `test/support/` holds what they read.
- `test/support/testing.lua` — the whole test framework: a few checks and a
  `done()` that exits non-zero, since each file's exit status is its result.
- `test/support/synth_constants.lua` — the reference constants for the
  synthetic theatre. Its twin is `synth.rs` in `dcsterrain-core`, and a Rust
  test parses this file and fails when the two disagree, so a change here runs
  the Rust workflow as well as the Lua one.

The tests cover only what a running DCS cannot be driven to do on demand: the
encoders, grid computation, the journal and resume, the frame-budget state
machine, and injected failures. Each supplies its own fakes, sized to the one
function under test. The sweeps that read terrain are verified against a
running theatre instead (ADR 0005), so nothing here stubs a terrain module.
