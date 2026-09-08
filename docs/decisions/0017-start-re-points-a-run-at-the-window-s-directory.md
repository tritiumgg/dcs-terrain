# ADR 0017: Start re-points a run at the directory the window names

## Status

Accepted

## Context

**Affects:** `extractor-hook.md` "Lifecycle", whose states a run may now
re-enter from `done`; ADR 0014's "Start resets nothing" clause; `plan.md` X13,
and X10 in so far as a live run is begun from the window.

ADR 0011 made the window the only surface the config has, and X13's Start
writes `Config/DcsTerrainExtract.lua` from the controls. That leaves a question
neither record answers: what the *running process* does with settings that
changed. The config file is read once, at load, before there is a window; a run
is built from it there and holds `dir` from that moment.

Two facts make "assign the new directory and start" wrong.

The first is that a run accumulates state describing one output directory. Its
manifest carries the theatre identity, the grid, the per-sweep timings and the
pass record; `run.entries` is the tile list rebuilt into that manifest at every
save; `run.timing_ms` accumulates across a Stop deliberately, because ADR 0014
decided the manifest "records the whole of the work done in this output
directory rather than the last attempt at it". Every one of those sentences is
true of a directory, not of a run.

The second is the order of a phase change. `M.start` re-enters idle and saves
nothing, but the first frame that finds terrain enters prepare, and `M.enter`
saves the manifest on entry — before any prepare job has run, which is where
`M.prepare_resume` lives. So a run whose `dir` was swapped and whose manifest
was not writes the old directory's manifest into the new directory, listing
tile files that are not there. An extract whose manifest names a missing tile
fails validation permanently, and nothing later can repair it: the tiles it
claims were never written here. The resume check that would have refused the
mismatch runs one phase change too late to prevent it.

A changed crop is the same failure wearing different clothes. The grid is
planned from the crop, so a crop changed between runs makes a manifest that
disagrees with the one on disk about the grid — and the disagreeing manifest is
written before the check that would have caught it.

Separately, and forced by the same section: `M.start` refuses unless the state
is `stopped`, and `M.stop` refuses in `stopped` and in `done`. A run that
reaches `done` therefore has no way back, and the window's buttons are inert
for the rest of the session. This is not a corner: with no sweeps yet
registered, `M.jobs` is three empty lists, every phase's queue finishes on its
first frame, and a run reaches `done` within a few frames of Start — which is
exactly the run X13's live acceptance presses Start on.

## Decision

Start applies the settings the window holds to the live run before beginning
it, through `M.retarget(run, config)`. Where `output_dir` and the crop are
unchanged, it replaces the config and leaves everything else, so Stop and Start
resume exactly as ADR 0014 says. Where either changed, everything the run
accumulated about the previous directory goes back to what `M.new_run` builds:
the manifest, the tile entries, the timings, the identity and the frame
counters. The run table is edited in place and never replaced, because
`M.callbacks(run)` closed over it when the bootstrap registered it, and a
replacement would be invisible to the callbacks that drive it.

`M.start` accepts `done` as well as `stopped`, and always re-enters idle from
either. Retargeting is legal from those two states alone. They are the two in
which `run.queue` is nil and no job is holding a directory it read at its own
`start`, which is what makes replacing the run's state underneath it safe.

### Alternatives considered

**Reset on a changed `output_dir` alone, and leave a changed crop to the resume
check.** Rejected. `M.prepare_resume` does compare grids and does refuse a
mismatch, but it runs inside a prepare job, and the manifest was overwritten
when the phase was entered. The user gets a refusal after the damage, about a
directory that now holds a manifest describing neither run. A rule whose
failure mode is "correct diagnosis, one step too late" is worse than no rule.

**Refuse a changed directory outright, and tell the user to restart DCS.**
Rejected, and it fails on the first case rather than an edge one: an installed
hook that has never been configured has no `output_dir` at all, so the run is
built with `dir` nil and the very first Start is always a change. This would
refuse the workflow it exists to serve.

**Rebuild the run rather than resetting it.** Rejected: the callbacks hold the
original table. Re-registering a fresh run against `DCS.setUserCallbacks` on
every Start would make the number of live callback sets a function of how many
times somebody pressed a button.

**Keep `M.start` refusing from `done`, and require a DCS restart for a second
extract.** Rejected. It makes the window a single-use control, and it cannot
be reconciled with retargeting at all: the state a user is in when they want a
different directory is usually the state where the last one finished.

## Consequences

The window can be used more than once in a session, which it could not before,
and a second extract into a different directory is a directory name and a
press.

**ADR 0014's "Nothing is reset" is narrowed.** It now reads: nothing is reset
unless the output directory or the crop changed. Its reasoning survives intact
for the case it was written about — a Stop and a Start against the same
directory — because that is exactly the case in which nothing is reset.

**A finished run restarted unchanged looks like a button that did nothing.**
It re-enters idle, finds the terrain, prepares, resumes, discovers every tile
already journalled, and returns to `done` in a handful of frames. That is
correct and it is invisible: the status line reads *Finished.* both before and
after. Nothing in this decision fixes it, and a run that genuinely has work
left behaves the same way at the start of it.

**`M.retarget` and `M.new_run` have to agree, and nothing makes them.** A field
added to `new_run` by a later task and not added here survives a retarget as a
stale value describing the old directory — the same class of bug this record
exists to prevent, reintroduced quietly. The defence is a test asserting the
two produce the same key set, which fails when they drift rather than when the
stale field happens to matter.

**Timings stop being cumulative across a directory change.** A run moved to a
new directory reports only the work done since, which is what the new
directory's manifest should say, and means the two manifests no longer sum to
the session.

**Done tests that change.** X13's gains Start from a finished run and a
directory changed between runs. X10's live run is begun from the window rather
than from a config file, so the crop it names is the one typed into the
controls.

Nothing here is provisional: both failures were read out of the code rather
than measured, and neither depends on a DCS build. What would reopen it is a
sweep that caches `run.dir` at a time other than its own `start`, which would
make `stopped` and `done` insufficient as the states where retargeting is safe.
