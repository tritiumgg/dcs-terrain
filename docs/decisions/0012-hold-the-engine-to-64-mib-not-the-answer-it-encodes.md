# ADR 0012: Hold the engine to 64 MiB, not the answer it encodes

## Status

Accepted

## Context

**Affects:** `design.md` §7 (the server's memory) and §8 (the memory target
of ADR 0008); `plan.md` H4 and Q19.

ADR 0008 holds a query process to 64 MiB of peak commit. It was judged on the
query benchmark's cases, whose answers are modest, and every case passed.
GeoJSON on `sample`, `nearest` and `scenery_in` made answers at the row caps
large, and the process then passes the target. Peak commit, release build, one
fresh process per request, measured 2026-11-03:

| Request | Without GeoJSON | With GeoJSON |
|---|---|---|
| `sample`, `Synth`, 10 000 rows (the cap), `height` | 20.9 MiB | 64.0 MiB |
| `sample`, `Synth`, 10 000 rows, 25 layers | 72.0 MiB | 167.5 MiB |
| `sample`, `Synth`, 10 000 rows, all 57 layers | 133.2 MiB | 293.4 MiB |
| `point_info`, `Synth`, 10 000 points | 76.3 MiB | 187.2 MiB |
| `scenery_in`, Caucasus of 2.9.31.29434, 4 987 objects (cap 5 000) | 17.9 MiB | 46.9 MiB |

`nearest` returns at most 50 results, a few kilobytes.

The engine's own work here is small: `sample` of one point peaks at 1.9 MiB,
and the work of 10 000 rows is bounded by the block budget. The rest is the
answer. Each answer is built as a tree of JSON values, about ten times the size
of its text, and copied once more before it is written: `sample` at 57 layers
is 10.8 MB of text, 22.2 MB with GeoJSON. `point_info` was over the target
without GeoJSON before this change.

An answer's length is the caller's to bound. The MCP server writes an answer
over its text limit to a file. The user is not concerned with the memory an
answer takes, only the engine's, and wants the row caps kept.

## Decision

The 64 MiB of ADR 0008 holds the engine's work: decoding, the blocks and caches
it keeps, grids and searches. It does not hold the answer the engine encodes.
The row caps stay as they are. The engine is judged on the query benchmark's
cases as they stand, each a fixed request with a modest answer, and each must
peak under 64 MiB. A request whose answer is larger, rows or features toward a
cap or GeoJSON over many rows, is exempt and measured beside the target, as a
request that builds a cache entry is. The answer's memory is still cut where it
can be: encoding an answer as it is written, as `exposure` already does, is
its own task.

### Alternatives considered

- **Lower the row caps until every answer fits.** `sample` with GeoJSON would
  fall to about 2 000 rows. The user declined: the caps are what callers plan
  around.
- **Count the answer's allocations and subtract them.** A counting allocator
  told which allocations belong to the answer separates the two exactly, but
  costs code in every operation and a slower allocator, for a figure the
  benchmark's cases already give.
- **Read peak commit when the operation hands its answer over.** The GeoJSON
  is built inside the operation, so that peak already holds most of the
  answer.
- **Encode every answer as it is written, before this change ships.** It is
  the real cut, and it is its own task, not a condition of GeoJSON.

## Consequences

The benchmark's cases stay the check on the engine, and stay under 64 MiB.
A change that makes the engine itself grow shows there, because the engine's
work does not depend on how many rows the caller asks for beyond what the
block budget bounds.

A process answering a large request can reach about 300 MiB today: `sample`
with all 57 layers and GeoJSON at its cap. A caller with less memory than that
asks for fewer rows or layers. The MCP server holds no answer past its call, so
a large answer is a peak, not growth across a session.

A future case added to the benchmark with a large answer must be marked exempt,
or it judges the answer rather than the engine. Reopen this if the answer's
memory becomes the user's concern, or if encoding answers as they are written
does not bring the caps near the target.

Amends ADR 0008's memory target; the rest of ADR 0008 stands.
