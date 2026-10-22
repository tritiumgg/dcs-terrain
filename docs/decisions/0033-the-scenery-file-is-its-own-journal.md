# ADR 0033: The scenery file is its own journal, and its last box is asked again

## Status

Superseded by [ADR 0034](0034-the-extract-is-decoded-from-the-terrain-files.md)

## Context

**Affects:** `extractor-hook.md` "Mission-pass sweeps", the `scenery` entry,
and "Lifecycle", the resumable-iterator paragraph; `plan.md` X8b, X8c.

The only resume the documents describe is the tile journal:

> Every sweep is a resumable iterator over tiles. [...] On resume, tiles in
> the journal are skipped.

and the scenery sweep writes no tiles, only "`scenery.jsonl` in append mode
as spheres complete". The frozen format has no line for a finished search.
The file is over a gigabyte on Afghanistan (ADR 0032), and hook-state file
handles have no `seek`. `passes.mission.complete` is set when the mission
pass's jobs drain, and a directory extracted before the scenery job existed
already has it true with no scenery.

## Decision

The scenery file is its own journal, as the roads file is (ADR 0030). Each
box's lines are contiguous and the boxes come in lattice order. On every
Start the job reads the file back a chunk a step and classifies each line's
x and z into its box with the lattice's own edges (ADR 0032). A box must never
come before the one above it; a file whose boxes go backwards, or that holds
a line that does not parse or lies off the grid, refuses with its name and
asks for it to be moved aside. The read-back keeps the ids of the last box's
lines and nothing else. The sweep resumes at that box: it is searched again
whole, and only the objects whose id the file does not already hold are
appended, because its lines reach the file in slices and nothing marks the
last one, and a search after a Start need not return objects in the same
order. A cut-short last line is dropped by ADR 0030's copy, the head of the
file into a fresh file a chunk a step and swapped in. The job never consults
`passes.mission.complete`.

### Alternatives considered

**Cut the file back to the start of the last box and append it whole.** No
id set at all, but the cut is a copy of everything before the last box, over
a gigabyte on Afghanistan, on every Start. Rejected.

**A completion file beside the extract, one line per box.** Cheaper to read
back than the scenery file, but a file the frozen format does not have.
Rejected.

**Start the sweep again from nothing.** No read-back, but a Stop loses the
sweep, minutes on Afghanistan. Rejected.

**Skip the job when `passes.mission.complete` is true**, as the road jobs skip
on the hook pass's flag. The flag is the pass's, and a directory from before
this job has it true with no scenery. Rejected.

## Consequences

A Stop costs one box searched again. Every Start reads the whole file back, a
completed run's included; on Afghanistan that is over a gigabyte in 64 KB
steps, likely minutes, and is not yet measured. A kill mid-append also costs
one copy of the file.

An object whose id appears twice in the last box's search, which the
measurements never showed, is written once on a resume and as often as it
came back on a run that was never stopped.

The catalogue (X8c) cannot be built from what the sweep holds in memory: a
resumed sweep has not seen the boxes before its cursor, and the footprint
`type` and the `getDesc` fields are not in the line. X8c needs a source that
survives a resume.
