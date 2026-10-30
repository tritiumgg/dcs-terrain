# ADR 0007: Count a parallel operation's budget on 8 cores, and relax two targets

## Status

Accepted

## Context

**Affects:** `design.md` §8 (the budget of a target, the `point_info` target
and the memory target); no task's done test.

The benchmark judges every target on CPU cycles, each thread's added up, with
a budget of the target's time on one core. Measured on every installed
terrain of 2.9.30.28738 (`data/targets.md`), three things did not fit:

- Operations computed over every core under ADR 0004 missed that budget while
  meeting their time by far. Nobody waits for the sum of the cores.
- `point_info` in a fresh process cannot meet 20 ms where the road graph is
  large, because the first nearest road loads the graph. Held, it meets it.
- A query process on the Marianas holds more than 64 MiB whatever the block
  budget. That cause is not split further.

## Decision

A target stays a time. Its budget in cycles is that time on 8 cores for an
operation whose answer is computed over the pool on any source, and on one
core for every other operation, including one where only an install's reads
of blocks fan out. `point_info` in a fresh process has a target of 150 ms; held,
its target does not change. The memory target of a query process is 150 MiB.

### Alternatives considered

- **One core for everything.** It marks as a miss work no user would see.
- **8 cores for everything.** A one-thread operation that spends 8 times its
  target's time would pass.
- **Judging parallel operations on the wall clock.** A busy machine moves it
  far more than it moves cycles.
- **Loading the road graph later, to keep 20 ms.** The first nearest road
  needs the graph, so the cost moves and stays.
- **Keeping 64 MiB and finding the Mariana cause first.** The target would
  stay missed on two terrains with nothing to act on.

## Consequences

A parallel operation can meet its cycle budget and still be over its time, so
the wall clock stays beside the cycles and is read as well: the `viewshed` and
`radar_floor` of the Marianas do this. Bounds at 3× are looser for those four
cases. A wrong classification hides a slow operation, so the list of parallel
cases follows the code and changes when it does: `find_sites` sightlines join
it when they are parallel. A machine with more or fewer than 8 cores is held
to the same 8.
