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
local FakeFs = require("fakefs")

local logged = {}
E.log = function(message) logged[#logged + 1] = message end

local warned = {}
local real_warn = E.warn
E.warn = function(message) warned[#warned + 1] = message end

-- A disk with the one drive these configs name, because the drive has to be
-- there before Start will take a path on it. Groups that read what Start
-- wrote make a disk of their own.
local disk = FakeFs.new()
disk.mkdir("C:/")
E.fs = disk

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
T.eq("it is skinned with the editor's own", root.skin.skinData.params.name,
  "windowSkinME")
T.eq("the panel is in the window", root.children[1], E.gui.find("Panel"))
T.eq("and the label in the panel",
  E.gui.find("Panel").children[1], E.gui.find("Static"))
T.eq("it is draggable", calls:find("Window:setDraggable", 1, true) ~= nil, true)
T.eq("and not resizable",
  calls:find("Window:setResizable", 1, true) ~= nil, true)

-- Raised above DCS's own chrome, which is the whole of staying on screen. At
-- the default zero the window is drawn underneath the menu, the Mission Editor
-- and the map view -- invisible, while answering every question put to it, so
-- that it reads exactly like a window that has been destroyed.
T.eq("and raised above the chrome", root.zorder, E.WINDOW_Z_ORDER)
T.eq("which is not the default it would have had", root.zorder ~= 0, true)

-- 360 wide, and centered on the screen the fake reports, once the frame has
-- its size: every corner is somebody's in the Mission Editor.
T.eq("360 wide", root.bounds[3], 360)
T.eq("centered across", root.bounds[1], math.floor((2560 - 360) / 2))
T.eq("and down", root.bounds[2], math.floor((1440 - root.bounds[4]) / 2))

-- Built once. The caller asks on every frame, which is sixty times a second for
-- the length of a session, and a second window a frame would be a new one every
-- frame.
local made_after_first = #E.gui.made
local calls_after_first = #E.gui.calls
for _ = 1, 50 do E.build_window() end
T.eq("and never built twice", #E.gui.made, made_after_first)
T.eq("nor asked the library anything again", #E.gui.calls, calls_after_first)

--------------------------------------------------------------------------------
T.group("a build that breaks anywhere reports that it broke")
--------------------------------------------------------------------------------

-- Whichever call the library gives up on, the answer has to be the same: no
-- window, and build_window saying so. The failure this is written for is a
-- break in the tail -- the callbacks, the mouse hook, or the setVisible that
-- reveals the window -- because the layout is already done by then and it is
-- tempting to call the job finished. A window marked built after a failed show
-- stays hidden by the setVisible(false) it was laid out under, for the whole
-- session, with this function short-circuiting on every later frame.
--
-- Every budget rather than one: pinning the call number would pin the number of
-- calls a build happens to make today.
local inconsistent_at = nil
for budget = 0, 200 do
  fresh()
  E.gui.fail_after(budget)
  local ok = E.build_window()
  local sound
  if ok then
    local w = E.gui.find("Window")
    sound = not E.ui_failed and E.window.built == true
      and w ~= nil and w.visible == true
  else
    sound = E.window.built ~= true
  end
  if not sound then
    inconsistent_at = budget
    break
  end
end
T.eq("built is true only when the window is up", inconsistent_at, nil)

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
T.eq("and a box for each half of the center", controls.crop_x.class, "EditBox")
T.eq("the other half", controls.crop_z.class, "EditBox")
T.eq("and the radius", controls.crop_radius_m.class, "EditBox")
T.eq("four boxes and no more", #E.gui.all("EditBox"), 4)
T.eq("one tick", #E.gui.all("CheckBox"), 1)

-- No line per field: what is wrong with any of them goes on the one line over
-- the bar, which starts empty until a frame has filled the boxes.
T.eq("one line for what is wrong", E.window.message.class, "Static")
T.eq("empty until there is something to say", E.window.message.text, "")

-- Rows are placed in client coordinates, and a window's own bounds are the
-- frame. DCS grants a client area shorter than the frame by the header, so
-- measuring the rows against the frame is what put the buttons off the bottom
-- edge on screen while every offline assertion passed.
local win = E.gui.find("Window")
local frame = win.bounds
local panel = E.gui.find("Panel").bounds
local _, _, client_w, client_h = win:getViewBounds()
T.eq("the panel fills the client area",
  panel[3] .. "x" .. panel[4], client_w .. "x" .. client_h)
T.eq("the frame is taller than the client area it grants", frame[4] > client_h, true)

local last = E.window.buttons.stop.bounds
T.eq("the last row is inside the client area", last[2] + last[4] <= client_h, true)
T.eq("and so is the pick button above it",
  E.window.controls.crop_pick.bounds[2] < last[2], true)

-- The buttons run left to right off one list, so a button added to it is the
-- way this overflows, and it would do so silently.
local rightmost = E.window.buttons.stop.bounds
T.eq("the last button is inside the window",
  rightmost[1] + rightmost[3] <= frame[3], true)

-- The pick button belongs with the crop it fills in, not with the buttons that
-- act on the run: it sits under the boxes it fills, at their left edge.
T.eq("picking is a crop control", E.window.controls.crop_pick.class, "Button")
T.eq("below the crop boxes",
  E.window.controls.crop_pick.bounds[2] > E.window.controls.crop_x.bounds[2], true)
T.eq("starting where their row starts",
  E.window.controls.crop_pick.bounds[1], E.window.crop_labels[1].bounds[1])

-- No close button. The window refuses to close, so it does not offer to; the
-- refusal in onClose stays underneath for a skin that draws one anyway.
T.eq("the close button is off",
  win.skin.skinData.skins.header.skinData.params.hasCloseButton, false)
T.eq("well above the run's buttons",
  E.window.controls.crop_pick.bounds[2] < E.window.buttons.start.bounds[2], true)

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
T.eq("with its center", controls.crop_x.text, "-290000")
T.eq("both halves", controls.crop_z.text, "617000")
T.eq("and its radius", controls.crop_radius_m.text, "5000")

-- From here the boxes are the user's. A refill per frame would take a keystroke
-- back out of the box before the next one could be typed.
controls.output_dir.text = "C:/somewhere-else"
for _ = 1, 200 do E.on_frame(run) end
T.eq("what was typed survives the frames", controls.output_dir.text,
  "C:/somewhere-else")

-- A run whose config has no crop comes up unticked, with the boxes empty rather
-- than holding a center nobody asked for.
fresh()
E.attach_window()
E.on_frame(E.new_run({ config = { enabled = true, output_dir = "C:/extract" } }))
T.eq("no crop, no tick", E.window.controls.crop.state, false)
T.eq("and no center", E.window.controls.crop_x.text, "")

-- The line opens with the one thing to do next. At the main menu that is a
-- map, whatever the config holds; with one open it is the directory where
-- there is none and the button where there is. An instruction is kept current
-- while the run is stopped and nothing else has been said, so a map opening
-- moves it on; a press's words are not moved on.
T.eq("at the menu the line says to open a map",
  E.window.message.text, E.INSTRUCTION_MAP)
local real_terrain = E.terrain_id
E.terrain_id = function() return "Caucasus" end
local frame_run = E.window.run
E.on_frame(frame_run)
T.eq("with a map, a config with a directory says where the button is",
  E.window.message.text, E.INSTRUCTION_START)
fresh()
E.attach_window()
E.on_frame(E.new_run({ config = { enabled = true } }))
T.eq("and one without says to set it",
  E.window.message.text, E.INSTRUCTION_DIRECTORY)
E.terrain_id = real_terrain

-- A directory the run will make is fine; a drive it cannot is a problem from
-- the first frame, whether or not there is a map yet.
fresh()
E.attach_window()
E.on_frame(E.new_run({ config = { enabled = true, output_dir = "C:/gone" } }))
T.eq("a directory that is not there yet is not a problem",
  E.window.message.text, E.INSTRUCTION_MAP)
fresh()
E.attach_window()
E.on_frame(E.new_run({ config = { enabled = true, output_dir = "Q:/gone" } }))
T.eq("a drive that is not there is said at load",
  E.window.message.text, "Output directory's drive Q:/ does not exist.")

--------------------------------------------------------------------------------
T.group("the crop's boxes are on screen only while the tick is")
--------------------------------------------------------------------------------

-- Unticked, there is nothing to type into: the three boxes, their labels and
-- the pick button are hidden, and the tick stays, because it is how they come
-- back. Ticked from the config, they are there from the first frame.
local function crop_shown()
  local shown = {}
  for i = 1, 3 do
    shown[#shown + 1] = tostring(E.window.crop_labels[i].visible)
  end
  for _, name in ipairs({ "crop_x", "crop_z", "crop_radius_m", "crop_pick" }) do
    shown[#shown + 1] = tostring(E.window.controls[name].visible)
  end
  return table.concat(shown, " ")
end
local hidden = "false false false false false false false"
local visible = "true true true true true true true"

T.eq("unticked from the config hides them", crop_shown(), hidden)
T.eq("and leaves the tick", E.window.controls.crop.visible ~= false, true)

-- Hidden, the block leaves no blank space: the rows under it move up by its
-- height and the window shrinks by the same, where it stands.
local frame_h = E.window.frame.h
local block_h = E.window.crop_block_h
local bar_h = E.window.bar_block_h
local start_y = E.window.buttons.start.bounds[2]
local win = E.gui.find("Window")
T.eq("the block has a height", block_h > 0, true)
T.eq("the window is shorter by it", win.bounds[4], frame_h - block_h - bar_h)
T.eq("and the button moved up by it",
  E.window.buttons.start.bounds[2], start_y)

-- A tick from the user. The library toggles the state and then reports the
-- change, so the handler reads the state back rather than tracking it.
E.window.controls.crop.state = true
E.gui.press(E.window.controls.crop)
T.eq("ticking shows them", crop_shown(), visible)
T.eq("and the window is its full height again", win.bounds[4], frame_h - bar_h)
T.eq("with the button back under the block",
  E.window.buttons.start.bounds[2], start_y + block_h)
E.window.controls.crop.state = false
E.gui.press(E.window.controls.crop)
T.eq("and unticking hides them again", crop_shown(), hidden)
T.eq("without moving anything twice", win.bounds[4], frame_h - block_h - bar_h)

fresh()
E.attach_window()
E.on_frame(E.new_run({ config = { enabled = true, output_dir = "C:/extract",
  crop = { x = 1, z = 2, radius_m = 3 } } }))
T.eq("a crop in the config shows them from the first frame", crop_shown(),
  visible)

--------------------------------------------------------------------------------
T.group("what was wrong with the config file arrives on the line over the bar")
--------------------------------------------------------------------------------

-- The problems are found before there is a window, so the first frame is the
-- only moment they can be shown. Until this, the sole record of a broken crop
-- was a log line nobody staring at an empty control would go and read.
local function shown_for(bad_config)
  fresh()
  E.attach_window()
  local settings, problems, tags = E.validate_config(bad_config)
  E.on_frame(E.new_run({ config = settings, problems = problems, tags = tags }))
  return E.window.message.text
end

local bad = { enabled = true, output_dir = "C:/extract", crop = { x = 1 } }
-- Compared against the checker rather than a pasted sentence: the assertion is
-- that the window shows the one wording there is, not what that wording says.
T.eq("the line carries the crop's problem", shown_for(bad),
  E.problem_for_screen(E.field_problem("crop", bad.crop)))
-- A missing directory is what a fresh install has, and the instruction already
-- says to set one; the checker's line would say the same thing as an error.
-- So at load that one problem is left to the instruction, and the next shows.
T.eq("a missing directory is left to the instruction",
  shown_for({ enabled = true }), E.INSTRUCTION_MAP)
T.eq("and the problem after it is shown",
  shown_for({ enabled = true, crop = { x = 1 } }),
  E.problem_for_screen(E.field_problem("crop", { x = 1 })))

-- The screen's wording: the field's label for its key, and the explanation
-- after the comma dropped, because the line beside the button has room for
-- the finding and not for the reason. The log keeps both.
T.eq("the key becomes the label and the explanation goes",
  E.problem_for_screen("output_dir is not set, and there is no default for it"),
  "Output directory is not set.")
T.eq("for the escape explanation too",
  E.problem_for_screen("output_dir contains a control character, which is "
    .. "usually a backslash escape in a double-quoted path: \"C:\\x\""),
  "Output directory contains a control character.")
T.eq("a crop key becomes its box's label",
  E.problem_for_screen("crop.radius_m is not a positive number: -5"),
  "Radius is not a positive number.")
T.eq("and a message with no key is left alone",
  E.problem_for_screen("nonsense is not a config field"),
  "nonsense is not a config field.")

-- A problem belonging to no control is still a problem, and the line does not
-- need a box to point at.
T.eq("an unknown key is shown too",
  shown_for({ enabled = true, output_dir = "C:/extract", nonsense = 1 }),
  "nonsense is not a config field.")
T.eq("and nothing latched", E.ui_failed, false)

--------------------------------------------------------------------------------
T.group("Stop halts the run, and says so on a line of its own")
--------------------------------------------------------------------------------

-- The run reaches a press through the frame, so a window has to have been
-- ticked before its buttons mean anything.
local function window_on(run)
  fresh()
  E.attach_window()
  E.on_frame(run)
  return E.window.buttons
end

E.now_iso = function() return "2026-09-07T00:00:00Z" end

run = E.new_run({ config = { enabled = true, output_dir = "C:/extract" } })
run.state = E.STATE_HOOK
local buttons = window_on(run)
E.gui.press(buttons.stop)
T.eq("the run halts", run.state, E.STATE_STOPPED)
T.eq("and the line says what to do next",
  E.window.message.text:find("Start carries on", 1, true) ~= nil, true)

-- Pressing it again is not a failure and is not silence: a button that does
-- nothing and says nothing reads as a broken window.
E.gui.press(buttons.stop)
T.eq("nothing to stop twice", E.window.message.text, "Nothing to stop.")
T.eq("and the run is where it was", run.state, E.STATE_STOPPED)

-- A press arrives on DCS's own stack, from inside the widget library, so a
-- raise in a handler lands where nothing here catches it. Under the latch it
-- costs the window and nothing else.
run.state = E.STATE_HOOK
buttons = window_on(run)
local real_stop = E.stop
E.stop = function() error("the handler is broken", 0) end
E.gui.press(buttons.stop)
E.stop = real_stop
T.eq("a raising handler latches the window", E.ui_failed, true)
T.eq("and leaves the run alone", run.state, E.STATE_HOOK)

-- And once it has latched, a press does nothing at all rather than trying
-- again on every click for as long as DCS is open.
local reached = false
E.stop = function() reached = true return true end
E.gui.press(buttons.stop)
E.stop = real_stop
T.eq("a press after the latch is not attempted", reached, false)

--------------------------------------------------------------------------------
T.group("Start writes what is in the boxes and begins the run")
--------------------------------------------------------------------------------

local CONFIG = "C:/saved/Config/DcsTerrainExtract.lua"
E.config_path = function() return CONFIG end

-- A window with a run in it, its own filesystem, and whatever was typed.
local function typed(values)
  local fs = FakeFs.new()
  fs.mkdir("C:/")
  E.fs = fs
  local r = E.new_run({ config = { enabled = true } })
  local b = window_on(r)
  for name, value in pairs(values) do
    if name == "crop" then
      E.window.controls.crop.state = value
    else
      E.window.controls[name].text = value
    end
  end
  return r, b, fs
end

local started, press, fs = typed({
  output_dir = "C:/extract",
  crop = true,
  crop_x = "-290000",
  crop_z = "617000",
  crop_radius_m = "5000",
})
T.eq("the run has nowhere to write yet", started.dir, nil)
E.gui.press(press.start)
T.eq("the run leaves the stopped state", started.state, E.STATE_IDLE)
T.eq("and the settings were written", fs.files[CONFIG] ~= nil, true)
-- The run was built at load from a config with no directory in it, because
-- there was no window to type one into. Without this it would run with nowhere
-- to write, which is the whole first-use case.
T.eq("and it is pointed where the box says", started.dir, "C:/extract")

-- Read back rather than compared against the bytes: what matters is that the
-- file says next session what the boxes said this one.
local saved = E.read_config(CONFIG)
T.eq("the directory survives the file", saved.output_dir, "C:/extract")
T.eq("and the crop center", saved.crop.x, -290000)
T.eq("both halves of it", saved.crop.z, 617000)
T.eq("and the radius", saved.crop.radius_m, 5000)
T.eq("with the hook still switched on", saved.enabled, true)

-- The boxes are refilled from the validated table, so a pasted Windows path
-- comes back as the one that was actually saved.
started, press, fs = typed({ output_dir = "C:\\extracts\\caucasus", crop = false })
E.gui.press(press.start)
T.eq("a pasted path is normalised on screen", E.window.controls.output_dir.text,
  "C:/extracts/caucasus")
T.eq("and in the file", E.read_config(CONFIG).output_dir, "C:/extracts/caucasus")

--------------------------------------------------------------------------------
T.group("Start refuses before it writes anything")
--------------------------------------------------------------------------------

-- A bad box stops it where the user can see why, and nothing is written: a
-- config file holding a value already red on screen would come back next
-- session as a problem nobody caused.
local refused
refused, press, fs = typed({
  output_dir = "C:/extract",
  crop = true,
  crop_x = "12abc",
  crop_z = "617000",
  crop_radius_m = "5000",
})
E.gui.press(press.start)
T.eq("the run stays put", refused.state, E.STATE_STOPPED)
T.eq("nothing was written", fs.files[CONFIG], nil)
T.eq("the line names what is in the box",
  E.window.message.text, "X is not a finite number.")

-- The run makes the directory, so one that is not there yet starts; a drive
-- that is not there cannot be made, and stops it. A relative path and a
-- character Windows refuses are caught by the checker before the disk is
-- asked.
refused, press, fs = typed({ output_dir = "C:/nowhere", crop = false })
E.gui.press(press.start)
T.eq("a directory that is not there yet starts", refused.state, E.STATE_IDLE)
refused, press, fs = typed({ output_dir = "Q:/extract", crop = false })
E.gui.press(press.start)
T.eq("a drive that is not there stops it", refused.state, E.STATE_STOPPED)
T.eq("with nothing written", fs.files[CONFIG], nil)
T.eq("and the line says so", E.window.message.text,
  "Output directory's drive Q:/ does not exist.")
refused, press, fs = typed({ output_dir = "extract", crop = false })
E.gui.press(press.start)
T.eq("a relative path stops it", refused.state, E.STATE_STOPPED)
T.eq("and says so", E.window.message.text,
  "Output directory is not an absolute path.")
refused, press, fs = typed({ output_dir = "C:/ex?tract", crop = false })
E.gui.press(press.start)
T.eq("a forbidden character stops it", refused.state, E.STATE_STOPPED)
T.eq("and says so", E.window.message.text,
  "Output directory has a character Windows forbids.")

-- A finished run is refused the same way, and the refusal stays: the phase
-- has not changed, so "Finished." is not said again over it a frame later.
refused, press, fs = typed({ output_dir = "C:/extract", crop = false })
refused.state = E.STATE_DONE
E.on_frame(refused)
T.eq("a finished run says so", E.window.message.text, "Finished.")
E.window.controls.output_dir.text = "extract"
E.gui.press(press.start)
T.eq("Start after done is refused the same way", refused.state, E.STATE_DONE)
for _ = 1, 50 do E.on_frame(refused) end
T.eq("and the refusal stays up", E.window.message.text,
  "Output directory is not an absolute path.")

-- The one field with no default stops it the same way.
refused, press, fs = typed({ output_dir = "", crop = false })
E.gui.press(press.start)
T.eq("a blank directory stops it", refused.state, E.STATE_STOPPED)
T.eq("with nothing written", fs.files[CONFIG], nil)
T.eq("and the line says so", E.window.message.text,
  E.problem_for_screen(E.field_problem("output_dir", nil)))

-- Fixing it and pressing again replaces the complaint with the phase the run
-- moves into, on the next frame, rather than leaving it over a field that is
-- now fine.
E.window.controls.output_dir.text = "C:/extract"
E.gui.press(press.start)
E.on_frame(refused)
T.eq("the complaint goes",
  E.window.message.text:find("not set", 1, true), nil)
T.eq("and it runs", refused.state, E.STATE_IDLE)

-- Pressing Start during a run must not write either. The state is checked
-- first, so the next DCS start cannot come up pointed at a directory this run
-- never used.
local going
going, press, fs = typed({ output_dir = "C:/extract", crop = false })
going.state = E.STATE_HOOK
E.gui.press(press.start)
T.eq("the run is untouched", going.state, E.STATE_HOOK)
T.eq("and nothing was written", fs.files[CONFIG], nil)
T.eq("but it says why", E.window.message.text, "A run is already going. Stop it first.")

-- A config that cannot be saved does not cost the run. The extract is the
-- point; the file is only how the settings come back next time.
E.config_path = function() return nil end
local anyway
anyway, press, fs = typed({ output_dir = "C:/extract", crop = false })
E.gui.press(press.start)
T.eq("it starts regardless", anyway.state, E.STATE_IDLE)
T.eq("saying what was lost",
  E.window.message.text:find("not saved", 1, true) ~= nil, true)
T.eq("and warning once", #warned > 0, true)
E.config_path = function() return CONFIG end

--------------------------------------------------------------------------------
T.group("a window that breaks while Start reads it starts nothing")
--------------------------------------------------------------------------------

-- The failure the latch does not catch on its own. Every widget call answers
-- nil once it has raised, and nil is a legal answer here rather than an error:
-- a failed getState on the tick reads as an unticked crop, and a failed getText
-- reads as a blank box. So a library that starts raising during the read hands
-- back settings that look complete and validate clean -- and the run would be
-- started on them, the config file overwritten with the crop dropped, and a
-- whole theatre swept in place of the 10 km somebody asked for, with the window
-- dark and unable to say a word about it.
local broken
broken, press, fs = typed({
  output_dir = "C:/extract",
  crop = true,
  crop_x = "-290000",
  crop_z = "617000",
  crop_radius_m = "5000",
})
-- One more call, then everything raises. read_controls takes the directory
-- first and the tick second, so the tick is what breaks.
E.gui.fail_after(1)
E.gui.press(press.start)
T.eq("the window latched", E.ui_failed, true)
T.eq("no run was started", broken.state, E.STATE_STOPPED)
T.eq("and nothing was written", fs.files[CONFIG], nil)
-- The message line cannot carry this -- a latched window writes no text -- so
-- the log has to, and a user who pressed Start is owed more than a line saying
-- the window switched itself off.
T.eq("the log says the press was abandoned",
  warned[#warned]:find("no run was started", 1, true) ~= nil, true)

--------------------------------------------------------------------------------
T.group("the crop center is picked by arming, then clicking the map")
--------------------------------------------------------------------------------

-- The map is in another Lua state and cannot be reached from here, so the
-- conversion is stubbed and what is asserted is the arming: that a click does
-- nothing until the button says so, that a click somewhere other than the map
-- neither takes a coordinate nor disarms, and that one on the map does both.
local real_point = E.map_point_at

local picked
picked, press = typed({ output_dir = "C:/extract", crop = false })
E.map_point_at = function(x, y)
  -- Stands in for the map's own rectangle: inside it a click converts, outside
  -- it there is no map under the point and nothing comes back.
  if x >= 100 and y >= 100 then return -545142.85714286, 682000 end
  return nil
end

-- Unarmed, a click on the map is somebody using DCS.
E.on_map_click(500, 500)
T.eq("an unarmed click takes nothing", E.window.controls.crop_x.text, "")
T.eq("and leaves the crop alone", E.window.controls.crop.state, false)

E.gui.press(E.window.controls.crop_pick)
T.eq("arming says so on the button", E.window.controls.crop_pick.text,
  E.PICK_ARMED)

-- Armed, but the click was on the toolbar, or on this window. It stays armed:
-- disarming here would make a stray click cancel a thing the user just asked
-- for, with the only clue a button quietly changing back.
E.on_map_click(10, 10)
T.eq("a click off the map takes nothing", E.window.controls.crop_x.text, "")
T.eq("and stays armed", E.window.controls.crop_pick.text, E.PICK_ARMED)

-- To whole meters, the way the editor's status bar shows the cursor: the
-- fraction is a pixel's worth of noise, and a box holding it is harder to
-- read and to retype.
E.on_map_click(500, 500)
T.eq("a click on the map fills the center, in whole meters",
  E.window.controls.crop_x.text, "-545143")
T.eq("both halves of it", E.window.controls.crop_z.text, "682000")
-- Ticked, because picking a center is the deliberate act the tick records.
-- Left unticked, this followed by Start would sweep the whole theatre having
-- just been told where the user wanted to extract.
T.eq("and switches the crop on", E.window.controls.crop.state, true)
T.eq("and disarms", E.window.controls.crop_pick.text, E.PICK_IDLE)

-- A second click now that it is disarmed must not move the center again.
E.window.controls.crop_x.text = "left alone"
E.on_map_click(500, 500)
T.eq("and a later click is ignored", E.window.controls.crop_x.text, "left alone")

-- Pressing it twice is the way out, so an armed window is never a trap.
E.gui.press(E.window.controls.crop_pick)
T.eq("armed again", E.window.controls.crop_pick.text, E.PICK_ARMED)
E.gui.press(E.window.controls.crop_pick)
T.eq("and cancelled", E.window.controls.crop_pick.text, E.PICK_IDLE)
E.on_map_click(500, 500)
T.eq("a click after cancelling takes nothing",
  E.window.controls.crop_x.text, "left alone")

-- A radius away from a run, which is what the line over the bar then says.
E.window.controls.crop_x.text = "-545142.85714286"
E.gui.press(press.start)
T.eq("Start refuses without one", picked.state, E.STATE_STOPPED)
T.eq("naming the radius",
  E.window.message.text:find("Radius", 1, true) ~= nil, true)
E.window.controls.crop_radius_m.text = "5000"
E.gui.press(press.start)
T.eq("and takes it with one", picked.state, E.STATE_IDLE)

-- The seam itself, with no DCS in the process: no net global, so nil, and a
-- click that cannot be converted changes nothing.
E.map_point_at = real_point
T.eq("no net answers nothing", E.map_point_at(500, 500), nil)

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

--------------------------------------------------------------------------------
T.group("a crop is held against the map, where there is one")
--------------------------------------------------------------------------------

-- The theatre's bounds rectangle in meters, as the seam answers it with a map
-- open. Caucasus's, because those are the numbers anybody checking by hand
-- will reach for.
local real_bounds = E.terrain_bounds
E.terrain_bounds = function()
  return { min_x = -600000, min_z = -560000, max_x = 380000, max_z = 1130000 }
end

local held, hold = typed({ output_dir = "C:/extract", crop = true,
  crop_x = "-595000", crop_z = "0", crop_radius_m = "10000" })
E.gui.press(hold.start)
T.eq("a box past the edge is refused", held.state, E.STATE_STOPPED)
T.eq("naming the edge and the number that crossed it", E.window.message.text,
  "Crop reaches past the map, x below -600000.")

E.window.controls.crop_x.text = "-500000"
E.gui.press(hold.start)
T.eq("moved inside, it starts", held.state, E.STATE_IDLE)

-- The pick ignores a click off the theatre and stays armed for one on it.
E.stop(held)
local real_point_here = E.map_point_at
E.map_point_at = function(x, y)
  if x < 50 then return -700000, 0 end
  return -300000, 600000
end
E.gui.press(E.window.controls.crop_pick)
E.on_map_click(10, 500)
T.eq("a point off the map is not taken",
  E.window.controls.crop_x.text, "-500000")
T.eq("and the pick stays armed",
  E.window.controls.crop_pick.text, E.PICK_ARMED)
E.on_map_click(500, 500)
T.eq("one on it is", E.window.controls.crop_x.text, "-300000")
E.map_point_at = real_point_here

-- Without a map there is nothing to hold it against, so Start goes ahead and
-- the run does the check itself when the terrain appears.
E.terrain_bounds = function() return nil end
held, hold = typed({ output_dir = "C:/extract", crop = true,
  crop_x = "-595000", crop_z = "0", crop_radius_m = "10000" })
E.gui.press(hold.start)
T.eq("at the menu the check waits for the map", held.state, E.STATE_IDLE)

-- The run's own refusal reaches the line, once.
E.stop(held)
held.refusal = "crop reaches past the map, z above 1130000: box edge 1135000"
E.on_frame(held)
T.eq("the run's refusal is shown", E.window.message.text,
  "Crop reaches past the map, z above 1130000.")
local before = E.gui.count(E.window.message, "setText")
for _ = 1, 50 do E.on_frame(held) end
T.eq("and not again", E.gui.count(E.window.message, "setText"), before)

E.terrain_bounds = real_bounds

E.warn = real_warn

T.done()
