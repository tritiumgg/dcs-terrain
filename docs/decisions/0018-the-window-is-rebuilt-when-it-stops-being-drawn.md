# ADR 0018: The window is rebuilt when it stops being drawn

## Status

Accepted

## Context

**Affects:** ADR 0015's consequence that the window is "built once and hidden
rather than destroyed and rebuilt"; `plan.md` X13, and X12 in so far as the bar
it fills in is rebuilt with the window.

X13 shipped a window that appears at the main menu and vanishes the moment the
Mission Editor opens, never to return — which is where the task's own done test
begins. Everything below was measured on 2.9.29.27468 with Caucasus in the
editor, through the bridge, and confirmed against ED's own code where it says
anything.

**A window draws only on the screen that was current when it was created, and
that is symmetric.** Two windows built identically, one at the menu and one in
the editor: in the editor only the editor-made one was on screen, and at the
menu neither was — the menu returned to is a new screen, not the one the first
window was made on. Both reported `z=0`, `surfaceId=0`, `active=false`. So it is
not z-order, not the surface, and not the visibility flag.

**The Lua object survives intact and reports that it is visible.** An orphaned
window answers `getBounds`, `getText` and `getSurfaceId` correctly, its children
answer, writes to it succeed, and `getVisible()` returns `true` — with
`checkParentVisibility` set as well as without. Nothing the hook can ask the
window distinguishes drawn from not drawn.

**Nothing announces the change.** The model index for the hook state carries
only `DCS.onShowDialog`, `DCS.onShowStatusBar` and `DCS.onUserLogin`, no
callback for a screen change, and no symbol naming the current screen. ED's own
shipped hooks create no widgets at all.

**One call answers the question the window cannot.**
`Gui.FindWidgetAtScreenPoint(x, y)` returns the C++ widget pointer of whatever
is painted at a pixel, and `Gui.WidgetGetRoot` walks from there to the top-level
window. ED pairs exactly those two calls, at
`install:MissionEditor/modules/me_contextMenu.lua:88-95` and
`install:MissionEditor/modules/me_selectUnit.lua:47-54`, mapping the returned
pointer back through a table keyed by pointer. Measured here: a pixel over a
label returned that label's pointer.

**A rebuild loses nothing**, because an orphaned widget still returns its text.

**The frame is not the client area.** `getViewBounds` on a window created at
400 x 200 returned `0, 20, 400, 180`: a 20-pixel header the rows were being laid
out over, which is why the buttons were cut off. The inset is a property of the
skin rather than a constant, and MarkPresets' `fitWindow` — a mod doing this
successfully on this machine — measures it the same way rather than assuming it.

**ADR 0015's premise is wrong in one respect.** It preferred hiding to
destroying. DCS-SRS's overlay carries the comment `if you make the window
invisible, its destroyed` beside its own `setVisible(true)`, and works around it
with a hidden *skin* rather than a hidden window. Hiding is not the safe option
0015 took it for.

A prototype of the decision below ran 3 rebuilds over 5 395 frames, in both
directions, carrying typed text across every time; `kill` then succeeded on all
four windows including the three orphans.

## Decision

The window is laid out against its client rectangle and rebuilt whenever it is
measurably no longer drawn.

`build_window` places rows from a cursor in client coordinates, sets the size to
the content, reads `getViewBounds`, and grows the frame by the difference so
that the client area is exactly the content — measured and corrected rather than
assumed, once per build.

Every sixtieth frame, the run probes one pixel inside the window's own current
bounds, read rather than assumed because the window is draggable:
`FindWidgetAtScreenPoint` then `WidgetGetRoot`, both floored and both under the
latch. A root that is our window means it is drawn and the miss count resets. Two
**consecutive** misses mean it is not, and one miss alone is ignored, because
another window sitting over the probe point is indistinguishable from a screen
change on a single reading.

On the second miss the run reads every control's value off the orphaned widgets,
keeps the old window in a list, builds a new one on the current screen through
the same code, and restores into it what it read: the control values, the
window's position, the status line, the bar and the problem lines. The orphan is
never killed.

## Alternatives considered

**Rebuild on the callbacks that mark a mission changing.** `onMissionLoadEnd`,
`onSimulationStart` and `onSimulationStop` are already wired, and terrain
presence is already polled. Rejected: it detects the transitions DCS announces
and none of the others, and the failure the user hit — menu to Mission Editor —
is one it would catch only incidentally, through terrain. A rule that covers the
transitions we happen to be told about is not a rule about screens.

**Use `OverlayWindow`.** Rejected on measurement: it constructs and reports
itself visible, and does not draw at all, on its own screen or any other.

**Rebuild on a timer regardless.** Rejected: without the probe there is no way
to tell a window that needs rebuilding from one that does not, so every user
would pay a periodic flicker and a lost keystroke for a fault that happens a
handful of times a session.

**Trust `getVisible()`.** Rejected on measurement: it returns `true` for a
window that is not on screen, which is the whole difficulty.

**Hide the window and re-show it.** Rejected: hiding is reported to destroy the
window, and it would not help anyway — a re-shown window still belongs to the
screen it was created on.

**Kill the orphan.** Rejected, narrowly, and it is the closest call here.
Killing bounds the leak, and it succeeded four times out of four in the
prototype. But none of those was at mission end, which is the case ADR 0015's
warning actually came from — prior art on this machine where destroying widgets
took DCS down without raising anything catchable. A handful of leaked window
trees is a smaller cost than an access violation, and the orphan list leaves the
decision open to reverse once mission end has been measured.

## Consequences

The window survives a screen change, which is what X13's done test asks for and
what it could not do.

**A rebuild costs up to a second of absence and the keyboard focus.** The poll is
every sixtieth frame and acts on the second miss, so a screen change leaves the
window missing for up to two polls. Anyone typing when it happens finishes the
word somewhere else.

**A window covered by another for two consecutive polls is rebuilt needlessly.**
It reappears at its remembered position, in front, having lost focus. The
two-miss rule makes it rare rather than impossible, and there is no reading that
separates occlusion from a screen change.

**Orphaned window trees accumulate for the session** — one per screen change,
never reclaimed. Bounded by how often somebody changes screen and released when
DCS exits.

**ADR 0015 is narrowed, not superseded.** Its rule against speculative rebuilds
stands: nothing is rebuilt because it might be broken, only because a
measurement says it is not drawn. Its latch, its one-directional failure and its
reasoning about `pcall` are untouched, and the tests behind them still pass. What
no longer holds is its preference for hiding over rebuilding, and its statement
that the window is built once.

**Two native calls every sixtieth frame, forever.** Both are inside the latch, so
a library that starts raising switches the window off and stops probing with it.

**The offline tests model the probe rather than exercising it.** No widget can be
constructed outside DCS, so the fake grows a screen change and the assertions are
about what the hook does with the answer: that a rebuild preserves what was
typed, that one miss rebuilds nothing, and that neither reaches the run.

**Provisional in one respect.** The screen-ownership rule is measured across the
main menu and the Mission Editor only. A running mission was not tested, and
neither was mission end, which is exactly where ADR 0015's warning about
destroying widgets came from. Measuring either could reopen the choice to
abandon orphans rather than kill them.
