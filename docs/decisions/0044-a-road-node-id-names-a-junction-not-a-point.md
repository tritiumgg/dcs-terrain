# ADR 0044: A road node id names a junction, not a point, and the catch-all id is written null

## Status

Accepted

## Context

**Affects:** ADR 0039 "roads.jsonl, railroads.jsonl" (`a` and `b`) and its
Consequences paragraph on the road graph; `plan.md` E3, C9.

ADR 0039 writes each road and rail edge with the two u32 before its point
count as `a` and `b`, and builds the graph from them:

> Edges that share a node id meet at that node, so `pack` builds the graph
> from `a` and `b` and places each node at the mean of the endpoints that
> name it.

and leaves two things to check:

> that the two u32 before the zero word are the edge's node ids, and that
> edges sharing an id meet at one point. [...] E3 checks both, by the
> endpoints of edges sharing an id, and C9 reports the spread of each node's
> endpoints; a large one reopens the graph rule.

E3 decoded the map-wide `.rn4` of the eight installed maps on DCS
**2.9.30.28738**. On Caucasus, 15 267 point runs carry the zero word and are
edges; 12 221 of them are roads or rail, over 9 343 node ids.

**`a` is the node at the first vertex and `b` at the last.** Only 35 places
on Caucasus have two edge ends within 0.5 m of each other. At 34 of them the
ends carry the same id when `a` is read at the first vertex and `b` at the
last; read the other way round, none do. Node degrees are what a road
network has: 1 321 nodes of degree 1, 2 000 of 2, 4 014 of 3, 1 009 of 4.

**Edges stop short of their node.** The ends naming one node lie apart,
measured as the farthest end from their mean: on Caucasus roads p50 27 m,
p90 81 m, p99 176 m; rail p50 23 m, p90 83 m, p99 136 m. The inside of a
junction is drawn by 27 955 short runs of 3 to 5 points framed
`u32 3, u32, u32, u32 n`, whose two u32 range over the edge count. The
decoder does not read them.

**One id on each map is a catch-all.** Its ends are spread across the map,
so it names no junction:

| Map | Id | Ends | Spread | Widest other node | Ends with `0xFFFFFFFF` |
|---|---|---|---|---|---|
| Caucasus | 1697 | 66 | 412 km | 609 m | 3 |
| Nevada | 608 | 47 | 242 km | 2.2 km | 1 |
| Syria | 5004 | 721 | 536 km | 424 m | 14 |
| MarianasWWII | 22 | 177 | 345 km | 231 m | 0 |
| PersianGulf | 2036 | 331 | 520 km | 500 m | 21 |

The three other installed maps have one catch-all each too: Afghanistan
4244, GermanyColdWar 12436, MarianaIslands 68, with their widest other node
396 m, 633 m and 241 m.
Placed at the mean of its ends, the catch-all would join hundreds of road
ends hundreds of kilometers apart through one node. `0xFFFFFFFF` is the
value a u32 takes for "none".

## Decision

E3 writes `a` as the node id the file gives at the edge's first vertex and
`b` as the one at its last, except that it writes null for an end whose id
is `0xFFFFFFFF`, and for every end naming an id whose ends, over the map's
road and rail edges together, do not all lie within 10 km of their mean.
A null end joins no other edge. `pack` places each remaining node at the
mean of the ends that name it, as ADR 0039 says; the edge does not reach
that point, and how the graph bridges the gap is C9's.

### Alternatives considered

**Write the ids as read and let C9 find the catch-all.** It keeps the
extract a plain transcript of the file. It lost because the decoder is
where the file is understood: every reader of `roads.jsonl` would have to
know that one id on each map is not a node, and a reader that does not know
welds the map's dead ends together.

**Drop only the id with the most ends.** It needs no threshold. It lost
because nothing says a map has exactly one catch-all, and on a map whose
busiest real junction outnumbers it, it would drop the junction.

**Read the junction runs and connect the edges through them.** It would
give the graph the junction geometry the file has. It lost for now on what
it costs: the runs' framing is only partly decoded, and nothing the plan
names needs a junction's inside drawn.

## Consequences

The graph C9 builds from `a` and `b` has no node spanning the map, and every
node's ends lie within 10 km; on the eight maps measured, within 2.2 km.
`roads.jsonl` and `railroads.jsonl` now carry null in `a` or `b`: on
Caucasus 69 ends, on Syria 735.

The 10 km threshold sits two orders of magnitude from both sides on these
maps, but it is fitted to them. A map whose catch-all spans less than 10 km,
or whose real junction spans more, would be decoded wrong, and the opt-in
check reports the widest node of each map so that shows.

Node positions sit tens of meters from the edges that name them, up to
hundreds at the widest junctions. C9 must join each edge end to its node,
and its spread report reads these figures rather than near zero.
`core.md`'s road-graph steps 1 to 3, already replaced by ADR 0039, stay
replaced.

The junction runs stay unread. A query that needs a road's path through a
junction, rather than the junction as one point, reopens this.
