-- Offline tests for the status line and the frame that writes it.
--
-- Run from the repository root with a plain lua5.1.
--
-- The line is a pure function of the run and of whether a terrain is loaded, so
-- every line the window can show is reachable here with no widget in the
-- process. What does need a widget is the other half: that the label is written
-- when the line changes and left alone when it has not.

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
T.eq("stopped", E.window_status(run_in(E.STATE_STOPPED)), "Stopped.")
T.eq("idle", E.window_status(run_in(E.STATE_IDLE)), "Waiting for a theatre.")
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
T.eq("stopped", E.window_status(run_in(E.STATE_STOPPED)), E.STATUS_NO_TERRAIN)
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

local function writes()
  local n = 0
  for i = 1, #E.gui.calls do
    if E.gui.calls[i] == "Static:setText" then n = n + 1 end
  end
  return n
end

fresh()
terrain = nil
local run = run_in(E.STATE_STOPPED)
E.on_frame(run)
T.eq("the window is built on the first frame", E.window.built, true)
T.eq("and carries the line", E.gui.find("Static").text, E.STATUS_NO_TERRAIN)
T.eq("written once", writes(), 1)

-- The callback arrives about sixty times a second for the length of a session
-- and the line changes a handful of times in a run, so a write per frame is a
-- relayout a frame for a string nobody could see change.
for _ = 1, 200 do E.on_frame(run) end
T.eq("and not written again while it is the same", writes(), 1)

terrain = "Caucasus"
E.on_frame(run)
T.eq("a map opening changes it", E.gui.find("Static").text, "Stopped.")
T.eq("with one write", writes(), 2)

run.state = E.STATE_HOOK
E.on_frame(run)
T.eq("and so does the run moving on", E.gui.find("Static").text, "Sweeping the terrain.")
T.eq("with one more", writes(), 3)

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
