# ADR 0036: The packed base is 50 m on every map

## Status

Accepted

## Context

**Affects:** `core.md` "Grids" (the default `cell_size`) and the `cell_size`
row of the pack config; `design-and-facts.md` "Design decisions" (the packed
base); `plan.md` C5b.

The core spec sets the default by area:

> Base resolution is the pack config `cell_size`: 50 (the extract's) or
> 100, produced by resampling; the default is 50 when the authored
> rectangle is under 500 000 km² and 100 above.

The rule keeps large maps small, and it paired with a live extraction whose
cost grew with area and held a running sim the whole time. Under ADR 0034,
rasterizing Caucasus's height from its files takes about 30 s with no sim, so
extraction no longer argues for a coarse base. Only size does.

What a coarse base costs was measured on Caucasus, DCS **2.9.30.28738**. True
height is the decoded mesh, which is what `GetHeight` returns (99.95 % of
cells exact). On 300 random 1.6 km land windows, 7.6 M points at 10 m spacing
offset from every grid, a grid at each cell size was rounded to whole meters
and read back by bilinear interpolation. Absolute error against the mesh, in
meters, p50 / p90 / p99:

| Cell | All land | Flat, under 5° | Moderate, 5–20° | Steep, over 20° | `i16` height, Caucasus |
|---|---|---|---|---|---|
| 25 m | 0.15 / 0.37 / 0.74 | 0.15 / 0.36 / 0.47 | 0.16 / 0.37 / 0.91 | 0.17 / 0.43 / 1.36 | 1 126 MiB |
| 50 m | 0.17 / 0.45 / 1.95 | 0.15 / 0.36 / 0.63 | 0.20 / 0.66 / 2.30 | 0.25 / 1.18 / 3.45 | 281 MiB |
| 100 m | 0.23 / 1.32 / 5.22 | 0.16 / 0.42 / 1.52 | 0.37 / 1.97 / 5.92 | 0.74 / 3.50 / 8.77 | 70 MiB |
| 200 m | 0.37 / 4.20 / 13.84 | 0.20 / 0.95 / 4.13 | 1.25 / 5.94 / 14.97 | 2.80 / 10.14 / 22.38 | 18 MiB |

The 0.15 m floor is rounding to whole meters. At 100 m the steep-ground tail
reaches 9 m, enough to turn a sightline over a ridge.

## Decision

The default `cell_size` is 50 on every map, whatever its area. The pack config
still accepts 100, chosen explicitly by the user.

### Alternatives considered

**Keep the 500 000 km² rule.** Afghanistan and Cold War Germany would stay
near a quarter of their 50 m size. They lost because siting and
line-of-sight are the product, and steep ground is where both are decided.

**25 m.** It halves the 50 m tail on steep ground, at four times the size. A
p99 of 3.5 m on steep ground is inside the margin a siting query works to, so
the extra size buys too little.

## Consequences

Large maps pack four times larger than the rule gave them. Measured authored
areas, 945 000 and 607 500 km², put Afghanistan's height grid at about
378 M cells, 721 MiB, and Cold War Germany's at about 243 M cells, 463 MiB. Every derived layer scales the
same way, and so does pack time for the distance transforms and the horizon
layers.

The extract was already 50 m, so the extract format does not change. C5b's
done test, which exercises the rule's two branches, is restated for a single
default and an explicit override.

The study measured Caucasus only. A map whose mesh is much finer than 50 m in
its built areas would show a larger tail, which would reopen the choice of
25 m for that map.
