# ADR 0025: The bar is the running sweep's own count, and the run reports no fraction of the whole

## Status

Accepted

## Context

**Affects:** `extractor-hook.md` "Lifecycle" (every sweep is a resumable
iterator, which now also counts) and "Performance targets" (which is not a
source of weights); `plan.md` X12, and the `progress()` contract of X5 to
X8c.

The frozen text gives each sweep a target time on the measured machine:

> Caucasus authored area at 50 m, on the measured machine: config and tables
> under a minute; the pre-sweep about a minute; `water` and `height` about
> two minutes each; roads about 25 minutes at 1 km seeds (see the roads
> sweep); `surface` about six minutes; `scenery` a few minutes

Those minutes are not measurements of a sweep. Nobody has run one. Each is a
per-call cost from the probe log, measured on one machine, multiplied by an
estimate of how many calls the sweep makes, and the roads figure depends on
the frame rate as well, because path calls are budgeted per frame. X12's plan
row asked for an overall fraction weighted by "the measured per-sweep costs",
and the first cut of X12 built one from these minutes. The maintainer did not
want a bar that depended on their machine, or one whose weights were not
measured, and both objections hold: the ratios would drift where sweeps are
bound by different things, and the counts behind them are guesses.

Until X12 the window's bar moved only at a phase change, with the two passes
given equal halves, and the sweeps had no way to say how far through their
own work they were. ADR 0011 keeps anything of this kind out of the config.

## Decision

A job's `start` may return a second function, `progress`, answering
`done, total` in the sweep's own unit of work, or nil when it cannot count.
A count is taken only as two finite numbers with a positive total, and `done`
is held inside `[0, total]`. The window asks for the count every frame, so a
sweep answers it from counters it already keeps rather than by counting. A sweep that resumes reports `done` over its
whole work with journalled tiles counted as done, so its bar climbs on a
resume rather than falling back; this binds the sweeps of X5 to X8c.

The run reports no fraction of the whole. What it says is which sweep it is
on, of how many, counted over the walk prepare, hook, mission in the order
the queues are built, and how far that sweep has got where it can count.
The window's bar is the running sweep's own count as a whole percentage:
empty where the sweep cannot count or none is running, so it drops between
sweeps, and full at done. The line beside it says which sweep of how many,
"Sweeping the terrain: roads, 5 of 9, 812 of 2210.", and ADR 0024's record
and heartbeat carry the same position in place of a percentage. Nothing in
the hook knows what a sweep costs, and nothing pretends to.

### Alternatives considered

**An overall fraction weighted by the design's minutes per sweep.** Built
first, and taken out. The minutes are one machine's per-call costs times a
guess at the counts; the bar would have been describing the guess, and its
speed would have changed at every sweep boundary on any other machine.

**An unweighted fraction, one share per sweep.** Rejected: roads is most of
the run, so the bar would cross most of its width in a few minutes and then
crawl.

**Weights from the extract's own timings.** The manifest records each
sweep's time, so a second run in the same directory could weight by the
first. Rejected: a first run has none, a resumed run's timings include the
skip-through, and the number is still one machine's.

**Report only the remaining work on a resume.** Rejected. A resumed water
sweep with every tile journalled would then report a full sweep of nothing,
and its bar would sit at the start. Counting the journalled tiles as done
costs each sweep one lookup it already makes.

## Consequences

The bar moves through every sweep that counts, and restarts at each one. It
never says how far the whole run is, because the hook does not know; a
reader who wants an estimate has the sweep's position, its count and the
heartbeat's elapsed time to extrapolate from.

**The bar drops to nothing between sweeps**, and stays hidden through a
sweep that cannot count. That is the truth about those moments and it will
look like a stall to somebody who expects one bar for the run.

**`progress()` counts journalled tiles as done.** Every sweep from X5 on
carries that obligation, or a resume shows its bar falling back.

**Nothing on screen until sweeps exist.** The one registered job today
finishes in a frame, so a live run's bar shows nothing before done. That is
correct and not a regression.

**The plan row's "overall fraction weighted by the measured per-sweep costs"
is not built**, and X11's measured timings are not needed for the bar. If a
whole-run estimate is ever wanted, it needs those timings first, and a new
record.
