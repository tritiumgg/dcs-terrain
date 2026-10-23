# ADR 0041: Water wins over land, and river over lake and sea, at a cell

## Status

Accepted

## Context

**Affects:** ADR 0039 "Tile layers" (the winning triangle, and the `height`
and `water` rows); `plan.md` E2.

ADR 0039 defines the triangle a cell takes from the finest `.surface5` mesh:

> the *winning triangle* is the covering finest-mesh triangle chosen by the
> precedence rule of `docs/data/file-formats.md`: a road strip first, then the
> higher block index, then the higher triangle index.

and `docs/data/file-formats.md` gives its water class as `TRITYPE & 1` sea,
else `& 2` lake, else `& 4` river, else land. `height` is `floor(h + 0.5)` of
the interpolated height.

E2's decoder applies those rules to Caucasus from the terrain files of DCS
**2.9.30.28738** (files dated 2025-11-14) and scores it against the live
Caucasus extract taken on **2.9.29.27468** over the same files, on the 141.2 M
cells that extract wrote. 87 cells take a different water class from the game.
Listing every triangle covering each one sorts them into three kinds:

- **43 cells: a lake over land.** Two lake triangles overlap a land triangle
  of the same block whose index is higher, so the land wins under the
  documented rule. The game reports the lake.
- **42 cells: river with another water bit.** One triangle covers the cell,
  with type 6 (lake and river), 5 (sea and river) or 70. The game reports
  river; the documented order gives lake or sea.
- **4 cells: land 0.5 m over a lake.** The game reports land.

On the same run, heights are 99.9462 % exact. Rounding the interpolated height
through f32, as the game evidently computes it, makes them 99.9485 % exact
(land 99.9195 %), the figures the reverse-engineering decode recorded.
Interpolating wholly in f32 changes nothing further.

Measured precedence variants, all with river first and f32 rounding:

| Rule after road strips | water cells that differ | heights exact | over 2 m |
|---|---|---|---|
| later block, later triangle (ADR 0039) | 45 | 99.9485 % | 1 707 |
| later block, earlier triangle | 33 | 99.9486 % | 1 644, worst 35 m |
| the highest surface at the point | 69 | 99.9485 % | 1 742 |
| **water, then later block, later triangle** | **4** | **99.9485 %** | **1 707, worst 29 m** |

Putting water before road strips gives the same 4 cells on Caucasus.

## Decision

At each cell centre the winning triangle is the covering finest-mesh triangle
that ranks highest by, in order: a road strip (`TRITYPE & 8`); a water
triangle (`TRITYPE & 7`, any of sea, lake or river); the higher block index;
the higher triangle index. Its `water` class is river (3) where `TRITYPE & 4`,
else sea (2) where `& 1`, else lake (1) where `& 2`, else land (0). Its
`height` is the interpolated height rounded to f32, then `floor(h + 0.5)`.
The `surface` rule of ADR 0039 is unchanged and reads this winning triangle.

### Alternatives considered

**Keep ADR 0039's rule.** It is what the format notes describe, and 87 cells
in 141.2 M is one in 1.6 M. It lost because the triangles listed under
Context show the cause, not noise: water laid over a land triangle is a
second layer, the way a road strip is, and the game reads it as one.

**The highest surface wins.** A downward ray would see the highest triangle,
and it explains the 4 cells of land over a lake. It lost on the count, 69
cells, and on the Roki tunnel, where the game reports a road strip running
under a mountain triangle.

**The earlier triangle wins within a block.** It leaves 63 fewer cells more
than 2 m off, but 29 more water cells wrong than the decision, and its worst
height error is 35 m rather than 29 m.

## Consequences

Caucasus water differs from the game on 4 cells of 141.2 M, and heights are
99.9485 % exact, 99.99859 % within 1 m. E2's done test reads "heights 99.95 %
exact and water all but 67 cells"; both hold, the first at two decimals.

The rule is fitted to one map. Water over land, and river over lake and sea,
were seen only on Caucasus; on another map a different layering would show as
water cells that differ, and only E9's cross-checks can see it, since no other
map has a live extract. The 4 cells of land over a lake remain: a rule that
also fixed them was not found.

The sampler and the rasterizer share the rule, so the `height` a later task's
on-the-ground check reads is the same one the tiles hold.
