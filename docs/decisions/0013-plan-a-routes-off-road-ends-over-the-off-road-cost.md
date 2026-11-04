# ADR 0013: Plan a route's off-road ends over the off-road cost

## Status

Accepted

## Context

**Affects:** `queries.md` `route`, `route_alternatives` and `trafficability`;
`plan.md` Q20 and Q21.

`route` snaps each end to the nearest point of its network and plans only the
way between the snapped points. The stretch from an off-road end to the road is
not planned, not checked, and not counted in the length or the climb. The guide's
FARP convoy, from `airport:Kutaisi`, starts 504 m from its road.

`trafficability` judges only the straight line from a point to its nearest road,
every 25 m, by the off-road cost per meter of the cell under it. A hill or a river
on that line makes the point unreachable, even where a way round exists. Past
20 km it gives up.

Snapping to the nearest road can pick a piece joined to nothing else. Every road
route from `airport:Kobuleti` on Caucasus fails with `no_route` for that reason.

The game's own router, `terrain.findPathOnRoads`, also snaps to the nearest road,
and V1 compared `route` against it with snapped ends (ADR 0009). DCS's ground units
drive straight between off-road waypoints: nothing at a live install checks an
off-road way, as nothing checks `trafficability` today.

The engine holds 64 MiB (ADR 0008, ADR 0012). The largest route case measured,
`route_alternatives` over 50 km on GermanyCW, peaks at 58.5 MiB.

## Decision

`route` takes `offroad`, false by default. With it, each end's way to the road
is searched over the 50 m cells within `max_offroad` of the end (5 km by default),
16 neighbors, each cell costing its off-road cost per meter, the one
`trafficability` uses for slope against `max_slope_deg` and for cover, times
`offroad_weight` (2 by default), and, with observers, times one plus
`exposure_weight` times whether they see it. Water, a cell with no ground and a
slope past the limit are not crossed. Every cell of the network the search
reaches is a start, or a goal, of one road search, carrying its off-road cost, so
the route is the least-cost one over off-road way, roads and off-road way
together. A piece of road joined to nothing is then a start that leads nowhere.
When both ends lie within reach of each other and the way off the road the whole
way costs least, that is the route. The ways are straightened wherever a straight
cut costs no more. `offroad` on railroads is refused. Without `offroad`, `route`
answers as before.

`trafficability` runs the same search from its point to the network within
`max_distance`, 5 km by default, and `reachable` means a way was found; the
straight line stays in the answer, judged as before. `max_offroad` takes at most
what `max_distance` does, `max_slope_deg` is bounded as `trafficability` bounds it,
and `offroad_weight` is at least 1.

`offroad_weight` is the tool's own, like the cover and slope factors, and not
measured against the game's ground units. It is above 1 so that a route keeps to
the road where the ground is no shorter: at 1, open flat ground costs a meter of
road, and a route would cut every field.

### Alternatives considered

- **Choose a snapped road that leads somewhere, and nothing else.** It fixes
  Kobuleti; the off-road stretch stays unplanned and uncounted.
- **Judge a straight line from each end to its road, and count it.** It plans the
  stretch but cannot go round a hill or a river, as `trafficability` cannot today.
- **One search over the cells the whole way, roads as cheap cells.** The cells
  between two ends 50 km apart, with the road graph beside them, pass 64 MiB, and
  a road as cells loses the passages the road file allows (ADR 0009).
- **`offroad` on by default.** It changes every route answer that exists, and the
  route V1 compares with the game is the snapped one.

## Consequences

A route can start and end anywhere a vehicle can reach, with its whole length and
climb counted, and a route from an end beside an isolated piece of road answers.
`route`'s answer gains `offroad_from` and `offroad_to`, and its totals cover the
whole way when `offroad` is set; `snapped_from` and `snapped_to` are null only on a
way that never meets the road. `trafficability`'s answer changes shape: a caller
reading `reachable` as the straight line's verdict must read `straight` instead.
By default it now searches 5 km where it judged a line up to 20 km: a road
between the two is `too_far` unless the caller raises `max_distance`, since the
search's cells, not a line's, grow with the reach.

The off-road ways are unvalidated: their cost model is this tool's own, and the
game's ground units path by rules of their own between waypoints. The search's
cells grow with the square of `max_offroad`; its largest value is the most that
holds the engine to 64 MiB on GermanyCW, measured. Without `offroad`, Kobuleti's
routes still fail, now with a message that says to add it.

Reopen this if a live comparison of ground units' off-road paths becomes possible,
or if callers ask for a route that leaves the road between its ends.
