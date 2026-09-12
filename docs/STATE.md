# State

Where the work is. Updated at the end of every working session, before handing
back. This file holds progress; `plan.md` holds the task graph and
`decisions/` holds what has diverged from the frozen documents.

**Last updated:** 2026-09-12T00:46Z

## Done

Newest first. Max 5 entries — drop the oldest when adding a sixth.

1. **X6: the pre-sweep, `water` and `height`.** Prepare is `identity`,
   `presweep`, `grid`: the pre-sweep walks a 5 km lattice one cell a step, a
   road snap first and a 2 km line of heights only where a road lies 5 to
   25 km off (ADR **0027**, after the spec's rule authored Caucasus's roadless
   mountains), every far snap kept as the disc it clears; it skips itself on
   a crop run and on a directory whose manifest carries the rectangle. The
   hook pass is `config`, `tables`, `water`, `height`, one tile a step at the
   terrain module's own per-call speed; water builds `run.skip` and a resumed
   sweep reads journalled tiles back for the sea ones. *Verified by:* an agent
   offline, then the maintainer on Caucasus 2.9.29.27468: a 5 km Kutaisi
   crop's cells equal the bridge's, the whole map's rectangle is 470 x 785 km
   in 19 s (38 km past the hull on the north where roads run, 14 km and 56 km
   short of it over roadless mountains and sea), water 118 s, height 71 s,
   and Stop then Start resumed without measuring again. PR **52**.
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
5. **X13: the window has controls, and the live pass found two bugs the offline
   tests could not.** ADR **0017**, ADR **0018**. PR **47**.

## Next

One task. The thing to pick up immediately.

**X7** — roads and railroads, each as two jobs, seeds then paths; carry 2. A
snap costs by its distance to the nearest road (84 ms at 500 km, measured in
X6), so seeds are placed only inside the authored rectangle's non-skipped
tiles and the pre-sweep's disc bound applies to seeds too. *Verified by:* an
agent offline over the driver, then a sampled path re-read through the bridge
on the Kutaisi crop; a maintainer watching the whole-map roads sweep start.

## Then

Max 5 entries, in dependency order. Task ids from `plan.md`.

1. **X8a / X8b / X8c / X9** — the mission pass, scenery, the model catalogue
   and failure handling. X ends here until `check-extract` exists.
2. **C1 / C2 / C3** — the Rust interlude X10 needs: scaffold, `synth` on the F2
   constants, `check-extract`. Closes P2 and MS0 past.
3. **X10** — the 10 x 10 km Kutaisi crop that `check-extract` accepts; its
   authored pair is null (ADR 0009, ADR 0026).
4. **C4** — `pack` reads the extract: the first consumer of the X10 crop, and
   where carry 1 is discharged.
5. **C5a** — `pack` skeleton: schema, `meta` with the authored rectangle
   (ADR 0008), one transaction.

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
