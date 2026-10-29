# Exposure on every installed terrain

Does `exposure`'s least floor equal the sites' own radar floors, does its
count of sites agree with `visible`, and what does it cost? Measured
2026-10-29 on DCS build 2.9.30.28738 (`autoupdate.cfg` timestamp
`20261006-075422`), Windows 11 Pro 10.0.26200, release build. The work was
capped at 5 threads (`RAYON_NUM_THREADS=5`) beside two other builds, with
cold scratch caches and the OS cache warm. The opt-in test is
`exposure_over_many_sites_on_every_terrain` in
`dcsterrain/crates/dcsterrain-core/tests/radar.rs`.

Eight terrains: every one installed. Kola has no `entry.lua` on this
machine.

## Method

Per terrain, seven sites 10 m above their ground: the airdrome `nearest`
finds to the grid's center, and six points on a ring 12 km out, every 60°.
Each has a 20 km radius. The region is the default, the box about every
site's radius: 1 280 × 1 216 cells at 50 m, of which 1.22 M some site
reaches, 0.89 M of them two or more. The target is 30 m above the ground.

- **Against each site's floor.** `radar_floor` for each site alone, read at
  every cell of the region. `floor_agl` and `floor_msl` must equal the least
  of the sites' floors bit for bit. `seen_count` must equal the number of
  sites whose floor is at or below 30 m. A cell no site reaches must be
  infinite with a count of 0.
- **Against `visible`.** At 200 random cells some site reaches, each site
  within its radius is asked whether it sees a target 30 m above the cell.
  The sites that do are counted against `seen_count`.
- **Cost.** `exposure` run cold, then warm, then warm with `geojson`.
  Wall-clock is in milliseconds; CPU cycles are the process's over all its
  threads (`QueryProcessCycleTime`), in millions.

## Results

| Terrain | cold ms / Mcycles | warm ms / Mcycles | with GeoJSON ms | cells off the sites' floors | count agrees with `visible` | seen at 30 m |
|---|---|---|---|---|---|---|
| Afghanistan | 771 / 7 317 | 678 / 6 886 | 724 | 0 | 193 of 200 | 0.576 |
| Caucasus | 310 / 3 989 | 244 / 3 400 | 296 | 0 | 199 of 200 | 0.437 |
| GermanyCW | 1 197 / 9 489 | 1 137 / 9 321 | 1 176 | 0 | 198 of 200 | 0.521 |
| MarianaIslands | 703 / 6 419 | 642 / 6 042 | 668 | 0 | 199 of 200 | 0.722 |
| MarianaIslandsWWII | 506 / 4 717 | 463 / 4 531 | 506 | 0 | 199 of 200 | 0.766 |
| Nevada | 432 / 4 034 | 276 / 3 746 | 348 | 0 | 199 of 200 | 0.431 |
| PersianGulf | 510 / 5 637 | 455 / 5 491 | 527 | 0 | 200 of 200 | 0.177 |
| Syria | 316 / 4 020 | 308 / 3 995 | 359 | 0 | 200 of 200 | 0.507 |

- **The least floor and the count are the sites' own**, at every one of
  12.5 M cells over the eight terrains.
- **The count agrees with `visible`** at 1 587 of 1 600 cells. At every one
  of the 13 others, some site's floor lies within 25 m of the target. The
  raster floor's own error is the only source of a miscount: its p99 is
  41 m at 20 km (`radar-floor.md`).
- **Seven 20 km sites cost 0.24–1.2 s warm**, 3.4–9.3 Gcycles. That is
  35–160 ms a site, in line with one radar floor sweep there
  (`radar-floor.md`). Drawing four bands of unseen area adds 26–72 ms. The
  slowest terrain,
  GermanyCW, would take about 1.6 s for ten sites against the 3 s target.

## What was not built

- **Sweeping the sites in parallel.** Each sweep already spreads its
  radials over every core, so a second level of parallelism would add
  memory, one floor raster per site in flight, and little speed.
- **Clipping each sweep to the region.** Here the region is the box about
  the sites, so a sweep has nothing outside it to skip. A region much
  smaller than the sites' radii would sweep cells it then drops. No request
  has needed that yet.
