# ADR 0004: Spread a query's work over every core, below normal priority

## Status

Accepted

## Context

**Affects:** `design.md` §3 (the cache's build and query threads) and §8
(the targets, stated for one thread); `plan.md` Q16.

The design ran index builds on at most two threads at low priority and
queries on one, with `--threads` to override both, so the tool never takes
the CPU from a DCS server beside it. `--threads` was not built.

The performance pass measured where the time goes on 2.9.30.28738
(`data/query-bench.md`). The large requests are each many independent
pieces of work: a viewshed's radials, a horizon batch's cells, the tiles
under a window, the blocks under scattered points. On an 8-core machine,
spreading those pieces over every core, against the same code on one
thread, OS cache warm:

| Case | Before the pass | One thread | Every core |
|---|---|---|---|
| Caucasus `viewshed` 20 km | 99 ms | 99 ms | 15 ms |
| GermanyCW `viewshed` 20 km | 87 ms | 80 ms | 12 ms |
| GermanyCW horizon first touch | 933 ms | 703 ms | 134 ms |
| GermanyCW 10 k scattered points | 1 027 ms | 470 ms | 180 ms |

Over every case of the benchmark on Caucasus and GermanyCW, warm, the pass is
1.80× on one thread and 2.2× on every core. A small request (one point, a
1 km layer, one sightline) stays on the calling thread either way, because
handing work out costs more than it saves there.

A Windows thread below normal priority runs only when no thread of normal
priority wants the core. Lowering a worker's CPU priority this way leaves its
I/O priority at normal, unlike the background mode the betweenness build uses.
Interleaved with workers at normal priority, on GermanyCW with DCS running,
the medians matched within 5 %.

## Decision

A request spreads its parallel work over a pool of worker threads, one per
core, each below normal CPU priority on Windows, so a DCS server beside the
tool takes the CPU whenever both want it and the tool uses the cores DCS
leaves idle. The calling thread keeps its priority. Work too small to pay
for handing out stays on the calling thread. `--threads N` caps the pool,
the road index's threads and the betweenness build, which keeps its two
threads in background mode. Results do not depend on how the work falls:
every parallel result is merged in a fixed order.

### Alternatives considered

- **One thread, as designed.** It keeps the rule literally, and costs the
  large requests most of what the pass gained: a 20 km viewshed stays at
  99 ms rather than 15 ms.
- **Every core at normal priority.** No faster when the machine is idle, and
  it competes with a DCS server for the CPU when it is not.
- **Background mode for the workers, as the betweenness build has.** It also
  lowers I/O priority, so a query's block reads would wait behind every other
  reader of the disk.

## Consequences

- A large request finishes several times sooner on an idle machine, and no
  later than one thread would beside a busy DCS server, short of the time
  slices the OS still hands a below-normal thread.
- A request may use every core for a moment. A user who wants it to take
  fewer gives `--threads`.
- The targets in `design.md` §8 are no longer single-thread figures.
- Elsewhere than Windows nothing is lowered, since DCS runs only on Windows.
- Reopen it if a DCS server beside a running query is measured losing frames.
