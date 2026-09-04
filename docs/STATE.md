# State

Where the work is. Updated at the end of every working session, before handing
back. This file holds progress; `plan.md` holds the task graph and
`decisions/` holds what has diverged from the frozen documents.

**Last updated:** 2026-09-04T20:24Z

## Done

Newest first. Max 5 entries — drop the oldest when adding a sixth.

1. **The project setup, ported from `dcs-bridge`.** A pull request template,
   `mise` tasks, and five tools: `ledger.sh` retrieves a claim and lints the 18
   stamps, beside `statecheck.sh`, `nospecrefs.sh`, `readmeopen.sh` and
   `hooktest.sh`. Seven hooks load this file at session start, refuse an
   unstamped stop, refuse a write to `docs/spec/`, check every shell command,
   and check a commit before and after; a `docs` workflow runs the five on
   every change. The ADRs are reformatted to four headings, cited `ADR NNNN`.
2. **X3 — the grid and the journal.** Grid snapping, tile addressing, the
   pre-sweep lattice, the `tiles.jsonl` journal, the manifest and resume, in
   `extractor/DcsTerrainExtract.lua` with 9 offline test files and 487 checks.
   A JSON decoder came with it, because resume must read the manifest back and
   X2b's `autoupdate.cfg` is strict JSON. Two reference vectors pin the
   snapping and neither subsumes the other: ADR 0007's Caucasus rectangle
   catches rounding to the nearest cell, F2's synth rectangle catches snapping
   inward, and two smaller vectors exist because both reference rectangles
   agree with the wrong extent formula. Shipped as an 11-PR stack, which is
   also when `CLAUDE.md` gained its delivery and subagent rules.
3. **X1 — the Lua encoders.** `extractor/DcsTerrainExtract.lua` holds `i16le`,
   `u8`, `json` and `normalise_list`; 121 checks in `encoders.lua` over
   `testing.lua`, the whole framework the later offline tasks get. `i16le`
   clamps to ±32767 and so cannot reach −32768: nodata comes from
   `I16_NODATA_BYTES`, and a clipped mountain never reads as a hole.
   `normalise_list` tags its result, the only thing separating an empty DCS
   list from an empty object once both are one Lua table. The hook returns its
   module table and registers nothing without DCS globals, which is how the
   offline tests reach into a one-file hook.
4. **F2 — reference constants for the synthetic theatre.**
   `extractor/test/support/synth_constants.lua` and
   `dcsterrain-core/src/synth.rs`, 149 constants each, and a Rust test that
   parses the Lua file and fails on any name or value the two do not share, so
   the pair cannot drift — the hole ADR 0005 left when it dropped the
   byte-for-byte harness comparison. Positions are offsets from the grid
   origin, so raising the size adds land and moves nothing; 70 km is the
   default because a smaller theatre cannot hold an all-sea tile clear of the
   fill margin.
5. **F1 — the extract format is frozen at v1.** ADR 0007 records what v1 is:
   every field traced to the DCS call behind it, measured live on
   2.9.29.27468 across three theatres and, where no map load was needed, all
   eight installed (an airdrome's sub-tables are keyed from 0 and are often
   empty, a runway's name is its two edge names joined, `missionNodes`
   positions are positional arrays, `towns` is keyed by name);
   an unrecognised surface string encoded 254, so 255 means nodata alone; the
   Caucasus fill triple 5.000005 / `land` / 0 that the probe log left
   unmeasured; and a manifest example built from measured numbers. ADR 0008
   gives the packed `meta` the authored rectangle and makes its bounds keys
   metres.

## Next

One task. The thing to pick up immediately.

**X4 — the state machine and frame budget**: idle, prepare, hook pass, mission
pass, done, sliced across `onSimulationFrame` on `os.clock()`.

Blocked on nothing now that X3 is in, and offline like X1 and X3. It is the
last piece before a sweep can run at all, and every sweep from X5 on is an
iterator it drives.

## Then

Max 5 entries, in dependency order. Task ids from `plan.md`.

1. **X2a / X2b** — config load and validation, then `dcs_build` from
   `autoupdate.cfg` and the `terrain_fingerprint`; ADR 0007 carries the
   Caucasus head hashes and digest as X2b's test vector.
2. **X5 / X6 / X7** — the tables, `water` and `height`, and roads sweeps.
   Verified against a running theatre through the bridge, not offline.
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
