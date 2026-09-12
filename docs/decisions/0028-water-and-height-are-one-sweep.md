# ADR 0028: Water and height are one sweep

## Status

Accepted

## Context

**Affects:** `extractor-hook.md` "Hook-pass sweeps", the `water, height`
entry, and "Performance targets"; `extract-format.md` "manifest.json", the
`timing_ms` example; ADR 0025's count of sweeps; `plan.md` X6, X11.

The frozen design makes the two hook-pass layers two sweeps, and says why:

> **water, height**: two separate sweeps over the tile grid in tile order
> `tx` outer, `tz` inner, and within a tile row-major. Sequential access is
> what makes `GetSurfaceType` cheap (0.00065 ms versus 0.113 ms scattered),
> so never interleave layers within a tile.

The probe behind that sentence measured one call at scattered points against
the same call at points 1 m apart. It did not measure two different calls at
one point in turn, which is what interleaving the layers would do. Measured
on Saipan, **DCS 2.9.29.27468** (`timestamp` 20260902-093323), through the
bridge in the hook state, one 256 by 256 tile at 50 m walked row-major:

| Walk | Time |
|---|---|
| `GetSurfaceType` over the tile, then `GetHeight` over the tile | 93 ms |
| Both calls at each cell, one pass | 43 ms |

And 40 000 cells along one row: surface alone 0.125 µs a cell, height alone
0.125 µs, both in turn 0.225 µs, the two passes one after the other 0.275 µs.
The second call at a cell finds the terrain the first has just loaded, so the
fused walk costs less than either layer walked alone. The first live sweeps
on Caucasus, two passes as designed, took 118 s for water and 71 s for
height over 2 294 tiles, and 53 s and 8 s on Marianas.

Two passes also cost the fill test a call. Each sweep compares its own
return with the triple first and makes the other calls only on a match, so on
a land-fill theatre the water sweep pays a height call on every land cell,
and the height sweep then reads the same height again.

## Decision

Water and height are one hook-pass sweep, `water+height`, that walks the
grid once in the order the design gives and reads both calls at each cell.
The fill test is one test on the two returns and, where the fill is sea, the
seabed. A tile is one step and writes water then height, each with its own
journal line, so the extract format is unchanged: two layers, two tile files,
a `tiles` entry each. A tile that is fill throughout is written in neither
layer; a tile whose every water byte is sea is written for water only, and
both join the skip set for the sweeps after, which no longer needs a sweep
ordering to exist. `timing_ms` carries one key, `water+height`, in place of
two, and the sweep count the window and the log report drops by one.

A layer that already has a journal line always gets its file, whatever the
tile now reads, because a line cannot be taken back and a line for a file
that is not there never validates.

### Alternatives considered

**Keep two sweeps.** They run at the call floor as designed, and the roads
sweep dominates a whole-map run. Rejected: the floor for two passes is twice
the floor for one, measured, and the change is contained in one job.

**Two sweeps sharing a cache of the first pass's heights.** The water sweep
already reads a height on every land cell; keeping them for the height sweep
would save its calls. Rejected: 148 million heights on Caucasus is more than
the hook state should hold, and the fused walk saves the same calls and the
tile's second load besides.

## Consequences

The two layers cost about what one did: roughly half the time of the two
passes on every theatre, and one call fewer per land cell on a land-fill
theatre.

The progress line names one sweep where the design named two, `water+height,
6 of 6`, and a reader of `timing_ms` looking for `water` and `height` finds
one key. `check-extract` and `pack` read the layers and the `tiles` entries,
which are unchanged.

A tile of failed calls, nodata in the layer that failed and real in the
other, is written and journalled in both, as before.

**Frozen text that now reads false:** the "two separate sweeps" and "never
interleave layers within a tile" sentences of `extractor-hook.md` "Hook-pass
sweeps", its "about two minutes each" target, and the `timing_ms` example of
`extract-format.md` "manifest.json".
