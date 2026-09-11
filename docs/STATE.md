# State

Where the work is. Updated at the end of every working session, before handing
back. This file holds progress; `plan.md` holds the task graph and
`decisions/` holds what has diverged from the frozen documents.

**Last updated:** 2026-09-11T19:57Z

## Done

Newest first. Max 5 entries — drop the oldest when adding a sixth.

1. **X5 offline: the grid job, `config.json` and the seven tables.** Prepare
   is `identity` then `grid`: the grid is planned from the crop, the output
   directory is opened or resumed (the disk manifest's timings folded in
   once), and a run with no crop refuses until the pre-sweep exists. The hook
   pass is `config` (the theatre's facts, twenty lat/lon samples, the fill
   triple off three corners 500 km out) then `tables`, one file a step with
   `progress()`. No rectangle is read from a theatre file: ADR **0026**,
   measured against ED's own use and five theatres' values. Every shaper is
   terrain agnostic and never raises: an unexpected value is null with a log
   line, a table the theatre cannot give is empty. *Verified by:* an agent,
   offline, over fakes shaped as measured; the live diff of every table
   against the bridge is still to do, because DCS was closed.
2. **X12: the run says where it is, and claims no more.** A job's `start`
   may return `progress()` beside its step; the run keeps a record refreshed
   every second that the window's line reads ("Sweeping the terrain: water,
   3 of 9, 1234 of 5000."), logs a `heartbeat` line every ten seconds and
   one `totals` line at done. The bar is the running sweep's own count.
   ADRs **0024**, **0025**.
3. **X2b: the run knows what it is extracting, and the build is the terrain's
   version.** The first prepare job reads `dcs_build` off `autoupdate.cfg`,
   finds `terrain_dir` by the shallowest id in an `entry.lua`, and records the
   three data files by path, size and payload field; no hash, no time. A step
   may return `M.REFUSED`. ADRs **0022**, **0023**. Verified live on Caucasus
   and Sinai.
4. **X14: the window belongs in the editor, and says one thing at a time.**
   Skins off `me_aircraft_group.dlg`, one line beside one Start/Stop slot;
   ADR **0019** (1 km floor, box inside the bounds) and ADR **0020** (absolute
   path on a drive that exists). PR **48**.
5. **X13: the window has controls, and the live pass found two bugs the offline
   tests could not.** ADR **0017**, Start re-points a run at the directory the
   boxes name; ADR **0018** raises the window above DCS's chrome. PR **47**.

## Next

One task. The thing to pick up immediately.

**X5, live, then its pull request.** With DCS in the Mission Editor on
Caucasus: Start with a 5 km crop at Kutaisi, then diff `config.json` and every
table against the same calls made through the bridge, and read the manifest's
null authored pair. *Verified by:* an agent through the bridge, with a
maintainer watching the line say "config, 3 of 4" then "tables, 4 of 4, k of
7". Then the adversarial review's fixes, `mise run docs`, push, PR.

## Then

Max 5 entries, in dependency order. Task ids from `plan.md`.

1. **X6** — the pre-sweep as a prepare job between `identity` and `grid`, the
   only rectangle source on every theatre (ADR 0026); then `water` and
   `height`, where carry 2 is discharged.
2. **X7** — roads and railroads, each as two jobs, seeds then paths.
3. **X8a / X8b / X8c / X9** — the mission pass, scenery, the model catalogue
   and failure handling. X ends here until `check-extract` exists.
4. **C1 / C2 / C3** — the Rust interlude X10 needs: scaffold, `synth` on the F2
   constants, `check-extract`. Closes P2 and MS0 past.
5. **X10** — the 10 x 10 km Kutaisi crop that `check-extract` accepts; its
   authored pair is null (ADR 0009, ADR 0026).

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
| 3 | The pre-sweep job runs before `grid`, so it cannot learn from `prepare_resume` that the directory resumes: it has to read the manifest itself and skip when one is there, or a re-measured lattice moves the grid by a cell. `grid` already passes no grid to the resume check on the pre-sweep path. The water job owns `run.skip`, resets it at its start, and must refuse on a write failure, or `height` reads that tile as fill. Fill tiles are re-swept on every Start, because the format forbids a journal line for an omitted tile; accept it and log the count. | X6 |
| 4 | Roads: seed ids are positions in a fixed plan (lattice row-major at 1 km, then airdromes by numeric id, then towns by name), so a resume re-derives them; merge after every snap, into the lowest-id kept seed within 100 m, no chaining; neighbours are the 4 nearest kept seeds, ties by id; a pair is emitted at its lower id, so no seen set. Resume by streaming `roads.jsonl` in counted reads as steps and counting seed and pair lines; repair a partial tail by a streamed copy and rename, never a whole-file rewrite, because Caucasus is hundreds of MB and `write_file` holds the string. | X7 |
