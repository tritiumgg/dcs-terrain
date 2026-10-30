# The FARP and SAM examples replayed on Caucasus

Do the tool's sources reproduce the FARP and SAM siting examples a live
probe answered on DCS 2.9.29.27278? Measured 2026-10-30 on DCS build
2.9.30.28738 (`autoupdate.cfg` timestamp `20261006-075422`), Windows 11 Pro
10.0.26200, release build, 5 threads (`DCSTERRAIN_THREADS=5`) beside two
other sessions, scratch caches built before each timing. The opt-in test is
`dcsterrain/crates/dcsterrain-core/tests/replay.rs`.

The oracle is the pre-reset probe log's "FARP siting example, live" and
"SAM siting example, live", with the setup in the design's "Worked tasks"
(commit `4b85bd0`).

## Method

A golden-oracle regression test with its tolerances in constants, each
with its reason:

- **Candidates** are paired with the probe's by an exact bottleneck
  assignment, exhaustive with pruning, which minimizes the farthest pair
  rather than comparing by rank. A swap of two near-equal candidates is
  therefore no failure. A pair matches within one lattice step, 100 m, in
  each axis: the same or a neighboring lattice point.
- **Rejection counts** must each be within 0.5 % of the probe's. A cell
  can differ because its value lies at a threshold or because the build
  changed its class.
- **Spans and road distances** must be within twice the probe's printed
  precision: 0.02 m and 2 m.
- **The unmask point** must be within 150 m: the probe's rounding to
  0.1 km plus the 100 m route step.

## The seven western points

The sources never list them, so they were fixed first. They are 10 km from
SAM site 1, not from the airdrome, at bearings 225 to 315 degrees every
15, 100 m above the ground. The probe names bearings 225 and 270, and a
137 m target at 10 km on bearing 270 from the site.

The tool's point heights pin the center:

| Point | Ground | Probe |
|---|---:|---:|
| 10 km west of site 1 | 37.25 m (+100 m = 137 m) | 137 m |
| Site 1 | 48.38 m | 48 m |
| 2 km west of site 1 | 108.12 m | 108 m |

About the airdrome instead, site 1 is seen from bearings 300 and 315.

## FARP

The probe's search is recomputed on its own lattice:

- 201 × 201 points every 100 m from 10 km south and west of Kutaisi's
  reference point.
- Point-mode heights at the center and at the corners of 120 × 80 m at
  0° and 90°, 363 609 points.
- The road graph's nearest edge, then the probe's cascade and its ranking
  key `span / 2.5 + road / 3000`, which its table is ordered by.

The probe's prose says "by span then road", but its spans are not in
order. The probe says only that it tested two orientations. Several
readings of its "land" and "span" were tried, and only these reproduce its
counts:

- **Land** is the center not in water. Testing the `surface` code instead
  gave 1 511 rejections.
- **Span** is taken over both orientations' corners and the center
  together. The best of the two orientations, each one alone, or the
  corners without the center each gave 2 554–2 979 rejections and missed
  the eight by kilometers.

| Rejected at | Replay | Probe |
|---|---:|---:|
| Water | 1 221 | 1 220 |
| Span over 2.5 m | 3 181 | 3 170 |
| Within 3 km of the airdrome | 2 719 | 2 721 |
| Outside 200 m to 3 km of a road | 7 181 | 7 205 |
| Kept | 26 099 | 26 085 |

All eight candidates are the probe's, paired 0 m apart and in its order:

| # | x | z | Span | Probe | Road (m) | Probe |
|---|---:|---:|---:|---:|---:|---:|
| 1 | −286787 | 677859 | 0.01 | 0.01 | 234 | 233 |
| 2 | −275487 | 684259 | 0.13 | 0.13 | 217 | 218 |
| 3 | −287787 | 684759 | 0.12 | 0.12 | 233 | 233 |
| 4 | −283387 | 679459 | 0.15 | 0.15 | 211 | 211 |
| 5 | −287487 | 675859 | 0.16 | 0.16 | 207 | 206 |
| 6 | −294387 | 676359 | 0.14 | 0.14 | 236 | 235 |
| 7 | −282187 | 693859 | 0.18 | 0.17 | 222 | 222 |
| 8 | −280487 | 688559 | 0.16 | 0.15 | 257 | 257 |

