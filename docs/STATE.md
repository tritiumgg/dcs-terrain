# State

Where the work is. Updated at the end of every working session, before handing
back. This file holds progress; `plan.md` holds the task graph and
`design.md` and `queries.md` hold the design.

**Last updated:** 2026-11-02T06:12Z

## Done

Newest first. Max 5 entries — drop the oldest when adding a sixth.

1. **Towns by cluster.** A place is any building names do not rule out, sized by
   floor at about 100 m² a person, industrial buildings only linking; no
   per-terrain overrides. Census 0.63; Afghanistan and the Marianas have towns
   and cities (`data/places.md`). *Verified by:* an agent, through
   `mise run check`, and opt-in on the 8 terrains of 2.9.31.29434.
2. **N13: passages in the road graph.** Routes search the road file's own table
   and connectors, every edge is decoded, and the sortie keeps refused routes
   (ADR 0009). *Verified by:* an agent, opt-in on Caucasus of 2.9.31.29434 against
   V1's replies: 22 of 22 refused, 198 of 198 answered, largest +0.24 %; worst peak
   60.0 MiB on 8 terrains.
3. **C1 on Caucasus: the screenshot method.** Camera and screenshot calls
   proven, 241 models shot and judged, a rule that a name saying no building
   makes `misc`, 17 models moved, method in `plan.md`
   (`data/scenery-review.md`). *Verified by:* live: an agent drove the capture
   and read every frame with the user at DCS, on 2.9.31.29434.
4. **V1: the sortie.** `tools/validate` plans, runs and scores it. Caucasus on
   2.9.31.29434, seed 2: every check passes; routes on a target changed after two
   missed it, because the game snaps its ends to another road
   (`data/validation.md`). *Verified by:* live: an agent drove the bridge with the
   user at DCS.
5. **S1: the skill.** `skill/SKILL.md` and two references, which operation
   answers which question, by example, with what the numbers mean. *Verified
   by:* an agent: 13 fresh subagents given only the skill reached the right
   operations on Caucasus of 2.9.31.29434, and the FARP answer hashed equal to
   the direct `query`; a test holds every example to the command line.

## Next

One task. The thing to pick up immediately.

**R1** — release builds for the three platforms.
*Verified by:* CI, live.

## Then

Max 5 entries, in dependency order. Task ids from `plan.md`.

1. **R2** — the guide a newcomer follows.
2. **C1** — the models a name and a box cannot judge, from screenshots.

Milestone in view: **MS3 Assistant and release**.

## Carries

Things a later task must not lose. Max 10 — `CLAUDE.md` has the rules.

| # | Carry | Discharged by |
|---|---|---|
