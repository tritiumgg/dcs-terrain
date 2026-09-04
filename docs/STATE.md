# State

Where the work is. Updated at the end of every working session, before handing
back. This file holds progress; `plan.md` holds the task graph and
`decisions/` holds what has diverged from the frozen documents.

**Last updated:** 2026-09-04

## Done

Newest first. Max 5 entries — drop the oldest when adding a sixth.

1. **F1 — the extract format is frozen at v1.** ADR-0007 records what v1 is:
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
2. **Language servers pinned in `mise.toml`.** `rust-analyzer` and
   `lua-language-server`, provisioned by `mise install` like the rest of the
   toolchain. A root `.luarc.json` sets the Lua server to 5.1 and declares the
   hook-state globals `DCS`, `net`, `log` and `lfs`, so `string.pack` and the
   rest of what 5.1 lacks are flagged where they are written rather than where
   the hook is loaded.
3. **CI workflows, ahead of P2.** `.github/workflows/rust.yml` and `lua.yml`,
   one per language, each scoped to the paths it tests, so a documentation
   commit starts no runner. In-flight runs are superseded on a new push, and
   Windows and macOS wait on Linux so a broken commit costs one runner rather
   than three. The repository is public, so standard runners burn no Actions
   minutes. P2 stays open: its done test needs C1 and X1 to exist, and the Lua
   job has no tests to run until X1.
4. **Kotlin client cut.** It becomes its own project, consuming the MCP server
   and the CLI like any other client. Deleted `kotlin/`, tasks K1–K3c,
   `kotlin-consumer.md` and its ledger pair — the one exception ever made to
   the frozen rule, agreed with the user, and no other frozen file joined to
   them. `design-and-facts.md` and `core.md` keep their stale Kotlin
   references. MS4 is now the server alone, proved by `query` and the server
   agreeing; C1 gains a schema snapshot test, which is what K1 used to catch.
   `mise.toml` loses `java` and `node`, leaving Rust, Lua and Python.
5. **P1 — repository layout.** The `dcsterrain/` Cargo workspace with its three
   crates builds; `extractor/` and `tools/validate/` exist with a README saying
   what belongs in each. `mise.toml` and `rust-toolchain.toml` pin the
   toolchain and `tools/lua51/` builds Lua 5.1.5 on Windows, all per ADR-0006.
   The `.gitignore` `target/` rule no longer anchors to the repository root.

## Next

One task. The thing to pick up immediately.

**F2 — reference constants for the synthetic theatre**, `synth_constants.lua`
and `synth.rs`, the same numbers in both, each naming the other in a comment.
Take the fill triple, the `water` codes and the layer table from ADR-0007.

Blocked on nothing.

## Then

Max 5 entries, in dependency order. Task ids from `plan.md`.

1. **C1** — workspace scaffold, `types`, `clap` CLI stubs, `schemars`, stderr
   logging. Head of the longest dependency chain.
2. **X1** — Lua encoders `i16le`, `u8`, `json` and the ADR-0007 list
   normalisation, with offline Lua tests. Runs in parallel with C1.
3. **C2 / C3** — `synth` and `check-extract`, which unlock MS0.
4. **X3** — grid computation from each bounds source, tile addressing, the
   journal and resume.
5. **X2a / X2b** — config load and validation, then `dcs_build` from
   `autoupdate.cfg` and the `terrain_fingerprint`; ADR-0007 carries the
   Caucasus head hashes and digest as X2b's test vector.

Milestone in view: **MS0 Contract** (P1, P2, F1, F2, C1, C2, C3, X1, X3).
Proof is `check-extract` accepting the Rust synthetic extract and the hook's
encoders and grid computation reproducing the F2 constants byte for byte.

First real data: **X10**, a 10 x 10 km Kutaisi extract that `check-extract`
accepts. Component X is developed on the Windows machine (ADR-0005);
everything from C4 on is built on macOS against that crop.

## Carries

Things a later task must not lose. Max 10 — see the rules below.

| # | Carry | Discharged by |
|---|---|---|
| 1 | Lua 5.1 folds numeric literals at compile time, and the result is order-dependent: `-0.0` and folded infinities in one chunk can print as another constant in that chunk. Assert on computed values, not folded literals. DCS's interpreter and ours behave identically here. | X1 |

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
