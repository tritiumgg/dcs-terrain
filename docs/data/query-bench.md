# The query benchmark: before and after the performance pass

Measured on 2.9.30.28738 (`autoupdate.cfg` timestamp 20261006-075422), a
Ryzen 7 7800X3D (8 cores, 16 threads), 32 GB, NVMe, Windows 11, DCS not
running. "Before" is the code the pass started from, "after" the pass with
every kept change; the two ran back to back on the idle machine, each over
the same requests from the same seeded generator. Times are milliseconds
per request; p50 and p95 are over every round's requests (3 rounds after a
warm-up). Cold drops the terrain's files and the cache's from the OS cache
before each round. "One thread" is the after build with
`RAYON_NUM_THREADS=1`, warm only.

Reproduce with `cargo bench -p dcsterrain-cli --bench query`; the file's
header lists its settings. `DCSTERRAIN_BENCH_BASELINE` prints each median
against an earlier run's JSON.

These times are wall clock, from before the benchmark judged on cycles, and
stand as the record of that pass. `data/targets.md` has the current figures
in cycles with the wall beside them.

## Summary

Geometric mean of before ÷ after over each set of cases:

| Terrain | OS cache | Cases | Every core | One thread |
|---|---|---|---|---|
| Caucasus | warm | 41 | 1.97× | 1.60× |
| Caucasus | cold | 41 | 1.64× | — |
| GermanyCW | warm | 41 | 2.47× | 2.02× |
| GermanyCW | cold | 41 | 2.10× | — |
| both | both | 164 | 2.02× | — |
| synthetic | — | 39 | 1.25× | 1.06× |

Cold gains least where a request reads many blocks the OS must fetch: 10 k
scattered points over GermanyCW are bound by the disk (1.7 s cold against
0.18 s warm).

Allocations per request, warm, before → after: GermanyCW `nearest` road
1 859 538 → 38; 10 k scattered points 4.1 GiB → 1.4 GiB allocated; open
44 083 → 11 042; horizon first touch 1.6 GiB → 0.64 GiB.

## Cache size

Unchanged for every entry. `tables.json` takes entry version 2 (its
projection moves to the front), so each cache rebuilds it once.

| Entry | Caucasus | GermanyCW |
|---|---|---|
| `mesh.toc` | 211 083 | 752 119 |
| `roads.graph` | 9 042 566 | 143 689 728 |
| `scene.nodes` | 2 023 660 | 25 685 363 |
| `tables.json` | 568 592 | 2 352 262 |
| `horizon/`, per tile | 8 367 | 8 381 |

## Where the time went, and what changed

Profiled with span timers per phase (no sampling profiler runs without
elevation on Windows):

- `nearest` road and rail scanned every edge, allocating per edge: 268 ms
  on GermanyCW's 465 k edges. A grid index per network, built on first
  search, answers in 0.07 ms; `dist_road` seeds from the same index.
- Preparing a block cost 1–1.4 ms against 0.05–0.25 ms to read it, most of
  it listing big triangles in hundreds of 400 m bins (one coastal GermanyCW
  block: 950 triangles, 2.78 M entries). Each triangle is now listed at the
  finest of 16 bin sizes it spans few bins of.
- Block arrays are read straight into their vectors; a few points test
  triangles in block-local meters first.
- A window's tiles, the blocks under scattered points or a sightline, a
  viewshed's radials and a horizon batch's cells each spread over every
  core (ADR 0004).
- Opening: `entry.lua` opened by name, the projection checked from the head
  of `tables.json` with the rest parsed on first use, the scene index and
  table of contents decoded in bulk, the road graph's points in one buffer.
- Release builds compile as one codegen unit.

Tried and discarded: a 128 MiB block budget (8 % on GermanyCW `visible`,
noise elsewhere); 4-cell finest bins (points 2× faster, tiles and preparing
slower); decoding the graph on several threads (slower, memory bound); thin
and fat LTO and `panic = "abort"` (within noise once one codegen unit is
set); a cached copy of the window's bilinear in the viewshed (within noise).

Still slow: one call of the binary end to end is 8.5–20 ms, about 5 ms of it
the process starting, which `--version` takes too; a request that reads
scenery on GermanyCW spends about 19 ms classing its models, in every
process. `describe` and `geo` now pay the tables parse on their first call,
so their p95 rose (GermanyCW `describe` 2.8 → 7.9 ms) while their p50 held.
Points over a prepared block visit one bin of each size, about 8 % more
work per point there, which the parallel paths hide: 10 k clustered points
are 1.04–1.28× faster all the same.

