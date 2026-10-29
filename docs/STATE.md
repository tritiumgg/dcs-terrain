# State

Where the work is. Updated at the end of every working session, before handing
back. This file holds progress; `plan.md` holds the task graph and
`design.md` and `queries.md` hold the design.

**Last updated:** 2026-10-29T18:50Z

## Done

Newest first. Max 5 entries — drop the oldest when adding a sixth.

1. **Q16: the performance pass (ADR 0004).** `cargo bench --bench query`
   times every case on synth and, opt-in, an install, cold and warm, with
   allocations and the cache's size; a grid index for road edges; triangles
   binned at a size they span few bins of; large work spread over every
   core below normal priority, `--threads` to cap it; `tables.json` parsed
   on first use (entry version 2). *Verified by:* an agent, through
   `mise run check`, and opt-in on 2.9.30.28738: 2.02× over 164 install
   cases, answers unchanged, no cache entry larger (`data/query-bench.md`).
2. **N11: `visible` over the mesh's triangles (ADR 0003).** `Source::along`
   clips a segment to every triangle under it; `sightline::exact` sweeps
   them by rank, the gap linear between crossings; the synthetic terrain
   cuts its closed form every eighth of a cell; clearance reaches the ends.
   *Verified by:* an agent, through `mise run check`, and opt-in on
   2.9.30.28738 over 2000 lines per terrain: agrees with the raster on
   99.8 % to a 2 m target; the raster hid 3.5–4.3 % of lines to a ground
   target; 2.4× (Caucasus) to 8× (GermanyCW) cheaper (`data/sightline-exact.md`).
3. **Q2: `visible`, `viewshed`; the GeoJSON writer.** A sightline steps
   the raster every half cell, reading tiles only as it reaches them, its
   ends on the mesh triangle's height; `viewshed` sweeps one radial per
   outer mask cell and judges each cell against the radial nearest it;
   outlines run along cell edges, simplified to half a cell; every geometry
   is projected into an RFC 7946 `FeatureCollection`. *Verified by:* an
   agent, through `mise run check` (hill clearance within 1 m of the closed
   form; 50 of 20 108 cells differ from brute force, the share equal), and
   opt-in on 2.9.30.28738: Caucasus `visible` 12–17 ms, `viewshed` 120 ms
   at 20 km and 644 ms at 50 km.
4. **Q1: `describe`, `sample`, `geo`, `nearest`, `scenery_in`.** `query`
   dispatches through a table of handlers and takes `--allow-crs-residual`;
   `Source::points` answers block by block, testing a block of few points
   triangle by triangle; the synthetic terrain gained tables; MGRS for
   `geo`; the radius layers' radii are in `queries.md`. *Verified by:* an
   agent, through `mise run check`, and opt-in on 2.9.30.28738: 10 k
   scattered points 1.5 s through `sample` on GermanyCW, OS cache warm;
   10 k clustered on MarianaIslands 74 ms.
5. **N10: `dcsterrain check`.** It reports each terrain file's class and
   version and why no decoder reads it, each cache entry's state as found,
   the horizon tiles, the tables with the Lua each ran and left unresolved
   and the hook import, the catalog's top models and tallest structures,
   and the validation figures; it exits 1 naming each problem, and a file
   no decoder reads refuses only what reads its role. *Verified by:* an
   agent, through `mise run check` (a renamed class and a bumped version
   each refused by name, the tables still read), and opt-in on
   2.9.30.28738: all eight terrains pass; Caucasus in 0.4 s with an empty
   cache.
## Next

One task. The thing to pick up immediately.

**Q3** — criteria, the closed set; tile skipping; LOS criteria last.
*Verified by:* an agent.

## Then

Max 5 entries, in dependency order. Task ids from `plan.md`.

1. **Q11** — `airfield_info`.
2. **Q10** — `point_info`.
3. **Q4** — `score_site`, `find_sites`.
4. **Q5** — `radar_floor`; `coverage`, `approach_spectrum`, `unmask_profile`.
5. **Q6** — `route`, `route_alternatives`, `chokepoints`, `trafficability`.

Milestone in view: **MS1 Engine on the install**.

## Carries

Things a later task must not lose. Max 10 — `CLAUDE.md` has the rules.

| # | Carry | Discharged by |
|---|---|---|
| 1 | The hook still writes v1, so `extractor/test/constants.lua` pins `FORMAT_VERSION` to 1 rather than the shared 2; restore it, and drop the fill and road-seed constants if nothing else reads them. | X1 |
