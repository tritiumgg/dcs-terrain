# extractor

The DCS extractor hook and its offline Lua tests. Lua 5.1, because that is what
the DCS hook state runs: no `string.pack`, no `bit`, no LuaJIT, and file handles
without `seek`.

- `DcsTerrainExtract.lua` — the hook. A user copies it to
  `Scripts/Hooks/` in their Saved Games folder, with a config table at
  `Config/DcsTerrainExtract.lua`. It is disabled until that config exists.
- `test/` — offline tests, run with a plain `lua5.1` interpreter.

The tests cover only what a running DCS cannot be driven to do on demand: the
encoders, grid computation, the journal and resume, the frame-budget state
machine, and injected failures. Each supplies its own fakes, sized to the one
function under test. The sweeps that read terrain are verified against a
running theatre instead (ADR-0005), so nothing here stubs a terrain module.
