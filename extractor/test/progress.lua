-- Offline tests for progress reporting: which sweep the run is on, the record
-- it keeps, and the heartbeat it logs.
--
-- Run from the repository root with a plain lua5.1.
--
-- The run reports no fraction of the whole (ADR 0025): what it knows is which
-- sweep it is on, of how many, and how far that sweep has got where the sweep
-- can count. The first groups ask that of runs built by hand; the driver at
-- the end drives a whole run through ten thousand frames and reads the log.

package.path = "extractor/?.lua;extractor/test/support/?.lua;" .. package.path

local T = require("testing")
local E = require("DcsTerrainExtract")

E.log = function() end

local function job(name)
  return { name = name, start = function() return function() return E.DONE end end }
end

-- A run in `state` over the given job lists, with no queue.
local function run_with(state, jobs)
  local run = E.new_run({ config = {}, jobs = jobs })
  run.state = state
  return run
end

-- Stands the run's queue at `index` in the current phase, running the job
-- there with `done`/`total` as its count; no count leaves the job unstarted.
local function at(run, index, done, total)
  run.queue = E.new_queue(run.jobs[run.state])
  run.queue.index = index
  if done then
    run.queue.step = function() return E.MORE end
    run.queue.progress = function() return done, total end
  end
  return run
end

local function position(run)
  local p, n = E.sweep_position(run)
  if p == nil then
    return "nil"
  end
  return p .. " of " .. n
end

--------------------------------------------------------------------------------
T.group("the sweep's position is counted over prepare, hook and mission")
--------------------------------------------------------------------------------

-- identity, then water, height and roads, then surface: five sweeps.
local JOBS = {
  prepare = { job("identity") },
  hook = { job("water"), job("height"), job("roads") },
  mission = { job("surface") },
}

T.eq("stopped is on no sweep", position(run_with(E.STATE_STOPPED, JOBS)), "nil")
T.eq("nor is idle", position(run_with(E.STATE_IDLE, JOBS)), "nil")
T.eq("nor is done", position(run_with(E.STATE_DONE, JOBS)), "nil")
T.eq("nor a state that is not a phase", position(run_with("elsewhere", JOBS)), "nil")

T.eq("the first sweep of prepare", position(at(run_with(E.STATE_PREPARE, JOBS), 1)), "1 of 5")
T.eq("the first sweep of the hook pass", position(at(run_with(E.STATE_HOOK, JOBS), 1)), "2 of 5")
T.eq("roads", position(at(run_with(E.STATE_HOOK, JOBS), 3)), "4 of 5")
T.eq("the mission pass with no queue stands at its first sweep",
  position(run_with(E.STATE_MISSION, JOBS)), "5 of 5")
T.eq("surface", position(at(run_with(E.STATE_MISSION, JOBS), 1)), "5 of 5")

-- Between the last job finishing and the phase changing, the index points
-- past the list, and the position is held to the count.
T.eq("a queue past its end is the last sweep",
  position(at(run_with(E.STATE_HOOK, JOBS), 4)), "4 of 5")

-- A phase with nothing registered is skipped over in the count, and a run
-- with nothing registered anywhere is on no sweep.
T.eq("an empty phase counts for nothing", position(at(run_with(E.STATE_MISSION,
  { prepare = {}, hook = { job("water") }, mission = { job("surface") } }), 1)), "2 of 2")
T.eq("nothing registered is no sweep", position(run_with(E.STATE_HOOK,
  { prepare = {}, hook = {}, mission = {} })), "nil")

--------------------------------------------------------------------------------
T.group("the bar is the running sweep's own count, held inside its total")
--------------------------------------------------------------------------------

T.eq("half way", E.window_progress(at(run_with(E.STATE_HOOK, JOBS), 1, 50, 100)), 50)
T.eq("rounded down to a whole percent",
  E.window_progress(at(run_with(E.STATE_HOOK, JOBS), 1, 1, 3)), 33)
