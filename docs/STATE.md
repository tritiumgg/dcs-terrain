# State

Where the work is. Updated at the end of every working session, before handing
back. This file holds progress; `plan.md` holds the task graph and
`design.md` and `queries.md` hold the design.

**Last updated:** 2026-10-30T17:44Z

## Done

Newest first. Max 5 entries — drop the oldest when adding a sixth.

1. **Q17: `find_sites` sightlines in parallel.** Each site on a free pool
   thread below normal priority, gathered in candidate order. *Verified by:*
   an agent, through `mise run check` (the same bytes on 1, 2, 3 and 8
   threads on `Synth`, ties included), and opt-in on the 8 terrains of
   2.9.30.28738: 0.18–7.3 s against 10 s, MarianaIslands 112 MiB; the
   Marianas are 1.14–1.32× over the 8-core cycle budget (`data/targets.md`).
2. **Benchmark decisions.** Cycle budgets on 8 cores for parallel cases,
   a 64 MiB block budget, `point_info` fresh 150 ms, memory 150 MiB (ADR
   0007), Q17 added. *Verified by:* an agent, through
   `mise run bench-bounds`, and opt-in on the Marianas at 64 MiB.
3. **Q8: benchmarks on cycles.** Every operation judged on reference cycles
   with the wall beside them, on each platform; `Synth` held to 3× the
   targets over 9 rounds by hand, not in CI; the install cold and warm, one
   call and a 100-line batch end to end. *Verified by:* an agent, through
   `mise run bench-bounds`, with 8 spinners beside it, and opt-in on the 8
   terrains of 2.9.30.28738. Targets missed, each with its cause and five
   decisions open (`data/targets.md`).
4. **Q7: `query` over every operation, batches, `schema`.** With no
   operation, JSON lines on stdin, each answered in order and flushed; a bad
   line is that line's `issues`; the terrain opens once, again only when its
   files change. `schema` adds batch lines and the issue codes; the README
   pass drops the banner. *Verified by:* an agent, through `mise run check`
   (goldens, one process per request, every answer against the schema), and
   opt-in on the 8 terrains of 2.9.30.28738: the 22 operations each answer,
   100 lines to one opening, 0.7–1.5 s. Kola's folder holds no terrain.
5. **Q15: `towns` and the `places` entry.** Settlement buildings linked at
   a 200 m gap by union-find over a grid hash, sized, given a form and
   named; outlines on request; `nearest` kind `place`, `point_info`'s place.
   *Verified by:* an agent, through `mise run check` (linkage equal to
   every pair, places and lookup against brute force on `Synth`), and
   opt-in on the 8 terrains of 2.9.30.28738: 123–21 788 places, built in
   0.5–28 s, 8 000 points' places equal to brute force; 72 Caucasus census
   towns rank at 0.63, the bar 0.6 agreed (`data/places.md`).

## Next

One task. The thing to pick up immediately.

**S1** — the skill an assistant loads.
*Verified by:* an agent.

## Then

Max 5 entries, in dependency order. Task ids from `plan.md`.

1. **V1** — the live validation sortie.
2. **R1** — release builds for the three platforms.
3. **R2** — the guide a newcomer follows.
4. **C1** — review the scenery catalog of every terrain without overrides.

Milestone in view: **MS3 Assistant and release**.

## Carries

Things a later task must not lose. Max 10 — `CLAUDE.md` has the rules.

| # | Carry | Discharged by |
|---|---|---|
