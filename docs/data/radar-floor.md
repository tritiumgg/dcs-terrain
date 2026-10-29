# Radar floor on every installed terrain

How far does the radar floor's raster sweep stand from the exact floor over
the mesh, do its bands agree with `visible`, and what do `radar_floor`,
`coverage`, `approach_spectrum` and `unmask_profile` cost? Measured
2026-10-29 on DCS build 2.9.30.28738 (`autoupdate.cfg` timestamp
`20261006-075422`), Windows 11 Pro 10.0.26200, release build, the work
capped at 5 threads (`RAYON_NUM_THREADS=5`) beside two other builds, cold
scratch caches, OS cache warm. The opt-in test is
`dcsterrain/crates/dcsterrain-core/tests/radar.rs`.

Eight terrains: every one installed. Kola's folder holds only `radio.lua`
on this machine, so it has no terrain to read.

## Method

Per terrain, four sites 10 m above their ground: the airdromes `nearest`
finds to the grid's center and to two points off it, and the highest ground
of 64 random points (on the two Marianas terrains that is the sea). Radius
20 km at 50 m, so 641 601 raster cells.

- **Nesting.** `radar_floor` with the default bands 30, 100, 300 and
  1 000 m and `geojson`: up to 300 vertices of each band, projected
  longitude and latitude, must lie inside the next band up or within
  10⁻⁶ degrees of its edge; and each band's `seen_fraction` is at least the
  one below.
- **Bands against `visible`.** 100 random cells per site: at each band,
  whether the raster puts the cell in it (`floor_agl ≤ h`) against whether
  `visible` sees a target `h` above the mesh's ground at the cell's center.
- **Raster against exact.** The same cells through the point form, which
  judges each over the mesh's triangles: the raster's `floor_msl` minus the
  exact one. The point floor itself must be the least altitude `visible`
  passes: seen 1 mm above it, and, where it is over 0.1 m above the ground,
  not seen 5 cm below it.
- **The others.** `coverage` at 30, 100 and 300 m with `geojson`;
  `approach_spectrum` at the same bands every 5°; `unmask_profile` along a
  route 30 m above the ground from 20 km in to 1 km, every 100 m, down the
  middle of the widest 30 m blind arc or due north when there is none, its
  first-seen point checked with `visible`.

## Results

These are from before ADR 0006, the sweep stepping the raster alone; the
last section has the sweep as it now runs.

Medians over the sites, in milliseconds. Agreement is cells at
30/100/300/1 000 m, of 100 per site.

| Terrain | floor | points (100) | coverage | spectrum | unmask | bands agree with `visible` | raster − exact, abs p50 / p95 / p99 | mean |
|---|---|---|---|---|---|---|---|---|
| Afghanistan | 133 | 149 | 56 | 26 | 81 | 395/399/400/400 of 400 | 1.1 / 20.9 / 34.3 m | −4.6 m |
| Caucasus | 96 | 9 | 70 | 26 | 11 | 398/400/400/400 of 400 | 0.3 / 2.9 / 5.8 m | −0.5 m |
| GermanyCW | 131 | 40 | 80 | 26 | 31 | 399/396/398/400 of 400 | 0.5 / 4.0 / 22.7 m | −1.1 m |
| MarianaIslands | 153 | 832 | 59 | 25 | 1 010 | 296/299/299/300 of 300 | 0.0 / 7.2 / 17.7 m | −1.5 m |
| MarianaIslandsWWII | 153 | 552 | 58 | 27 | 634 | 394/400/400/400 of 400 | 0.0 / 96.4 / 329.7 m | −13.7 m |
| Nevada | 103 | 12 | 67 | 27 | 14 | 398/399/396/399 of 400 | 1.6 / 88.8 / 171.7 m | −12.8 m |
| PersianGulf | 130 | 73 | 69 | 27 | 38 | 398/400/400/398 of 400 | 1.0 / 25.0 / 50.1 m | −5.1 m |
| Syria | 108 | 21 | 88 | 31 | 17 | 399/400/400/400 of 400 | 0.4 / 51.2 / 123.4 m | −7.7 m |

MarianaIslands has three sites: two of its airdromes found the same one.

- **The bands nest** on every site: 29 393 vertices checked, none outside.
- **The point floor is exact**: 3 100 of 3 100 cells.
- **The bands agree with `visible`** at 12 360 of 12 400 cell-bands, 99.7 %,
  the share `viewshed` reached against the raster walk.

## Where the raster departs from the exact floor

The raster floor runs low, by a few meters at the median and by up to
330 m in the tail, and the tail is on the sites where the ground falls away
steeply from the site. The floor at range `D` is `eye + m × D`, `m` the
steepest slope to the ground short of it, so an error `e` in a height at
range `d` moves the floor by `e × D / d`. The raster stores heights to the
meter and the first sample lies 25 m out: half a meter of rounding there is
400 m at 20 km. Stepping every half cell and the bilinear between cell
centers also shave crests, which is why the mean is negative.

