# ADR 0013: The progress log is append-only, and `dcs.log` gets a warning as well as a phase line

## Status

Accepted

## Context

**Affects:** `extractor-hook.md` "Files"; `plan.md` X2a, X12, X13.

The frozen text names the two destinations and what goes to each:

> Progress log: `Logs/DcsTerrainExtract.log` in Saved Games, one line per
> tile and per phase change, plus `dcs.log` `INFO` lines at phase changes
> only.

It does not say how the file is opened, whether it survives across runs, or what
happens to a line that is not a phase change but is not routine either.

Three facts shape the answer. A full Caucasus run writes on the order of tens of
thousands of lines over an hour, so the file is not small but it is not large.
DCS is more often killed than exited cleanly, which decides the question of a
held handle against itself: a buffered handle loses its tail exactly in the case
where the log is the only record of what the run was doing. And a resumed run
continues an extract the previous run started, so its lines belong with the
earlier ones rather than in place of them.

ADR 0012 already routes config problems to `dcs.log` at `WARNING`, which is the
first line that is neither a phase change nor a per-tile line. The hook has
others: a failed manifest write, a terrain that unloaded mid-run, and the failure
notes the spec asks for.

## Decision

The progress log is opened in append mode for each line — open, write, close —
and is never truncated or rotated. It accumulates across runs and across DCS
sessions; a user who wants a clean one deletes it. Lines end in a bare LF, like
every other file the hook writes.

Per-line open and close is the cost of crash safety, and it is affordable at the
rate the hook writes: one line per tile is tens of thousands over an hour, not
per frame.

The log path is a value, not a function: `M.log_path` is nil until something sets
it, and `M.log` returns immediately while it is. That is what keeps the offline
tests off the disk without a fake for `lfs`, and it is why a test can drive the
real logging code over the same filesystem seam every other test uses.

`dcs.log` receives an `INFO` line at each phase change, as the spec says, and a
`WARNING` line for anything the user has to act on — a config problem, a manifest
write that failed, a terrain that went away mid-run. Every such call is wrapped
in `pcall`, and `log` is read from the global table at the call rather than at
load, so the file stays loadable in a plain interpreter.

### Alternatives considered

**Hold the file handle open and flush after each line.** Rejected. It is the
faster design and the failure mode is the one that matters: DCS killed mid-run
leaves the tail of the log unwritten, and the tail is the part that says what the
run was doing when it stopped. It would also add `flush` to the `M.fs` seam for
one caller.

**Truncate the log at the start of each run.** Rejected because a resumed run
continues the extract the previous run began, and its log lines are the second
half of one story. Truncating would delete the record of the failure that caused
the resume.

**Rotate at a size limit.** Rejected as machinery for a file that reaches a few
megabytes at worst. If a theatre ever makes it a problem, deleting the file is
still the fix.

**Send everything to `dcs.log` and drop the separate file.** Rejected: the spec
asks for both, and a per-tile line in `dcs.log` would bury every other
subsystem's output at the rate this one writes.

## Consequences

The offline tests keep working untouched, because `M.log_path` is nil by default
and ten of the fourteen existing test files never stub the logger. A test that
wants the real thing sets the path and reads the fake filesystem.

**The log file grows without limit**, across every DCS session the hook is
enabled for. Nothing in the hook will ever truncate it, and no warning is issued
as it grows. That is a housekeeping cost accepted deliberately.

**Per-line open and close happens inside the frame budget and is not itself
budgeted.** A slow open on Windows is a fraction of a 5 ms frame, and one line
per tile is not a per-frame cost, but the figure is not measured. X11 records the
run's timings against the performance targets and is where it would show up.

X12's heartbeat is a third kind of line in the same file and needs no new
destination. X13's window is a third destination, and the phase change is already
funnelled through one function so it has one place to attach.
