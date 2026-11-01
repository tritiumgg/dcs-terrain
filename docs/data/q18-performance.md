# Q18: each operation against its single-thread baseline

Measured on a Ryzen 7 7800X3D (8 cores, 16 threads), 32 GB, NVMe, Windows 11, DCS
not running. Baseline and final on 2.9.31.29434 (`autoupdate.cfg` timestamp
`20261027-141234`), 3 rounds each, the baseline being main at the start of the
task. The final code and its bench run on the system heap, as the baseline does.

## Method

An operation's figure is the geometric mean, over the 8 terrains, of the medians
of its held cases, warm, on one thread (`RAYON_NUM_THREADS=1`), in reference
cycles with the wall beside them. A held case is one terrain open and every
request answered on it; `fresh`, `cli_*`, `open`, `index_empty_cache` and the
horizon's first touch are other cases' figures. The bench prints this table. The
slowest 11 by baseline had to reach 3×, the fastest 11 had to reach 2×.

Answers are compared by a hash of each of the bench's 3 810 requests, with
`elapsed_ms` left out, baseline build against final build on one DCS build. The
only difference is `describe`'s size of `roads.graph`, 8 lines of the 3 810. The
hashes ran on the same core with `mimalloc` in the binary; the allocator cannot
reach an answer's bytes.

## Results

| Rank | Operation | Baseline Mc / ms | Final Mc / ms | One thread | Goal | All cores, wall ms | Gain |
|---|---|---|---|---|---|---|---|
| 1 | `exposure` | 5595.059 / 1337.332 | 3023.532 / 721.696 | 1.85× | 3× **missed** | 395.571 → 211.528 | 1.87× |
| 2 | `validate_placements` | 2739.989 / 650.617 | 1379.531 / 323.849 | 1.99× | 3× **missed** | 580.650 → 326.487 | 1.78× |
| 3 | `radar_floor` | 939.573 / 224.605 | 491.808 / 117.316 | 1.91× | 3× **missed** | 96.615 → 56.990 | 1.70× |
| 4 | `approach_spectrum` | 873.985 / 208.984 | 405.547 / 96.844 | 2.16× | 3× **missed** | 59.487 → 24.453 | 2.43× |
| 5 | `find_sites` | 846.327 / 184.720 | 484.670 / 115.732 | 1.75× | 3× **missed** | 77.022 → 48.028 | 1.60× |
| 6 | `coverage` | 535.434 / 127.991 | 298.896 / 71.359 | 1.79× | 3× **missed** | 45.908 → 28.281 | 1.62× |
| 7 | `viewshed` | 363.295 / 86.798 | 147.324 / 35.125 | 2.47× | 3× **missed** | 20.786 → 8.315 | 2.50× |
| 8 | `chokepoints` | 136.435 / 26.073 | 51.510 / 12.271 | 2.65× | 3× **missed** | 23.060 → 10.112 | 2.28× |
| 9 | `trafficability` | 125.171 / 21.041 | 25.327 / 6.051 | 4.94× | 3× met | 18.804 → 5.239 | 3.59× |
| 10 | `route_alternatives` | 38.045 / 9.119 | 19.865 / 4.760 | 1.92× | 3× **missed** | 8.172 → 4.486 | 1.82× |
| 11 | `airfield_info` | 29.816 / 7.132 | 19.521 / 4.668 | 1.53× | 3× **missed** | 6.844 → 4.286 | 1.60× |
| 12 | `route` | 11.901 / 2.848 | 5.438 / 1.304 | 2.19× | 2× met | 2.518 → 1.108 | 2.27× |
| 13 | `sample` | 10.557 / 2.538 | 7.524 / 1.816 | 1.40× | 2× **missed** | 2.273 → 1.665 | 1.37× |
| 14 | `describe` | 8.897 / 2.139 | 8.305 / 1.993 | 1.07× | 2× **missed** | 2.059 → 1.998 | 1.03× |
| 15 | `unmask_profile` | 3.658 / 0.885 | 1.956 / 0.467 | 1.87× | 2× **missed** | 0.925 → 0.477 | 1.94× |
| 16 | `point_info` | 3.080 / 0.747 | 2.431 / 0.595 | 1.27× | 2× **missed** | 0.810 → 0.650 | 1.25× |
| 17 | `visible` | 1.544 / 0.367 | 0.643 / 0.151 | 2.40× | 2× met | 0.328 → 0.161 | 2.04× |
| 18 | `towns` | 1.429 / 0.348 | 0.565 / 0.140 | 2.53× | 2× met | 0.356 → 0.144 | 2.47× |
| 19 | `score_site` | 1.096 / 0.262 | 0.748 / 0.176 | 1.47× | 2× **missed** | 0.256 → 0.195 | 1.31× |
| 20 | `geo` | 0.485 / 0.121 | 0.426 / 0.106 | 1.14× | 2× **missed** | 0.120 → 0.107 | 1.12× |
| 21 | `scenery_in` | 0.175 / 0.053 | 0.060 / 0.020 | 2.92× | 2× met | 0.052 → 0.020 | 2.60× |
| 22 | `nearest` | 0.126 / 0.039 | 0.102 / 0.033 | 1.24× | 2× **missed** | 0.040 → 0.033 | 1.21× |

