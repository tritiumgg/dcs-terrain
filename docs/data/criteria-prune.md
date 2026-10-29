# Siting criteria: pruned against exhaustive evaluation

Measured on 2.9.30.28738 (`autoupdate.cfg` timestamp 20261006-075422),
Windows 11, NVMe, DCS not running, 5 worker threads, a release build of
`cargo test --release -p dcsterrain-core --test criteria -- --nocapture` with
`DCSTERRAIN_INSTALL` set and `DCSTERRAIN_THREADS=5`, while other work shared
the machine. Each terrain's cache was a fresh scratch directory. Kola's folder
in this install holds only `radio.lua`; the install lists eight terrains.

The region is 20 × 20 km around each terrain's first placed airdrome, with a
footprint 60 × 40 m at 0° and 90°. Two sets of criteria:

- **geometric**, a candidate every 100 m (40 401): `slope_max` 5°,
  `span_max` 3 m, `surface_in` land, `cover_in` plain or field,
  `dist_road_band` 50–2000 m, `dist_water_min` 200 m, `dist_airdrome_band`
  3–15 km, `scenery_in_footprint_max` 0, `clearing_max` 10 within 150 m,
  `cover_fraction_min` forest 0 within 300 m, `dist_cover_band` town
  0.5–5 km, `tpi_class_in` flat, lower or valley. The tiles are read once
  before either run is timed.
- **sight**, a candidate every 500 m (1 681): `slope_max` 10°, `masked_from`
  north ±30° at 1°, `enclosure_min` 0.5°, `not_visible_from` three observers
  8 km out at 10 m with at most a third seeing, `visible_to` the airdrome at
  30 m. The pruned run goes first and computes the horizon of the tiles it
  keeps cold; the exhaustive run finds those cached and computes the rest.

In every run the pruned evaluation returned exactly the sites the exhaustive
one passed, with identical terms.

| Terrain | Set | Pass | Tiles | Skipped | Pruned ms | Exhaustive ms |
|---|---|---:|---:|---:|---:|---:|
| Afghanistan | geometric | 8 387 | 49 | 6 | 141 | 386 |
| Afghanistan | sight | 62 | 49 | 7 | 2 096 | 2 767 |
| Caucasus | geometric | 10 092 | 56 | 11 | 99 | 177 |
| Caucasus | sight | 80 | 56 | 19 | 957 | 676 |
| GermanyCW | geometric | 14 432 | 56 | 0 | 132 | 180 |
| GermanyCW | sight | 13 | 56 | 9 | 687 | 1 021 |
| MarianaIslands | geometric | 37 | 49 | 33 | 51 | 154 |
| MarianaIslands | sight | 2 | 49 | 35 | 626 | 5 215 |
| MarianaIslandsWWII | geometric | 6 | 49 | 17 | 63 | 156 |
| MarianaIslandsWWII | sight | 47 | 49 | 17 | 5 069 | 86 097 |
| Nevada | geometric | 6 375 | 49 | 13 | 85 | 142 |
| Nevada | sight | 136 | 49 | 0 | 1 252 | 274 |
| PersianGulf | geometric | 11 476 | 56 | 6 | 126 | 303 |
| PersianGulf | sight | 8 | 56 | 43 | 920 | 958 |
| Syria | geometric | 21 702 | 49 | 1 | 142 | 182 |
| Syria | sight | 8 | 49 | 7 | 808 | 564 |

"Pass" counts candidates with an orientation that passes. A tile is a 64-cell
grid tile, 3.2 km square, holding a candidate.

What it shows:

- Over sea and steep ground the zone maps skip most tiles (33 of 49 around
  Rota), and the time falls with them. On open land with many passing
  sites little is skipped, and the gain is the per-site order failing
  sites early: 1.3–2.9× on the geometric set.
- The sightlines dominate the sight set, so the gain follows the share of
  candidates that reach them: 17× around Agana, where the exhaustive run
  draws 6 724 lines across the island. Where few are skipped, the pruned
  run's cold horizon costs more than the exhaustive run's cached one
  (Nevada, Syria, Caucasus), which is the order of the runs, not the
  pruning.
