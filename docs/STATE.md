# State

Where the work is. Updated at the end of every working session, before handing
back. This file holds progress; `plan.md` holds the task graph and
`decisions/` holds what has diverged from the frozen documents.

**Last updated:** 2026-09-04

## Done

Newest first. Max 5 entries — drop the oldest when adding a sixth.

1. **F2 — reference constants for the synthetic theatre.**
   `extractor/test/support/synth_constants.lua` and
   `dcsterrain-core/src/synth.rs`, 149 constants each, and a Rust test that
   parses the Lua file and fails on any name or value the two do not share, so
   the pair cannot drift — the hole ADR-0005 left when it dropped the
   byte-for-byte harness comparison. Positions are offsets from the grid
   origin, so raising the size adds land and moves nothing; 70 km is the
   default because a smaller theatre cannot hold an all-sea tile clear of the
   fill margin.
2. **F1 — the extract format is frozen at v1.** ADR-0007 records what v1 is:
   every field traced to the DCS call behind it, measured live on
   2.9.29.27468 across three theatres and, where no map load was needed, all
   eight installed (an airdrome's sub-tables are keyed from 0 and are often
   empty, a runway's name is its two edge names joined, `missionNodes`
   positions are positional arrays, `towns` is keyed by name);
   an unrecognised surface string encoded 254, so 255 means nodata alone; the
   Caucasus fill triple 5.000005 / `land` / 0 that the probe log left
   unmeasured; and a manifest example built from measured numbers. ADR-0008
   gives the packed `meta` the authored rectangle and makes its bounds keys
   metres.
3. **Language servers pinned in `mise.toml`.** `rust-analyzer` and
   `lua-language-server`, provisioned by `mise install` like the rest of the
   toolchain. A root `.luarc.json` sets the Lua server to 5.1 and declares the
   hook-state globals `DCS`, `net`, `log` and `lfs`, so `string.pack` and the
   rest of what 5.1 lacks are flagged where they are written rather than where
   the hook is loaded.
4. **CI workflows, ahead of P2.** `.github/workflows/rust.yml` and `lua.yml`,
   one per language, each scoped to the paths it tests, so a documentation
   commit starts no runner. In-flight runs are superseded on a new push, and
   Windows and macOS wait on Linux so a broken commit costs one runner rather
   than three. The repository is public, so standard runners burn no Actions
   minutes. P2 stays open: its done test needs C1 and X1 to exist, and the Lua
   job has no tests to run until X1.
5. **Kotlin client cut.** It becomes its own project, consuming the MCP server
   and the CLI like any other client. Deleted `kotlin/`, tasks K1–K3c and one
   frozen document pair — the only exception ever made to the frozen rule,
   agreed with the user, and nothing else joined to it. MS4 is now the server
   alone; C1 gains the schema snapshot test that K1 used to catch. `mise.toml`
   loses `java` and `node`, leaving Rust, Lua and Python.

## Next

One task. The thing to pick up immediately.

**X1 — the Lua encoders**: `i16le`, `u8`, `json` and the ADR-0007 list
normalisation, with offline Lua tests on boundary values, nested tables,
quotes, unicode, empty arrays and objects, and a DCS table keyed from 0.

Blocked on nothing. First of the twelve X tasks that need no Rust at all, and
`plan.md` now runs all of them before C, because this machine is the only one
that can do them.

## Then

Max 5 entries, in dependency order. Task ids from `plan.md`.

1. **X3 / X4** — grid computation from each bounds source, tile addressing,
   the journal and resume; then the state machine and frame budget.
2. **X2a / X2b** — config load and validation, then `dcs_build` from
   `autoupdate.cfg` and the `terrain_fingerprint`; ADR-0007 carries the
   Caucasus head hashes and digest as X2b's test vector.
3. **X5 / X6 / X7** — the tables, `water` and `height`, and roads sweeps.
   Verified against a running theatre through the bridge, not offline.
4. **X8a / X8b / X8c / X9** — the mission pass, scenery, the model catalogue
   and failure handling. X ends here until `check-extract` exists.
5. **C1 / C2 / C3** — the Rust interlude X10 needs, and no more of C than
   that: scaffold, `synth` on the F2 constants, `check-extract`. Closes P2 and
   MS0 on the way past.

Milestone in view: **MS0 Contract** (P1, P2, F1, F2, C1, C2, C3, X1, X3).
Proof is `check-extract` accepting the Rust synthetic extract and the hook's
encoders and grid computation reproducing the F2 constants byte for byte.

First real data: **X10**, a 10 x 10 km Kutaisi extract that `check-extract`
accepts, which is where X ends for now. X11 still waits for C4 onward to read
that crop, so a field missing from the frozen format costs a crop re-run and
not a theatre. Everything from C4 on is built on macOS against the crop.

## Carries

Things a later task must not lose. Max 10 — see the rules below.

| # | Carry | Discharged by |
|---|---|---|
| 1 | Lua 5.1 folds numeric literals at compile time, and the result is order-dependent: `-0.0` and folded infinities in one chunk can print as another constant in that chunk. Assert on computed values, not folded literals. DCS's interpreter and ours behave identically here. | X1 |
| 2 | `synth` produces no all-fill tile at any size: the fill margin is 2 km and a tile is 12.8 km, so no tile is fill throughout. Test the absent-fill-tile read against a hand-built manifest, not against a generated extract. The all-sea case is generated, as one interior tile. | C4 |

### Rules for this file

Keep the whole file under about 120 lines. It is read at the start of every
session, so it competes with the specs for attention.

- **Done** holds 5 entries, one or two lines each. Older history is in git.
- **Next** holds exactly one task. If two things are genuinely next, one of
  them is first.
- **Then** holds 5 entries. Beyond that, `plan.md` is the task graph
  — do not restate it here.
- **Carries** hold 10 entries, hard cap. Every carry names the task that
  discharges it, and is **deleted** on discharge, not marked done.

A carry is something **learned while working** that no document already
records. Before writing one, check `plan.md`, the specs and `decisions/` — if
any of them already says it, there is no carry. A task's own deliverable or
done test is not a carry; neither is a rule from `CLAUDE.md`, nor anything
readable from `git log`. Most sessions add none, and an empty table is the
normal state.

A carry is also short-lived, not a decision. If one cannot name a discharging
task, or survives three turns of the Next slot, write it as an ADR and delete
the row. If the table is full and something new must be carried, that is the
signal — promote the oldest to an ADR rather than growing the table.

Nothing here duplicates `plan.md`, `docs/decisions/`, or a spec.
Point at those instead of copying them.
