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
T.group("the bar follows the running sweep's own count")
--------------------------------------------------------------------------------

-- A run with four jobs across the walk: the identity read, water and height,
-- then surface. The bar is the running sweep's own count and nothing else;
-- which sweep of the four it is goes on the line, and nothing claims to know
-- how far the whole run is.
local function job(name)
  return { name = name, start = function() return function() return E.DONE end end }
end

local function four_sweeps(state)
  local r = E.new_run({ config = {}, jobs = {
    prepare = { job("identity") },
    hook = { job("water"), job("height") },
    mission = { job("surface") },
  } })
  r.state = state
  return r
end

-- Puts the run part way through its current phase: `index` names the running
-- job, and `done`/`total` is what that job answers.
local function part_way(r, index, done, total)
  r.queue = E.new_queue(r.jobs[r.state])
  r.queue.index = index
  if done then
    r.queue.step = function() return E.MORE end
    r.queue.progress = function() return done, total end
  end
  return r
end

T.eq("stopped is nothing", E.window_progress(four_sweeps(E.STATE_STOPPED)), 0)
T.eq("and so is waiting", E.window_progress(four_sweeps(E.STATE_IDLE)), 0)
T.eq("a sweep that has not started",
  E.window_progress(part_way(four_sweeps(E.STATE_HOOK), 1)), 0)
T.eq("a quarter through water",
  E.window_progress(part_way(four_sweeps(E.STATE_HOOK), 1, 1, 4)), 25)
T.eq("water done and height not started is nothing again",
  E.window_progress(part_way(four_sweeps(E.STATE_HOOK), 2)), 0)
T.eq("half through height",
  E.window_progress(part_way(four_sweeps(E.STATE_HOOK), 2, 2, 4)), 50)
T.eq("the second pass with no queue",
  E.window_progress(four_sweeps(E.STATE_MISSION)), 0)
T.eq("half through surface",
  E.window_progress(part_way(four_sweeps(E.STATE_MISSION), 1, 3, 6)), 50)
local mute = part_way(four_sweeps(E.STATE_MISSION), 1)
mute.queue.step = function() return E.MORE end
T.eq("a sweep that cannot count shows nothing", E.window_progress(mute), 0)
T.eq("and finished is full", E.window_progress(four_sweeps(E.STATE_DONE)), 100)
-- A state added later without a bar reads as nothing rather than taking the
-- bar down with it, the same way the status line handles one.
T.eq("a state nobody gave a bar", E.window_progress(four_sweeps("elsewhere")), 0)

local function bar_writes()
  return E.gui.count(E.window.bar, "setValue")
end

fresh()
terrain = "Caucasus"
run = four_sweeps(E.STATE_STOPPED)
E.on_frame(run)
local bar = E.gui.find("HorzProgressBar")
T.eq("the bar is a percentage", bar.range[1] .. ".." .. bar.range[2], "0..100")
T.eq("and starts empty", bar.value, 0)
T.eq("and hidden, with nothing on it to show", bar.visible, false)
T.eq("written once", bar_writes(), 1)

for _ = 1, 200 do E.on_frame(run) end
T.eq("and left alone while nothing moves", bar_writes(), 1)

-- A phase change alone moves nothing: the bar is a sweep's, and no sweep is
-- counting yet.
run.state = E.STATE_MISSION
E.on_frame(run)
T.eq("a pass with no sweep counting leaves it", bar_writes(), 1)
T.eq("and hidden", bar.visible, false)

part_way(run, 1, 3, 6)
E.on_frame(run)
T.eq("a sweep counting moves it", bar.value, 50)
T.eq("and puts it on screen", bar.visible, true)
T.eq("with one write", bar_writes(), 2)

-- Only when the whole percentage does: sixty frames of a count creeping
-- inside one percent are no writes at all.
local creep = 0
run.queue.progress = function() return 3 + creep, 6 end
for _ = 1, 60 do
  creep = creep + 0.0001
  E.on_frame(run)
