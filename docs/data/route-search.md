# Route search on every installed terrain

Does `route`'s A* find the same least cost as Dijkstra's algorithm on a
real road network, and what does a route cost? Measured 2026-10-29 on DCS
build 2.9.30.28738 (`autoupdate.cfg` timestamp `20261006-075422`), Windows 11
Pro 10.0.26200, release build, the query pool capped at 5 threads, with two
other builds and test runs sharing the machine: times are upper bounds.

## Method

The opt-in test `a_star_equals_dijkstra_on_every_installed_terrain` in
`query/route.rs`, run with `DCSTERRAIN_INSTALL` set and a scratch cache, so
each terrain's road graph was built first. For each of the 8 terrains with
an `entry.lua` (Kola's folder has none), 100 pairs of points, the same each
run: each point within 300 m of a road node, the two 2 to 80 km apart.

- **A\* and Dijkstra:** the same search over the same planner, the
  default `max_detour_factor` of 3, no avoid regions, no observers; A\* with
  the straight line to the far end as its estimate, Dijkstra with none. The
  test fails if one finds a route the other does not, or if their costs
  differ by more than 1 in 10⁹.
- **Route op:** the whole `route` operation for the first 20 pairs of each
  terrain, the search, the legs' heights every 25 m from the mesh, and the
  answer written.
- **Alternatives:** `route_alternatives` with the defaults (k 3, stretch
  1.5, overlap 0.5, penalty 1.6) for the first 5 pairs; betweenness was not
  built, so the chokepoints were null.

## Results

A\* equalled Dijkstra on all 712 pairs the network joined. A pair it did
not join is one whose ends lie on networks apart, mostly on the Marianas'
islands, or whose route runs past the detour allowed.

| terrain | road edges | graph build s | pairs routed | A\* p50 / p95 ms | Dijkstra p50 / p95 ms | settled A\* / Dijkstra p50 | length p50 km | detour p50 / p95 | route op p50 / p95 ms | alternatives p50 ms | routes p50 |
|---|---|---|---|---|---|---|---|---|---|---|---|
| Afghanistan | 328 868 | 9.0 | 95 | 1.57 / 5.72 | 2.95 / 6.92 | 4 797 / 11 752 | 62.6 | 1.78 / 2.21 | 13.1 / 26.7 | 40 | 3 |
| Caucasus | 8 764 | 0.8 | 99 | 0.05 / 0.14 | 0.10 / 0.26 | 175 / 516 | 67.9 | 1.47 / 1.98 | 8.3 / 13.2 | 9 | 3 |
| GermanyCW | 464 872 | 15.0 | 98 | 1.39 / 2.84 | 3.24 / 6.77 | 3 877 / 13 013 | 105.4 | 1.97 / 2.40 | 15.9 / 20.4 | 16 | 3 |
| MarianaIslands | 22 661 | 0.4 | 75 | 0.66 / 1.94 | 0.77 / 1.81 | 3 731 / 5 765 | 27.2 | 2.26 / 3.24 | 43.3 / 90.1 | 204 | 3 |
| MarianaIslandsWWII | 17 913 | 0.4 | 59 | 0.59 / 1.08 | 0.59 / 1.02 | 1 355 / 2 179 | 19.3 | 2.26 / 2.88 | 26.7 / 64.2 | 88 | 3 |
| Nevada | 45 645 | 2.6 | 100 | 0.62 / 2.25 | 1.43 / 2.99 | 2 723 / 7 118 | 40.5 | 1.60 / 2.14 | 4.9 / 7.6 | 14 | 3 |
| PersianGulf | 90 185 | 3.5 | 87 | 0.84 / 2.53 | 1.68 / 4.57 | 2 698 / 6 213 | 89.0 | 1.95 / 3.75 | 7.3 / 16.2 | 19 | 3 |
| Syria | 426 633 | 9.8 | 99 | 1.15 / 2.70 | 2.60 / 6.49 | 3 041 / 9 758 | 93.1 | 1.72 / 2.44 | 10.3 / 15.0 | 20 | 3 |

- The search is a few milliseconds at most; the `route` operation is
  dominated by reading the mesh for the legs' heights, most on the
  Marianas, whose ground is the densest.
- A\* settles 2 to 3 times fewer nodes than Dijkstra on the large networks
  and about the same on the island terrains, where the straight line
  crosses water the roads go round.
- Every `route` p95 is under the 500 ms target for a 50 km route, cold
  open aside; the graph build is the one-time cost a cache entry holds.

Route lengths against the game's own `findPathOnRoads` are not compared
here; that needs a running DCS.
