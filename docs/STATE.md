# State

Where the work is. Updated at the end of every working session, before handing
back. This file holds progress; `plan.md` holds the task graph and
`design.md` and `queries.md` hold the design.

**Last updated:** 2026-10-29T04:53Z

## Done

Newest first. Max 5 entries — drop the oldest when adding a sixth.

1. **N9: horizon tiles and `enclosure`.** `horizon::compute` marches 32
   bins per 200 m cell to 40 km with a max-pyramid skip, at most 64 tiles
   per call; an install keeps one file per tile in `horizon/` through
   `cache::Parts`, re-stamped per tile after an update; `Source::horizon`
   and `Derived::Enclosure` read them. *Verified by:* an agent, through
   `mise run check` (brute-force march agrees; hill angle within 0.25°),
   and opt-in on 2.9.30.28738: Caucasus 20 × 20 km first touch 3.4–4.9 s,
   cached 7–10 ms, one new cell 0.24 s.
2. **N8: per-query layers.** `layers::derive` computes `valid`, slope,
   aspect, roughness, TPI at any radius, capped distances, counts, densities
   and cover shares over a rectangle of cells from one padded window;
   `Source` gained `road_graph`, `structures_in` and `instances_in`.
   *Verified by:* an agent, through `mise run check`, and opt-in on
   2.9.30.28738: every triangle of each cover material subset carries its
   bit on all eight terrains; each layer over 20 × 20 km in 0.3–57 ms warm.
3. **N7: scenery classes.** Every model has a class and, if a building, a
   settlement family, each with its source; made in memory from the model
   table, not cached. `Scenery::each_structure_in` drops `effect`; the
   catalog counts each model's objects from the pool's ids. The Caucasus
   overrides moved into the crate. *Verified by:* an agent, through
   `mise run check`, and opt-in on 2.9.30.28738: all 87 Caucasus overrides
   apply, its 17 143 smoke objects are `effect`; all eight terrains classify
   in ≤ 27 ms and count every object.
4. **N6: the road graph, and one cache directory per terrain
   (ADR 0002).** `roads.graph` holds the junction graph of roads and rails,
   contracted, with each edge's length and the water under it;
   `roads.betweenness` is built from it on two low-priority threads. The
   road file is scanned through a 16 MiB window. An entry whose files
   survive an update is re-stamped, not rebuilt; `cache prune` is gone.
   The synthetic terrain gained an east road, so the bridge carries the
   most. *Verified by:* an agent, through `mise run check`, and opt-in on
   2.9.30.28738: Caucasus's graph in 0.5 s, GermanyCW's in 9.8 s and its
   betweenness in 29 s; a build change re-stamps both in 29 ms.
5. **N5: `tables.json` and the hook import.** One JSON cache entry, keyed
   inside the JSON, holds the airdromes, runways, stands, beacons, radio,
   towns, nodes and the fit; `index --hook` merges the airdrome fields only
   the sim knows, refusing another build's directory by name, and `index`
   refuses a fit over 2 m without `--allow-crs-residual`. *Verified by:* an
   agent, through `mise run check`, and opt-in on 2.9.30.28738: every
   Caucasus table, and with the hook every airdrome field, equals the live
   extract; all eight terrains build in 54–560 ms, up to 2.4 MB.

## Next

One task. The thing to pick up immediately.

**N10** — `dcsterrain check --install --terrain`.
*Verified by:* an agent, opt-in on Caucasus.

## Then

Max 5 entries, in dependency order. Task ids from `plan.md`.

1. **Q1** — `describe`, `sample`, `geo`, `nearest`, `scenery_in`; `query
   --allow-crs-residual`.
2. **Q2** — `visible`, `viewshed`; the shared GeoJSON writer.
3. **Q3** — criteria, the closed set; tile skipping; LOS criteria last.
4. **Q11** — `airfield_info`.
5. **Q10** — `point_info`.

Milestone in view: **MS1 Engine on the install**.

## Carries

Things a later task must not lose. Max 10 — `CLAUDE.md` has the rules.

| # | Carry | Discharged by |
|---|---|---|
| 1 | The hook still writes v1, so `extractor/test/constants.lua` pins `FORMAT_VERSION` to 1 rather than the shared 2; restore it, and drop the fill and road-seed constants if nothing else reads them. | X1 |
| 3 | `tables.json` keeps `lua_included`, the shared scripts each `dofile` ran, and `lua_unresolved`, each name nothing assigns and each module the sandbox stood in for: report both in `check`, with the hook import's `dropped`. | N10 |
| 6 | On MarianaIslands the blocks under one point outgrow the 32 MiB budget: 10 k lattice points over a single block take 250–290 ms, and 35–41 ms with every block held (2.9.30.28738). Answer every `sample` block by block, not only scattered points. | Q1 |
| 7 | `sample` and the criteria name `count_building`, `count_industrial`, `tree_density` and the `cover` shares, but no document gives the radius each is measured within; `layers::Derived` takes one. Choose the defaults and write them into `queries.md`. | Q1 |
