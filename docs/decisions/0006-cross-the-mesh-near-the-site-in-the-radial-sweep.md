# ADR 0006: Cross the mesh's triangles near the site in the radial sweep

## Status

Accepted

## Context

**Affects:** `design.md` §8 (the `viewshed` and `radar_floor` targets) and
§10 (risk 4); `queries.md` `viewshed` and `radar_floor`; `plan.md` Q5.

The radial sweep behind `viewshed` and `radar_floor`, and so `coverage` and
`approach_spectrum`, stepped the 50 m height raster every half cell. Each
radial kept the steepest slope from the eye to the ground so far, and the
floor at a cell is the eye plus that slope times the range. The raster
stores heights to the meter. An error `e` in a height at range `d` therefore
moves the floor at range `D` by `e × D / d`: half a meter of rounding at the
first sample, 25 m out, is 400 m at 20 km. ADR 0003 already judges `visible`
over the mesh's own triangles, and the point form of `radar_floor` is exact
over them, so the raster floor can be held to the exact one.

The user asked to try exact radials over the whole radius (C) first, and to
fall back to the mesh near the site only (B) if C's time cost was
significant.

Measured on 2.9.30.28738 over all 8 installed terrains, four sites each, a
20 km radius at 50 m, 5 threads. Times are medians over the sites, warm.
Cycles are the process's CPU cycles over all its threads
(`QueryProcessCycleTime`). The error is the raster floor less the exact point
floor at 100 cells per site. All of it is recorded in
[`data/radar-floor.md`](../data/radar-floor.md).

| Mesh out to | ms, median per terrain | Gcycles, median per terrain | floor − exact, p99 per terrain |
|---|---|---|---|
| none (the raster) | 20–29 | 0.34–0.46 | 5.8–330 m |
| 1 km (B) | 41–199 | 0.48–1.26 | 5.8–41 m |
| all the way (C) | 136–1 188 | 1.6–11.6 | 1.8–8.9 m |

- **C's cost.** It passes the 300 ms target for `radar_floor` on four
  terrains: MarianaIslands at 1.2 s, MarianaIslandsWWII at 0.8 s and up to
  2.5 s, Afghanistan at 0.5 s and GermanyCW at 0.31 s; Syria stands at
  0.30 s. It spends 4 to 30 times the raster's
  cycles. The cost is the clipping of every triangle each radial crosses:
  1 369 triangles per 20 km radial around Guam, against 39 on a Nevada
  summit.
- **Reading the blocks.** Before this decision, every segment read its
  blocks again. The blocks under one site can outgrow the 32 MiB block
  budget together, so around Guam each 500 m segment cost 9 ms. Reading them
  once for all the radials, as `points` does, brought the 2 513 radials of a
  20 km sweep from about 43 s to 1.2–1.7 s.
- **B's gain.** It takes out the worst of the error, where the ground falls
  away from the site:
  - Nevada: p99 from 172 to 33 m.
  - MarianaIslandsWWII: p99 from 330 to 41 m.
  - Syria: p99 from 123 to 10 m.

  It changes nothing at a site whose near ground is level (Caucasus,
  GermanyCW), where the error lies farther out.

## Decision

Each radial of the sweep takes its steepest slope over the mesh's own
triangles out to 1 km from the site, and steps the raster beyond. The
triangles under all the radials are read in one batch: each block once,
clipped against every radial over it (`Source::alongs`). Out to each step
of a radial, the steepest slope is the greatest at a crossing or at the
step's end, since over one triangle's plane the slope is monotonic
(`sightline::horizon`). It allows half of `visible`'s 0.1 mm tolerance, as
the exact point floor does. `viewshed` and `radar_floor` both take it, since
they share the sweep.

### Alternatives considered

- **The mesh all the way (C).** Its floor is within 9 m of the exact one at
  p99 on every terrain, but it costs 1.2 s on MarianaIslands and up to
  2.5 s on MarianaIslandsWWII, past the 300 ms target. That is the
  significant cost on which the user chose to fall back.
- **500 m of mesh.** On the Nevada summit measured first it left p99 at
  25 m against 17 m for 1 km, at about the same cost.
- **The raster alone.** This was the design. It is cheapest, and its bands
  agree with `visible` on 99.7 % of cell-bands. But its floor runs low by up
  to 330 m at p99 at exactly the sites a radar stands on.
- **The exact floor at every cell.** That is one `along` per cell, 641 601
  of them at 20 km: far past C.

## Consequences

- The floor near a site that stands above falling ground is close to
  exact. The rest of the error is the raster farther out, up to 41 m at p99
  on MarianaIslandsWWII.
- `radar_floor` takes 41–199 ms rather than 20–29 ms for the sweep. With
  its GeoJSON bands it takes 91–303 ms, past the 300 ms target only on
  MarianaIslandsWWII by a few ms.
- `viewshed` changes a little, toward `visible`.
- The sweep reads mesh blocks as well as raster tiles.
- `Source::alongs` is there for any operation that asks many segments at
  once.
- This would reopen if clipping the triangles grew cheaper, which would
  make C affordable; it is 4–30 times the raster's cycles now. It would
  also reopen on a terrain whose blocks near a site are much denser than
  Guam's.
