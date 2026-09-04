# extractor

The DCS extractor hook and its offline Lua tests. Lua 5.1, because that is what
the DCS hook state runs: no `string.pack`, no `bit`, no LuaJIT, and file handles
without `seek`.

- `DcsTerrainExtract.lua` — the hook. A user copies it to
  `Scripts/Hooks/` in their Saved Games folder, with a config table at
  `Config/DcsTerrainExtract.lua`. It is disabled until that config exists.
- `test/` — offline tests, run with a plain `lua5.1` interpreter. Every `.lua`
  file directly under it is a test; `test/support/` holds what they read.
- `test/support/synth_constants.lua` — the reference constants for the
  synthetic theatre. Its twin is `synth.rs` in `dcsterrain-core`, and a Rust
  test parses this file and fails when the two disagree, so a change here runs
  the Rust workflow as well as the Lua one.

The tests cover only what a running DCS cannot be driven to do on demand: the
encoders, grid computation, the journal and resume, the frame-budget state
machine, and injected failures. Each supplies its own fakes, sized to the one
function under test. The sweeps that read terrain are verified against a
running theatre instead (ADR-0005), so nothing here stubs a terrain module.
