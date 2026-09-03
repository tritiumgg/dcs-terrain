# State

Where the work is. Updated at the end of every working session, before handing
back. This file holds progress; `plan.md` holds the task graph and
`decisions/` holds what has diverged from the frozen documents.

**Last updated:** 2026-09-03

## Done

Newest first. Max 5 entries — drop the oldest when adding a sixth.

1. **ADR-0005**: the extractor is verified live through the `dcs-api-bridge`
   MCP, and offline Lua tests cover only what DCS cannot be driven to do on
   demand (X1–X4, X9). No stub harness is vendored. Rephrased eleven done
   tests and MS0's proof, which loses its byte-identical cross-language check
   — the format contract is proven at X10 instead. Component X is therefore
   developed on the Windows machine; the Rust, Kotlin and MCP work is on
   macOS, which carries the only toolchain. F1's done test now names the
   consumers its sign-off must walk.
2. `CLAUDE.md` rules: the `dcs-scripting` skill is forbidden, replaced by
   `dcs-api-lookup` + the bridge + the read-only install, with the four
   citations left in the frozen specs marked as ignorable. A DCS version bump
   is no longer an ADR trigger — the install being ahead of the spec is normal,
   and only a measurement that differs is an ADR. Install paths are discovered
   through the bridge (`lfs.currentdir()`), never recorded in the repository.
3. Deleted the seven measured projection sample sets — nothing reads them and
   the fitted results are in the probe log. `fit.py` moved to
   `tools/probe-theatre/fit.py`, where V2 packages it. ADR-0004.
4. `CLAUDE.md`: code never cites `docs/`, only ADR ids, so comments cannot rot
   against frozen specs. Recorded the Windows/macOS split. Dropped the stale
   bridge-transport rule and the hardcoded DCS build; Claude reads `version`
   from `autoupdate.cfg`. No non-project skills are named. Tightened the carry
   rule and cut the table from 10 padded rows to 1 real one.
5. Dropped the numeric prefixes and `spec-` prefixes, and moved everything
   frozen into `docs/spec/`. One rule now: nothing in `docs/spec/` is edited.
   Ledgers re-stamped and all 2 036 anchors re-verified. ADR-0001, ADR-0002.

## Next

One task. The thing to pick up immediately.

**P1 — repository layout.** Create the `dcsterrain/` Cargo workspace, the
`extractor/`, `kotlin/` and `tools/validate/` trees per the target layout in
`CLAUDE.md`. Done when the tree exists and the README states "no terrain data
is committed" — the README clause is already done.

Blocked on nothing.

## Then

Max 5 entries, in dependency order. Task ids from `plan.md`.

1. **F1** — freeze the extract format v1: confirm every field against the probe
   log, decide `water` codes and `nodata` values, write the manifest example by
   hand.
2. **F2** — reference constants for the synthetic theatre, `synth_constants.lua`
   and `synth.rs`, same numbers.
3. **C1** — workspace scaffold, `types`, `clap` CLI stubs, `schemars`, stderr
   logging. Head of the longest dependency chain.
4. **X1** — Lua encoders `i16le`, `u8`, `json` with offline Lua tests. Runs in
   parallel with C1.
5. **C2 / C3** — `synth` and `check-extract`, which unlock MS0.

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
