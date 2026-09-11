# ADR 0024: The progress log carries a heartbeat and a totals line

## Status

Accepted

## Context

**Affects:** `extractor-hook.md` "Files" (the progress log's line kinds) and
"Lifecycle" step 5 (log totals at done); ADR 0013's closing consequence;
`plan.md` X12.

The frozen text defines the progress log by its two kinds of line:

> Progress log: `Logs/DcsTerrainExtract.log` in Saved Games, one line per
> tile and per phase change, plus `dcs.log` `INFO` lines at phase changes
> only.

and, at the end of the lifecycle:

> **done**: log totals, write the final manifest, stop registering work.

Neither says what "totals" are, and the two line kinds leave a gap: the roads
sweep writes no tiles, and the performance targets put it at about 25 of the
run's roughly 40 minutes, so for over half of a full run the log says nothing
and a window showing only the phase stands still. ADR 0013 anticipated the fix
in its consequences: "X12's heartbeat is a third kind of line in the same file
and needs no new destination." ADR 0011 keeps every cadence out of the config,
and ADR 0015 binds anything the window shows to its latch.

Measured facts that shape the choice: `os.clock` is wall time on the C
runtime DCS ships with, which the frame budget already relies on; the log is
opened, appended and closed per line (ADR 0013), so a line costs one file
open; and a run can wait at the main menu for hours between Start and a map
being opened.

## Decision

While a run is in prepare, the hook pass or the mission pass, it keeps a
record of where it is, refreshed every second of wall clock: the phase, the
sweep, which sweep that is of how many over the whole walk, the sweep's own
count where it has one (ADR 0025), and elapsed seconds since the terrain was
found this attempt. The window's line is the phase's sentence with the
record's words after it, "Sweeping the terrain: water, 3 of 9, 1234 of
5000.", written when the words change. Every ten seconds the same record is
written to the progress log as one line, the third kind:

```
heartbeat hook water 3/9 1234/5000 elapsed 812 s
heartbeat hook roads 5/9 elapsed 812 s
```

The count is absent where the sweep cannot count, and no line carries a
fraction of the whole run, because the hook has none (ADR 0025). Only a
frame that leaves
the run in its phase reports, so a heartbeat never names a phase the run has
just left, and the record is cleared at every phase change, so a resume
cannot show the last attempt's sweep. Nothing goes to `dcs.log`, and no
heartbeat is written in idle, whose wait is already logged when it ends.

At done, after the phase line, one totals line:

```
totals 1234 tiles 56789 frames 2456 s in sweeps
```

from the tiles journalled, the frames taken and the sum of the sweeps'
timings, all of which accumulate across a Stop and a Start into the same
directory (ADR 0014, ADR 0017). Each sweep's own time is already in the log
where it finished and in the manifest, so the line does not repeat them.

### Alternatives considered

**The per-tile lines are progress enough.** Rejected. Roads writes no tiles
and is over half the run, so the log would still be silent for twenty-five
minutes, and a line per tile says which tile, not how far.

**Heartbeats in `dcs.log` as well.** Rejected for the reason ADR 0013 gives
for per-tile lines: at this hook's rate they would bury every other
subsystem's output in the one file everybody reads.

**A frame count between heartbeats.** Rejected. The editor's frame rate
varies with what else is on screen, and a reader compares the log against a
clock.

**One cadence for the line and the log.** Rejected, narrowly. A count on
screen should move faster than a log should grow: a second is what somebody
watching wants, and at that rate a full run writes about 2 400 heartbeats,
most of the file. Ten seconds for the log is 240 lines a run, and the second
constant costs nothing.

**Elapsed since Start.** Rejected. A run can wait at the main menu for hours
before a map is opened, and none of that is work; elapsed counts from the
terrain being found.

## Consequences

Somebody reading the log during roads sees the count move every ten seconds,
and somebody watching the window sees it move every second.

**A press's words are replaced by the next record.** "A run is already going.
Stop it first." used to stand on the line until the next phase; now the next
record, due within a second, replaces them. Refusals are unaffected, because
they exist only in the stopped and done states, which keep no record.

**The log grows faster.** About 240 lines a run on top of the tile lines, in a
file ADR 0013 never rotates.

**Nothing on screen until sweeps exist.** The one registered job today
finishes in a frame, so a live run's line and bar show no record before done.
That is correct and not a regression.

The progress log's definition in `extractor-hook.md` "Files" now reads short
by one kind of line, and its lifecycle step 5 has its totals defined here.
