# ADR 0014: A run waits in a stopped state, and Start always re-enters idle

## Status

Accepted

## Context

**Affects:** `extractor-hook.md` "Lifecycle", which names five states and begins
at idle; `plan.md` X4 and X13.

The frozen lifecycle opens at "**idle**: waiting for a terrain", written for a
hook with no interface, where loading the file was the only way to say "go". ADR 0011 moved configuration into a window with a Start
button, so a run now has to exist, and be visible, before anybody has asked for
it. None of the frozen five means "loaded, showing a window, doing nothing" —
idle is already polling DCS twice a second for terrain the user may not want
swept.

Stop raises a second question the lifecycle does not answer. `M.enter` clears
`run.queue` on every state change, so a run halted part-way through a pass has
no queue to put back, and pressing Start has to re-enter something. Whichever
state it re-enters, each job's `start(run)` runs again from the top, because a
queue is built from the phase's job list rather than restored.

What that costs is already settled: journalled tiles are skipped, and
`prepare_resume` takes the grid from an existing manifest, so re-entering a pass
redoes no work and re-entering prepare re-runs no pre-sweep. Across a DCS
restart there is no choice at all -- the process is gone, and the run comes back
through stopped, idle and prepare whatever Stop did.

## Decision

A run begins in a new `stopped` state ahead of `idle`, and returns there when
Stop is pressed. A stopped frame does nothing and is not counted: `run.frames`
measures the work a run cost, and a hook sitting at the main menu for an hour
did none.

**Start always re-enters `idle`**, whether it is the first press or one after a
Stop. Stop saves the manifest and drops the queue; it resets nothing else, so
`run.frames` and the accumulated timings carry across and the manifest records
the work done in the output directory rather than the last attempt at it. Start
resets `run.idle_frames`, because that counts one wait for terrain and a second
press is a second wait. A run stopped before prepare has no manifest to save,
and `save` returning false there is not a failure.

A pass is stamped `started_at` on the first entry only, because a pass can be
entered twice now and re-stamping would say it started after it finished.

`stopped` and `idle` are announced through the same phase-change path as every
other state, so the log carries one kind of line for a state change rather than
two, and a reader watching `dcs.log` sees a run begin and end.

The window attaches through two no-op functions rather than by replacing
anything: `on_phase(state)`, called where a phase change is already announced,
and `on_frame(run)`, called from the frame callback and outside `run_frame` —
outside, because `run_frame` returns immediately in `stopped` and in `done`,
which between them are most of a session and are exactly when a window still has
to be on screen and answering.

### Alternatives considered

**Start resumes the phase Stop left.** Rejected. It gets back to work a few
frames sooner, but buys that with a second resume path that only runs when the
user did not restart DCS, so the common path and the tested path differ. The
saving is small anyway, because each job's `start(run)` re-runs either way.

**Stop ends the run; Start builds a fresh one.** Rejected. The simplest machine,
but it loses the difference between pausing and abandoning.

**No stopped state: begin in idle and let the window gate Start by not
registering callbacks until it is pressed.** Rejected. Registration has to
happen at load, because `onSimulationFrame` drives the window as well as the run
— a window that appears only after Start is a window nobody can press.

## Consequences

The lifecycle has six states, so the frozen "Lifecycle" list is one short. Its
description of idle is otherwise unchanged.

**The manifest describes the directory, not the attempt.** A run stopped and
started three times reports the frames and timings all three cost. Stamping
`started_at` once is forced rather than chosen: a pass can now be entered twice
— Stop during the mission pass and the hook pass is re-entered with its work
already done — and re-stamping would leave a manifest saying a pass started
after it finished, which is the state a run killed in between would be found in.

**A user cannot tell a stopped run from a finished one by looking at the output
directory.** Both leave a manifest and a journal, and neither carries the
distinction; it has to be inferred from whether both passes are complete.
Stop during prepare is worse: `ensure_output_dirs` runs before the manifest
exists, so an early Stop leaves an empty tile tree that looks like a failed
extract.

**`idle` now appears in the log**, so anything counting phase lines sees one
more than before.

The sweeps are unaffected: a job is still built from the run it will sweep at
the moment the pass reaches it, which is what makes re-entry cheap.

**Provisional in two respects.** That re-entering a pass redoes no work rests on
the journal skip, which no sweep implements yet. A sweep ignoring it would turn
every Stop into lost work, and would reopen this record rather than merely being
a bug in that sweep.

And `on_phase` and `on_frame` are unguarded. Nothing overrides them yet, so
nothing can raise, but a window that did would take the run with it: `on_phase`
runs inside the phase change and before the save, so a raise there advances the
state and loses the manifest. That a failing window must never touch the run is
a separate decision, and the wrapper belongs with the code that first has
something to wrap.