T.eq("a count past the total is full",
  E.window_progress(at(run_with(E.STATE_MISSION, JOBS), 1, 60, 6)), 100)
T.eq("a count below zero is empty",
  E.window_progress(at(run_with(E.STATE_MISSION, JOBS), 1, -60, 6)), 0)
T.eq("a count that is not a count is empty",
  E.window_progress(at(run_with(E.STATE_MISSION, JOBS), 1, "three", 6)), 0)
T.eq("a sweep that has not started is empty",
  E.window_progress(at(run_with(E.STATE_MISSION, JOBS), 1)), 0)
T.eq("and done is full", E.window_progress(run_with(E.STATE_DONE, JOBS)), 100)

--------------------------------------------------------------------------------
T.group("a driver over ten thousand frames: a heartbeat per interval")
--------------------------------------------------------------------------------

-- The same shape of run the manifest test drives: 49 100 steps of a
-- millisecond each against a budget that fits five a frame, so the run needs
-- nearly all of the frames below. The clock is stepped by hand, so a second
-- of it is a thousand steps and the cadences can be brought down to where
-- the run sees dozens of each.
local FakeFs = require("fakefs")

local now = 0
E.clock = function() return now end
E.now_iso = function() return "2026-09-10T00:00:00Z" end
E.terrain_id = function() return "Caucasus" end
E.PROGRESS_S = 0.1
E.HEARTBEAT_S = 1

