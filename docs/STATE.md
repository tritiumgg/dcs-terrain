# State

Where the work is. Updated at the end of every working session, before handing
back. This file holds progress; `plan.md` holds the task graph and
`design.md` and `queries.md` hold the design.

**Last updated:** 2026-10-29T22:10Z

## Done

Newest first. Max 5 entries — drop the oldest when adding a sixth.

1. **Q6: routing.** A* over any graph of neighbors; `route` snaps each
   end part way along an edge and searches within the detour's ellipse,
   avoided regions cut, exposure costed; alternatives by the penalty
   method; `chokepoints` by betweenness, `not_ready` until `index` builds
   it; `trafficability` by an off-road cost field from slope and cover;
   a loop road in the synthetic terrain. *Verified by:* an agent, through
   `mise run check` (bridge, avoid box, the two alternatives, A* against
   Dijkstra and Bellman–Ford), and opt-in on the 8 terrains of
   2.9.30.28738: A* equal to Dijkstra on all 712 joined pairs
   (`data/route-search.md`).
2. **Q5: `radar_floor`; `coverage`, `approach_spectrum`, `unmask_profile`.**
   The viewshed sweep keeps each cell's floor; bands nest by construction;
   points and routes judged over the triangles, as `visible` judges.
   *Verified by:* an agent, through `mise run check`, and opt-in on
   2.9.30.28738 on all 8 terrains: bands nest, 99.7 % of cell-bands agree
   with `visible`, floor 96–153 ms at 20 km; the raster runs up to p99
   330 m low where the ground falls from the site (`data/radar-floor.md`).
3. **Q3: the siting criteria.** Every criterion compiles to a hard test
   and a 0–1 score over candidate sites; the layers read at a site's own
   cell are derived once, and each tile's zone map (least and greatest,
   or the codes held) skips it exactly; horizon criteria next, the rest in
   order of cost over failure share, sightlines last. *Verified by:* an
   agent, through `mise run check` (a passing and a failing site per
   criterion, a brute-force oracle, pruned against exhaustive), and opt-in
   on the 8 terrains of 2.9.30.28738: identical sites, 0–43 of 49–56 tiles
   skipped, 1.3–17× (`data/criteria-prune.md`).
4. **X1: the hook removed (ADR 0005).** `extractor/`, the Lua 5.1 build
   and tasks, `index --hook` and the import, and the `code`, `civilian`,
   `abandoned` and `class` fields are gone; `tables.json` is entry version
   3 and re-stamped after an update like the rest. *Verified by:* an
   agent, through `mise run check`, and opt-in on 2.9.30.28738: `check`
   and `describe` on all 8 terrains, each old entry rebuilt, no hook field.
5. **ADR 0005: drop the hook.** No operation reads the fields only it
   supplies but the airdrome id; the beacon files place that id by position
   where the name join fails. X1 now removes the hook, N12 adds the join.
   *Verified by:* reading, against 870 shipped missions on 2.9.30.28738.
## Next

One task. The thing to pick up immediately.

**N12** — airdrome ids by position (ADR 0005).
*Verified by:* an agent, opt-in.

## Then

Max 5 entries, in dependency order. Task ids from `plan.md`.

1. **Q11** — `airfield_info`.
2. **Q10** — `point_info`.
3. **Q4** — `score_site`, `find_sites`.
4. **Q12** — `exposure`.
5. **Q7** — `dcsterrain query` over every operation, `schema`.

Milestone in view: **MS1 Engine on the install**.

## Carries

Things a later task must not lose. Max 10 — `CLAUDE.md` has the rules.

| # | Carry | Discharged by |
|---|---|---|
