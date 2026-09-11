-- Offline tests for progress reporting: which sweep the run is on, the record
-- it keeps, and the heartbeat it logs.
--
-- Run from the repository root with a plain lua5.1.
--
-- The run reports no fraction of the whole (ADR 0025): what it knows is which
-- sweep it is on, of how many, and how far that sweep has got where the sweep
-- can count. Every case here is a run built by hand and asked.

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

T.done()
