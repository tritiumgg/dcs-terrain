# ADR 0019: A crop is refused under a kilometer of radius and outside the map

## Status

Accepted

## Context

**Affects:** `extractor-hook.md` "Config table", whose `crop_m` is any
`{min_x, min_z, max_x, max_z}` box; ADR 0011's crop as a center and a radius;
`plan.md` X10, X13, X14.

The frozen config table says what a crop is and nothing about what one may
be. The checker that ADR 0011 gave the window refused only shape: three finite
numbers and a radius above zero. A radius of one millimeter passed, and so did a
center five hundred kilometers out to sea, because nothing anywhere compared
the crop to the theatre.

Neither failure crashes anything. The grid is planned from the box, and a cell
outside the theatre's bounds rectangle reads as fill, so a crop off the map
extracts an empty tile set and a crop of one cell extracts one cell. Both cost
a run to find out, on a machine where a run is the scarce thing, and both
produce a packed file no query can use.

Two facts bound what a check can do. The extract cell is 50 m, and the derived
layers the packed file exists for take windows of 300 m and 2 km around a
cell: a crop smaller than that holds cells with no usable layer over them. And
the only extent every theatre publishes is the bounds rectangle, `SW_bound`
and `NE_bound` from `terrain.GetTerrainConfig`, in kilometers, readable only
while a terrain is loaded. On Caucasus it is the raster (x −600..380 km,
z −560..1130 km, measured through the bridge on 2.9.29.27468 and matching the
probe log) and not the authored land; the authored hull is known for Caucasus
alone and costs a pre-sweep to find elsewhere.

## Decision

A crop's radius is at least 1 000 m, and its box, the center plus and minus
the radius on each axis, lies inside the theatre's bounds rectangle. Both are
refusals, never adjustments: the radius floor is a config problem like any
other, phrased `crop.radius_m is under 1000 m: <value>`, and a box that
crosses the rectangle is `crop reaches past the map, <axis> <below|above>
<bound>: box edge <value>`, naming the first edge crossed in whole meters,
with the finding before the colon because the window shows only that much.

The rectangle check runs wherever there is a theatre to check against: at
Start, when a map is open; in the run itself the first time idle finds a
terrain, so a config written at the main menu is refused before prepare has
written anything, with the reason warned to the log and left on the run for
the window to show; and at the pick, which ignores a click off the theatre and
stays armed. A Start at the main menu goes ahead, because there is nothing to
check against there and the run will.

The check is against the bounds rectangle, not the authored hull. A crop in
the sea inside the rectangle passes and extracts sea, which is what it asked
for.

### Alternatives considered

**Clamp the box to the rectangle.** Rejected: a box cut back silently is a
different extract from the one asked for, and the manifest would record a
crop the user never typed.

**A floor of one cell, or of 500 m.** Rejected: a one-cell crop is a shape
check dressed as a limit, and a 1 km box has the derived layers as edge
effects throughout. Two kilometers is the smallest box the 2 km window can be
taken anywhere in.

**Check against the authored hull.** Rejected: it is known for one theatre and
found by a pre-sweep on the rest, which is a run's worth of work to refuse a
run.

## Consequences

A crop that cannot produce a usable extract is refused at the press, or at the
first frame with a terrain, rather than after a run. X10's 5 km crop around
Kutaisi passes both checks.

**A config written at the menu is refused later than it was written.** The
user sees the reason when the map opens, on the window's line, and in
`dcs.log`; nothing is written to the output directory first.

**The floor is a number the specs did not have**, and it is a project number
rather than a DCS one: it follows from the derived layers' windows, and a
change to those windows reopens it.

**Provisional in one respect.** The bounds rectangle was read on Caucasus
alone. The probe log's table carries every installed theatre's rectangle, and
a theatre whose `SW_bound` or `NE_bound` is missing or degenerate makes the
seam answer nil, which switches the check off for that theatre rather than
refusing every crop on it.
