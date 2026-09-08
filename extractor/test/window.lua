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
T.eq("and so is the crop line above it",
  E.window.lines.crop.bounds[2] < last[2], true)

-- The buttons run left to right off one list, so a button added to it is the
-- way this overflows, and it would do so silently.
local rightmost = E.window.buttons.map.bounds
T.eq("the last button is inside the window",
  rightmost[1] + rightmost[3] <= frame[3], true)

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
  E.window.message.text:find("Start again", 1, true) ~= nil, true)

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
T.eq("and the crop centre", saved.crop.x, -290000)
T.eq("both halves of it", saved.crop.z, 617000)
T.eq("and the radius", saved.crop.radius_m, 5000)
T.eq("with the hook still switched on", saved.enabled, true)
T.eq("and it says so", E.window.message.text:find("next time", 1, true) ~= nil, true)

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
  E.window.lines.crop.text:find("12abc", 1, true) ~= nil, true)
T.eq("and the message points at it",
  E.window.message.text, "Not started: see the lines above.")

-- The one field with no default stops it the same way.
refused, press, fs = typed({ output_dir = "", crop = false })
E.gui.press(press.start)
T.eq("a blank directory stops it", refused.state, E.STATE_STOPPED)
T.eq("with nothing written", fs.files[CONFIG], nil)
T.eq("and its own line", E.window.lines.output_dir.text,
  E.field_problem("output_dir", nil))

-- Fixing it and pressing again clears the line rather than leaving the old
-- complaint under a field that is now fine.
E.window.controls.output_dir.text = "C:/extract"
E.gui.press(press.start)
T.eq("the line clears", E.window.lines.output_dir.text, "")
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
  E.window.message.text:find("were not saved", 1, true) ~= nil, true)
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
T.group("the crop centre can come off the map")
--------------------------------------------------------------------------------

-- The map lives in another Lua state and this file cannot reach it here, so the
-- seam is stubbed and what is asserted is what the window does with an answer.
local real_map = E.map_position

local picked
picked, press = typed({ output_dir = "C:/extract", crop = false })
E.map_position = function() return -545142.85714286, 682000 end
E.gui.press(press.map)
T.eq("the centre lands in the boxes", E.window.controls.crop_x.text,
  "-545142.85714286")
T.eq("both halves of it", E.window.controls.crop_z.text, "682000")
-- Ticked, because reading a centre off the map is the deliberate act the tick
-- records. Left unticked, a press here followed by Start would sweep the whole
-- theatre having just been told where the user wanted to extract.
T.eq("and the crop is switched on", E.window.controls.crop.state, true)
T.eq("with a line saying what is still missing",
  E.window.message.text:find("radius", 1, true) ~= nil, true)

-- A radius away from a run, which is what the crop's own line then says.
E.gui.press(press.start)
T.eq("Start refuses without one", picked.state, E.STATE_STOPPED)
T.eq("naming the radius", E.window.lines.crop.text:find("radius_m", 1, true) ~= nil,
  true)
E.window.controls.crop_radius_m.text = "5000"
E.gui.press(press.start)
T.eq("and takes it with one", picked.state, E.STATE_IDLE)

-- No map is the main menu, and no net at all is this interpreter. Both are the
-- same nil, and neither touches what is in the boxes.
picked, press = typed({ output_dir = "C:/extract", crop = false, crop_x = "1" })
E.map_position = function() return nil end
E.gui.press(press.map)
T.eq("nothing was taken", E.window.controls.crop_x.text, "1")
T.eq("the crop is left alone", E.window.controls.crop.state, false)
T.eq("and it says where to look",
  E.window.message.text:find("Mission Editor", 1, true) ~= nil, true)

-- The seam itself, with no DCS in the process: no net global, so nil.
E.map_position = real_map
T.eq("no net answers nothing", E.map_position(), nil)

--------------------------------------------------------------------------------
T.group("a screen change is noticed and the window is rebuilt onto the new one")
--------------------------------------------------------------------------------

-- A window is drawn only on the screen that was current when it was made, and
-- goes on answering every question -- getVisible included -- once that screen
-- has gone. So the window cannot be asked about itself; the screen is asked
-- what it is drawing instead.
local function ticks(run, n)
  for _ = 1, n do E.on_frame(run) end
end

local moved
moved, press, fs = typed({ output_dir = "C:/extract", crop = false })
E.window.controls.output_dir.text = "C:/typed-by-hand"
local first = E.window.root
T.eq("a window to start with", first ~= nil, true)

-- Poll every 60 frames and act on the second miss, so two polls have to pass.
E.gui.change_screen()
ticks(moved, E.WINDOW_POLL_FRAMES)
T.eq("one miss rebuilds nothing", E.window.root, first)
T.eq("and the old window still claims to be visible", first.visible, true)

