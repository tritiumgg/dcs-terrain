# `find_sites` on every installed terrain

Measured on 2.9.30.28738 (`autoupdate.cfg` timestamp 20261006-075422),
Windows 11, NVMe, DCS not running, 5 worker threads, a release build of
`cargo test --release -p dcsterrain-core --test sites -- --nocapture` with
`DCSTERRAIN_INSTALL` set and `DCSTERRAIN_THREADS=5`, while other work shared
the machine. Each terrain's cache was a fresh scratch directory, its scenery
index and road graph built before the timing. Kola's folder in this install
holds only `radio.lua`; the install lists eight terrains.

The request is a FARP's: a 20 × 20 km box around each terrain's first placed
airdrome, a candidate every 100 m (40 401), a footprint 60 × 40 m at 0° and
90°, spacing 1 km, limit 50, weights `slope_max` 2 and `dist_road_band` 0.5.
The criteria: `slope_max` 5°, `span_max` 3 m, `surface_in` land, `cover_in`
plain or field, `dist_road_band` 50–2000 m, `dist_water_min` 200 m,
`dist_airdrome_band` 3–15 km, `scenery_in_footprint_max` 0, `clearing_max`
10 within 150 m, `tpi_class_in` flat, lower or valley.

On every terrain, each candidate returned passed when scored alone with
`score_site` at its orientation, with identical terms and the same weighted
score; no two lay nearer than 1 km; and the answer equaled a brute-force
search: every site evaluated with nothing skipped, each position's best
orientation, sorted, and each candidate tested against every one kept.

`find_sites` is the second of two runs, so its tiles are read; "rescore" is
every candidate's `score_site` call together.

| Terrain | Pass | Tiles skipped | Returned | `find_sites` ms | Rescore ms | Brute force ms |
|---|---:|---:|---:|---:|---:|---:|
| Afghanistan | 13 620 | 0 | 50 | 205 | 53 | 448 |
| Caucasus | 13 626 | 10 | 50 | 141 | 51 | 241 |
| GermanyCW | 19 367 | 0 | 50 | 170 | 30 | 253 |
| MarianaIslands | 38 | 33 | 9 | 58 | 7 | 202 |
| MarianaIslandsWWII | 19 | 17 | 11 | 57 | 9 | 180 |
| Nevada | 13 313 | 5 | 50 | 100 | 17 | 163 |
| PersianGulf | 17 410 | 3 | 50 | 169 | 71 | 322 |
| Syria | 26 700 | 0 | 50 | 169 | 22 | 255 |

The ranking and suppression are a sort and one pass over the sites that
pass; the criteria's evaluation is the time. All eight are under the 500 ms
target for a geometric search of 20 × 20 km, so nothing was optimized.
