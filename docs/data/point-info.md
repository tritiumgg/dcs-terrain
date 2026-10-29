# `point_info` on every installed terrain

Is each point's nearest town the one a scan of every town finds, are its
bearings right, does water carry a depth, and what does a point cost?
Measured 2026-10-29 on DCS build 2.9.30.28738 (`autoupdate.cfg` timestamp
`20261006-075422`), Windows 11 Pro 10.0.26200, release build, the query pool
capped at 5 threads, with other builds and test runs sharing the machine:
times are upper bounds, and one terrain's warm time can exceed its cold one.
Cycles are the process's CPU cycles over the same span, across every thread.

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
- **Depth:** the test fails if a water point has no depth or a negative one,
  or a land point has one; it counts the water points whose depth is above
  zero.
- **Times:** tables and road graph built first; then the whole request,
  cold (first) and warm (again, the same answer asserted); then each step of
  the request alone over the same points.

## Algorithm

The town is found by a linear scan of every town, exact, O(towns) per
point: distances only, then the names of the winners. The tables hold 22 to
1 691 towns, so a scan is 0.5 to 11 µs; a k-d tree would save microseconds
on a request whose other fields cost hundreds, so there is none. Before the
scan was made to compare distances only, it copied every town's name and
sorted them all: 2 to 236 µs per point.

The true bearing is the normal section's azimuth, the direction to the point
in the town's local east-north plane through earth-centered coordinates; it
departs from the geodesic's as the square of the distance. The grid bearing
is from DCS x. They differ by the meridian convergence.

## Results

All 24 000 nearest towns agreed with the scan, and every bearing within
500 km of its town met its bound.

| terrain | towns | on mesh | in water | water with depth | depth p50 / max m | tables + roads ms / Mcycles | cold ms / Mcycles | warm ms / Mcycles | warm µs per point | town km p50 / max | convergence max ° | true bearing off geodesic, max ° within 500 km / beyond |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Afghanistan | 1 197 | 2 885 | 283 | 283 | 248 / 251 | 9 614 / 24 084 | 5 673 / 25 411 | 4 809 / 23 753 | 1 603 | 193 / 1 138 | 7.17 | 1.5e-4 / 6.9e-4 |
| Caucasus | 1 691 | 3 000 | 1 223 | 1 223 | 99 / 100 | 882 / 2 334 | 2 546 / 10 552 | 2 107 / 10 178 | 702 | 214 / 888 | 8.45 | 1.0e-4 / 2.9e-4 |
| GermanyCW | 127 | 2 724 | 806 | 806 | 23 / 88 | 15 916 / 44 793 | 8 130 / 40 092 | 11 587 / 41 897 | 3 862 | 174 / 605 | 11.50 | 7.7e-5 / 1.1e-4 |
| MarianaIslands | 77 | 3 000 | 2 997 | 2 997 | 3 986 / 8 033 | 424 / 1 495 | 9 317 / 38 100 | 6 923 / 29 096 | 2 308 | 665 / 1 371 | 0.96 | 1.8e-4 / 1.2e-3 |
| MarianaIslandsWWII | 22 | 3 000 | 2 997 | 2 997 | 3 985 / 8 200 | 366 / 1 214 | 4 192 / 17 956 | 4 114 / 17 892 | 1 371 | 809 / 1 692 | 0.91 | 1.8e-4 / 2.0e-3 |
| Nevada | 219 | 3 000 | 2 | 2 | 1 / 1 | 2 974 / 4 617 | 2 123 / 8 953 | 1 413 / 8 272 | 471 | 120 / 447 | 2.21 | 1.0e-4 / none |
| PersianGulf | 385 | 2 964 | 583 | 583 | 50 / 3 344 | 3 728 / 5 580 | 2 091 / 8 076 | 1 243 / 7 275 | 414 | 319 / 950 | 3.21 | 1.5e-4 / 5.4e-4 |
| Syria | 1 151 | 3 000 | 788 | 787 | 1 758 / 4 320 | 10 605 / 27 415 | 3 177 / 15 386 | 2 506 / 14 395 | 835 | 150 / 541 | 4.06 | 1.3e-4 / 1.5e-4 |

Each step of a warm request alone, over the same 3 000 points, ms /
Mcycles:

| terrain | ground | slope | aspect | nearest road | nearest town | nearest airdrome |
|---|---|---|---|---|---|---|
| Afghanistan | 86 / 768 | 819 / 5 106 | 803 / 4 988 | 3 044 / 12 693 | 28 / 118 | 1 / 6 |
| Caucasus | 48 / 472 | 586 / 2 988 | 594 / 3 032 | 850 / 3 550 | 34 / 141 | 1 / 5 |
| GermanyCW | 137 / 1 610 | 1 876 / 11 533 | 1 722 / 10 966 | 3 859 / 16 042 | 4 / 16 | 5 / 21 |
| MarianaIslands | 13 / 105 | 182 / 881 | 177 / 859 | 6 160 / 25 784 | 3 / 13 | 1 / 4 |
| MarianaIslandsWWII | 5 / 51 | 206 / 1 173 | 223 / 1 249 | 3 943 / 16 327 | 2 / 6 | 1 / 5 |
| Nevada | 49 / 355 | 501 / 3 207 | 504 / 3 228 | 345 / 1 441 | 6 / 23 | 1 / 5 |
| PersianGulf | 46 / 362 | 477 / 2 904 | 479 / 2 921 | 194 / 813 | 10 / 40 | 1 / 6 |
| Syria | 76 / 721 | 929 / 5 625 | 957 / 5 765 | 517 / 2 158 | 25 / 106 | 3 / 13 |

- Every water point but one carries a depth above zero; the one, on Syria,
  reads 0 m. The Marianas' sea reads up to 8.2 km deep, Caucasus's Black Sea
  stops at 100 m.
- Afghanistan's 283 water points are sea, though the country has none. The
  grid runs south to 22.5° N, and they lie at 22.5–25° N, 59–69° E: the
  Gulf of Oman and the Arabian Sea off Oman and Pakistan. Read in the mesh
  at four of them, every triangle under the point is in ED's own
  `Sea5.1/Sea` material subset and carries the sea bit. The blocks there
  are coarse, 37–146 km across, and their depth stops at 251 m, a seabed ED
  caps rather than the sea's own depth. They are the terrain's water as
  authored, neither filler nor a misread, so `point_info` reports them as
  sea.
- The nearest road dominates where points lie far from any road: the road
  index widens ring by ring over empty sea. Over land it is a fraction of a
  millisecond a point. A full request of 10 000 points over open sea takes
  about 20 s.
- Slope and aspect each derive the tiles under the points, so the tiles are
  derived twice; one pass for both would halve that share.
- Uniform points over the whole grid, sea and all, are the worst case. On
  Nevada and PersianGulf, nearly all land, a point costs under half a
  millisecond warm.
- The convergence reaches 11.5° on GermanyCW and 8.5° on Caucasus, so a
  bearing that does not say which north it is from can be wrong by that
  much.