## Memory

Peak commit (`PeakPagefileUsage`) of one process of the binary answering all of
a case's requests, every case on every terrain. Target 64 MiB, ADR 0008.

| Terrain | Start, MiB | Final, MiB | Worst case now |
|---|---|---|---|
| Afghanistan | 960.8 | 50.2 | `exposure_10_sites` |
| Caucasus | 137.6 | 47.2 | `exposure_10_sites` |
| GermanyCW | 315.8 | 57.3 | `sample_10k_scattered` |
| MarianaIslands | 151.4 | 59.7 | `exposure_10_sites` |
| MarianaIslandsWWII | 129.9 | 61.1 | `exposure_10_sites` |
| Nevada | 142.5 | 49.2 | `exposure_10_sites` |
| PersianGulf | 147.8 | 55.2 | `exposure_10_sites` |
| Syria | 220.5 | 59.4 | `exposure_10_sites` |

Exempt and measured beside it: `dcsterrain index` from an empty cache on
GermanyCW peaks at about 1.5 GiB, by a coarse poll of the process before the
last changes to the code; it was not measured again.

The Marianas' memory that ADR 0007 left untraced: 458 blocks, most tiny, one of
811 k triangles, 19 MiB to read and 27 MiB prepared, raw and prepared copies of
several standing together; and the pieces of every radial held at once before
any was reduced. Peak resident on GermanyCW, mapped pages included, reaches
223 MiB in `point_info`; that is the OS's to drop.

## The cache bar

One precompute: each network's grid in `roads.graph`, with the flat layout
(entry version 2) counted against it. Size against the entry of main:

| Terrain | Cache before, B | After, B | Added |
|---|---|---|---|
| Afghanistan | 84 932 613 | 88 196 277 | 3.8 % |
| Caucasus | 12 800 083 | 12 893 997 | 0.7 % |
| GermanyCW | 176 968 014 | 182 256 934 | 3.0 % |
| MarianaIslands | 5 726 985 | 5 938 895 | 3.7 % |
| MarianaIslandsWWII | 4 612 641 | 4 779 091 | 3.6 % |
| Nevada | 22 727 316 | 23 177 692 | 2.0 % |
| PersianGulf | 26 840 408 | 27 709 118 | 3.2 % |
| Syria | 122 648 116 | 127 274 210 | 3.8 % |

Against the same code with the grid built in memory, one thread, warm: Syria
`trafficability` 443 → 38 Mc, `chokepoints_20km` 468 → 91, `find_sites_20km`
444 → 73, each over 1.5×. Without the grid in the cache every process that wants a
road pays for building it, so the added index time is repaid within a few calls.
Four edges to a cell rather than one, and edges of 28 bytes rather than 32,
brought Afghanistan from 6.9 % to 3.8 %.

## Tried and discarded

- Block budgets: 8 MiB halved `point_info` and `visible`; 16 MiB is kept, and 4 MiB
  during `exposure` only.
- `mimalloc`. In the bench it beat the system heap on small requests (`sample`
  1.85× against 1.40×, `unmask_profile` 2.14× against 1.87×, `point_info` 1.65×
  against 1.27×, `exposure` 1.97× against 1.85×). In the binary it holds
  far more: worst case 635 MiB (`GermanyCW/exposure_10_sites`) with its defaults,
  and 585 MiB with purge delay 0 and no eager commit, 214 cases over the target.
  Memory is the harder bound, so the system heap stays. One setting was tried.
