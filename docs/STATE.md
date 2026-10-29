# State

Where the work is. Updated at the end of every working session, before handing
back. This file holds progress; `plan.md` holds the task graph and
`design.md` and `queries.md` hold the design.

**Last updated:** 2026-10-29T15:49Z

## Done

Newest first. Max 5 entries — drop the oldest when adding a sixth.

1. **Q2: `visible`, `viewshed`; the GeoJSON writer.** A sightline steps
   the raster every half cell, reading tiles only as it reaches them, its
   ends on the mesh triangle's height; `viewshed` sweeps one radial per
   outer mask cell and judges each cell against the radial nearest it;
   outlines run along cell edges, simplified to half a cell; every geometry
   is projected into an RFC 7946 `FeatureCollection`. *Verified by:* an
   agent, through `mise run check` (hill clearance within 1 m of the closed
   form; 50 of 20 108 cells differ from brute force, the share equal), and
   opt-in on 2.9.30.28738: Caucasus `visible` 12–17 ms, `viewshed` 120 ms
   at 20 km and 644 ms at 50 km.
2. **Q1: `describe`, `sample`, `geo`, `nearest`, `scenery_in`.** `query`
   dispatches through a table of handlers and takes `--allow-crs-residual`;
   `Source::points` answers block by block, testing a block of few points
   triangle by triangle; the synthetic terrain gained tables; MGRS for
   `geo`; the radius layers' radii are in `queries.md`. *Verified by:* an
   agent, through `mise run check`, and opt-in on 2.9.30.28738: 10 k
   scattered points 1.5 s through `sample` on GermanyCW, OS cache warm;
   10 k clustered on MarianaIslands 74 ms.
3. **N10: `dcsterrain check`.** It reports each terrain file's class and
   version and why no decoder reads it, each cache entry's state as found,
   the horizon tiles, the tables with the Lua each ran and left unresolved
   and the hook import, the catalog's top models and tallest structures,
   and the validation figures; it exits 1 naming each problem, and a file
   no decoder reads refuses only what reads its role. *Verified by:* an
   agent, through `mise run check` (a renamed class and a bumped version
   each refused by name, the tables still read), and opt-in on
   2.9.30.28738: all eight terrains pass; Caucasus in 0.4 s with an empty
   cache.
4. **N9: horizon tiles and `enclosure`.** `horizon::compute` marches 32
   bins per 200 m cell to 40 km with a max-pyramid skip, at most 64 tiles
   per call; an install keeps one file per tile in `horizon/` through
   `cache::Parts`, re-stamped per tile after an update; `Source::horizon`
   and `Derived::Enclosure` read them. *Verified by:* an agent, through
   `mise run check` (brute-force march agrees; hill angle within 0.25°),
   and opt-in on 2.9.30.28738: Caucasus 20 × 20 km first touch 3.4–4.9 s,
   cached 7–10 ms, one new cell 0.24 s.
5. **N8: per-query layers.** `layers::derive` computes `valid`, slope,
   aspect, roughness, TPI at any radius, capped distances, counts, densities
   and cover shares over a rectangle of cells from one padded window;
   `Source` gained `road_graph`, `structures_in` and `instances_in`.
   *Verified by:* an agent, through `mise run check`, and opt-in on
   2.9.30.28738: every triangle of each cover material subset carries its
   bit on all eight terrains; each layer over 20 × 20 km in 0.3–57 ms warm.

## Next

One task. The thing to pick up immediately.

**N11** — the exact sightline experiment, walking mesh triangle edges.
*Verified by:* an agent, opt-in.

## Then

Max 5 entries, in dependency order. Task ids from `plan.md`.

1. **Q3** — criteria, the closed set; tile skipping; LOS criteria last.
2. **Q11** — `airfield_info`.
3. **Q10** — `point_info`.
4. **Q4** — `score_site`, `find_sites`.
5. **Q5** — `radar_floor`; `coverage`, `approach_spectrum`, `unmask_profile`.

Milestone in view: **MS1 Engine on the install**.

## Carries

Things a later task must not lose. Max 10 — `CLAUDE.md` has the rules.

| # | Carry | Discharged by |
|---|---|---|
| 1 | The hook still writes v1, so `extractor/test/constants.lua` pins `FORMAT_VERSION` to 1 rather than the shared 2; restore it, and drop the fill and road-seed constants if nothing else reads them. | X1 |
