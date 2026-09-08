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
  the three fields it holds are in that record. The window carries a box for
  the output directory, a tick and three boxes for the crop with a button that
  picks the centre off the map, a line under each field for whatever is wrong
  with it, and Start and Stop. Start re-points the run at whatever the boxes now
  say (ADR 0017), and the window is raised above DCS's own chrome so it stays on
  screen (ADR 0018). *The sweeps that
  fill an extract are not built yet, so a run reaches `done` in a few frames
  and writes nothing.*
  A run writes `Logs/DcsTerrainExtract.log` in the same Saved Games folder, one
  line per tile and per phase change, appended across runs and never rotated;
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