The recomputation took 1.3 s and 5.4 Gcycles.

The differences are a centimeter of span and a meter of road distance on
some candidates, and a few cells at each threshold. These are within the
probe's own rounding, so nothing measured shows a terrain change between
the two builds.

**`find_sites` with the same request** (weights `span_max` 1 and
`dist_road_band` 2800/3000 give the probe's key up to a constant) passes
26 113 cells, against the probe's 26 085, in 78 ms and 0.38 Gcycles. None
of its eight is the probe's. Three things differ:

- Its `span_max` reads heights rounded to the meter on the 50 m raster,
  and every one of its eight spans 0 m.
- It judges one orientation at a time.
- Its road distance runs from cell center to cell center, and each of its
  eight reads exactly 200 m.

On this plain the ranking therefore ties, and ties break by position.
Reproducing the probe through `find_sites` would need `span_max` on
point heights and the road band on the road geometry, which is a design
change. The test reports this rather than asserting it.

## SAM

Site 1 at (−293887, 682059), from a 6 m mast:

| Bearing | 225 | 240 | 255 | 270 | 285 | 300 | 315 |
|---|---:|---:|---:|---:|---:|---:|---:|
| Clearance (m) | −180.8 | −110.2 | −65.5 | −43.3 | −32.8 | −16.9 | −6.5 |

`visible` is false from all seven, in 3.4 ms. The first blocking ground is
a rise to 55.5 m about 140 m west of the site. The probe named the rise
2 km out and the ridge at 6 km: both mask too, but they are not the first
ground above the line.

- Site 1 sees all 25 protected points at 100 m above the ground, as the
  probe found.
- An ingress at 100 m from 60 km due west is first seen 7 800 m from the
  airdrome, against the probe's 7.8 km, by `unmask_profile` at a 100 m
  step in 90 ms.

`find_sites` with the SAM request returns 1 of the probe's five within a
cell. Its arc is about the airdrome, because a criterion's observers are
fixed points, while the probe ranked by an arc about each site.

## The other terrains

The same two `find_sites` request shapes ran once on each of the seven
other terrains. Each terrain takes the first airdrome in its tables with a
FARP site about it; on PersianGulf the first, Abu Musa, is a small island
with none.

Each request answered with candidates, and each candidate passed scored
alone with `score_site` at its orientation, with identical terms.

| Terrain | Airdrome | FARP pass | FARP ms | FARP Gcycles | SAM pass | SAM s | SAM Gcycles |
|---|---|---:|---:|---:|---:|---:|---:|
| Caucasus | Kutaisi | 26 113 | 81 | 0.39 | 2 086 | 4.4 | 18.6 |
| Afghanistan | Herat | 3 182 | 69 | 0.76 | 5 293 | 140.3 | 600 |
| GermanyCW | Wittstock | 13 846 | 78 | 0.71 | 5 993 | 32.9 | 138 |
| MarianaIslands | Rota_Int | 97 | 31 | 0.26 | 433 | 10.5 | 43.9 |
| MarianaIslandsWWII | Agana_Airfield | 419 | 100 | 0.83 | 2 614 | 879.3 | 4 829 |
| Nevada | Creech | 7 196 | 53 | 0.31 | 2 330 | 9.7 | 40.7 |
| PersianGulf | Bandar_Abbas_airfield | 7 956 | 37 | 0.21 | 2 142 | 8.4 | 35.4 |
| Syria | Abu_al-Duhur | 8 733 | 58 | 0.48 | 6 422 | 19.4 | 81.0 |

The SAM request is 10 201 sites, and those that pass its other criteria
each take 32 sightlines of up to about 21 km.

Its cost varies two hundredfold:

- On Caucasus it is about 70 µs a sightline.
- On MarianaIslandsWWII it is about 10 ms, 14.7 minutes for one request.

That is a cost of the line-of-sight criteria, not of the replay, and is
left to them.
