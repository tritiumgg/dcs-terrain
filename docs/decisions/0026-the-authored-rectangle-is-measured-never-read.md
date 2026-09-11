# ADR 0026: The authored rectangle is measured, never read from a theatre file

## Status

Accepted

## Context

**Affects:** `extract-format.md` "manifest.json", the `authored_bounds_m` and
`authored_bounds_source` rule; `extractor-hook.md` "Lifecycle" step 2;
`design-and-facts.md` "The hook-side `terrain` module is the primary
extraction API", the `nodesMapBorders` paragraph, and "Geometry facts"; ADR
0009's "config supplies no `authored_bounds_m`" clause; ADR 0011's
"`authored_bounds_m` comes from `nodesMapBorders` or the pre-sweep";
`plan.md` X5, X6, X10, X11 and the V tasks.

The frozen format gives the authored rectangle two sources:

> `authored_bounds_m` is `nodesMapBorders` from the theatre's `entry.lua`
> when the extractor is given it in config (`authored_bounds_source:
> "config"`), else the bounding rectangle, expanded by 10 km, of the authored
> cells of a 5 km pre-sweep over the bounds rectangle
> (`authored_bounds_source: "presweep"`).

and the hook runs the pre-sweep only "when `crop_m` and `authored_bounds_m`
are both nil". ADR 0011 removed the config field and had the value derived
instead: "`authored_bounds_m` comes from `nodesMapBorders` or the pre-sweep".
The design document already knew the value was not the same thing on every
theatre:

> The authored hull is `nodesMapBorders` in `entry.lua` where present
> (Caucasus in `entry.lua`; the other seven ship it in
> `MissionGenerator/nodesMap.lua`, where it is the node-map image extent and
> not the hull: Afghanistan's equals its bounds rectangle and Cold War
> Germany's exceeds it). Only the Caucasus value bounds the authored area.

What that leaves is a rule keyed on which file an assignment sits in. The
project's standing requirement is that every rule works the same way on every
theatre, including ones never measured, so the rule was checked against ED's
own use of the value and against what the install ships. Measured on **DCS
2.9.29.27468** (`timestamp` 20260902-093323), read-only, from the install:

**ED uses the value in one place, to position a picture.** The only reference
in ED's Lua is `MissionEditor/modules/me_generator_dialog.lua:180`,
`nodesMap:setMapBorders(base.unpack(theatreOfWar.nodesMapBorders))`, on the
line after `nodesMap:setSkin(SkinUtils.setStaticPicture(theatreOfWar.nodesMapFile, ...))`.
The value is the geographic extent of the mission generator's background
image, `nodesMap.png`. Nothing in ED treats it as the extent of built terrain.

**Every installed theatre carries it, and the file is an authoring choice.**
`entry.lua` assigns it on Caucasus and Nevada; `MissionGenerator/nodesMap.lua`
assigns `theatre.nodesMapBorders` on the other seven. Both land in the same
field of the same table.

**Where it can be compared with the bounds rectangle, it is a picture crop.**
In DCS metres, `{minX, minZ, maxX, maxZ}` as the files spell it:

| Theatre | File | Value | Against `SW_bound`/`NE_bound` |
|---|---|---|---|
| Caucasus | `entry.lua` | −418619.1875, 113728.15625, 26382.5, 943187.0625 | the built-terrain hull (ADR 0007) |
| Afghanistan | `nodesMap.lua` | −1180128, −534000, 532000, 756240 | equals the bounds rectangle (−1180, −534) / (532, 757) km |
| Persian Gulf | `nodesMap.lua` | −218768.75, −392081.9375, 197357.90625, 333129.125 | a central crop of (−460, −900) / (800, 800) km |
| Cold War Germany | `nodesMap.lua` | −696284.0625, −1525514, 190499.921875, 119030.007812 | exceeds (−600, −1100) / (200, −300) km |
| Nevada | `entry.lua` | −497851.625, −328660.90625, −167608.921875, 210510.859375 | not measured; Nevada's bounds are not in the probe log |

So on Caucasus the picture was cut to the hull; on Afghanistan it is the whole
map; on Persian Gulf it is the middle of it. A rule that reads the value when
it is in `entry.lua` is right on Caucasus by the artist's choice, and a new
theatre that puts an image extent in its entry file would have real terrain
silently excluded from, or coarse unauthored terrain silently included in,
its extract.

## Decision

The extractor never reads `nodesMapBorders`, from `entry.lua` or from any
other file. The authored rectangle has one source on every theatre: the
pre-sweep of `extractor-hook.md`, which measures the built terrain by post
density and road proximity, and it runs for every whole-map extract, Caucasus
included. `authored_bounds_source` is therefore `"presweep"` when the
pre-sweep ran and null when it did not (a crop run, ADR 0009); the value
`"config"` is never written. Until the pre-sweep is built (task X6), a
whole-map run on any theatre refuses with the reason that no crop was given
and the pre-sweep does not exist yet, and a crop run has a null rectangle on
every theatre.

### Alternatives considered

**Read `nodesMapBorders` from `entry.lua` when it is there.** Caucasus works
with no probe, and the frozen text and ADR 0011 both say it. Rejected: the
value is a picture extent, the file it sits in is where an author happened to
write an assignment, and the rule is correct on one theatre by accident.
Nevada carries the value in its entry file too and nothing on disk says
whether it is the hull.

**Read it and cross-check it against the bounds rectangle.** A value equal to
or larger than the bounds rectangle is plainly not the hull and could be
discarded. Rejected: a value smaller than the bounds is not thereby the hull
either. Persian Gulf's is smaller and is a crop of the middle of the map. No
check on the number alone separates "hull" from "cropped picture".

**Read it and confirm it with a cheaper probe along its edges.** Rejected as
more machinery than the pre-sweep it was meant to avoid: the pre-sweep is
about a minute on the largest theatre, once per whole-map extract.

## Consequences

Every whole-map extract pays for the pre-sweep, about a minute on Afghanistan
and less on Caucasus, and the hook has no fast path for the one theatre that
ships its hull. That is the cost of one rule for every theatre.

A crop extract records a null authored rectangle on every theatre. On Caucasus
that is a rectangle the theatre could have supplied, and `pack` marks `valid`
without it exactly as ADR 0009 already says it does for every other theatre.

Caucasus's shipped hull stops being an input and becomes a check: validation
compares the pre-sweep's rectangle on Caucasus against
`{−418619.1875, 113728.15625, 26382.5, 943187.0625}` to within the pre-sweep's
10 km margin, which is the one theatre where the probe can be verified against
an authored answer. X11's done test gains that comparison.

**Frozen text that now reads false:** the `"config"` source in
`extract-format.md` "manifest.json" and the `nodesMapBorders` sentences in
`design-and-facts.md`. ADR 0011's "from `nodesMapBorders` or the pre-sweep"
is reduced to the pre-sweep, and ADR 0009's null case widens from "config
supplies no `authored_bounds_m`" to every crop run.

**Done tests that change.** X5 reads no rectangle from a file. X6 keeps the
pre-sweep and it is no longer optional on any theatre. X10's crop extract has
null `authored_bounds_m` and `authored_bounds_source`, which `check-extract`
accepts under ADR 0009. X11 adds the hull comparison above.

**Provisional in one respect.** Nevada's `entry.lua` value is recorded above
and not compared with its bounds, because Nevada was not installed when the
probe log was taken. Measuring it would add a row to the table and change
nothing in the decision: even a Nevada value that turned out to be the hull
would be one more theatre right by an author's choice.