end
T.eq("a count inside one percent is not written", bar_writes(), 2)

-- The next sweep starts from nothing: between one finishing and the next
-- counting, the bar is empty again and off the screen.
run.state = E.STATE_HOOK
part_way(run, 1, 1, 4)
E.on_frame(run)
T.eq("a new sweep starts low", bar.value, 25)
part_way(run, 2)
E.on_frame(run)
T.eq("and between sweeps it is empty", bar.value, 0)
T.eq("and hidden again", bar.visible, false)

run.state = E.STATE_DONE
E.on_frame(run)
T.eq("and finishing fills it", bar.value, 100)
T.eq("one write per change", bar_writes(), 5)


--------------------------------------------------------------------------------
T.group("the line names the sweep the run is in, once per record")
--------------------------------------------------------------------------------

-- The run keeps a record of where it is, refreshed about once a second, and
-- the line is the phase's sentence with the record's words after it. Built
-- by hand here, with the words the run would put in it.
local function record(text)
  return { text = text }
end

terrain = "Caucasus"
run = four_sweeps(E.STATE_HOOK)
T.eq("with no record, the phase alone", E.window_status(run),
  "Sweeping the terrain.")
run.progress = record("water, 2 of 4, 1234 of 5000")
T.eq("a record names the sweep and its count", E.window_status(run),
  "Sweeping the terrain: water, 2 of 4, 1234 of 5000.")
run.progress = record("roads, 4 of 9")
T.eq("one without a count names the sweep", E.window_status(run),
  "Sweeping the terrain: roads, 4 of 9.")
run.state = E.STATE_MISSION
run.progress = record("surface, 9 of 9, 3 of 6")
T.eq("in the scenery pass too", E.window_status(run),
  "Sweeping the scenery: surface, 9 of 9, 3 of 6.")
run.state = E.STATE_PREPARE
run.progress = record("presweep, 2 of 9, 10 of 400")
T.eq("and while preparing", E.window_status(run),
  "Preparing: presweep, 2 of 9, 10 of 400.")
-- A state with no sentence of its own has no full stop to take off.
run.state = "rehearsing"
run.progress = record("lines")
T.eq("a bare state name takes the words after it", E.window_status(run),
  "rehearsing: lines.")
-- Without a map, nothing about the sweep: the map is what to say.
terrain = nil
run.state = E.STATE_HOOK
T.eq("no map outranks the record", E.window_status(run), E.STATUS_NO_TERRAIN)

-- Written when the words change: a fresh record with a moved count is one
-- write, the same record over sixty frames is none, and a fresh record whose
-- words are the same -- a sweep that cannot count -- is none either.
fresh()
terrain = "Caucasus"
run = four_sweeps(E.STATE_HOOK)
E.on_frame(run)
-- The first frame also writes the instruction into the line before the
-- phase replaces it, so the counts below are relative to that.
local base = writes()
run.progress = record("water, 2 of 4, 1 of 5000")
E.on_frame(run)
T.eq("a record is written", E.window.message.text,
  "Sweeping the terrain: water, 2 of 4, 1 of 5000.")
T.eq("once", writes(), base + 1)
for _ = 1, 60 do E.on_frame(run) end
T.eq("and not again while it stands", writes(), base + 1)
run.progress = record("water, 2 of 4, 2 of 5000")
E.on_frame(run)
T.eq("a moved count is a write", writes(), base + 2)
run.progress = record("roads, 4 of 9")
E.on_frame(run)
T.eq("a new sweep is a write", writes(), base + 3)
run.progress = record("roads, 4 of 9")
for _ = 1, 60 do E.on_frame(run) end
T.eq("the same words again are not", writes(), base + 3)
run.progress = false
E.on_frame(run)
T.eq("the record going is the phase alone again", E.window.message.text,
  "Sweeping the terrain.")

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
