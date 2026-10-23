# ADR 0043: Router points lie within a lane of the decoded centerline

## Status

Accepted

## Context

**Affects:** `plan.md` E3 (its done test); ADR 0034's roads row.

E3's done test reads:

> opt-in: Caucasus router points within 1.5 m of a decoded edge at p90

The router points are the `path` points of the live Caucasus extract's
`roads.jsonl` and `railroads.jsonl`, written by the hook on DCS
**2.9.29.27468**. E3's decoder reads the map's `roads/Caucasus.rn4` from an
install of DCS **2.9.30.28738**; the terrain files are the same. Measured as
the horizontal distance from each router point to the nearest segment of a
decoded edge of its kind:

| Kind | Router points | p50 | p90 | p99 | Beyond 100 m |
|---|---|---|---|---|---|
| road | 1 310 665 | 1.43 m | 1.58 m | 4.40 m | 1 183 |
| rail | 1 515 635 | 2.18 m | 2.42 m | 2.92 m | 0 |

ADR 0034 recorded the same offset from the reverse-engineering decode, p50
1.42 m for roads and 2.18 m for rail, and read it as a lane or track offset:
the router returns a path a vehicle drives, the file stores the centerline.
The 1.5 m threshold sits inside that offset, so it fails a decode that is
right.

## Decision

E3's router check passes when the router points lie within 2 m of a decoded
road edge at p90 and within 3 m of a decoded rail edge at p90, each kind
measured against edges of its own kind.

### Alternatives considered

**Match ADR 0034's p50 to 0.1 m.** Tighter, and a sharper test that the
decode is the same one. It lost because it tests ED's lane width as much as
the decoder: a build that moves the router's lane fails a decode that has not
changed.

**Keep 1.5 m for roads alone.** Roads still fail it, at 1.58 m.

## Consequences

E3's opt-in check holds on Caucasus, with 0.42 m of margin for roads and
0.58 m for rail.

A decoder that put every road a lane off its centerline, about 1.5 m, would
still pass. The check catches edges in the wrong place, the wrong kind or
missing, not a sub-lane bias; the router's own offset hides one that small.

The 1 183 road points beyond 100 m of any decoded road are 0.09 % of the
total and do not move p90. Whether they are router paths over the junction
connectors the decoder does not read, or over ground no road covers, is not
measured.
