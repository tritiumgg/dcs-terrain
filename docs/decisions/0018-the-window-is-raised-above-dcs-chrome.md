# ADR 0018: The window is raised above DCS's own chrome

## Status

Accepted

## Context

**Affects:** `plan.md` X13, X14. ADR 0015 is untouched: it turns out to have been
right, and the Consequences say why.

X13 shipped a window that appeared at the main menu and vanished the moment the
Mission Editor opened, never to return. Measured on 2.9.29.27468 with Caucasus
in the editor, through the bridge.

**A window created at the default z-order of zero is drawn underneath DCS's own
chrome.** The menu, the Mission Editor and the map view are all drawn over it.
Two windows built side by side at the main menu, identical but for one call —
one left at zero, one at `setZOrder(10001)` — behave differently the moment the
Mission Editor opens: the raised one is on screen and the other is not. In the
editor, after a click on the map, the pixel test reads `covered` for the window
at zero and `OURS` for the raised one.

**The object survives and says it is visible throughout.** An orphaned-looking
window answers `getBounds`, `getText` and `getSurfaceId` correctly, its children
answer, writes to it succeed, and `getVisible()` returns `true` — with
`checkParentVisibility` set as well as without. Nothing the hook can ask the
window distinguishes covered from drawn, which is why being underneath reads
exactly like having been destroyed.

**What can be asked is the screen.** `Gui.FindWidgetAtScreenPoint(x, y)` returns
the widget painted at a pixel and `Gui.WidgetGetRoot` walks to the window owning
it — the pair ED uses at
`install:MissionEditor/modules/me_contextMenu.lua:88-95`. That is how the
covering was identified: after a map click the pixel over our own label answered
with another window's handle.

**`setVisible(true)` does not lift a covered window; `setZOrder` does.** Tried in
order on a covered window: `setVisible(true)` left it covered, `setZOrder(10001)`
brought it back immediately.

**10001 is ED's own number.** `MissionEditor/modules/mul_voicechat.lua:233` calls
`windowSettUser:setZOrder(10001)` for a window that has to stay up.

**`getActive` is broken in this build.** `Window:getActive()` raises
`attempt to call field 'WindowGetActive' (a nil value)`, so it is not available
as a test or a remedy.

**The frame is not the client area.** `getViewBounds` on a window created at
400 x 200 returned `0, 20, 400, 180`: a 20-pixel header the rows were being laid
out over, which is why the buttons were cut off. The inset belongs to the skin
rather than to this file, and MarkPresets' `fitWindow` measures it the same way
rather than assuming it.

## Decision

The window is laid out against its client rectangle and raised above DCS's own
chrome when it is built.

`build_window` places rows from a cursor in client coordinates, sets the size to
the content, reads `getViewBounds`, and grows the frame by the difference so the
client area is exactly the content — measured and corrected rather than assumed,
once per build. A measurement of zero or less is refused rather than used, and
where the second reading fails the size that was asked for is used rather than
the first, which is short by the inset the frame has already grown by.

`setZOrder(M.WINDOW_Z_ORDER)`, which is 10001, is called once at build. That is
the whole of staying on screen: no polling, no rebuilding, and nothing to detect.

### Alternatives considered

**Detect that the window is not drawn and rebuild it on the current screen.**
Written, tested and thrown away the same day, and worth recording because the
reasoning was plausible. A window built at the menu is absent in the editor, and
one built in the editor is absent at the menu; that symmetry was read as "a
window belongs to the screen it was created on". Both were at z-order zero, so
both were merely underneath, and what looked like ownership was two different
things being drawn on top. Rebuilding did work, at the cost of a probe every
sixtieth frame, a leaked window tree per screen change, a rule about how many
consecutive misses count, a guard for a window dragged off the edge, and a
flicker every time the user clicked the map. The last of those was the clue: a
map click covers the window rather than destroying it, and nothing that destroys
a window could be undone by `setZOrder`.

**`setVisible(true)` on a covered window.** Rejected on measurement: it does
nothing. The window already believes it is visible.

**`OverlayWindow`.** Rejected on measurement: it constructs, reports itself
visible, and does not draw at all.

**A z-order lower than 10001.** There is no map of DCS's z-order space to pick
one from, and the only documented value in ED's own code is the one it uses for
the same purpose. A smaller number can be measured for later if the window turns
out to sit above something it should not.

## Consequences

The window stays on screen across the main menu, the Mission Editor and a click
on the map, which is what X13's done test asks for, and it costs one call at
build time.

**It is above everything, including things it should perhaps be below.** A
modal dialog the user opens in the editor may be covered by it. Nothing was
measured about that, and the window is draggable, so the cost is moving it
rather than losing anything. X14 is where a smaller number gets measured if this
turns out to be a nuisance.

**ADR 0015 stands unchanged.** Its rule that the window is built once and never
rebuilt is not narrowed after all — the fault it appeared to have was not a
fault. Its latch, its one-directional failure and its reasoning about `pcall`
were never in question.

**The offline tests model the layer rather than exercising it.** No widget can
be constructed outside DCS, so the fake records the z-order it is given and the
assertion is that the window is raised above the default. That a raised window
is actually drawn over DCS's chrome is a live measurement, recorded here.

**Provisional in one respect.** The behaviour is measured across the main menu
and the Mission Editor, including a map click. A running mission was not tested,
and neither was a modal dialog over the window.
