# State

Where the work is. Updated at the end of every working session, before handing
back. This file holds progress; `plan.md` holds the task graph and
`design.md` and `queries.md` hold the design.

**Last updated:** 2026-10-30T08:20Z

## Done

Newest first. Max 5 entries — drop the oldest when adding a sixth.

1. **Q7: `query` over every operation, batches, `schema`.** With no
   operation, JSON lines on stdin, each answered in order and flushed; a bad
   line is that line's `issues`; the terrain opens once, again only when its
   files change. `schema` adds batch lines and the issue codes; the README
   pass drops the banner. *Verified by:* an agent, through `mise run check`
   (goldens, one process per request, every answer against the schema), and
   opt-in on the 8 terrains of 2.9.30.28738: the 22 operations each answer,
   100 lines to one opening, 0.7–1.5 s. Kola's folder holds no terrain.
2. **Q15: `towns` and the `places` entry.** Settlement buildings linked at
   a 200 m gap by union-find over a grid hash, sized, given a form and
   named; outlines on request; `nearest` kind `place`, `point_info`'s place.
   *Verified by:* an agent, through `mise run check` (linkage equal to
   every pair, places and lookup against brute force on `Synth`), and
   opt-in on the 8 terrains of 2.9.30.28738: 123–21 788 places, built in
   0.5–28 s, 8 000 points' places equal to brute force; 72 Caucasus census
   towns rank at 0.63, the bar 0.6 agreed (`data/places.md`).
3. **Q13: `validate_placements`.** Point, box, circle and polygon: every
   cell the shape covers part of for water, depth, slope and cover; scenery
   by box, trees by circle, and the runway surface's own triangles, by
   separating axes and exact distance, touching counted within 1 µm.
   *Verified by:* an agent, through `mise run check` (each issue kind and
   shape on `Synth`, edge-only contact, coverage against clipping), and
   opt-in on the 8 terrains of 2.9.30.28738: a building, water and runway
   each reported, 1 000 placements a terrain equal to brute force,
   0.76–2.5 s warm (`data/placements.md`).
4. **Q9: the FARP and SAM replay on Caucasus.** The probe of 2.9.29.27278
   recomputed from point heights and the road graph: its eight FARP
   candidates 0 m off by bottleneck assignment, each rejection count within
   0.4 %; SAM site 1 masked from the seven western points, 10 km about the
   site; unmask at 7 800 m as the probe. `find_sites` alone ties on the
   plain. *Verified by:* an agent, opt-in on 2.9.30.28738, and the request
   shapes on the 7 other terrains (`data/replay.md`).
5. **Q11: `airfield_info`.** Runways from the tables, true courses by the
   normal section on WGS84, widths the median of nine stations across the
   mesh's runway paint; helipads answer their stands, airdromes with no id
   their `name_id`. *Verified by:* an agent, through `mise run check`, and
   opt-in on the 8 terrains of 2.9.30.28738: all 565 airdromes answer, 363
   runways equal the tables, widths and elevations a scan's; 0.6–5.1 ms warm
   p50 (`data/airfield-info.md`).
## Next

One task. The thing to pick up immediately.

**Q8** — benchmarks on `Synth` and, opt-in, the install.
*Verified by:* an agent, opt-in.

## Then

Max 5 entries, in dependency order. Task ids from `plan.md`.

1. **S1** — the skill an assistant loads.
2. **V1** — the live validation sortie.
3. **R1** — release builds for the three platforms.
4. **R2** — the guide a newcomer follows.
5. **C1** — review the scenery catalog of every terrain without overrides.

Milestone in view: **MS1 Engine on the install**.

## Carries

Things a later task must not lose. Max 10 — `CLAUDE.md` has the rules.

| # | Carry | Discharged by |
|---|---|---|
