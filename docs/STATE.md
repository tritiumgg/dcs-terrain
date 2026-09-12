# State

Where the work is. Updated at the end of every working session, before handing
back. This file holds progress; `plan.md` holds the task graph and
`decisions/` holds what has diverged from the frozen documents.

**Last updated:** 2026-09-12T00:34Z

## Done

Newest first. Max 5 entries — drop the oldest when adding a sixth.

1. **X6, offline: the pre-sweep, `water` and `height`.** Prepare is
   `identity`, `presweep`, `grid`: the pre-sweep walks a 5 km lattice one
   cell a step (a road snap first, a 2 km line of heights only where a road
   lies 5 to 25 km off, ADR 0027), skips itself on a crop run and on a directory
   whose manifest already carries this theatre's rectangle, and refuses a
   crop extract or another theatre's before paying the minute. The hook pass
   is `config`, `tables`, `water`, `height`, one tile a step; water builds
   `run.skip` (fill and all-sea tiles) and a resumed water sweep reads each
   journalled tile back to find the sea ones. *Verified by:* an agent offline
   over closed-form fakes, every byte of a 2 x 2 tile grid written down; then
   a 5 km Kutaisi crop run by the maintainer on Caucasus 2.9.29.27468, ten
   cells re-read through the bridge equal to the bytes, journal min/max equal
   to the tiles, 58 ms a water tile. The whole-map half is Next.
2. **X5: the grid job, `config.json` and the seven tables.** Prepare is
   `identity` then `grid`; the hook pass is `config` (the theatre's facts,
   twenty lat/lon samples, the fill triple off three corners 500 km out) then
   `tables`, one file a step with `progress()`. No rectangle is read from a
   theatre file: ADR **0026**. Every shaper is terrain agnostic and never
   raises. *Verified by:* an agent offline, then live on Caucasus: all eight
   files re-read through the bridge matched byte for byte.
3. **X12: the run says where it is, and claims no more.** A job's `start`
   may return `progress()` beside its step; the run keeps a record refreshed
   every second that the window's line reads, logs a `heartbeat` line every
   ten seconds and one `totals` line at done. ADRs **0024**, **0025**.
4. **X2b: the run knows what it is extracting, and the build is the terrain's
   version.** `dcs_build` off `autoupdate.cfg`, `terrain_dir` by the
   shallowest id in an `entry.lua`, the three data files by path, size and
   payload field. ADRs **0022**, **0023**. Verified live on Caucasus and Sinai.
5. **X14: the window belongs in the editor, and says one thing at a time.**
   Skins off `me_aircraft_group.dlg`; ADR **0019**, ADR **0020**. PR **48**.

## Next

One task. The thing to pick up immediately.

**X6, the whole-map half of the live pass, again under ADR 0027** — the
first whole-map run authored Turkey's and Crimea's roadless mountains and
drew 590 000 km², which packs at 100 m; the rule is now a road within 5 km,
or breakpoints with a road within 25 km. Rerun on Caucasus from the session
talking to the user: the rectangle contains the hull ADR 0026 records and
exceeds it only along the north strip that has roads; then Stop and Start
resuming with "authored rectangle kept from the manifest". *Verified by:* a
maintainer at the install, watching the bar; PR **52**.

## Then

Max 5 entries, in dependency order. Task ids from `plan.md`.

1. **X7** — roads and railroads, each as two jobs, seeds then paths; carry 2.
2. **X8a / X8b / X8c / X9** — the mission pass, scenery, the model catalogue
   and failure handling. X ends here until `check-extract` exists.
3. **C1 / C2 / C3** — the Rust interlude X10 needs: scaffold, `synth` on the F2
   constants, `check-extract`. Closes P2 and MS0 past.
4. **X10** — the 10 x 10 km Kutaisi crop that `check-extract` accepts; its
   authored pair is null (ADR 0009, ADR 0026).
5. **C4** — `pack` reads the extract: the first consumer of the X10 crop, and
   where carry 1 is discharged.

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
| 2 | Roads: seed ids are positions in a fixed plan (lattice row-major at 1 km, then airdromes by numeric id, then towns by name), so a resume re-derives them; merge after every snap, into the lowest-id kept seed within 100 m, no chaining; neighbours are the 4 nearest kept seeds, ties by id; a pair is emitted at its lower id, so no seen set. Resume by streaming `roads.jsonl` in counted reads as steps and counting seed and pair lines; repair a partial tail by a streamed copy and rename, never a whole-file rewrite, because Caucasus is hundreds of MB and `write_file` holds the string. | X7 |
