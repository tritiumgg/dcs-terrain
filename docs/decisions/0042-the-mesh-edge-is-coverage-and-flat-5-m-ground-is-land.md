# ADR 0042: The mesh edge is coverage, and flat 5 m ground is land

## Status

Accepted

## Context

**Affects:** ADR 0039 "Coverage" and its Consequences; ADR 0007 "The Caucasus
fill triple"; `plan.md` E2.

ADR 0039 replaced the live sweep's fill test with the mesh:

> **Coverage.** A cell is *outside the mesh* when no finest-mesh triangle
> covers its centre, and it is `nodata` in every layer. This replaces the fill
> test.

and left one check for E2:

> E2's opt-in comparison checks that the cells outside the mesh are the cells
> the live Caucasus extract wrote as fill; a difference reopens the coverage
> rule.

The fill test it replaced, from ADR 0007, marked a cell `nodata` when the sim
returned the Caucasus fill triple: `GetHeight` 5.000005, `GetSurfaceType`
`land`. That is what the sim returns 500 km outside the map.

E2's decoder rasterized Caucasus from the terrain files of DCS
**2.9.30.28738** onto the grid of the live Caucasus extract taken on
**2.9.29.27468** over the same files, 9 400 by 15 700 cells:

- No cell the live extract wrote as data lies outside the mesh, and no tile
  it left unwritten holds a covered cell.
- **6 389 965 cells the live extract wrote as fill are covered by the mesh.**
  In every one the winning triangle is land, and its height rounds to 5 m.
  Sampled triangles there are flat, all three corners at exactly 5.0 m, of
  type 0 or 64. They lie in 264 tiles, none of them wholly: along the north
  and east edges of the grid, and in a band down the Black Sea coastal
  lowland.

So the two differ, and in one direction only: the live fill was the mesh's
flat 5 m ground, which the sim reports with the same value as no terrain at
all.

## Decision

The coverage rule of ADR 0039 stands: a cell is `nodata` only where no
finest-mesh triangle covers its centre. A cell whose winning triangle is flat
land at 5 m is land at 5 m, like any other, and `height`, `water` and
`surface` are written for it. No fill value is tested.

### Alternatives considered

**Write flat 5 m land as `nodata`, as the live extract did.** It would keep
v1 and v2 extracts alike on those 6.4 M cells. It lost because the mesh says
those cells are ground, triangles like any other, and the game reports a
height and a land surface for them. A test on the height alone would also take any
real ground that happens to be exactly 5 m for missing.

## Consequences

A v2 Caucasus extract has 6.4 M more cells of data than the live one, all of
them land at 5 m: sightlines, siting and routing see that ground where a v1
pack saw a hole. Comparisons against a v1 extract must skip its fill cells,
which E2's opt-in check does.

Off the mesh, the sim still reports 5.000005 m land, and the extract writes
`nodata`. A query that leaves the mesh therefore sees a hole where the game
sees flat ground. That was already so under ADR 0039.

ADR 0007's fill triple no longer describes anything an extract writes. The
check ADR 0039 left for E2 is answered; the coverage rule is not reopened.
