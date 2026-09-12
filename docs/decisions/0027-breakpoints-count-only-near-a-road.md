# ADR 0027: The pre-sweep's breakpoints count only near a road

## Status

Accepted

## Context

**Affects:** `extract-format.md` "manifest.json", the authored-cell definition
and the `config.json.presweep` record; `design-and-facts.md` "Geometry facts",
the post-density rule and its "counts as authored" sentence, and "Design
decisions"; `extractor-hook.md` "Lifecycle" step 2; ADR 0026's consequence
that X11 compares the rectangle to the Caucasus hull within 10 km; `plan.md`
X6, X11.

The frozen format defines the cell the pre-sweep looks for:

> A cell is authored when a 2 km line from its centre sampled at 10 m has at
> least 60 samples whose second difference of `GetHeight` is non-zero, or
> `getClosestPointOnRoads("roads", ...)` snaps within 5 km

and the design says where the rule came from and what it must keep:

> Detailed terrain without roads exists (Zahedan, Quetta: 120 to 160
> breakpoints, roads 37 to 80 km away) and counts as authored.

Both were measured on Afghanistan and Cold War Germany, where the raster
outside the built hull is sparse and reads 0 to 35 breakpoints. The first
whole-map pre-sweep on Caucasus, **DCS 2.9.29.27468** (`timestamp`
20260902-093323), read differently. It authored 7 628 of 66 248 cells and
drew a rectangle of x −500 000..65 000, z −90 000..955 000: 565 by 1 045 km,
590 000 km², against the hull's 445 by 830 km, 369 000 km². Decoding the
bitmask against the hull showed three groups outside it, and the bridge read
them back:

| Where | Cells | Breakpoints, 10 m over 2 km | at 1 m over 500 m | Height | Nearest road |
|---|---|---|---|---|---|
| South of the hull, Turkey's mountains | about 1 000 | 97 to 144 | 68 to 336 | 750 to 2 400 m | 85 to 139 km |
| West of the hull, the Crimean mountains | about 20 | 82 to 117 | 182 to 296 | 800 to 1 200 m | 173 to 200 km |
| One coastal cell farther west | 1 | exactly 60 | 2 | 0.8 to 3.4 m | 262 km |
| North of the hull | about 100 | 23 to 28 | 5 to 9 | 8 to 66 m | 7 m to 4.6 km |
| Inside the hull, the Kutaisi plain | reference | 49 | 14 | 48 to 59 m | 1.4 km |

On this theatre the raster outside the hull is a rough elevation model of real
mountains, and the second difference counts how rough the ground is, not
whether anybody built it: the hull's own plain reads fewer breakpoints than
Turkey's ranges. The north strip is the other case: real roads beyond the
picture crop ED ships, which are built terrain the hull leaves out. The lone
coastal cell sat exactly on the threshold and added 200 km of width alone.

Two costs follow from the rectangle. `core.md` sets the packed cell size by
the authored rectangle, 50 m under 500 000 km² and 100 m above, so the
measured rectangle would pack Caucasus at 100 m. And the roads sweep seeds
every square kilometer that is not fill or sea, snapping each seed; a snap was
measured at 0 ms beside a road, 14 ms mid-sea 103 km from one and 84 ms in a
fill corner 513 km from one, and `Terrain.FindNearestPoint(x, z, radius)`,
the editor's own fallback, ignores its radius. The roadless area is exactly
the expensive area.

## Decision

A 5 km cell is authored when a road lies within 5 km of its center. It is also
authored when its line reads at least 60 breakpoints **and a road lies within
25 km**; where the snap answers nothing at all, which is no road reachable
from that point, the breakpoints decide alone. Rough ground with its nearest
road beyond 25 km is a model of real terrain and is not built.

`config.json.presweep` records the new distance as `breakpoint_road_max_m`
beside `breakpoint_min` and `road_max_m`.

The pre-sweep asks for the snap first, because it decides most cells by
itself: a road within 5 km makes the cell authored with no line read, a road
beyond 25 km rules it out the same way, and only the ring between reads its
line. Every snap made is kept as the disc it clears, since the nearest road
to a point d from its snap is at least d − r from any point r away, so a cell
within d − 25 km of an earlier query is neither asked nor sampled.

X11's check changes. The pre-sweep's rectangle on Caucasus contains the hull
ADR 0026 records and exceeds it only where the theatre has roads; X11 records
the measured overshoot rather than requiring 10 km.

### Alternatives considered

**Keep the rule and accept the rectangle.** The extract would cover 60% more
tiles, the roads and scenery sweeps would grow with the roadless area, and the
pack would drop to 100 m. Rejected for the last reason alone.

**Roads only.** Simplest, and it matches the Caucasus hull to within the
north strip. Rejected: on Marianas the snap answers nil from Pagan, which has
detailed ground and no road, and the probe log records it as caught by the
breakpoints alone. The nil case keeps that.

**Raise the breakpoint threshold.** Turkey's ranges read 120 to 144 where the
hull's plain reads 49. No threshold separates built from unbuilt when the
count measures roughness.

**Bound the snap by a radius.** `FindNearestPoint` takes one and ignores it,
measured at the same 84 ms and 513 km with a 5 km radius.

## Consequences

Zahedan and Quetta, at 37 and 80 km from a road, no longer count unless a
road lies within 25 km, and the design's sentence that they count is reduced
to the nil case. Whether either sits at an extreme of Afghanistan's rectangle,
where dropping it would move the box, is unmeasured; X11 on Afghanistan is
where the 25 km is confirmed or moved, and this record is provisional in that
one number.

A theatre that answers a far snap from an island with no road of its own
loses that island's detailed ground from the rectangle. Marianas answered nil
there in the probe log; another theatre may not.

The pre-sweep is cheaper: lines are read only in the ring between 5 and 25 km
of a road, and a far snap clears everything nearer than what it found.

**Frozen text that now reads false:** the authored-cell definition in
`extract-format.md` "manifest.json", the "counts as authored" sentence in
`design-and-facts.md` "Geometry facts", and ADR 0026's 10 km comparison for
X11.
