# ADR 0009: Route over the road file's own passages

## Status

Accepted

## Context

**Affects:** `design.md` §3.2 (the `.rn4` row), §4 (`roads.graph`), §6.5 and the
route target; `queries.md` `route`; `data/file-formats.md`; `plan.md` N13.

The road graph joined stretches at the nodes their junction ids name. V1's first
sortie, Caucasus of 2.9.31.29434, seed 1, measured that against
`terrain.findPathOnRoads`: of 220 routes, 198 compared, 90.9 % within 5 %,
largest 22.3 %, the tool longer in 189; and the game refused 22, all rail, which
the tool answered (`data/validation.md`).

The `.rn4`'s undecoded node table turned out to be the game's routing graph, on
all eight installed terrains. It has exactly two ends to an edge. A link to the
edge's other end runs along it, a link to an end of the same junction is a
passage, which a connector draws, and a `oneway` edge has its run one way
(`data/file-formats.md`). Each end has one connection point on its edge. The
polyline runs on past it, p50 37 m and p99 382 m on Caucasus, which is what made
the tool long. At a junction, 390 of 2 003 rail pairs and 471 of 20 096 road
pairs have no passage.

Reading the table also showed that the decoder lost edges. It took a run for an
edge by a zero before its point count, which is a property count, and refused a
vertex within 1 m of x = 0: 96 267 edges on Syria, 20 630 on GermanyCW, 6 606 on
MarianaIslands, 4 138 on PersianGulf, 391 on Nevada, 112 on Afghanistan, plus
those near the line, 9 on Caucasus. Its climbing offset between connector numbers
and edges was that loss.

The goal the user set is the most efficient route among those the game accepts,
and likewise for alternatives: the game cannot be asked at query time, so the rule
has to come from the file. Refusing what the game refuses is required, because a
route the game will not drive is worth nothing to a mission.

Over V1's replies, routes that alternate a passage and a run along a stretch,
stay on one network and start or stop anywhere on a stretch's line refuse all 22
and answer all 198, at most 0.24 % longer than the game, p50 0.03 %. Without
alternating all 22 route, out along a stem and back, so turning back is what the
game refuses; without keeping to one network 1 of the 22 routes, by one of the
31 connectors from rail to road. Taking a connector as one way wrongly refuses
166 of the 198. On 18 routes the tool is shorter, by 0.97 % at most: most by how
far the game's own path runs past the ends asked, and 3 by 0.02–0.04 % more than
that, over passages the file lists.

## Decision

The decoder walks the file's own framing: the table, then each edge in order by a
head carrying the ids the table gives it. `roads.graph` holds, beside the edges
every other operation reads, a run for each stretch and the passes between stretch
ends, with each end's place and its share along the edge. `route` and
`route_alternatives` search those, alternating a pass and a run, so a route
never turns back along a stretch and takes no way through a junction the file does
not list; a pass between road and rail is left out, and a `oneway` stretch runs one
way. Nodes, contraction and `Adjacency` stay for betweenness, `chokepoints` and every
other use of the graph, and snapping stays on the edges.

The cache bar for speed does not apply to this growth: it covers data needed for
correct answers, and only that, since no version without it gives them. The growth
is recorded per terrain below, and every operation stays within ADR 0008's 64 MiB.

### Alternatives considered

- **Trim each edge at its connector and keep nodes.** It fixes the length and cannot
  say that a turn is forbidden, so rail stays wrong.
- **Compare headings at a junction** to forbid sharp turns. It is a heuristic the
  file does not state, and the refused turns are not sharp: a switch's two branches
  run nearly parallel.
- **Store forbidden turns only.** Smaller, but stations and stretch boundaries
  still have to be stored, and each exception is decoded through a default the file
  does not hold.
- **Read the table from the `.rn4` per query.** It is unindexed, 110 MiB on GermanyCW,
  and connectors are scattered through a 2.6 GiB file.

## Consequences

- Terrains that gain roads change every answer that reads them; those answers were
  wrong. Edge ids, betweenness and `roads.jsonl` ids shift with them.
- A leg now includes the pass that led onto it, so a route has more legs where a
  contracted edge holds several stretches.
- `route` no longer builds an adjacency in memory, and its search holds an entry per
  stretch end. `roads.graph` grows:

| Terrain | `roads.graph`, bytes | Whole cache, bytes | Growth of the cache |
|---|---|---|---|
| Afghanistan | 71 789 640 → 99 486 500 | 88 196 277 → 115 894 101 | +31 % |
| Caucasus | 9 136 480 → 9 974 572 | 12 893 997 → 13 732 149 | +6.5 % |
| GermanyCW | 148 978 648 → 245 057 676 | 182 256 934 → 278 399 066 | +53 % |
| MarianaIslands | 5 144 208 → 9 633 128 | 5 938 895 → 10 438 911 | +76 % |
| MarianaIslandsWWII | 3 935 760 → 6 802 740 | 4 779 091 → 7 646 059 | +60 % |
| Nevada | 20 498 240 → 24 392 616 | 23 177 692 → 27 072 932 | +17 % |
| PersianGulf | 16 705 336 → 25 719 960 | 27 709 118 → 36 751 910 | +33 % |
| Syria | 106 338 144 → 171 971 016 | 127 274 210 → 193 224 262 | +52 % |

  Each run costs 28 bytes and each pass 8, with 4 for each end and edge in the
  offsets; the figures include the edges recovered. The whole cache is `index`'s,
  no horizon tiles. The worst peak commit of any case on any terrain is 60.0 MiB,
  `exposure_10_sites` on MarianaIslands, and of a route 58.5 MiB,
  `route_alternatives_50km` on GermanyCW.

- `oneway` is read from the table and has been seen working nowhere live: Caucasus
  has none. V1 on another terrain would settle it. `inversetraffic` is decoded and
  unused.
- The fit is to replies on one terrain, so V1's re-run, on a sample not seen here, is
  what would reopen this. A route the game answers and the tool refuses reopens it at
  once.
