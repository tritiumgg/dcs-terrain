-- Offline tests for the window's chrome.
--
-- Run from the repository root with a plain lua5.1.
--
-- Nothing drives the window yet: these call build_window directly, because what
-- is being asserted is that a window gets built, gets built once, and cannot be
-- closed. What ticks it is the next branch's problem.

package.path = "extractor/?.lua;extractor/test/support/?.lua;" .. package.path

local T = require("testing")
local E = require("DcsTerrainExtract")
local FakeGui = require("fakegui")

local logged = {}
E.log = function(message) logged[#logged + 1] = message end

local warned = {}
local real_warn = E.warn
E.warn = function(message) warned[#warned + 1] = message end

-- The latch and the window are module state and survive between groups, so a
-- group that did not clear them would be testing the previous group's failure.
local function fresh(mode)
  E.gui = FakeGui.new()
  if mode then E.gui[mode]() end
  E.ui_failed, E.ui_failure = false, nil
  E.window = { built = false }
  for i = #warned, 1, -1 do warned[i] = nil end
end

--------------------------------------------------------------------------------
T.group("the window is built once, in an order that can be drawn")
--------------------------------------------------------------------------------

fresh()
T.eq("it builds", E.build_window(), true)
T.eq("and says so", E.window.built, true)
T.eq("a window, a panel and a label", #E.gui.made, 3)

-- Hidden, skinned, then shown. A widget with correct bounds and a true
-- visibility flag still draws before its parent has recomputed, so a window
-- made visible first flickers into the editor half-placed.
local calls = table.concat(E.gui.calls, " ")
local at_new = calls:find("Window.new", 1, true)
local at_hide = calls:find("Window:setVisible", 1, true)
local at_skin = calls:find("Window:setSkin", 1, true)
T.eq("made first", at_new < at_hide, true)
T.eq("hidden before it is skinned", at_hide < at_skin, true)

local root = E.gui.find("Window")
T.eq("and visible at the end", root.visible, true)
T.eq("it carries the title", root.text, E.WINDOW_TITLE)
T.eq("it is skinned", root.skin.skinData.params.name, "windowSkin")
T.eq("the panel is in the window", root.children[1], E.gui.find("Panel"))
T.eq("and the label in the panel",
  E.gui.find("Panel").children[1], E.gui.find("Static"))
T.eq("it is draggable", calls:find("Window:setDraggable", 1, true) ~= nil, true)
T.eq("and not resizable",
  calls:find("Window:setResizable", 1, true) ~= nil, true)

-- Built once. The caller asks on every frame, which is sixty times a second for
-- the length of a session, and a second window a frame would be a new one every
-- frame.
local made_after_first = #E.gui.made
local calls_after_first = #E.gui.calls
for _ = 1, 50 do E.build_window() end
T.eq("and never built twice", #E.gui.made, made_after_first)
T.eq("nor asked the library anything again", #E.gui.calls, calls_after_first)

--------------------------------------------------------------------------------
T.group("it refuses to close")
--------------------------------------------------------------------------------

-- The native side hides the window and then fires the callback, which is what
-- makes re-asserting visibility win. Measured against the real title bar's X
-- before it was written this way, and the fake reproduces that ordering.
fresh()
E.build_window()
root = E.gui.find("Window")
T.eq("up", root.visible, true)
root:close()
T.eq("still up after a close", root.visible, true)
root:close()
T.eq("and after another", root.visible, true)

-- But only while it is alive. Once the latch is set nothing is updating the
-- window and it carries whatever line it last showed, so refusing would trap
-- dead chrome on somebody's screen. Going through the seam is what gives that
-- for nothing: a latched ui_method does nothing and the native hide stands.
E.ui_failed = true
root:close()
T.eq("a dead window can be dismissed", root.visible, false)
E.ui_failed = false

--------------------------------------------------------------------------------
T.group("a missing widget library is not a failure")
--------------------------------------------------------------------------------

-- Which is this interpreter, and any DCS whose widget library moved. Nothing
-- raises, nothing latches, and no window is built.
fresh("no_library")
T.eq("it does not build", E.build_window(), false)
T.eq("no window", E.window.built, false)
T.eq("nothing was made", #E.gui.made, 0)
T.eq("and nothing latched", E.ui_failed, false)
T.eq("and nothing was said", #warned, 0)

-- Asked once and then left alone. The caller asks on every frame until it
-- succeeds, and a failed lookup is about a tenth of a millisecond, so five
-- hundred frames of asking is most of a second spent on an answer that cannot
-- change.
local asked = #E.gui.calls
for _ = 1, 500 do E.build_window() end
T.eq("it stopped asking", #E.gui.calls, asked)
T.eq("having recorded why", E.window.unavailable, true)

--------------------------------------------------------------------------------
T.group("a library that is there and broken latches")
--------------------------------------------------------------------------------

-- The other half of the pair. A library that raises is a failure and is
-- reported; one that is absent is a fact and is not.
fresh("fail_every")
T.eq("it does not build", E.build_window(), false)
T.eq("no window", E.window.built, false)
T.eq("it latched", E.ui_failed, true)
T.eq("and said so once", #warned, 1)

-- And it stays not built, however many times it is asked.
for _ = 1, 100 do E.build_window() end
T.eq("still no window", E.window.built, false)
T.eq("and still one line", #warned, 1)

E.warn = real_warn

T.done()
