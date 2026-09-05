# State

Where the work is. Updated at the end of every working session, before handing
back. This file holds progress; `plan.md` holds the task graph and
`decisions/` holds what has diverged from the frozen documents.

**Last updated:** 2026-09-05T00:20Z

## Done

Newest first. Max 5 entries — drop the oldest when adding a sixth.

1. **X13 spiked, and three of seven branches open.** Every spike unknown is
   measured on 2.9.29.27468 with the editor on Caucasus, each step read back by
   `DCS.makeScreenShot`, so an agent can see the window rather than report it.
   Every widget the controls need constructs from the hook state; an unskinned
   one draws nothing. A window refuses to close by overriding `onClose` to
   re-assert `setVisible(true)`, checked against the real title-bar X.
   `net.dostring_in("gui", ...)` reaches `MapWindow`: `getCurPosition()` matched
   the editor's status bar to the digit, `getMapBounds()` answers in
   **kilometres**, and its Draw layer takes a `Polygon` whose points are
   *relative* to the anchor and whose fill needs the ring closed explicitly.
   Open: `field_problem` with a `tags` return, the config file under an empty
   environment, and ADR 0014's stopped state.
2. **X2a, and a change of direction: configuration moves into a window.** Most
   of the frozen config table turned out not to be a question a user can answer,
   so three ADRs came out of planning it — **0011** cuts sixteen fields to
   `enabled`, `output_dir` and a crop that is a centre and a radius, deriving or
   fixing the rest; **0012** makes a bad field one log line and its default,
   with `output_dir` alone blocking a run; **0013** makes the progress log
   append-only. A 4-PR stack.
3. **X4 — the state machine and frame budget.** idle to prepare to hook to
   mission to done, driven by the four DCS callbacks. A phase is a list of jobs
   the sweeps register: a job is `{name, start}`, `start(run)` returns a step,
   and a step returns `MORE` or `DONE`, so X5 onward add a sweep without
   touching the machine. A frame runs steps until `frame_budget_ms` is spent,
   checked between steps and never inside one, and always runs at least one, so
   a budget smaller than a step still finishes. The manifest is saved at each
   phase change and each sweep end, never per tile. **Prepare has no jobs yet.**
   ADR 0010 came out of building it: the server-state pass is gated on loaded
   terrain, not on a mission. A 6-PR stack.
4. **ADR 0010 — `server`-state calls need terrain, not a mission.** Measured
   on 2.9.29.27468, editor open on Caucasus, no mission: `land.getHeight`
   returns the acceptance value at Kutaisi, `land.getSurfaceType` the `RUNWAY`
   enum, `world.searchObjects` real scenery. The crash the phase-`sim` rule
   generalised was a call reaching the state with no terrain at all. So a whole
   extract can be taken from a bare editor map, which also stops a mission
   altering what is extracted: a heliport static clears scenery within 150 m,
   into the scene the terrain module reads. `CLAUDE.md` is rewritten with it.
5. **The project setup, ported from `dcs-bridge`.** A pull request template,
   `mise` tasks, and five tools: `ledger.sh` retrieves a claim and lints the 18
   stamps, beside `statecheck.sh`, `nospecrefs.sh`, `readmeopen.sh` and
   `hooktest.sh`. Seven hooks load this file at session start, refuse an
   unstamped stop, refuse a write to `docs/spec/`, check every shell command,
   and check a commit before and after; a `docs` workflow runs the five on
   every change. The ADRs are reformatted to four headings, cited `ADR NNNN`.

## Next

One task. The thing to pick up immediately.

**X13 branch 4 — the window itself**, on the three open PRs: the seam (`M.gui`,
`M.ui`, a one-way latch), the chrome, and the bootstrap that reads the config at
load and registers. First branch a maintainer can install and see. An ADR lands
with it: a failing widget call abandons the window and never touches the run.
Then controls, wiring, map pick, crop overlay.

*Verified by:* an agent, offline and by screenshot — except whether
`require("Skin")` answers at hook-**load** time, which needs a live install.

## Then

Max 5 entries, in dependency order. Task ids from `plan.md`.

1. **X2b** — `dcs_build` from `autoupdate.cfg`, `terrain_dir` by scanning
   `Mods/terrains/*/entry.lua` for the id (ADR 0011), and the
   `terrain_fingerprint`; ADR 0007 carries the Caucasus head hashes and digest
   as its vector. The install reads 2.9.29.27468 / 20260902-093323, the build
   ADR 0007 measured.
2. **X12 / X5 / X6 / X7** — progress reporting first, so the sweeps are written
   against its `progress()` contract; then the tables, `water` and `height`, and
   roads, each an `add_job("hook", ...)`. Verified through the bridge.
3. **X8a / X8b / X8c / X9** — the mission pass, scenery, the model catalogue
   and failure handling. X ends here until `check-extract` exists.
4. **C1 / C2 / C3** — the Rust interlude X10 needs and no more: scaffold,
   `synth` on the F2 constants, `check-extract`. Closes P2 and MS0 past.
5. **X10** — the live cropped run, once C3 exists.

Milestone in view: **MS0 Contract** (P1, P2, F1, F2, C1, C2, C3, X1, X3).
Proof is `check-extract` accepting the Rust synthetic extract and the hook's
encoders and grid computation reproducing the F2 constants byte for byte.

First real data: **X10**, a 10 x 10 km Kutaisi extract that `check-extract`
accepts, which is where X ends for now. X11 still waits for C4 onward to read
that crop, so a field missing from the frozen format costs a crop re-run and
not a theatre. Everything from C4 on is built on macOS against the crop.

## Carries

Things a later task must not lose. Max 10 — `CLAUDE.md` has the rules.

| # | Carry | Discharged by |
|---|---|---|
| 1 | `synth` produces no all-fill tile at any size: the fill margin is 2 km and a tile is 12.8 km, so no tile is fill throughout. Test the absent-fill-tile read against a hand-built manifest, not against a generated extract. The all-sea case is generated, as one interior tile. | C4 |
| 2 | The `water` sweep's fill and sea skip set does not survive a restart and cannot be rebuilt from the manifest and journal. An all-fill tile is *absent* from the journal, which reads the same as not yet swept; and per-tile `min`/`max` cannot identify an all-sea tile, because they are taken over non-nodata samples only, so a tile that is part fill and part sea also reads `min = max = 2`. Rehydrate the set by re-reading the written `water` tile bytes. Getting it wrong makes `pack` read a fill cell as sea. | X6 |
| 3 | Measured on 2.9.29.27468: `require` of `dxgui`, `Window`, `Panel`, `Static` and `HorzProgressBar` all answer from the hook state with no `package.path` change, and `HorzProgressBar` carries `new`, `setRange` and `setValue`. The raw calls are `install:dxgui/bind/ProgressBar.lua`: `ProgressBarSetRange(w, min, max)` and `ProgressBarSetValue(w, v)`; the bind classes are thin wrappers over the `Gui.*` globals of the same name, so either layer works from a hook. `Gui.SetTaskbarProgressState` has a measured arity of 2 and would put progress on the taskbar button. Also measured: `Skin` and its seven skins answer, an unskinned widget draws nothing, and every input widget the controls need constructs. Unmeasured: whether `require("Skin")` answers at hook-load time rather than at bridge-eval time. | X13 |
| 4 | A live run with no sweeps registered writes no output directory and reaches `done` in a few frames: `M.jobs` is three empty lists, the queue finishes at once, and `M.save` returns false with no manifest to write. Say so in X13's live testing steps or a tester will look for the extract and call the hook broken. | X13 |