- A bit set in place of sorting the group records near a box: no change.
- Leaves of 4 triangles against 8: within noise.
- Fewer than 64 KiB of arrays is copied; 1 MiB left private copies of a band's
  blocks over the target.

## Shortfalls and their measured causes

What was tried is in brackets. On the small operations part of each miss is the
system heap's time on a tree of small strings, as the `mimalloc` figures above
show.

| Operation | Largest phase now | Cause |
|---|---|---|
| `exposure`, `radar_floor`, `coverage`, `approach_spectrum`, `viewshed` | The radial sweep: raster slopes, then the cell loop | Answers must stay byte-identical, so every step of every radial runs and exact `hypot` stands wherever a floor keeps the range. (Wide registers for slopes, heights and cells; filters for `hypot` and `atan2`.) |
| `validate_placements` | Rasterizing tiles for placements scattered over the terrain, then hierarchies | Each placement reaches blocks no other has read. (Scan once, then hierarchy; wide rows; a grid over group records.) |
| `find_sites` | Sightlines over the mesh in `find_sites_los_36`; the rest in its layers | Sightlines × triangles. (Corner rejection, spans without a second sort.) |
| `chokepoints` | Tiles and blocks under the region | 2.65× here, 2.77–2.85× with `mimalloc`; the miss stands either way. |
| `route_alternatives`, `unmask_profile` | Ground under each sample, then the answer's JSON tree | 2.62–2.68× and 2.14–2.22× with `mimalloc`; on the system heap 1.92× and 1.87×. Allocation is the difference. |
| `airfield_info` | Points and on-runway tests, then hierarchies on first touch | Each new airdrome is new blocks. (Chunked points.) |
| `sample`, `score_site`, `point_info` | Blocks and tiles first touched, and the answer's JSON tree | The cases with layers are tile and block reads; the rest is JSON. |
| `describe` | Opening and reading each cache entry's head, the kernel's time | 1.02–1.17× by terrain, repeated side by side. No gain to be had without caching what a call must report fresh. |
| `geo`, `nearest` | The request's and the answer's JSON trees | The computation is microseconds; `query::answer` returns a `serde_json::Value`, sorted maps of owned strings. (`mimalloc` gave 1.38× and 1.45×, and is discarded above.) |

## Cold and all cores

All cores: baseline and final on the system heap on one build, wall ms; 528 held
and CLI cases, geometric gain 1.44×. Slower than the baseline by over 5 %:

| Case | Baseline ms | Final ms | Cause |
|---|---|---|---|
| `layer_count_*` on 4 terrains, `layer_dist_builtup` on 3 | 1.0–2.5 | 1.65–4.53 | The group grid's build, once in a process, on its second ask: 3.8 MiB twice on PersianGulf. A case's 3 requests a round put that ask on the median. The next ask does not pay it. Not measured apart. |
| `index_empty_cache` Caucasus, GermanyCW | 1 365; 14 364 | 1 571; 16 020 | The grid `roads.graph` now holds. |
| `*/fresh` and `cli_visible`, 5–9 % | 3.6–12.2 | 3.8–13.3 | Opening a terrain in a fresh process. Unsettled: 5–9 % sits within what the opening varies by, and the cause was not traced. |

Cold: baseline against the final core with `mimalloc` in the bench, from the run
before it was removed, so these are not figures of the shipped allocator; 528
cases, geometric gain 1.51×. On that run, repeated side by side:
`MarianaIslandsWWII` small windows 7–19 % slower, 2–5 ms on 13–26 (`layer_dist_water`
21.5 → 26.2, `chokepoints_20km` 38.7 → 43.2), MarianaIslands 3–11 %,
PersianGulf `airfield_info` 4.9 → 5.5. Attributed to large blocks read through a
view with a hierarchy built at once; not profiled. Unsettled on the system heap.
`sample_10k_lattice` does not regress on one build: 22.16 → 22.15 on
MarianaIslandsWWII, 21.46 → 18.78 on MarianaIslands.
