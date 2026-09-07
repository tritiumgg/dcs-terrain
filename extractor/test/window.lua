-- Offline tests for the window's chrome and its controls.
--
-- Run from the repository root with a plain lua5.1.
--
-- Most of these call build_window directly, because what is being asserted is
-- that a window gets built, gets built once, cannot be closed, and comes up
-- with a control for every field a config has. Filling those controls needs a
-- frame, so the group that asserts it drives on_frame instead.

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
T.eq("one window", #E.gui.all("Window"), 1)
T.eq("one panel", #E.gui.all("Panel"), 1)
T.eq("one bar", #E.gui.all("HorzProgressBar"), 1)

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
T.group("every config field gets a control and a line of its own")
--------------------------------------------------------------------------------

fresh()
E.build_window()

-- Through the names, not through positions: what matters is that the field the
-- config section names has a control, whatever order the rows came out in.
local controls = E.window.controls
T.eq("a box for the directory", controls.output_dir.class, "EditBox")
T.eq("a tick for the crop", controls.crop.class, "CheckBox")
T.eq("and a box for each half of the centre", controls.crop_x.class, "EditBox")
T.eq("the other half", controls.crop_z.class, "EditBox")
T.eq("and the radius", controls.crop_radius_m.class, "EditBox")
T.eq("four boxes and no more", #E.gui.all("EditBox"), 4)
T.eq("one tick", #E.gui.all("CheckBox"), 1)

-- One line per field, not per box: a crop reports the first thing wrong with
-- it, so three boxes share one line.
T.eq("a line for the directory", E.window.lines.output_dir.class, "Static")
T.eq("a line for the crop", E.window.lines.crop.class, "Static")
T.eq("both start empty", E.window.lines.output_dir.text, "")
T.eq("and stay that way until something is wrong", E.window.lines.crop.text, "")

-- The rows decide the height, so the last of them has to be inside the window.
-- Getting this wrong hides a control off the bottom of the frame, and without
-- this assertion only a screenshot would say so.
local frame = E.gui.find("Window").bounds
local last = E.window.lines.crop.bounds
T.eq("the last row is inside the window", last[2] + last[4] <= frame[4], true)
T.eq("and the panel covers the window", E.gui.find("Panel").bounds[4], frame[4])

--------------------------------------------------------------------------------
T.group("the first frame puts the config in the boxes, and then leaves them")
--------------------------------------------------------------------------------

fresh()
E.attach_window()
local run = E.new_run({ config = {
  enabled = true,
  output_dir = "C:/extract",
  crop = { x = -290000, z = 617000, radius_m = 5000 },
} })
E.on_frame(run)

controls = E.window.controls
T.eq("the directory is on screen", controls.output_dir.text, "C:/extract")
T.eq("the crop is ticked", controls.crop.state, true)
T.eq("with its centre", controls.crop_x.text, "-290000")
T.eq("both halves", controls.crop_z.text, "617000")
T.eq("and its radius", controls.crop_radius_m.text, "5000")

-- From here the boxes are the user's. A refill per frame would take a keystroke
-- back out of the box before the next one could be typed.
controls.output_dir.text = "C:/somewhere-else"
for _ = 1, 200 do E.on_frame(run) end
T.eq("what was typed survives the frames", controls.output_dir.text,
  "C:/somewhere-else")

-- A run whose config has no crop comes up unticked, with the boxes empty rather
-- than holding a centre nobody asked for.
fresh()
E.attach_window()
E.on_frame(E.new_run({ config = { enabled = true, output_dir = "C:/extract" } }))
T.eq("no crop, no tick", E.window.controls.crop.state, false)
T.eq("and no centre", E.window.controls.crop_x.text, "")

--------------------------------------------------------------------------------
T.group("what was wrong with the config file arrives under its own field")
--------------------------------------------------------------------------------

-- The problems are found before there is a window, so the first frame is the
-- only moment they can be shown. Until this, the sole record of a broken crop
-- was a log line nobody staring at an empty control would go and read.
local function shown_for(bad_config)
  fresh()
  E.attach_window()
  local settings, problems, tags = E.validate_config(bad_config)
  E.on_frame(E.new_run({ config = settings, problems = problems, tags = tags }))
  return E.window.lines
end

local bad = { enabled = true, output_dir = "C:/extract", crop = { x = 1 } }
local lines = shown_for(bad)
-- Compared against the checker rather than a pasted sentence: the assertion is
-- that the window shows the one wording there is, not what that wording says.
T.eq("the crop line carries the crop's problem", lines.crop.text,
  E.field_problem("crop", bad.crop))
T.eq("and the directory, which was fine, says nothing", lines.output_dir.text, "")

lines = shown_for({ enabled = true })
T.eq("a missing directory is its own line", lines.output_dir.text,
  E.field_problem("output_dir", nil))

-- A problem belonging to no control: there is no box for a field that does not
-- exist. It is worth a log line and nothing on screen, and reaching setText
-- through the nil it tags would take the window down over a typo in a file.
lines = shown_for({ enabled = true, output_dir = "C:/extract", nonsense = 1 })
T.eq("an unknown key marks no line", lines.output_dir.text, "")
T.eq("nor the other one", lines.crop.text, "")
T.eq("and nothing latched", E.ui_failed, false)

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