## Every case

| case | OS | before p50 | after p50 | × | before p95 | after p95 | one thread p50 |
|---|---|---|---|---|---|---|---|
| Caucasus/open | warm | 4.09 | 1.86 | 2.20 | 4.25 | 1.94 | 1.85 |
| Caucasus/describe | warm | 1.54 | 1.52 | 1.01 | 1.75 | 2.9 | 1.54 |
| Caucasus/sample_point | warm | 0.076 | 0.044 | 1.73 | 0.282 | 0.152 | 0.043 |
| Caucasus/sample_point/fresh | warm | 4.22 | 1.89 | 2.23 | 4.46 | 2.03 | 1.93 |
| Caucasus/sample_100_scattered | warm | 11.3 | 3.96 | 2.86 | 14.2 | 4.57 | 6.33 |
| Caucasus/sample_10k_scattered | warm | 309 | 74.8 | 4.13 | 313 | 79.1 | 167 |
| Caucasus/sample_10k_clustered | warm | 9.83 | 7.66 | 1.28 | 20.3 | 9.88 | 8.74 |
| Caucasus/sample_region_5km | warm | 8.7 | 5.31 | 1.64 | 15.3 | 5.6 | 5.55 |
| Caucasus/layer_valid | warm | 1.07 | 0.838 | 1.27 | 2.73 | 0.88 | 0.921 |
| Caucasus/layer_slope | warm | 1.49 | 0.764 | 1.95 | 2.27 | 1.08 | 0.716 |
| Caucasus/layer_aspect | warm | 1.67 | 1.06 | 1.57 | 2.35 | 1.28 | 1.39 |
| Caucasus/layer_roughness | warm | 3.13 | 1.08 | 2.89 | 7.6 | 1.23 | 1.25 |
| Caucasus/layer_tpi_300 | warm | 1.27 | 0.95 | 1.34 | 9.4 | 1.16 | 1.18 |
| Caucasus/layer_tpi_2000 | warm | 1.83 | 1.17 | 1.57 | 11.5 | 1.36 | 1.46 |
| Caucasus/layer_dist_road | warm | 1.69 | 0.847 | 2.00 | 9.89 | 6.72 | 0.827 |
| Caucasus/layer_dist_rail | warm | 0.846 | 0.692 | 1.22 | 8.14 | 4.05 | 0.794 |
| Caucasus/layer_dist_water | warm | 5.84 | 1.56 | 3.75 | 8.89 | 2.78 | 2.78 |
| Caucasus/layer_dist_builtup | warm | 1.45 | 1.45 | 1.00 | 3.74 | 3.05 | 1.44 |
| Caucasus/layer_count_building | warm | 0.96 | 0.904 | 1.06 | 3.17 | 2.42 | 0.894 |
| Caucasus/layer_count_industrial | warm | 0.938 | 0.903 | 1.04 | 3.26 | 2.44 | 0.899 |
| Caucasus/layer_tree_density | warm | 0.657 | 0.607 | 1.08 | 2.06 | 1.41 | 0.583 |
| Caucasus/layer_share_forest | warm | 1.06 | 1.02 | 1.04 | 1.48 | 1.56 | 1.15 |
| Caucasus/layer_enclosure | warm | 0.917 | 0.814 | 1.13 | 1.09 | 0.953 | 0.81 |
| Caucasus/horizon_first_touch | warm | 180 | 43.1 | 4.18 | 183 | 43.5 | 182 |
| Caucasus/geo_100 | warm | 0.099 | 0.092 | 1.08 | 0.107 | 1.45 | 0.089 |
| Caucasus/nearest_road | warm | 8.61 | 0.025 | 344.40 | 9.63 | 2.16 | 0.025 |
| Caucasus/nearest_rail | warm | 0.866 | 0.054 | 16.04 | 0.9 | 0.276 | 0.054 |
| Caucasus/nearest_town | warm | 0.227 | 0.244 | 0.93 | 0.243 | 0.268 | 0.231 |
| Caucasus/nearest_airdrome | warm | 0.004 | 0.004 | 1.00 | 0.008 | 0.006 | 0.004 |
| Caucasus/nearest_beacon | warm | 0.022 | 0.023 | 0.96 | 0.027 | 0.037 | 0.022 |
| Caucasus/nearest_node | warm | 0.029 | 0.031 | 0.94 | 0.033 | 0.04 | 0.03 |
| Caucasus/nearest_scenery | warm | 3.31 | 3.33 | 0.99 | 244 | 234 | 3.33 |
| Caucasus/scenery_in_1km | warm | 0.302 | 0.301 | 1.00 | 2.55 | 1.88 | 0.303 |
| Caucasus/scenery_in_1km/fresh | warm | 6.75 | 3.79 | 1.78 | 7.19 | 3.9 | 3.81 |
| Caucasus/visible | warm | 0.817 | 0.345 | 2.37 | 8.31 | 1.92 | 0.464 |
| Caucasus/visible/fresh | warm | 6.71 | 2.58 | 2.59 | 13 | 4.53 | 2.51 |
| Caucasus/viewshed_10km | warm | 25.4 | 3.23 | 7.87 | 27.8 | 5.23 | 21.7 |
| Caucasus/viewshed_20km | warm | 99.2 | 14.9 | 6.65 | 99.7 | 15.8 | 99 |
| Caucasus/cli_describe | warm | 13.3 | 11.3 | 1.17 | 13.8 | 11.8 | 11.3 |
| Caucasus/cli_visible | warm | 12.4 | 8.53 | 1.45 | 15.2 | 9.55 | 8.59 |
| Caucasus/index_empty_cache | warm | 123 | 128 | 0.96 | 123 | 128 | 131 |
| Caucasus/open | cold | 4.29 | 1.84 | 2.33 | 5.96 | 3.11 | - |
| Caucasus/describe | cold | 1.57 | 1.53 | 1.03 | 2.31 | 4.21 | - |
| Caucasus/sample_point | cold | 0.545 | 0.522 | 1.04 | 0.996 | 0.814 | - |
| Caucasus/sample_point/fresh | cold | 4.84 | 2.43 | 1.99 | 6.14 | 3.36 | - |
| Caucasus/sample_100_scattered | cold | 55.6 | 47.6 | 1.17 | 75.1 | 66.9 | - |
| Caucasus/sample_10k_scattered | cold | 1.03e+03 | 1.02e+03 | 1.00 | 1.06e+03 | 1.07e+03 | - |
| Caucasus/sample_10k_clustered | cold | 9.21 | 8.29 | 1.11 | 19.3 | 11.5 | - |
| Caucasus/sample_region_5km | cold | 8.74 | 5.99 | 1.46 | 15.2 | 6.36 | - |
| Caucasus/layer_valid | cold | 1.48 | 1.49 | 0.99 | 3.66 | 2.41 | - |
| Caucasus/layer_slope | cold | 2 | 1.5 | 1.33 | 2.7 | 2.13 | - |
| Caucasus/layer_aspect | cold | 2.38 | 2.06 | 1.16 | 2.87 | 2.53 | - |
| Caucasus/layer_roughness | cold | 3.55 | 1.97 | 1.80 | 8.3 | 2.62 | - |
| Caucasus/layer_tpi_300 | cold | 2.68 | 2.08 | 1.29 | 10.1 | 4.39 | - |
| Caucasus/layer_tpi_2000 | cold | 1.98 | 1.99 | 0.99 | 14.9 | 2.28 | - |
| Caucasus/layer_dist_road | cold | 1.69 | 0.872 | 1.94 | 10.4 | 8.54 | - |
| Caucasus/layer_dist_rail | cold | 0.913 | 0.747 | 1.22 | 8.91 | 5.82 | - |
| Caucasus/layer_dist_water | cold | 8.67 | 3.18 | 2.72 | 9.44 | 5.38 | - |
| Caucasus/layer_dist_builtup | cold | 1.47 | 1.52 | 0.97 | 4.95 | 4.46 | - |
| Caucasus/layer_count_building | cold | 1.65 | 1.64 | 1.01 | 4.48 | 3.75 | - |
| Caucasus/layer_count_industrial | cold | 0.981 | 0.927 | 1.06 | 4.29 | 3.61 | - |
| Caucasus/layer_tree_density | cold | 2.29 | 2.22 | 1.03 | 6.88 | 6.35 | - |
| Caucasus/layer_share_forest | cold | 2.1 | 1.84 | 1.14 | 2.56 | 2.13 | - |
| Caucasus/layer_enclosure | cold | 1.29 | 1.28 | 1.01 | 1.59 | 1.39 | - |
| Caucasus/horizon_first_touch | cold | 207 | 69.1 | 2.99 | 210 | 69.7 | - |
| Caucasus/geo_100 | cold | 0.101 | 0.099 | 1.02 | 0.124 | 1.68 | - |
| Caucasus/nearest_road | cold | 8.63 | 0.025 | 345.40 | 8.84 | 2.21 | - |
| Caucasus/nearest_rail | cold | 0.864 | 0.054 | 16.00 | 0.917 | 0.228 | - |
| Caucasus/nearest_town | cold | 0.228 | 0.243 | 0.94 | 0.272 | 0.276 | - |
| Caucasus/nearest_airdrome | cold | 0.004 | 0.004 | 1.00 | 0.008 | 0.007 | - |
| Caucasus/nearest_beacon | cold | 0.022 | 0.024 | 0.92 | 0.028 | 0.029 | - |
| Caucasus/nearest_node | cold | 0.029 | 0.031 | 0.94 | 0.033 | 0.044 | - |
| Caucasus/nearest_scenery | cold | 7.72 | 7.73 | 1.00 | 711 | 689 | - |
| Caucasus/scenery_in_1km | cold | 0.328 | 0.313 | 1.05 | 3.64 | 3.35 | - |
| Caucasus/scenery_in_1km/fresh | cold | 6.73 | 3.85 | 1.75 | 9.82 | 7.03 | - |
| Caucasus/visible | cold | 2.08 | 1.47 | 1.41 | 8.96 | 4.38 | - |
| Caucasus/visible/fresh | cold | 8.67 | 4.28 | 2.03 | 13.9 | 7 | - |
| Caucasus/viewshed_10km | cold | 26 | 4.56 | 5.71 | 33.1 | 10 | - |
| Caucasus/viewshed_20km | cold | 108 | 23.5 | 4.61 | 110 | 24.3 | - |
| Caucasus/cli_describe | cold | 12.7 | 11.6 | 1.10 | 15.2 | 13.8 | - |
| Caucasus/cli_visible | cold | 14.6 | 11 | 1.33 | 16.2 | 12.2 | - |
| Caucasus/index_empty_cache | cold | 147 | 152 | 0.97 | 147 | 152 | - |
| GermanyCW/open | warm | 10.7 | 3.52 | 3.03 | 11.2 | 3.95 | 3.48 |
| GermanyCW/describe | warm | 2.53 | 2.34 | 1.08 | 2.8 | 7.89 | 2.29 |
| GermanyCW/sample_point | warm | 0.039 | 0.024 | 1.62 | 1.43 | 0.682 | 0.024 |
| GermanyCW/sample_point/fresh | warm | 11 | 3.61 | 3.05 | 12.7 | 4.22 | 3.62 |
| GermanyCW/sample_100_scattered | warm | 22.3 | 5.37 | 4.16 | 27.8 | 6.12 | 11.6 |
| GermanyCW/sample_10k_scattered | warm | 1.03e+03 | 180 | 5.70 | 1.03e+03 | 182 | 470 |
| GermanyCW/sample_10k_clustered | warm | 8.2 | 7.27 | 1.13 | 18 | 15.9 | 7.84 |
| GermanyCW/sample_region_5km | warm | 15 | 5.66 | 2.65 | 15.7 | 7.92 | 6.84 |
| GermanyCW/layer_valid | warm | 0.781 | 0.822 | 0.95 | 0.792 | 0.862 | 0.682 |
| GermanyCW/layer_slope | warm | 1 | 0.837 | 1.19 | 3.71 | 0.929 | 0.82 |
| GermanyCW/layer_aspect | warm | 0.859 | 0.849 | 1.01 | 1.25 | 1.04 | 0.687 |
| GermanyCW/layer_roughness | warm | 1.1 | 1.15 | 0.95 | 4.29 | 3.3 | 1.16 |
| GermanyCW/layer_tpi_300 | warm | 1.22 | 0.945 | 1.29 | 6.79 | 0.978 | 1.1 |
| GermanyCW/layer_tpi_2000 | warm | 7.37 | 3.15 | 2.34 | 15.5 | 6.28 | 7.44 |
| GermanyCW/layer_dist_road | warm | 25.5 | 0.716 | 35.56 | 165 | 64.6 | 0.662 |
| GermanyCW/layer_dist_rail | warm | 3.85 | 1.15 | 3.34 | 147 | 49.8 | 1.04 |
| GermanyCW/layer_dist_water | warm | 2.12 | 1.13 | 1.87 | 16.8 | 5.26 | 2.02 |
| GermanyCW/layer_dist_builtup | warm | 2.13 | 2.08 | 1.03 | 39.6 | 27.6 | 2.01 |
| GermanyCW/layer_count_building | warm | 0.693 | 0.661 | 1.05 | 37.6 | 28 | 0.635 |
| GermanyCW/layer_count_industrial | warm | 0.919 | 0.862 | 1.07 | 38.7 | 27.1 | 0.781 |
| GermanyCW/layer_tree_density | warm | 0.679 | 0.651 | 1.04 | 18.5 | 8.35 | 0.629 |
| GermanyCW/layer_share_forest | warm | 1.11 | 0.85 | 1.31 | 6.53 | 1.06 | 0.909 |
| GermanyCW/layer_enclosure | warm | 1.27 | 1.11 | 1.14 | 1.36 | 1.19 | 1.07 |
| GermanyCW/horizon_first_touch | warm | 933 | 134 | 6.96 | 1.04e+03 | 135 | 703 |
| GermanyCW/geo_100 | warm | 0.096 | 0.098 | 0.98 | 0.111 | 5.5 | 0.094 |
| GermanyCW/nearest_road | warm | 269 | 0.074 | 3639.68 | 284 | 5.36 | 0.07 |
| GermanyCW/nearest_rail | warm | 13 | 0.042 | 310.29 | 14.4 | 0.263 | 0.039 |
| GermanyCW/nearest_town | warm | 0.016 | 0.017 | 0.94 | 0.018 | 0.028 | 0.018 |
| GermanyCW/nearest_airdrome | warm | 0.023 | 0.024 | 0.96 | 0.028 | 0.087 | 0.023 |
| GermanyCW/nearest_beacon | warm | 0.06 | 0.065 | 0.92 | 0.062 | 0.082 | 0.06 |
| GermanyCW/nearest_node | warm | 0.004 | 0.004 | 1.00 | 0.004 | 0.006 | 0.004 |
| GermanyCW/nearest_scenery | warm | 27.2 | 26.3 | 1.04 | 631 | 617 | 26.1 |
| GermanyCW/scenery_in_1km | warm | 0.083 | 0.078 | 1.06 | 36.6 | 26.2 | 0.077 |
| GermanyCW/scenery_in_1km/fresh | warm | 48.5 | 30.7 | 1.58 | 50 | 31.9 | 30.8 |
| GermanyCW/visible | warm | 0.874 | 0.298 | 2.93 | 22.7 | 5.68 | 0.368 |
| GermanyCW/visible/fresh | warm | 11.5 | 4.24 | 2.71 | 31 | 7.94 | 4.03 |
| GermanyCW/viewshed_10km | warm | 20.3 | 2.96 | 6.88 | 49.8 | 9.77 | 19.7 |
| GermanyCW/viewshed_20km | warm | 87.1 | 11.8 | 7.41 | 87.5 | 12.3 | 80.1 |
| GermanyCW/cli_describe | warm | 21.2 | 19.1 | 1.11 | 22.2 | 19.9 | 19 |
| GermanyCW/cli_visible | warm | 18.9 | 10.6 | 1.78 | 22.1 | 11.1 | 10.5 |
| GermanyCW/index_empty_cache | warm | 865 | 922 | 0.94 | 865 | 922 | 982 |
| GermanyCW/open | cold | 10.7 | 3.5 | 3.05 | 12.6 | 4.94 | - |
| GermanyCW/describe | cold | 2.32 | 2.3 | 1.01 | 2.92 | 9.36 | - |
| GermanyCW/sample_point | cold | 0.523 | 0.495 | 1.06 | 2.58 | 2.19 | - |
| GermanyCW/sample_point/fresh | cold | 11.8 | 4.49 | 2.62 | 13.4 | 5.95 | - |
| GermanyCW/sample_100_scattered | cold | 71.6 | 61.6 | 1.16 | 85.1 | 71.5 | - |
| GermanyCW/sample_10k_scattered | cold | 2.41e+03 | 1.69e+03 | 1.43 | 2.47e+03 | 1.8e+03 | - |
| GermanyCW/sample_10k_clustered | cold | 8.77 | 8.43 | 1.04 | 19.8 | 18 | - |
| GermanyCW/sample_region_5km | cold | 16.5 | 7.55 | 2.19 | 22.2 | 13.8 | - |
| GermanyCW/layer_valid | cold | 1.65 | 2.93 | 0.56 | 2.09 | 4.53 | - |
| GermanyCW/layer_slope | cold | 1.85 | 1.36 | 1.37 | 4.46 | 1.93 | - |
| GermanyCW/layer_aspect | cold | 1.55 | 1.41 | 1.10 | 3.52 | 3.67 | - |
| GermanyCW/layer_roughness | cold | 2.92 | 3.06 | 0.95 | 6.06 | 5.11 | - |
| GermanyCW/layer_tpi_300 | cold | 3.27 | 2.46 | 1.33 | 7.49 | 3.66 | - |
| GermanyCW/layer_tpi_2000 | cold | 10.5 | 6.23 | 1.69 | 20.9 | 14 | - |
| GermanyCW/layer_dist_road | cold | 27 | 0.721 | 37.47 | 172 | 94.7 | - |
| GermanyCW/layer_dist_rail | cold | 2.74 | 1.13 | 2.43 | 151 | 77.6 | - |
| GermanyCW/layer_dist_water | cold | 2.7 | 1.72 | 1.57 | 23.9 | 12.9 | - |
| GermanyCW/layer_dist_builtup | cold | 13.9 | 13.7 | 1.01 | 40.9 | 34.7 | - |
| GermanyCW/layer_count_building | cold | 1.06 | 1.02 | 1.04 | 38.9 | 33.6 | - |
| GermanyCW/layer_count_industrial | cold | 6.87 | 6.67 | 1.03 | 39.5 | 33.6 | - |
| GermanyCW/layer_tree_density | cold | 3.84 | 3.73 | 1.03 | 19.8 | 14.9 | - |
| GermanyCW/layer_share_forest | cold | 3.05 | 1.8 | 1.70 | 7.46 | 2.62 | - |
| GermanyCW/layer_enclosure | cold | 1.95 | 1.98 | 0.98 | 2.12 | 2.08 | - |
| GermanyCW/horizon_first_touch | cold | 1.04e+03 | 244 | 4.26 | 1.05e+03 | 250 | - |
| GermanyCW/geo_100 | cold | 0.101 | 0.097 | 1.04 | 0.111 | 6.17 | - |
| GermanyCW/nearest_road | cold | 267 | 0.072 | 3705.42 | 271 | 6.19 | - |
| GermanyCW/nearest_rail | cold | 12.9 | 0.041 | 314.78 | 14 | 0.168 | - |
| GermanyCW/nearest_town | cold | 0.016 | 0.017 | 0.94 | 0.02 | 0.026 | - |
| GermanyCW/nearest_airdrome | cold | 0.023 | 0.024 | 0.96 | 0.027 | 0.037 | - |
| GermanyCW/nearest_beacon | cold | 0.058 | 0.062 | 0.94 | 0.065 | 0.068 | - |
| GermanyCW/nearest_node | cold | 0.003 | 0.003 | 1.00 | 0.004 | 0.005 | - |
| GermanyCW/nearest_scenery | cold | 27.7 | 27.4 | 1.01 | 2.58e+03 | 2.52e+03 | - |
| GermanyCW/scenery_in_1km | cold | 1.43 | 1.56 | 0.92 | 40.2 | 37.4 | - |
| GermanyCW/scenery_in_1km/fresh | cold | 49.8 | 34.2 | 1.45 | 55.5 | 40.9 | - |
| GermanyCW/visible | cold | 3.1 | 2.38 | 1.30 | 29 | 15.2 | - |
| GermanyCW/visible/fresh | cold | 15.5 | 6.52 | 2.38 | 32.5 | 13.5 | - |
| GermanyCW/viewshed_10km | cold | 21.6 | 4.09 | 5.29 | 60.6 | 19 | - |
| GermanyCW/viewshed_20km | cold | 89.3 | 13.9 | 6.44 | 89.6 | 14.4 | - |
| GermanyCW/cli_describe | cold | 21.4 | 20.1 | 1.07 | 24.2 | 23.3 | - |
| GermanyCW/cli_visible | cold | 22.2 | 13.6 | 1.63 | 25.3 | 16.4 | - |
| GermanyCW/index_empty_cache | cold | 1.07e+03 | 1.14e+03 | 0.93 | 1.07e+03 | 1.14e+03 | - |
