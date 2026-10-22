# ADR 0040: The extractor fits the projection, and `pack` checks it

## Status

Accepted

## Context

**Affects:** `extract-format.md` "config.json" (`crs`, `latlon_samples`) and
"towns.json"; `core.md` "Projection"; ADR 0038; `plan.md` E6, C5c, C10.

The frozen format has the extractor ask the sim for town positions and leaves
the fit to `pack`:

> **towns.json**: array of `{name, display_name, lat, lon, x, z}` from the
> theatre's `Map/towns.lua` (path given in config; the extractor reads it
> with `dofile` in a sandboxed environment), `x, z` from
> `terrain.convertLatLonToMeters`.

ADR 0038 kept the fit in `pack`, with the samples taken from the beacons of
`Beacons.lua` rather than from the sim. That leaves `towns.json` with no
source for `x, z`: `Map/towns.lua` gives latitude and longitude only, and
under ADR 0034 nothing asks the sim. The same holds for an airdrome's `lat`
and `lon` when the hook has not run, since the decoders give only DCS meters.

Task E6 already fits the beacon pairs: its done test is that the fit
reproduces the design's table on every installed map. The fit projects each
beacon pair once per candidate whole-degree meridian, a few dozen of them, and
takes no measurable time.

## Decision

`dcsterrain extract` fits the projection from the beacon pairs of
`Beacons.lua`, by the method ADR 0038 describes, and writes it to
`config.json` as `crs`: `{proj4, lon_0, easting, northing, k_0, n, rms_m,
max_m, source}`, where `source` is `"beacons"`. A map with fewer than three
beacons carrying both `position` and `positionGeo` takes its parameters from
a table compiled into the extractor (the design's table plus Nevada's row from
ADR 0038), with `source` `"table"` and `n`, `rms_m` and `max_m` null; a map in
neither stops the run. The extractor uses the fit to write each town's `x, z`
and each airdrome's `lat, lon` where no other source gives them.
`latlon_samples` is still written, one `{x, z, lat, lon}` per beacon pair, on
the table path too, where it holds the zero to two pairs there are.
`pack` no longer fits: it projects every `latlon_samples` entry through
`crs.proj4`, fails when a residual exceeds 2 m unless
`--allow-crs-residual`, and stores `crs.proj4` as `meta.crs_proj4`. With no
samples the check is skipped, and the pack report says so.

### Alternatives considered

**Keep the fit in `pack` (ADR 0038 as written).** The extract would carry
towns with null `x, z` and `pack` would fill them after fitting. It lost
because an extract would then not be self-describing: a reader of
`towns.json` gets coordinates only after a pack, and C5c, which loads the
`poi` table, would wait on C10. The fit has to exist in the extractor anyway
for E6's test.

**Fit in both and compare.** Two implementations of the same least squares
buys a cross-check that the residual test already gives.

## Consequences

An extract is complete in DCS meters on its own: every town and airdrome has
`x, z`, and every airdrome with a position has `lat, lon`. ADR 0038 is
superseded: its source for the samples and its fallback table stand, restated
here, and only where the fit runs changes.

`pack` trusts the extractor's parameters and only checks them, so a fit that
is wrong yet consistent with its own samples passes. The samples are the
beacons the fit came from, so the check catches a broken writer or a hand-edited
`crs`, not a bad fit. The design table, compiled into the extractor, is what
catches a bad fit on the maps it lists: E6 compares against it.

The fallback table moves from `pack` into the extractor. A map that gains
beacons later needs no table row; a map without them needs one before it can
be extracted at all, where before it failed at pack.

C10 shrinks to the residual check and `meta.crs_proj4`; its done test checks
that a hand-altered `crs` fails the residual. E6 writes `crs`, the town
coordinates and the airdrome lat/lon. C5c reads `x, z` from `towns.json`
directly.
