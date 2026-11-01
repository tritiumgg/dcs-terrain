# State

Where the work is. Updated at the end of every working session, before handing
back. This file holds progress; `plan.md` holds the task graph and
`design.md` and `queries.md` hold the design.

**Last updated:** 2026-11-01T20:44Z

## Done

Newest first. Max 5 entries — drop the oldest when adding a sixth.

1. **C1 on Caucasus: the screenshot method.** Camera and screenshot calls
   proven, 241 models shot and judged, a rule that a name saying no building
   makes `misc`, 15 models moved, method in `plan.md`
   (`data/scenery-review.md`). *Verified by:* live: an agent drove the capture
   and read every frame with the user at DCS, on 2.9.31.29434.
2. **V1: the sortie, first run.** `tools/validate` plans, runs and scores it.
   Caucasus on 2.9.31.29434: heights, water, `visible` and MGRS pass; routes
   miss, 90.9 % within 5 % and 22 rail routes the game refuses, so V1 stays
   open behind N13 (`data/validation.md`). *Verified by:* live: an agent drove
   the bridge with the user at DCS.
3. **S1: the skill.** `skill/SKILL.md` and two references, which operation
   answers which question, by example, with what the numbers mean. *Verified
   by:* an agent: 13 fresh subagents given only the skill reached the right
   operations on Caucasus of 2.9.31.29434, and the FARP answer hashed equal to
   the direct `query`; a test holds every example to the command line.
4. **Q18: performance.** Blocks, `roads.graph` and `scene.nodes` read through
   bounded views, a 4-way hierarchy over Morton-sorted triangles, wide radials,
   64 MiB of peak commit (ADR 0008). *Verified by:* an agent, through
   `mise run check`, and opt-in on the 8 terrains of 2.9.31.29434: worst peak
   61.1 MiB, answers unchanged; 5 of 22 operations reached their speed goal, the
   rest are recorded with causes (`data/q18-performance.md`).
5. **Q17: `find_sites` sightlines in parallel.** Each site on a free pool
   thread below normal priority, gathered in candidate order. *Verified by:*
   an agent, through `mise run check` (the same bytes on 1, 2, 3 and 8
   threads on `Synth`, ties included), and opt-in on the 8 terrains of
   2.9.30.28738: 0.18–7.3 s against 10 s, MarianaIslands 112 MiB; the
   Marianas are 1.14–1.32× over the 8-core cycle budget (`data/targets.md`).

## Next

One task. The thing to pick up immediately.

**N13** — connectors in the road graph.
*Verified by:* agent, opt-in.

## Then

Max 5 entries, in dependency order. Task ids from `plan.md`.

1. **V1** — re-run the routes on Caucasus, with the user at DCS.
2. **R1** — release builds for the three platforms.
3. **R2** — the guide a newcomer follows.
4. **C1** — the other seven terrains, from screenshots; Caucasus is done.

Milestone in view: **MS3 Assistant and release**.

## Carries

Things a later task must not lose. Max 10 — `CLAUDE.md` has the rules.

| # | Carry | Discharged by |
|---|---|---|
