# ADR 0015: A failing window is switched off, and never reaches the run

## Status

Accepted

## Context

**Affects:** `extractor-hook.md` "Failure handling"; `plan.md` X12 and X13.

The frozen failure handling is about the sweeps: a DCS call that throws is
caught, the tile is written with nodata cells, and the manifest carries a note.
It was written when the hook had no interface, so it says nothing about a widget
call, and there is no note a user could act on for one.

ADR 0011 moved configuration into a window, so the hook now makes a second kind
of call: one that draws. It fails for different reasons — a library that moved
between DCS builds, a skin name that stopped resolving, a handle freed under us
— and costs differently: a terrain call that throws costs one sample, a widget
call that throws costs nothing, because nothing reads it.

Two facts decide the shape. The window is driven from `onSimulationFrame`, which
arrives about sixty times a second for as long as DCS is open. And `on_phase` is
announced from inside the phase change, before `M.save`, so a raise there
advances the state and loses the manifest write — which ADR 0014 recorded as
provisional precisely because this wrapper did not exist yet.

`pcall` catches a Lua error and not an access violation, and prior art on this
machine found that destroying widgets at mission end could take DCS down without
raising anything catchable.

## Decision

Every widget call goes through `M.ui` or `M.ui_method`, and the first one that
fails switches the window off for the session: `M.ui_failed` latches, one line
goes to the log saying the window is off and the run is not, and every later
call returns nil without attempting anything. Never retried — a library that has
started raising will raise again, and a `pcall` and a traceback per call per
frame buys nothing.

The two attachment points go through the latch — `phase_change` calls
`M.ui(M.on_phase, state)`, the frame callback calls `M.ui(M.on_frame, run)` —
and that is what makes the failure one-directional. Nothing the window does can
raise into the run, and nothing it does is written into the manifest: a note
there would make the extract differ depending on whether anybody was watching.

`M.gui` is the seam the library is reached through, in the shape `M.fs` already
has. It answers nil where the library is absent, and that is not a failure: no
window is built and nothing latches. Present and raising is the failure; not
there is a fact.

### Alternatives considered

**Retry, so a transient failure recovers.** Rejected. Nothing here is
transient: a missing library, a moved API and a dead handle do not heal, and the
cost is paid every frame forever.

**Let the failure disable the run too.** Rejected, and it is the one that would
have been easy to write. A forty-minute sweep that stopped because a label would
not redraw is the tool failing at its job in order to report on its job.

**Record the failure in the manifest.** Rejected. It would make the extract
differ depending on whether the display worked, which is the property the
offline test asserts against. The logs carry it; neither is part of the extract.

**Wrap each call and carry on rather than latching.** Rejected as the closest
call. A broken bar beside a working status line degrades better in theory, but
every call site would then have to be right about a nil result, and the failure
modes available make a half-working window unlikely.

## Consequences

A user whose window fails gets one line in `dcs.log`, no window, and the extract
they asked for. **The failure cannot be reported on screen**, because the screen
is what failed, so how far the run has got is only in the progress log — and a
user watching a window vanish has to be told where to look, which the README
owes once the window is reachable.

**X12 is bound by this.** The progress bar is driven from the frame callback, so
it inherits the latch: a bar that will not update must not slow the sweep it
reports on, and must not retry.

Nothing here protects against a call that does not return: the only defence
against an access violation is not making the call, which is why the window is
built once and hidden rather than destroyed and rebuilt.

**Provisional in one respect.** The failure modes listed are reasoned about
rather than observed: nothing has yet seen a widget call fail on a real DCS. A
mode that turned out transient and common would reopen the latch.