Two checks on a 3 098 m high point of Nevada's, at (−67 484, 288 238),
over 10 259 cells, every
seventh row and column of its raster:

- The sweep against the raster walked along the true line to each cell:
  within 3 m at the worst cells, so judging a cell against its nearest
  radial costs nothing measurable. The error is all the raster's heights.
- Taking the steepest slope over the mesh's own triangles out to 500 m, and
  the raster beyond, brings the error from p50 6.8 / p95 71.3 / p99 129.0 /
  max 168.8 m to 1 / 14 / 25 / 45 m; out to 1 000 m, p99 17 and max 22 m.
  On Nevada's Creech, flat about the site, it changes nothing (p99 31 m both
  ways): there the error is the raster's farther out.

The mesh is cheap to cross where it is coarse: from that high point, 2 513
radials of 500 m cross one triangle each and take 2 ms in all, and the full
20 km radials 39 triangles each and 16 ms. Around Guam (MarianaIslands'
AntonioBWonPat) one `along` call takes about 17 ms, so 100 point floors
take 2.6 s and a 200-sample `unmask_profile` 5.3 s; the cost is in reaching
the triangles, not in the floor.

The bands, which are what a map draws, are barely touched: where the floor
is far off it is deep in a shadow, far above every band.

## The mesh near the site (ADR 0006)

The same sites and cells, measured again on 2026-10-29 on 2.9.30.28738 at 5
threads. This time the sweep alone was run three ways: over the raster, over
the mesh's triangles out to 1 km with the raster beyond, and over the mesh
all the way. Each was timed after the `radar_floor` call above had warmed
the caches. Wall-clock is in milliseconds; CPU cycles are the process's
over all its threads (`QueryProcessCycleTime`), in millions. Medians over
the sites, the slowest site after the slash. The error is the floor less
the exact point floor at the same cells, absolute p99, and the mean signed.

| Terrain | raster ms | Mcycles | p99 / mean | 1 km ms | Mcycles | p99 / mean | all ms | Mcycles | p99 / mean |
|---|---|---|---|---|---|---|---|---|---|
| Afghanistan | 23 / 24 | 375 | 34.3 / −4.6 m | 70 / 85 | 654 | 26.2 / −3.7 m | 490 / 605 | 7 340 | 5.8 / +0.1 m |
| Caucasus | 22 / 22 | 361 | 5.8 / −0.5 m | 41 / 55 | 488 | 5.8 / −0.5 m | 136 / 144 | 1 575 | 2.9 / −0.1 m |
| GermanyCW | 20 / 21 | 337 | 22.7 / −1.1 m | 89 / 174 | 703 | 22.7 / −1.1 m | 307 / 515 | 4 877 | 1.8 / 0.0 m |
| MarianaIslands | 23 / 26 | 377 | 17.7 / −1.5 m | 79 / 113 | 1 113 | 10.4 / −1.1 m | 1 188 / 1 328 | 11 586 | 3.9 / 0.0 m |
| MarianaIslandsWWII | 25 / 26 | 401 | 329.7 / −13.7 m | 199 / 440 | 1 256 | 40.7 / −2.3 m | 773 / 2 534 | 10 410 | 3.9 / +0.1 m |
| Nevada | 25 / 27 | 427 | 171.7 / −12.8 m | 57 / 63 | 647 | 32.9 / −3.2 m | 138 / 214 | 2 004 | 7.2 / +0.1 m |
| PersianGulf | 26 / 31 | 432 | 50.1 / −5.1 m | 44 / 59 | 480 | 21.0 / −2.0 m | 297 / 375 | 5 020 | 8.9 / 0.0 m |
| Syria | 29 / 31 | 464 | 123.4 / −7.7 m | 162 / 220 | 1 144 | 10.3 / −0.6 m | 300 / 404 | 3 538 | 8.3 / +0.1 m |

- **All the way** is within 9 m of the exact floor at p99 everywhere, but
  costs 4 to 30 times the raster's cycles. It passes 300 ms on four
  terrains, MarianaIslands' median at 1.2 s.
- **Out to 1 km** costs 0.1 to 0.9 Gcycles more than the raster. It takes
  out the tail where the ground falls from the site, MarianaIslandsWWII's
  p99 from 330 to 41 m. Where the near ground is level (Caucasus,
  GermanyCW) it changes nothing.
- **`radar_floor` with the mesh out to 1 km**, GeoJSON bands included, took
  91–303 ms at the median per terrain. Its bands still nest at every
  checked vertex (29 347), agree with `visible` on 12 370 of 12 400
  cell-bands, and the point floor is exact at 3 100 of 3 100 cells.
- **Before the blocks were read once for all the radials**, the 2 513
  segments of one 20 km sweep around Guam took about 43 s. Each re-read
  blocks of 100 000 to 680 000 triangles that together outgrew the 32 MiB
  block budget. Read once, they take 1.2–1.7 s for 1 369 triangles a radial
  and 80–94 ms out to 1 km.
