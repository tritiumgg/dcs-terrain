# `point_info` on every installed terrain

Is each point's nearest town the one a scan of every town finds, are its
bearings right, and what does a point cost? Measured 2026-10-29 on DCS build
2.9.30.28738 (`autoupdate.cfg` timestamp `20261006-075422`), Windows 11 Pro
10.0.26200, release build, the query pool capped at 5 threads, with other
builds and test runs sharing the machine: times are upper bounds. CPU is the
process's user and kernel time over the same span, across every thread.

## Method

The opt-in test `nearest_towns_are_every_towns_first_on_every_installed_terrain`
in `query/point_info.rs`, run with `DCSTERRAIN_INSTALL` set and a scratch
cache. For each of the 8 terrains with an `entry.lua` (Kola's folder has
none), 3 000 points drawn uniformly over the terrain's grid, the same each
run, so many lie far out at sea.

- **Nearest town:** for every point, a scan of every town in the tables
  keeping the first of the nearest. The test fails unless `point_info`'s town
  has the same position and distance, and its grid bearing equals
  `atan2(Δz, Δx)`.
- **True bearing:** against the geodesic's initial azimuth from Vincenty's
  inverse on WGS84, from the town to the point. The test fails past 1e-5°
  within 100 km or 2.5e-4° within 500 km; beyond 500 km it records the
  error only.
- **Times:** tables and road graph built first; then the whole request,
  cold (first) and warm (again, the same answer asserted); then the town
  lookup alone, per point.

## Algorithm

The town is found by a linear scan of every town, exact, O(towns) per
point: distances only, then the names of the winners. The tables hold 22 to
1 691 towns, so a scan is 0.4 to 12 µs; a k-d tree would save microseconds
on a request whose other fields cost hundreds, so there is none. Before the
scan was made to compare distances only, it copied every town's name and
sorted them all: 2 to 236 µs per point.

The true bearing is the normal section's azimuth, the direction to the point
in the town's local east-north plane through earth-centered coordinates; it
departs from the geodesic's as the square of the distance. The grid bearing
is from DCS x. They differ by the meridian convergence.

## Results

All 24 000 nearest towns agreed with the scan.

| terrain | towns | on mesh | in water | tables + roads wall / CPU ms | cold wall / CPU ms | warm wall / CPU ms | warm µs per point | town scan µs per point | town km p50 / max | convergence max ° | true bearing off geodesic, max ° |
|---|---|---|---|---|---|---|---|---|---|---|---|
| Afghanistan | 1 197 | 2 885 | 283 | 8 813 / 4 984 | 5 016 / 5 609 | 4 413 / 5 203 | 1 471 | 9.0 | 193 / 1 138 | 7.17 | 6.9e-4 |
| Caucasus | 1 691 | 3 000 | 1 223 | 879 / 547 | 2 564 / 2 438 | 2 099 / 2 422 | 700 | 12.2 | 214 / 888 | 8.45 | 2.9e-4 |
| GermanyCW | 127 | 2 724 | 806 | 13 604 / 10 234 | 6 765 / 8 266 | 6 257 / 8 078 | 2 086 | 1.5 | 174 / 605 | 11.50 | 1.1e-4 |
| MarianaIslands | 77 | 3 000 | 2 997 | 379 / 312 | 6 758 / 6 844 | 6 585 / 6 578 | 2 195 | 0.9 | 665 / 1 371 | 0.96 | 1.2e-3 |
| MarianaIslandsWWII | 22 | 3 000 | 2 997 | 370 / 312 | 3 967 / 4 031 | 4 006 / 4 172 | 1 335 | 0.4 | 809 / 1 692 | 0.91 | 2.0e-3 |
| Nevada | 219 | 3 000 | 2 | 2 919 / 1 141 | 2 195 / 2 078 | 1 439 / 2 062 | 480 | 1.9 | 120 / 447 | 2.21 | 1.0e-4 |
| PersianGulf | 385 | 2 964 | 583 | 3 383 / 1 359 | 2 176 / 2 172 | 1 235 / 1 734 | 412 | 3.0 | 319 / 950 | 3.21 | 5.4e-4 |
| Syria | 1 151 | 3 000 | 788 | 10 166 / 5 891 | 3 103 / 3 750 | 2 529 / 3 453 | 843 | 8.5 | 150 / 541 | 4.06 | 1.5e-4 |

Every error over 2.5e-4° is to a town more than 500 km away, past where the
bound is asserted.

Where a warm request's time goes, measured once per step over the same
3 000 points, wall ms:

| terrain | ground | slope | aspect | nearest road | nearest airdrome |
|---|---|---|---|---|---|
| Afghanistan | 62 | 739 | 757 | 3 087 | 12 |
| Caucasus | 47 | 568 | 574 | 847 | 8 |
| GermanyCW | 108 | 1 479 | 1 679 | 3 324 | 64 |
| MarianaIslands | 10 | 216 | 170 | 6 306 | 2 |
| MarianaIslandsWWII | 6 | 202 | 212 | 3 922 | 4 |
| Nevada | 55 | 567 | 582 | 376 | 7 |
| PersianGulf | 61 | 609 | 571 | 239 | 12 |
| Syria | 75 | 1 002 | 985 | 498 | 57 |

- The nearest road dominates where points lie far from any road: the road
  index widens ring by ring over empty sea. Over land it is a fraction of a
  millisecond a point.
- Slope and aspect each derive the tiles under the points, so the tiles are
  derived twice; one pass for both would halve that share.
- Uniform points over the whole grid, sea and all, are the worst case. On
  Nevada and PersianGulf, nearly all land, a point costs under half a
  millisecond warm; every terrain is within the target of a few
  milliseconds a point.
- The convergence reaches 11.5° on GermanyCW and 8.5° on Caucasus, so a
  bearing that does not say which north it is from can be wrong by that
  much.
