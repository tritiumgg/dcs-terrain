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

### Why routes miss

The tool is longer in 189 of the 198 routes compared, and the
game's path lies on the decoded roads: routing each step of three of its paths
through the tool, every step snapped within 5 m. Those steps show the excess is
at junctions: a 5 m step of the game's path is 184 m in the tool.

At the one junction read in detail, id 3359 near x 12476, z 470208, edge 1681
runs about 90 m past the junction to its last vertex, and the graph joins the
junction at that vertex, so a route walks out to it and back. The ends naming
one id were read as stopping short of the junction; this one overruns it.

On rail the tool finds routes the game refuses. Asked for three refused routes
from their first point to each of the tool's waypoints in turn, the game
answers up to one waypoint and for none after it.

### Connectors

Measured from the road file with a throwaway scan, 2.9.31.29434:

- 27 955 runs framed `u32 3, u32 a, u32 b, u32 n`, points. Of 15 262 values of
  `a` and `b`, 15 259 map by geometry to an edge whose polyline holds the run's
  end within 3 m.
- That edge's file ordinal is the value less an offset that climbs from 0 to 9
  through the file, not always evenly. Why is not known.
- 27 819 connectors join two edges sharing a junction id, 125 do not, 11 did
  not map.
- At a junction, pairs of edges with no connector: rail 390 of 2 003, road 471
  of 20 096.

The reading that fits is that a connector is a permitted passage through a
junction, attached where the road really reaches it: the overrun would end at
the attachment, and a rail switch would forbid some turns, which a graph of
nodes cannot express. That is not proved: no refused route has been tied to a
pair without a connector.
