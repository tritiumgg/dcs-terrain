# Exact sightline against the raster walk

Does a sightline judged exactly over the mesh's triangles agree with the
half-cell raster walk `visible` uses, and what does each cost? Measured
2026-10-29 on DCS build 2.9.30.28738 (`autoupdate.cfg` timestamp
`20261006-075422`), Windows 11 Pro 10.0.26200, release build, one thread.

## Method

2000 sightlines per terrain, the same each run: a start anywhere on the
grid, a direction at random, a length of 1 to 40 km, kept only when both
ends stand on the mesh. The observer stands 2, 10, 100 or 1000 m above its
ground, a quarter each; the target on its ground (0 m) or 2 m above it, half
each. Both ends' heights are the winning mesh triangle's, as `visible` takes
them.

- **Raster:** the walk `visible` makes, the bilinear raster height every
  half cell (25 m), strictly between the ends; a fresh set of raster windows
  per line, as one request has.
- **Exact:** every finest-mesh triangle under the segment clipped to the
  stretch it covers; between two crossings the ground is the
  highest-ranked triangle's plane, so the gap to the line is linear and its
  least value lies at a crossing. Blocked where the gap falls below
  −0.1 mm. Clearance reaches the ends, so it tends to each end's own height
  above its ground; "same span" re-runs the exact sweep over only the
  raster's first to last step.

Each method was timed through a terrain opened for it alone, after one pass
of both had warmed the OS cache: "cold" is its first pass, with empty block
and tile caches; "warm" its second. The default 32 MiB block budget.

```bash
DCSTERRAIN_INSTALL="<install root>" DCSTERRAIN_SIGHTLINES=2000 \
  mise exec -- cargo test --release -p dcsterrain-core --test sightline -- --nocapture
```

## Cost per sightline, ms

| | Caucasus raster | Caucasus exact | GermanyCW raster | GermanyCW exact |
|---|---|---|---|---|
| p50 | 1.87 | 0.60 | 2.31 | 0.37 |
| p99 | 11.4 | 9.3 | 763 | 30.1 |
| max | 14.7 | 10.6 | 1068 | 35.8 |
| 2000 lines | 5.7 s | 2.4 s | 55.9 s | 6.9 s |

Cold and warm differ by under 5 %: across 2000 lines spread over the whole
terrain the caches churn either way, and the cost is reading and preparing
blocks (exact) or rasterizing tiles (raster). GermanyCW's raster p99 is the
tiles a long line crosses, each rasterized from every block under it.

Triangles a line crosses: Caucasus p50 8, p99 488, max 814; GermanyCW p50
21, p99 1330, max 2439. The mesh is coarse where the ground is plain or
sea: on GermanyCW a 10 km line through the center crosses 183 triangles,
one per 55 m.

## Agreement

"Hidden by one only" counts lines one method hides and the other sees.
Clearance differences are over lines both see across, raster minus exact,
absolute.

| Caucasus | lines | hidden raster | hidden exact | raster only | exact only | clearance same span p50 / p99 / max, m | to the ends p50 / p99, m |
|---|---|---|---|---|---|---|---|
| all | 2000 | 439 | 403 | 37 | 1 | 0.00 / 0.43 / 1.59 | 0.12 / 13.1 |
| target 0 m | 1031 | 250 | 214 | 36 | 0 | 0.00 / 0.38 / 1.20 | 0.12 / 17.2 |
| target 2 m | 969 | 189 | 189 | 1 | 1 | 0.00 / 0.46 / 1.59 | 0.12 / 11.1 |
| eye 2 m | 494 | 158 | 149 | 9 | 0 | 0.00 / 0.47 / 0.58 | 0.00 / 0.49 |
| eye 10 m | 481 | 138 | 128 | 11 | 1 | 0.00 / 0.37 / 1.59 | 0.01 / 0.73 |
| eye 100 m | 514 | 105 | 88 | 17 | 0 | 0.00 / 0.45 / 1.20 | 0.17 / 2.39 |
| eye 1000 m | 511 | 38 | 38 | 0 | 0 | 0.00 / 0.41 / 0.81 | 1.42 / 20.4 |

| GermanyCW | lines | hidden raster | hidden exact | raster only | exact only | clearance same span p50 / p99 / max, m | to the ends p50 / p99, m |
|---|---|---|---|---|---|---|---|
| all | 2000 | 808 | 769 | 44 | 5 | 0.01 / 0.52 / 6.88 | 0.43 / 16.9 |
| target 0 m | 1035 | 468 | 427 | 44 | 3 | 0.00 / 0.49 / 6.88 | 0.57 / 19.6 |
| target 2 m | 965 | 340 | 342 | 0 | 2 | 0.02 / 0.60 / 2.15 | 0.37 / 13.0 |
| eye 2 m | 501 | 313 | 301 | 14 | 2 | 0.00 / 0.46 / 0.62 | 0.00 / 0.97 |
| eye 10 m | 470 | 283 | 266 | 17 | 0 | 0.00 / 0.48 / 0.89 | 0.03 / 1.75 |
| eye 100 m | 513 | 187 | 177 | 13 | 3 | 0.01 / 0.48 / 0.73 | 0.21 / 2.37 |
| eye 1000 m | 516 | 25 | 25 | 0 | 0 | 0.08 / 0.60 / 6.88 | 1.49 / 20.4 |

## What it shows

- **A target 2 m up:** the two agree on 99.8 % (Caucasus) and 99.8 %
  (GermanyCW) of lines, and their clearances over the same span within
  0.6 m at p99. The raster's error there is its 1 m height rounding, not
  its 25 m step.
- **A target on the ground:** the raster hides 36 of 1031 (Caucasus) and 44
  of 1035 (GermanyCW) lines the exact sweep sees, 3.5 % and 4.3 %; the
  exact sweep hides 0 and 3 the raster sees. A line ending on the ground
  has no margin near the target for heights rounded to the meter, which is
  the likely cause; where along those lines the raster blocks was not
  recorded.
- **Clearance to the ends:** a high observer's line falls steeply to the
  target, and the raster's last step is 25 m short of it, so its least gap
  is up to 20 m larger than the exact one, which reaches the target's own
  2 m.
- **Cost:** the exact sweep is cheaper everywhere measured: 2.4 times in
  total on Caucasus, 8 times on GermanyCW, and its GermanyCW p99 is 30 ms
  against 763 ms.
