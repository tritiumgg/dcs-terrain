# ADR-0008: The packed file's `meta` carries the authored rectangle, and its bounds keys are metres

- **Status:** Accepted
- **Date:** 2026-09-04
- **Affects:** `core.md` "Packed file", the `meta` key list and the `check`
  subcommand; `plan.md` tasks C5a and C6.

## Context

`core.md` lists `bounds_sw_x`, `bounds_sw_z`, `bounds_ne_x` and `bounds_ne_z`
among the `meta` keys and does not give their unit. They come from the
manifest's `bounds_km`, which `extract-format.md` defines as
`GetTerrainConfig("SW_bound")` and `NE_bound` "in kilometres as DCS gives
them", while every other coordinate in both formats is metres. Caucasus reads
SW (−600, −560) and NE (380, 1130) as kilometres, so the ambiguity is a factor
of a thousand and `check`'s test that `poi`, `airdrome` and `beacon` rows lie
inside bounds passes under either reading.

The `valid` grid is 1 where "`height` is not nodata, `water` is not 2, and the
cell is inside `authored_bounds_m` if known". `authored_bounds_m` is a
manifest field, and the manifest also records `authored_bounds_source`, which
is `config` when the value came from `nodesMapBorders` and `presweep` when the
extractor found the hull itself. The `meta` list carries neither. A packed
file therefore cannot say which rectangle its own `valid` layer was cut to, or
whether that rectangle was authored by ED or inferred by a 5 km lattice, and
ADR-0004 removed the samples that would let anyone re-derive it.

The two sources differ in trustworthiness. On Caucasus `nodesMapBorders`
bounds the authored area; on the other seven theatres the equivalent value is
the node-map image extent, which Afghanistan sets to its own bounds rectangle
and Cold War Germany sets larger than its bounds, so the extractor's pre-sweep
finds the hull instead.

## Decision

`meta.bounds_sw_x`, `bounds_sw_z`, `bounds_ne_x` and `bounds_ne_z` are DCS
metres. `pack` multiplies the manifest's `bounds_km` by 1000 when it writes
them.

`meta` gains `authored_bounds_min_x`, `authored_bounds_min_z`,
`authored_bounds_max_x`, `authored_bounds_max_z` and
`authored_bounds_source`, copied from the manifest without conversion.
`check` requires all five, `describe` returns them, and a reader that wants to
know what `valid` means reads them rather than inferring it from the grid.

## Consequences

C5a writes five more `meta` keys and C5a's done test — `check` reports every
`meta` key — covers them. Nothing else changes shape: the grid origin and
extents already record the effect of a crop, so `meta` does not repeat
`crop_m`.

A packed file now states whether its authored rectangle came from ED or from a
pre-sweep, which is the difference between a hull that is right and a hull
that is a lattice's best guess. A consumer comparing two packed files of the
same theatre can see that one was cut to a different rectangle.

`check`'s inside-bounds test becomes a real test rather than one that passes
under either unit.

The keys are per-theatre provenance and not a query surface; no operation in
`query-operations.md` reads them.

## Alternatives considered

**State the unit and add nothing.** One line, no new keys. Rejected: it leaves
the packed file unable to describe its own `valid` layer, and the extract that
could answer is on another machine or deleted by the time anyone asks.

**Store the whole manifest as a `meta` JSON blob.** `terrain_fingerprint` is
already stored that way, so the precedent exists. Rejected: the manifest holds
the tile list, which is thousands of rows that `grid_tile` already carries,
and a blob nobody can query is not provenance.