ticks(moved, E.WINDOW_POLL_FRAMES)
T.eq("the second builds a new one", E.window.root ~= first, true)
T.eq("on the screen that is current now",
  E.window.root.screen, E.gui.screen)
T.eq("carrying what was typed", E.window.controls.output_dir.text,
  "C:/typed-by-hand")
T.eq("the old one is kept, not killed", E.window.orphans[1], first)
T.eq("and not killed", first.killed, nil)
T.eq("counted", E.window.rebuilds, 1)

-- Settled again: no further rebuilds while the screen holds, which is the
-- assertion that a poll firing every second does not churn.
local second = E.window.root
ticks(moved, E.WINDOW_POLL_FRAMES * 4)
T.eq("no rebuild while it is drawn", E.window.root, second)
T.eq("still one", E.window.rebuilds, 1)

-- The frame after a rebuild must not refill the boxes from the run's config,
-- which would take back the very text the rebuild just carried across.
ticks(moved, 1)
T.eq("and the typed value survives the frame after",
  E.window.controls.output_dir.text, "C:/typed-by-hand")

-- The status line and the bar are written only when they change, and a rebuilt
-- window starts with neither cache. They come back because the frame writes them
-- before it polls -- an ordering this pins, since nothing else would notice it
-- being swapped.
T.eq("the status line is filled in again",
  E.window.status.text, E.window_status(moved))
T.eq("and so is the bar", E.window.bar.value, E.window_progress(moved.state))

-- Dragged, then the screen changes: it comes back where it was left.
E.window.root.bounds = { 400, 300, E.window.root.bounds[3], E.window.root.bounds[4] }
E.gui.change_screen()
ticks(moved, E.WINDOW_POLL_FRAMES * 2)
T.eq("rebuilt where it was dragged to", E.window.root.bounds[1], 400)
T.eq("both axes", E.window.root.bounds[2], 300)

-- Dragged off the edge, with the screen never changing. Nothing is painted off
-- the screen, so the probe answers nothing whether the window is fine or gone --
-- and rebuilding on that would put it back in the same off-screen place, fail
-- the same probe, and do it again every couple of seconds for the session,
-- leaking a window tree and taking the keyboard each time.
local before_drag = E.window.rebuilds
local orphans_before = #E.window.orphans
E.window.root.bounds = { -600, -400, E.window.root.bounds[3], E.window.root.bounds[4] }
ticks(moved, E.WINDOW_POLL_FRAMES * 6)
T.eq("a window dragged off the edge is left alone", E.window.rebuilds, before_drag)
T.eq("and no orphan was made", #E.window.orphans, orphans_before)

-- Dragged back on, the screen change it missed is noticed as normal.
E.window.root.bounds = { 400, 300, E.window.root.bounds[3], E.window.root.bounds[4] }
E.gui.change_screen()
ticks(moved, E.WINDOW_POLL_FRAMES * 2)
T.eq("and it works again once it is back on screen",
  E.window.rebuilds, before_drag + 1)

-- What the window was saying comes back with it.
E.gui.press(press.stop)
local said = E.window.message.text
T.eq("there is something to carry", said ~= "" and said ~= nil, true)
E.gui.change_screen()
ticks(moved, E.WINDOW_POLL_FRAMES * 2)
T.eq("the message line is restored", E.window.message.text, said)

--------------------------------------------------------------------------------
T.group("the run is untouched by any of it")
--------------------------------------------------------------------------------

-- The point of the whole window: it may cost itself and never the extract.
local guarded = E.new_run({ config = { enabled = true, output_dir = "C:/extract" } })
guarded.state = E.STATE_HOOK
window_on(guarded)
local frames_before = guarded.frames
for _ = 1, 5 do
  E.gui.change_screen()
  ticks(guarded, E.WINDOW_POLL_FRAMES * 2)
end
T.eq("the run did not move", guarded.state, E.STATE_HOOK)
T.eq("nor count a frame of its own", guarded.frames, frames_before)
T.eq("five screen changes, five rebuilds", E.window.rebuilds, 5)
T.eq("and nothing latched", E.ui_failed, false)

-- A library that cannot answer what is drawn is never polled, so it behaves the
-- way it did before it could be asked: built once and left alone.
fresh()
E.attach_window()
E.gui.can_probe = function() return false end
local unpolled = E.new_run({ config = { enabled = true, output_dir = "C:/e" } })
E.on_frame(unpolled)
local only = E.window.root
E.gui.change_screen()
ticks(unpolled, E.WINDOW_POLL_FRAMES * 4)
T.eq("no probing, no rebuild", E.window.root, only)
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
