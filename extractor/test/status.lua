-- Offline tests for the status line, the bar beneath it, and the frame that
-- writes them.
--
-- Run from the repository root with a plain lua5.1.
--
-- Both are pure functions of the run -- the line of its state and of whether a
-- terrain is loaded, the bar of its state alone -- so everything the window can
-- show is reachable here with no widget in the process. What does need a widget
-- is the other half: that each is written when it changes and left alone when
-- it has not.

package.path = "extractor/?.lua;extractor/test/support/?.lua;" .. package.path

local T = require("testing")
local E = require("DcsTerrainExtract")
local FakeGui = require("fakegui")

E.log = function() end

-- The terrain seam, answering whatever the test in front of it wants. Nil is
-- the main menu and a string is a map open in the Mission Editor; nothing here
-- needs a terrain module, because nothing here reads terrain.
local terrain = nil
E.terrain_id = function() return terrain end

local function run_in(state)
  local run = E.new_run({ config = {} })
  run.state = state
  return run
end

--------------------------------------------------------------------------------
T.group("with a map open the line is the state the run is in")
--------------------------------------------------------------------------------

terrain = "Caucasus"
T.eq("stopped has nothing to say", E.window_status(run_in(E.STATE_STOPPED)), nil)
T.eq("idle", E.window_status(run_in(E.STATE_IDLE)), "Waiting for a map.")
T.eq("prepare", E.window_status(run_in(E.STATE_PREPARE)), "Preparing.")
T.eq("hook", E.window_status(run_in(E.STATE_HOOK)), "Sweeping the terrain.")
T.eq("mission", E.window_status(run_in(E.STATE_MISSION)), "Sweeping the scenery.")
T.eq("done", E.window_status(run_in(E.STATE_DONE)), "Finished.")

-- A state with no line of its own falls back to its name rather than to nil.
-- Nothing reaches this today; what it buys is that a state added later without a
-- sentence shows a bare word instead of taking the window down at setText.
local odd = run_in("rehearsing")
T.eq("an unknown state is its own name", E.window_status(odd), "rehearsing")

--------------------------------------------------------------------------------
T.group("with no map the line says which one to open")
--------------------------------------------------------------------------------

-- Which is where the hook spends the start of every session: it loads at the
-- main menu, and DCS can sit there for hours.
terrain = nil
T.eq("stopped still has nothing to say", E.window_status(run_in(E.STATE_STOPPED)), nil)
T.eq("idle", E.window_status(run_in(E.STATE_IDLE)), E.STATUS_NO_TERRAIN)
T.eq("hook", E.window_status(run_in(E.STATE_HOOK)), E.STATUS_NO_TERRAIN)

-- Except once the run has finished. A user back at the main menu after a
-- completed extract is owed the result, and telling them to open a map is advice
-- pointing at nothing.
T.eq("done outlasts the terrain", E.window_status(run_in(E.STATE_DONE)), "Finished.")

--------------------------------------------------------------------------------
T.group("the frame builds the window and writes the line")
--------------------------------------------------------------------------------

local function fresh(mode)
  E.gui = FakeGui.new()
  if mode then E.gui[mode]() end
  E.ui_failed, E.ui_failure = false, nil
  E.window = { built = false }
  E.attach_window()
end

-- The one line's own writes. The window has other labels -- a caption per
-- field -- so counting every Static in the window would count those.
local function writes()
  return E.gui.count(E.window.message, "setText")
end

-- A stopped run at load says nothing of its own, so the first frame leaves
-- the instruction on the line: a map opening under a stopped run would
-- otherwise write "Stopped." over it.
fresh()
terrain = nil
local run = run_in(E.STATE_STOPPED)
E.on_frame(run)
T.eq("the window is built on the first frame", E.window.built, true)
T.eq("and carries the instruction, which is a map", E.window.message.text,
  E.INSTRUCTION_MAP)
T.eq("written once", writes(), 1)

-- The callback arrives about sixty times a second for the length of a session
-- and the line changes a handful of times in a run, so a write per frame is a
-- relayout a frame for a string nobody could see change.
for _ = 1, 200 do E.on_frame(run) end
T.eq("and not written again while it is the same", writes(), 1)

