# ADR 0003: Judge `visible` over the mesh's triangles, not the raster

## Status

Accepted

## Context

**Affects:** `design.md` §8 (the `visible` targets) and §10 (risk 4);
`queries.md` `visible`; `plan.md` N11.

`visible` stepped the 50 m height raster every half cell between its ends,
bilinear, each sample a height rounded to the meter, and judged the line by
those samples. The design named this as a risk: the raster is 2–3 m off the
mesh at p99 at random points and 4–8 m on steep ground, where grazing lines
are decided. N11 built the alternative: every finest-mesh triangle under the
segment, clipped to the stretch it covers; between two crossings the ground
is the highest-ranked triangle's plane, as at a point, so the gap to the line
is linear and its least value lies at a crossing. It is exact for the mesh
the game answers from.

Measured on 2.9.30.28738 over 2000 random sightlines of 1–40 km per terrain,
recorded in [`data/sightline-exact.md`](../data/sightline-exact.md):

- **A target 2 m up:** the two agree on 99.8 % of lines on Caucasus and on
  GermanyCW; over the same span their clearances differ by 0.6 m at p99.
- **A target on the ground:** the raster hides 36 of 1031 such lines on
  Caucasus and 44 of 1035 on GermanyCW that the exact sweep sees, 3.5 % and
  4.3 %. Its rounded height a
  step short of the target stands above a line that ends on the ground.
  The exact sweep hides 0 and 3 lines the raster sees.
- **Cost:** the exact sweep is cheaper on both: 2.4 times in total on
  Caucasus, 8 times on GermanyCW, whose raster p99 is 763 ms against 30 ms.
  The raster rasterizes every tile a line crosses from every block under it;
  the exact sweep reads the same blocks and visits only the bins the line
  crosses. Through the CLI on Caucasus, `elapsed_ms` fell from 12–17 to
  1–4 and the whole call from 64–79 ms to 42–48 ms.

The synthetic terrain is a closed form with no triangles.

## Decision

`visible` takes every mesh triangle under the segment from
`Source::along` and judges the line exactly over them, the highest rank
winning where triangles overlap, a stretch no triangle covers not blocking.
The line is blocked where the ground rises 0.1 mm or more above it, and the
blocking point is where it first does. `clearance_m` is the least gap
anywhere between the ends, so it reaches each end's own height above its
ground and is negative exactly when the line is blocked. The synthetic
terrain answers `along` by cutting the segment every eighth of a cell into
straight pieces between its closed-form heights, so its ends are the heights
`point` gives. `viewshed` keeps its raster radial sweep.

### Alternatives considered

- **Keep the raster walk.** It is slower everywhere measured and hides a
  ground-level target one time in twenty-five that the game would show.
- **Clearance over the raster's old span**, half a cell short of each end.
  It shows a ridge's margin without the ends' heights, but a line could then
  be blocked near an end while its clearance read positive.
- **Triangulate the synthetic terrain per cell** from corner heights. Its
  heights jump at the coast, the lake and the roads, and where the ground is
  concave a cell's plane stands above `point`'s height, so a target on the
  ground would be blocked by its own cell.
- **A raster fallback** for a source without triangles. The tests on the
  synthetic terrain would then never run the path users get.

## Consequences

- A target on the ground is judged as the mesh has it, and `visible` meets
  its end-to-end target.
- A high observer's clearance to a near target is now at most the target's
  own height above its ground; before, the raster's last step stood 25 m
  short of the target, and read up to 20 m more.
- Every source that answers `visible` must answer `along`; an install and
  the synthetic terrain do.
- `viewshed` and the radial sweeps built on it (`radar_floor`, `coverage`)
  still step the raster, so a cell's answer there can differ from
  `visible` to it on a ground-level target. Whether they move to the mesh is
  each one's own decision, measured when it is built.
- Neither method is checked against the game yet; the live validation
  compares `visible` with `terrain.isVisible`. A gap there reopens this.
