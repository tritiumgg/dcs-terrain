# ADR 0032: The scenery sweep searches a 2 km box lattice

## Status

Accepted

## Context

**Affects:** `extractor-hook.md` "Mission-pass sweeps", the `scenery` entry,
and "Performance targets"; `design-and-facts.md` "Design decisions", the
scenery paragraph; `extract-format.md` "Tables", the `scenery.jsonl` entry;
`plan.md` X8b.

The frozen hook sizes the sweep on Caucasus:

> **scenery**: one chunk per sphere of radius 15 km centred on a 20 km
> lattice covering the grid [...] The hook de-duplicates by `id` across
> spheres [...] Whole-theatre counts are around a million objects on
> Caucasus

and the design names a second shape it never measured: "Tile the bounds
rectangle with 20 km spheres (or boxes)". It does not say where the lattice
starts, how high a sphere sits, or whether an object outside a crop is kept.

Measured through the bridge on Afghanistan, DCS 2.9.30.28536, with a mission
running:

- `world.VolumeType.BOX` takes `params = {min = vec3, max = vec3}`, and a
  handler returning false stops the search.
- On a 3 x 3 km square in Kabul a box and a sphere around it return the same
  21 260 objects inside the square, none missing either way. The box also
  returns 180 objects whose position lies up to about 15 m outside it.
- The 20 km square around Kabul airfield holds 994 339 objects; the 15 km
  sphere on it, 1 453 844. The whole theatre holds more than ten million: a
  count stopped at the bridge's instruction budget after 16 s.
- A box of 21 260 objects answered through `net.dostring_in` as one
  tab-separated record per object takes 114 ms and returns 1.85 MB, about
  5.4 µs and 87 bytes an object. An empty box call costs about 20 µs.
- No type name in the Kabul square or a rural box near Bagram is a plant.
  The objects are houses, walls, containers, stalls, rubble and the like;
  `PARTHOUSEAFGHANISTAN_01` to `_09` are 13 000 of the Kabul square's 21 000.

One Kabul sphere is about 86 MB in one call and five seconds in one frame,
and the id set the spec de-duplicates with would hold more than ten million
entries in the GUI state.

## Decision

The scenery sweep searches axis-aligned boxes 2 km on a side, on a lattice
anchored on the grid origin, row-major, covering the grid; a box at the
grid's far edge is clipped to it. Box `(i, j)` spans `[b(i), b(i + 1))` in x
and the same in z, where `b(k) = origin + k * 2000`, and from 1 000 m below
sea level to 10 000 m above it. One function computes the edges for both the
chunk's literals and the hook's classification of a line, so the two compare
against the same doubles. The chunk keeps an object only when its `getPoint()`
x and z lie in its clipped box, lower edges included and upper edges not;
every object then belongs to exactly one box, so nothing is de-duplicated by
id, and an object outside the grid, which a crop cuts, is not written. Boxes
in the skip set are searched like any other: an empty one costs 20 µs and a
sea tile can hold a bridge or a harbor wall.

### Alternatives considered

**The spec's 15 km spheres on a 20 km lattice, de-duplicated by id.** Tens of
megabytes and seconds in one call over any dense city, and a ten-million-entry
set. Rejected.

**Spheres that own their square.** Removes the set, but keeps the call size,
and an object more than about 5 km above or below the sphere's center at a
corner of its square is missed by its own sphere. Rejected.

**Adaptive boxes, split when a capped search fills.** Keeps empty country to
a few large calls, but a resumed line could no longer name its box without
the tree that made it, and the fixed lattice's overhead is about five seconds
on Afghanistan. Rejected.

## Consequences

A dense 2 km box is about 10 000 objects, about 55 ms and under a megabyte in
one call. Afghanistan's grid is about 236 000 boxes, about five seconds of
call overhead. The sweep's call time on Afghanistan is about a minute for ten
million objects, before the hook's own work per object.

`scenery.jsonl` on Afghanistan is over a gigabyte, against the spec's million
lines on Caucasus. The packer from C5 on reads it in one pass, and every
Start reads it back (ADR 0033).

A crop's file holds only the crop's objects, so a siting query near a crop's
edge sees no scenery past it.

`extractor-hook.md`'s sphere sizes, its 3.2 MB and 525 ms figures and "around
a million objects" now read false for the sweep, and the design's "20 km
spheres (or boxes)" for the lattice. The X8b done test re-reads a box, not a
sphere. The probe log's scenery figures are Caucasus on 2.9.29.27278; the
figures above are Afghanistan on 2.9.30.28536.