-- A map opening under a stopped run moves the instruction on, and says no
-- phase: "Stopped." would be news about nothing.
terrain = "Caucasus"
E.on_frame(run)
T.eq("a map opening moves the instruction on", E.window.message.text,
  E.INSTRUCTION_DIRECTORY)
T.eq("with one write", writes(), 2)

run.state = E.STATE_HOOK
E.on_frame(run)
T.eq("the run moving on is said", E.window.message.text, "Sweeping the terrain.")
T.eq("with one write", writes(), 3)
for _ = 1, 200 do E.on_frame(run) end
T.eq("and once only", writes(), 3)

-- Without a map, a run that is going is told what it is waiting for.
terrain = nil
run.state = E.STATE_IDLE
E.on_frame(run)
T.eq("no map is said", E.window.message.text, E.STATUS_NO_TERRAIN)
T.eq("with one write", writes(), 4)

-- Something else said on the line -- a press -- is not written over by the
-- phase it was said under, and the phase is said again once it changes.
E.window.message:setText("Stopped. Start carries on from here.")
E.window.status_text = nil
run.state = E.STATE_STOPPED
E.on_frame(run)
T.eq("a stopped run leaves the press's words", E.window.message.text,
  "Stopped. Start carries on from here.")
run.state = E.STATE_IDLE
E.on_frame(run)
T.eq("and the next phase replaces them", E.window.message.text,
  E.STATUS_NO_TERRAIN)

--------------------------------------------------------------------------------
T.group("the bar moves at a phase change and stands still between")
--------------------------------------------------------------------------------

T.eq("stopped is nothing done", E.window_progress(E.STATE_STOPPED), 0)
T.eq("and so is waiting", E.window_progress(E.STATE_IDLE), 0)
-- Prepare is a handful of frames against tens of minutes: a bar that jumped
-- before any terrain had been read would be describing nothing.
T.eq("preparing has done no work", E.window_progress(E.STATE_PREPARE), 0)
T.eq("nor has the first pass, starting", E.window_progress(E.STATE_HOOK), 0)
T.eq("the second pass is half way", E.window_progress(E.STATE_MISSION), 50)
T.eq("and finished is full", E.window_progress(E.STATE_DONE), 100)
-- A state added later without a share reads as no progress rather than taking
-- the bar down with it, the same way the status line handles one.
T.eq("a state nobody gave a share", E.window_progress("elsewhere"), 0)

local function bar_writes()
  return E.gui.count(E.window.bar, "setValue")
end

fresh()
terrain = "Caucasus"
run = run_in(E.STATE_STOPPED)
E.on_frame(run)
local bar = E.gui.find("HorzProgressBar")
T.eq("the bar is a percentage", bar.range[1] .. ".." .. bar.range[2], "0..100")
T.eq("and starts empty", bar.value, 0)
T.eq("and hidden, with nothing on it to show", bar.visible, false)
T.eq("written once", bar_writes(), 1)

for _ = 1, 200 do E.on_frame(run) end
T.eq("and left alone while the phase holds", bar_writes(), 1)

run.state = E.STATE_MISSION
E.on_frame(run)
T.eq("the second pass moves it", bar.value, 50)
T.eq("and puts it on screen", bar.visible, true)
run.state = E.STATE_DONE
E.on_frame(run)
T.eq("and finishing fills it", bar.value, 100)
T.eq("one write per change", bar_writes(), 3)

--------------------------------------------------------------------------------
T.group("a window that cannot be built ticks a no-op")
--------------------------------------------------------------------------------

-- This interpreter, and any DCS whose widget library moved. The frame callback
-- still runs on every frame, so what must not happen is an index into a label
-- that was never made.
fresh("no_library")
E.on_frame(run)
E.on_frame(run)
T.eq("no window", E.window.built, false)
T.eq("nothing written", writes(), 0)
T.eq("and nothing latched", E.ui_failed, false)

-- The other half of the pair: a library that is there and raising latches, and
-- the frame after that does nothing at all rather than trying again.
fresh("fail_every")
local warned = 0
local real_warn = E.warn
E.warn = function() warned = warned + 1 end
E.on_frame(run)
E.on_frame(run)
T.eq("still no window", E.window.built, false)
T.eq("it latched", E.ui_failed, true)
T.eq("and said so once", warned, 1)
E.warn = real_warn

T.done()
