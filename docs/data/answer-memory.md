# The memory of a large answer

Peak commit (`PeakPagefileUsage`) of one fresh `dcsterrain query` process,
release build, answering one request read from stdin, its answer written to a
file. Measured 2026-11-05 on a Ryzen 7 7800X3D (8 cores, 16 threads), 32 GB,
NVMe, Windows 11, DCS not running. Caucasus is DCS 2.9.31.29434
(`autoupdate.cfg` timestamp `20261027-141234`), read in place.

ADR 0012 exempts these requests from the engine's 64 MiB, since the answer is
most of what they hold, and asked for the answer's own memory to be cut by
encoding each answer as it is written. Its figures are beside the ones here.

## The requests

- `sample` on `Synth`: 10 000 points (the row cap) on a 100 by 100 lattice at
  100 m from (−20 000, −35 000), with `height` alone, and with all 57 layers:
  the 4 ground layers, `valid`, `slope`, `aspect`, `roughness`, `tpi_300`,
  `tpi_2000`, the 4 distances, the 2 counts, `tree_density`, `enclosure`,
  the 7 `share_` layers and the 32 `horizon_` bins.
- `point_info` on `Synth`: the same 10 000 points.
- `scenery_in` on Caucasus: the circle of 5 940 m about (−355 000, 617 000),
  over Batumi, which holds 4 987 objects (the cap is 5 000).

Each with and without `geojson`. ADR 0012's requests were not recorded; its
figures stand beside these as measured, and the baseline column is these
requests on the code before this change.

## Results, MiB

| Request | ADR 0012 | Before | Keys in order | Rows straight | Final |
|---|---|---|---|---|---|
| `sample`, 1 layer | 20.9 | 35.6 | 25.2 | 19.2 | 19.2 |
| `sample`, 1 layer, GeoJSON | 64.0 | 78.7 | 51.4 | 33.0 | 23.3 |
| `sample`, 57 layers | 133.2 | 148.7 | 158.6 | 73.3 | 73.3 |
| `sample`, 57 layers, GeoJSON | 293.4 | 308.5 | 359.2 | 174.4 | 98.4 |
| `point_info`, 10 000 points | 76.3 | 76.5 | 79.8 | 26.1 | 26.0 |
| `point_info`, 10 000 points, GeoJSON | 187.2 | 187.0 | 204.2 | 91.9 | 39.1 |
| `scenery_in`, Caucasus, 4 987 objects | 17.9 | 17.7 | 20.7 | 14.3 | 14.3 |
| `scenery_in`, Caucasus, 4 987 objects, GeoJSON | 46.9 | 46.8 | 56.1 | 32.5 | 17.7 |

The answers are 0.52 and 1.65 MB of text for `sample` of one layer without
and with GeoJSON, 10.0 and 20.6 MB at 57 layers, 6.6 and 13.5 MB for
`point_info`, and 1.19 and 3.15 MB for `scenery_in`.

- **Before**: the answer built whole as a tree of JSON values with sorted
  keys, and written after.
- **Keys in order**: the same, the tree keeping keys in the order the response
  declares them. An ordered map costs more than a sorted one, so every tree
  grew.
- **Rows straight**: each answer encoded from its typed response as it is
  written, with no tree; the GeoJSON still a tree inside the response.
  Measured before the order of three GeoJSON features' properties was put
  right, a change that moves no memory; the other columns after it.
- **Final**: each GeoJSON feature encoded to text when it is drawn, the
  collection holding the text.

Every answer of the final build is byte for byte the answer of the keys in
order, `elapsed_ms` aside, and that one is equal as JSON to the answer before.
The same holds for one request of every operation on `Synth` and on Caucasus.

## What remains

`sample` at 57 layers stays over 64 MiB, 73 MiB without GeoJSON and 98 MiB
with it. Its response holds each row's 57 values in a map keyed by the
layer's name, and its GeoJSON holds them a second time as text; how much of
the 73 MiB each part and the engine's work take was not traced. The other
requests are under 40 MiB.

## With the scenery pools

Measured 2026-11-10 the same way, Caucasus of 2.9.31.29434, once
`scenery_in` took `pools` and `counts_only` and each row gained its
`pool`.

| Request | Peak, MiB | Answer, MB |
|---|---|---|
| objects, the circle of 5 940 m over Batumi, 4 987 rows | 14.5 | 1.21 |
| the same, GeoJSON | 17.9 | 3.16 |
| `trees`, the circle of 1 690 m about the same center, 4 979 rows | 12.6 | 1.15 |
| the same, GeoJSON | 15.9 | 3.04 |
| every pool, the circle of 5 940 m, `counts_only` (38 788 trees) | 12.6 | 0.00 |
| every pool, the whole terrain, `counts_only` (12 967 324 trees) | 12.5 | 0.00 |

The objects answer grew 0.2 MiB with the `pool` field. A count holds no
row: the whole terrain's 14.96 M objects, poles and trees are counted in
the memory of opening it, in 10.5 s. The whole terrain's trees, 12 967 324,
and lines, 1 069 315, are exactly the pools' sizes in `file-formats.md`, so
a count meets no object twice; the objects pool counts 919 680, `effect`
objects left out as the default classes leave them.