-- Every line the run logs, with the state the run was in when it logged it,
-- which is what "names the phase it is actually in" is checked against.
local logged, state_at = {}, {}
local driven
E.log = function(message)
  logged[#logged + 1] = message
  state_at[#state_at + 1] = driven and driven.state or "?"
end

local grid = E.grid_from_rect({ min_x = 0, min_z = 0, max_x = 51200, max_z = 51200 }, 50, 256)

local function open_output(run)
  E.ensure_output_dirs(run.dir)
  run.manifest = E.new_manifest({
    theatre = "Caucasus",
    dcs_build = "2.9.29.27468",
    dcs_build_timestamp = "20250101-120000",
    terrain_fingerprint = { surface5 = { size = 4194304 } },
    bounds_km = { sw = { -30, -45 }, ne = { 40, 25 } },
    grid = grid,
    omit_sea_tiles = true,
  })
end

-- Finishes after `steps` steps of a millisecond each, and counts them off
-- where `counts` is set; a job that does not count is the case the plan
-- names, and gets a heartbeat all the same.
local function sweep(name, steps, counts, on_start)
  return {
    name = name,
    start = function(run)
      if on_start then
        on_start(run)
      end
      local left = steps
      local step = function()
        now = now + 0.001
        left = left - 1
        return left > 0 and E.MORE or E.DONE
      end
      if not counts then
        return step
      end
      return step, function()
        return steps - left, steps
      end
    end,
  }
end

local fs = FakeFs.new()
E.fs = fs
driven = E.new_run({
  config = { output_dir = "C:/extract", frame_budget_ms = 5 },
  jobs = {
    prepare = { sweep("presweep", 2000, true, open_output) },
    hook = { sweep("config", 100, true), sweep("water", 20000, true),
      sweep("height", 15000, false) },
    mission = { sweep("surface", 12000, true) },
  },
})
E.start(driven)

local refreshes, last_record, water_record = 0, nil, nil
local frames = 0
for _ = 1, 10000 do
  E.run_frame(driven)
  frames = frames + 1
  if driven.progress and driven.progress ~= last_record then
    refreshes = refreshes + 1
    last_record = driven.progress
    if last_record.sweep == "water" and water_record == nil then
      water_record = last_record
    end
  end
  if driven.state == E.STATE_DONE then
    break
  end
end
T.eq("the run finished", driven.state, E.STATE_DONE)
T.eq("inside ten thousand frames", frames <= 10000, true)
T.eq("having driven the clock forty-nine seconds", string.format("%.1f", now), "49.1")

-- One heartbeat per second of clock: the first is due a second after Start
-- and the last inside the final second, and a phase ending on the frame one
-- was due pushes it to the next frame rather than losing it.
local heartbeats = {}
for i = 1, #logged do
  if logged[i]:match("^heartbeat ") then
    heartbeats[#heartbeats + 1] = { line = logged[i], state = state_at[i] }
  end
end
T.eq("about one heartbeat a second", math.abs(#heartbeats - 49) <= 1, true)

-- The record for the window is refreshed ten times as often, from the same
-- numbers.
T.eq("and about ten refreshes a second", math.abs(refreshes - 491) <= 5, true)

-- Every line parses the same way: phase, sweep, which sweep of how many, the
-- count where there is one, then elapsed, last.
local wrong_phase, malformed, back, last_position = 0, 0, 0, 0
local mute, counted_mute = 0, 0
for i = 1, #heartbeats do
  local h = heartbeats[i]
  local phase, name, pos, count, rest =
    h.line:match("^heartbeat (%a+) (%a+) (%d+)/(%d+)(.*) elapsed %d+ s$")
  if phase == nil then
    malformed = malformed + 1
  else
    if phase ~= h.state then
      wrong_phase = wrong_phase + 1
    end
    pos, count = tonumber(pos), tonumber(count)
    if count ~= 5 or pos < 1 or pos > count then
      malformed = malformed + 1
    end
    if pos < last_position then
      back = back + 1
    end
    last_position = pos
    if name == "height" then
      mute = mute + 1
      if rest ~= "" then
        counted_mute = counted_mute + 1
      end
    end
  end
end
T.eq("every heartbeat has the one shape", malformed, 0)
T.eq("every heartbeat names the phase the run was in", wrong_phase, 0)
T.eq("and the sweep never goes back within the attempt", back, 0)
T.eq("a sweep that cannot count still gets its heartbeats", mute > 10, true)
T.eq("with no count on them", counted_mute, 0)

-- The lines as a reader greps them.
T.eq("a counted line has the count after the position",
  heartbeats[1].line:match("^heartbeat prepare presweep 1/5 %d+/2000 elapsed 1 s$") ~= nil,
  true)
T.eq("an uncounted line goes from the position to elapsed",
  (function()
    for i = 1, #heartbeats do
      local l = heartbeats[i].line
      if l:match("^heartbeat hook height ") then
        return l:match("^heartbeat hook height 4/5 elapsed %d+ s$") ~= nil
      end
    end
  end)(), true)

-- No heartbeat while waiting for a map: idle has its own line when it ends.
local first_heartbeat, prepare_line = #logged + 1, nil
for i = 1, #logged do
  if logged[i] == "phase prepare" then prepare_line = i end
  if first_heartbeat > #logged and logged[i]:match("^heartbeat ") then
    first_heartbeat = i
  end
end
T.eq("none before the run has terrain", first_heartbeat > prepare_line, true)

-- The record the window reads, caught during water.
T.eq("the record names its phase", water_record.phase, E.STATE_HOOK)
T.eq("and its sweep", water_record.sweep, "water")
T.eq("and which sweep that is", water_record.position .. " of " .. water_record.count, "3 of 5")
T.eq("with the count in the window's words",
  water_record.text:match("^water, 3 of 5, %d+ of 20000$") ~= nil, true)
T.eq("and the same numbers on the log line",
  water_record.line:match("^heartbeat hook water 3/5 %d+/20000 elapsed %d+ s$") ~= nil,
  true)
T.eq("and cleared at done", driven.progress, false)

-- One line at done with what the run came to. No tile was written by these
-- sweeps, the frames are the ones driven, and the sweeps' time is the clock
-- they stepped: 49 100 ms, to the nearest second.
local totals
for i = 1, #logged do
  if logged[i] == "phase done" then
    totals = logged[i + 1]
  end
end
T.eq("totals follow the phase line", totals,
  string.format("totals 0 tiles %d frames 49 s in sweeps", driven.frames))

-- A count past 2^31, which this Lua's %d wraps negative: a sweep that counts
-- bytes gets there, and the record has to print what it was told.
local big = E.new_run({
  config = { output_dir = "C:/big", frame_budget_ms = 5 },
  jobs = {
    prepare = { sweep("presweep", 1, false, open_output) },
    hook = { { name = "water", start = function()
      return function() now = now + 0.001 return E.MORE end,
        function() return 3000000000, 4000000000 end
    end } },
    mission = {},
  },
})
E.start(big)
for _ = 1, 40 do E.run_frame(big) end
T.eq("a count past 2^31 prints whole", big.progress.text,
  "water, 2 of 2, 3000000000 of 4000000000")
T.eq("on the log line too",
  big.progress.line:match("^heartbeat hook water 2/2 3000000000/4000000000 ") ~= nil, true)

-- Stop keeps no record: a stopped run is sweeping nothing, whatever the
-- window's order of asking.
E.stop(big)
T.eq("Stop clears the record", big.progress, false)

-- A second attempt starts its own count: nothing is due on its first frame
-- of work, and it begins at the first sweep again.
local before = #logged
driven.progress = { text = "stale" }
E.start(driven)
T.eq("Start clears the record", driven.progress, false)
for _ = 1, 10000 do
  E.run_frame(driven)
  if driven.state == E.STATE_DONE then break end
end
local first_again
for i = before + 1, #logged do
  if logged[i]:match("^heartbeat ") then
    first_again = logged[i]
    break
  end
end
T.eq("the next attempt's first heartbeat is a second into it",
  first_again:match(" elapsed (%d+) s$"), "1")
T.eq("and is on the first sweep again",
  first_again:match("^heartbeat prepare presweep 1/5 ") ~= nil, true)

-- The clocks start with the work, not with Start. A run can wait at the menu
-- for hours before a map is opened; with the clocks stamped at Start, the
-- first frame of work would find a record and a heartbeat long overdue and
-- fire both, saying nothing had happened yet.
local waited = E.new_run({
  config = { output_dir = "C:/waited", frame_budget_ms = 5 },
  jobs = {
    prepare = { sweep("presweep", 1, false, open_output) },
    hook = { sweep("water", 3000, true) },
    mission = {},
  },
})
E.terrain_id = function() return nil end
E.start(waited)
E.run_frame(waited)
T.eq("waiting for a map", waited.state, E.STATE_IDLE)
now = now + 5000
E.terrain_id = function() return "Caucasus" end
E.callbacks(waited).onMissionLoadEnd()
local at_map = #logged
E.run_frame(waited)
T.eq("the map is found", waited.state, E.STATE_PREPARE)
for _ = 1, 20 do E.run_frame(waited) end
T.eq("and the work is under way", waited.state, E.STATE_HOOK)
local early = 0
for i = at_map + 1, #logged do
  if logged[i]:match("^heartbeat ") then early = early + 1 end
end
T.eq("nothing is due on the first frames of work", early, 0)
T.eq("and no record either", waited.progress, false)
local first_after_wait
for _ = 1, 400 do
  E.run_frame(waited)
  if logged[#logged]:match("^heartbeat ") then
    first_after_wait = logged[#logged]
    break
  end
end
T.eq("the first heartbeat comes a second into the work",
  first_after_wait:match("^heartbeat hook water 2/2 %d+/3000 elapsed 1 s$") ~= nil, true)

-- Retarget resets the clocks with everything else, by value and not only by
-- key: a stale stamp would make the next attempt's first heartbeat late or
-- early by however long the last one ran.
E.stop(waited)
T.eq("moved elsewhere", E.retarget(waited, { output_dir = "C:/elsewhere" }), true)
T.eq("the clocks are back to nothing",
  waited.progress_at .. " " .. waited.heartbeat_at .. " " .. waited.started_clock, "0 0 0")
T.eq("and so is the record", waited.progress, false)

T.done()
