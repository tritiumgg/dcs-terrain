# Live validation

The tool against a running DCS, through the bridge in the hook state, with the
Mission Editor open on the terrain. `tools/validate` draws the samples, and
`validation.json` holds every distribution.

## Caucasus, 2.9.31.29434, 2026-11-01

Timestamp 20261027-141234, seed 1, fingerprint
6293651208-203468160-1140069192. Not passed: routes miss their target.

| Check | Cases | Measured | Target |
|---|---|---|---|
| Height against `terrain.GetHeight` | 2 168 in 11 strata | largest 0.18 m, RMSE 0.004 m, p99 0.003 m | within 0.5 m |
| Water against `terrain.GetSurfaceType` | 2 168 | 100 % | 100 % |
| `visible` against `terrain.isVisible` | 600 random, 241 within 1 m of grazing | 100 %, 95 % interval from 99.4 % and 98.4 % | ≥ 99 % |
| MGRS against `coord.LOtoLL` and `coord.LLtoMGRS` | 460, seams included | largest 1.41 m, letters all equal, the projection alone 0.03 m | within 2 m |
| Route length against `terrain.findPathOnRoads` | 198 of 220 | 90.9 % within 5 %, p50 2.0 %, p95 6.5 %, largest 22.3 %, mean +2.4 % | 95 % within 5 % |
| Routes the game answers | 220 | 198: 22 refused, all rail | all |

The two largest height errors, 0.18 m and 0.08 m, are lake points; the next is
0.003 m. Rail is 38 compared, all within 5 %.

`net.dostring_in("server", …)` answered with `net.allow_dostring_in` unset and
`terrain.GetTerrainConfig("id")` returning `Caucasus`. `GetSurfaceType` answers
words: `land` and `sea` were seen. A chunk of 25 routes took up to 3.1 s of the
hook's CPU, a chunk of any other kind at most 100 ms.

### Why routes missed

The tool was longer in 189 of the 198 routes compared, and the game's path lies
on the decoded roads, within 5 m at every step of the three read. The tool joined
stretches at nodes, so a route walked out along a polyline's overrun and back,
and it could reverse through a rail switch by walking a stem out and back.

The router's table and connectors explain both (ADR 0009, `file-formats.md`).
The replies recorded above, routed offline by `tools/validate`'s `recorded` test
over the same snapped ends with no bound on the detour:

| Check | Cases | Before | Over the passages | Target |
|---|---|---|---|---|
| Longer than the game | 198 | 189 longer, largest +22.3 %, p50 +2.0 % | 180 longer, largest +0.24 %, p50 +0.03 % | none over 0.5 % |
| Routes the game refuses | 22 | 0 refused | 22 refused | all |
| Routes the game answers | 198 | all answered | all answered | all |

18 routes are shorter than the game's and all take only passages the file lists.
In 15 the difference is within how far the game's path starts and stops from the
ends asked: route 22, the largest at -0.97 %, runs one stretch and the game's path
stops 66 m past the end asked. Routes 56, 127 and 131 are shorter by more than
that, 0.02–0.04 %, and are untraced.

`route_alternatives` improves with it, on the 160 road routes at `k` 3: the first
route goes from p50 +2.3 % and largest +22.3 % of the game's length to +0.03 % and
+0.23 %, and the alternates from p50 +19 %, p90 +44 % to +15 %, +37 %, 137 of them
then and 134 now.

Against main, on every benchmark case of the 8 terrains, 56 of 547 answers change,
all reading roads: `route`, `route_alternatives`, `nearest` road and rail,
`point_info`, `describe` and `chokepoints`. Failing requests are the same before
and after. FARP and SAM still replay.

What each rule is worth, from a throwaway model of the same routes:

- Without the rule against turning back along a stretch, all 22 route: a route
  out along a stem and back passes a forbidden
  branch-to-branch turn.
- Without keeping passes to one network, 1 of the 22 routes, by a passage from
  rail to road.
- Reading a connector as one way refuses 166 of the routes the game answers, so
  connectors are not directed.

These are the same replies the design was fitted to. V1's re-run tests it on a
sample it has not seen, with the routes either side refuses kept in the sample.
