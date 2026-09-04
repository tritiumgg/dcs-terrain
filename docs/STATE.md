# State

Where the work is. Updated at the end of every working session, before handing
back. This file holds progress; `plan.md` holds the task graph and
`decisions/` holds what has diverged from the frozen documents.

**Last updated:** 2026-09-04T22:18Z

## Done

Newest first. Max 5 entries — drop the oldest when adding a sixth.

1. **X4 — the state machine and frame budget.** idle to prepare to hook to
   mission to done, driven by the four DCS callbacks. A phase is a list of jobs
   the sweeps register: a job is `{name, start}`, `start(run)` returns a step,
   and a step returns `MORE` or `DONE`, so X5 onward add a sweep without
   touching the machine. A frame runs steps until `frame_budget_ms` is spent,
   checked between steps and never inside one, and always runs at least one, so
   a budget smaller than a step still finishes. The manifest is saved at each
   phase change and each sweep end, never per tile. **Prepare has no jobs yet**
   — X2a and X2b give it its first. ADR 0010 came out of building it: the
   server-state pass is gated on loaded terrain, not on a mission. A 6-PR
   stack.
2. **ADR 0010 — `server`-state calls need terrain, not a mission.** Measured
   on 2.9.29.27468, editor open on Caucasus, no mission: `land.getHeight`
   returns the acceptance value at Kutaisi, `land.getSurfaceType` the `RUNWAY`
   enum, `world.searchObjects` real scenery. The crash the phase-`sim` rule
   generalised was a call reaching the state with no terrain at all. So a whole
   extract can be taken from a bare editor map, which also stops a mission
   altering what is extracted: a heliport static clears scenery within 150 m,
   into the scene the terrain module reads. `CLAUDE.md` is rewritten with it.
3. **The project setup, ported from `dcs-bridge`.** A pull request template,
   `mise` tasks, and five tools: `ledger.sh` retrieves a claim and lints the 18
   stamps, beside `statecheck.sh`, `nospecrefs.sh`, `readmeopen.sh` and
   `hooktest.sh`. Seven hooks load this file at session start, refuse an
   unstamped stop, refuse a write to `docs/spec/`, check every shell command,
   and check a commit before and after; a `docs` workflow runs the five on
   every change. The ADRs are reformatted to four headings, cited `ADR NNNN`.
4. **X3 — the grid and the journal.** Grid snapping, tile addressing, the
   pre-sweep lattice, the `tiles.jsonl` journal, the manifest and resume, in
   `extractor/DcsTerrainExtract.lua` with 9 offline test files and 487 checks.
   A JSON decoder came with it, because resume must read the manifest back and
   X2b's `autoupdate.cfg` is strict JSON. Two reference vectors pin the
   snapping and neither subsumes the other: ADR 0007's Caucasus rectangle
   catches rounding to the nearest cell, F2's synth rectangle catches snapping
   inward, and two smaller vectors exist because both reference rectangles
   agree with the wrong extent formula. Shipped as an 11-PR stack, which is
   also when `CLAUDE.md` gained its delivery and subagent rules.
5. **F2 — reference constants for the synthetic theatre.**
   `extractor/test/support/synth_constants.lua` and
   `dcsterrain-core/src/synth.rs`, 149 constants each, and a Rust test that
   parses the Lua file and fails on any name or value the two do not share, so
   the pair cannot drift — the hole ADR 0005 left when it dropped the
   byte-for-byte harness comparison. Positions are offsets from the grid
   origin, so raising the size adds land and moves nothing; 70 km is the
   default because a smaller theatre cannot hold an all-sea tile clear of the
   fill margin.

## Next

One task. The thing to pick up immediately.

**X2a — config load and validation, the progress log and the `dcs.log` lines**:
every bad field produces one log line and a disabled run.

It fills `M.log`, X4's no-op seam, and gives prepare its first job: the machine
has the phases, but nothing yet puts a manifest in them.

*Verified by:* an agent offline for the validation table; a maintainer at a
live install for the two log destinations and for the hook registering at all.

## Then

Max 5 entries, in dependency order. Task ids from `plan.md`.

1. **X2b** — `dcs_build` from `autoupdate.cfg` and the `terrain_fingerprint`;
   ADR 0007 carries the Caucasus head hashes and digest as its test vector.
   The install reads 2.9.29.27468 / 20260902-093323, the build ADR 0007
   measured.
2. **X5 / X6 / X7** — the tables, `water` and `height`, and roads sweeps, each
   an `add_job("hook", ...)` on X4's machine. Verified against a running
   theatre through the bridge, not offline.
3. **X8a / X8b / X8c / X9** — the mission pass, scenery, the model catalogue
   and failure handling. X ends here until `check-extract` exists.
4. **C1 / C2 / C3** — the Rust interlude X10 needs, and no more of C than
   that: scaffold, `synth` on the F2 constants, `check-extract`. Closes P2 and
   MS0 on the way past.
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
