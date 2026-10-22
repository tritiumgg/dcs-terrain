# ADR 0038: The projection is fitted from the map's beacons

## Status

Accepted

## Context

**Affects:** `extract-format.md` "config.json" (`latlon_samples`); `core.md`
"Projection"; `design-and-facts.md` the projection table and its note that
Kola and Nevada are not measured; `validation.md` "Measuring a new theatre or
build"; `plan.md` C10, V2; ADR 0004.

The extract format takes its projection samples from the running sim:

> `latlon_samples`: 20 entries `{x, z, lat,
> lon}` from `terrain.convertMetersToLatLon` on a regular lattice inside the
> grid, for `pack` to verify or fit the projection

Under ADR 0034 nothing is asked of the running sim. Each map's `Beacons.lua`
gives every beacon both as `position = {x, alt, z}` in DCS meters and as
`positionGeo = {latitude, longitude}` to six decimals, which is about 0.1 m.
Fitting a transverse Mercator (`k_0 = 0.9996`, WGS84) to those pairs on DCS
**2.9.30.28738** reproduces the design's measured table:

| Map | Beacons | rms / max residual | Against the table |
|---|---|---|---|
| Caucasus | 164 | 0.04 / 0.07 m | within 0.01 m |
| Syria | 151 | 0.04 / 0.07 m | within 0.01 m |
| Afghanistan | 49 | 0.04 / 0.07 m | within 0.01 m |
| Cold War Germany | 413 | 0.04 / 0.06 m | within 0.01 m |
| Marianas | 19 | 0.05 / 0.07 m | within 0.01 m |
| Persian Gulf | 101 | 0.04 / 0.07 m | within 0.01 m |
| Nevada | 45 | 0.04 / 0.07 m | not in the table: `lon_0` −117, offsets 193996.81 and 4410028.06 |

The residual floor is the six-decimal rounding. Any other whole-degree
meridian fits 1.7 km or worse, so `lon_0` is unambiguous. Nevada's runways,
converted with its fit, land on Nellis and Las Vegas within 0.002°.
Marianas WWII's `Beacons.lua` carries no positions.

## Decision

`dcsterrain extract` writes `latlon_samples` from every beacon in the map's
`Beacons.lua` that has both `position` and `positionGeo`, as `{x, z, lat, lon}`.
`pack` fits and checks them as before, with the residual limit unchanged. A
map with fewer than three such beacons takes its parameters from the design's
table, and `pack` refuses a map that is in neither.

### Alternatives considered

**Ship the table only.** Every measured map is in it, so it is the simplest
source. It lost because it covers only the maps measured so far, and Nevada,
the first new one, would have needed a live probe.

**Read the parameters from the sim through the hook's tables pass.** It works,
but it makes the hook required for every map, which ADR 0037 avoids.

## Consequences

A newly installed map gets its projection with no DCS session, and the
`tools/probe-theatre/` step of measuring one becomes a check rather than a
requirement. Nevada joins the measured maps through this ADR's table.

The samples are where the beacons are, not a lattice over the grid. They
cluster at airfields, so the fit is checked where beacons sit, and a map with
all its beacons in one corner is checked less well across the rest. The
residual is reported with the point count, so a thin set is visible.

Marianas WWII falls back to the table, which gives it the same parameters as
Marianas. Kola, not installed here, is unmeasured until it is.
