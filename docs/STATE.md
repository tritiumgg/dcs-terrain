# State

Where the work is. Updated at the end of every working session, before handing
back. This file holds progress; `plan.md` holds the task graph and
`design.md` and `queries.md` hold the design.

**Last updated:** 2026-10-29T21:43Z

## Done

Newest first. Max 5 entries — drop the oldest when adding a sixth.

1. **Q5: `radar_floor`; `coverage`, `approach_spectrum`, `unmask_profile`.**
   The viewshed sweep keeps each cell's floor; bands nest by construction;
   points and routes judged over the triangles, as `visible` judges.
   *Verified by:* an agent, through `mise run check`, and opt-in on
   2.9.30.28738 on all 8 terrains: bands nest, 99.7 % of cell-bands agree
   with `visible`, floor 96–153 ms at 20 km; the raster runs up to p99
   330 m low where the ground falls from the site (`data/radar-floor.md`).
2. **Q3: the siting criteria.** Every criterion compiles to a hard test
   and a 0–1 score over candidate sites; the layers read at a site's own
   cell are derived once, and each tile's zone map (least and greatest,
   or the codes held) skips it exactly; horizon criteria next, the rest in
   order of cost over failure share, sightlines last. *Verified by:* an
   agent, through `mise run check` (a passing and a failing site per
   criterion, a brute-force oracle, pruned against exhaustive), and opt-in
   on the 8 terrains of 2.9.30.28738: identical sites, 0–43 of 49–56 tiles
   skipped, 1.3–17× (`data/criteria-prune.md`).
3. **X1: the hook removed (ADR 0005).** `extractor/`, the Lua 5.1 build
   and tasks, `index --hook` and the import, and the `code`, `civilian`,
   `abandoned` and `class` fields are gone; `tables.json` is entry version
   3 and re-stamped after an update like the rest. *Verified by:* an
   agent, through `mise run check`, and opt-in on 2.9.30.28738: `check`
   and `describe` on all 8 terrains, each old entry rebuilt, no hook field.
4. **ADR 0005: drop the hook.** No operation reads the fields only it
   supplies but the airdrome id; the beacon files place that id by position
   where the name join fails. X1 now removes the hook, N12 adds the join.
   *Verified by:* reading, against 870 shipped missions on 2.9.30.28738.
5. **Q16: the performance pass (ADR 0004).** `cargo bench --bench query`
   times every case on synth and, opt-in, an install, cold and warm, with
   allocations and the cache's size; a grid index for road edges; triangles
   binned at a size they span few bins of; large work spread over every
   core below normal priority, `--threads` to cap it; `tables.json` parsed
   on first use (entry version 2). *Verified by:* an agent, through
   `mise run check`, and opt-in on 2.9.30.28738: 2.02× over 164 install
   cases, answers unchanged, no cache entry larger (`data/query-bench.md`).
## Next

One task. The thing to pick up immediately.

**N12** — airdrome ids by position (ADR 0005).
*Verified by:* an agent, opt-in.

## Then

Max 5 entries, in dependency order. Task ids from `plan.md`.

1. **Q11** — `airfield_info`.
2. **Q10** — `point_info`.
3. **Q4** — `score_site`, `find_sites`.
4. **Q6** — `route`, `route_alternatives`, `chokepoints`, `trafficability`.
5. **Q12** — `exposure`.

Milestone in view: **MS1 Engine on the install**.

## Carries

Things a later task must not lose. Max 10 — `CLAUDE.md` has the rules.

| # | Carry | Discharged by |
|---|---|---|
