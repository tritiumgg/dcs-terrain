-- Offline tests for the state machine.
--
-- Run from the repository root with a plain lua5.1.
--
-- No sweep exists yet, so every job here is a fake that counts its steps. That
-- is the point of the split: the machine decides when a phase starts, when it
-- ends and what the manifest says about it, and none of those answers depend
-- on what a sweep does with its frames.

package.path = "extractor/?.lua;extractor/test/support/?.lua;" .. package.path

local T = require("testing")
local E = require("DcsTerrainExtract")
local FakeFs = require("fakefs")

local now = 0
E.clock = function() return now end
E.now_iso = function() return "2026-09-04T09:12:44Z" end

local logged = {}
E.log = function(message) logged[#logged + 1] = message end

-- A map is always open here. Idle has its own test file.
E.terrain_id = function() return "Caucasus" end

local grid = E.grid_from_rect({ min_x = 0, min_z = 0, max_x = 51200, max_z = 51200 }, 50, 256)

-- A job that finishes after `steps` steps, each costing one millisecond.
local function job(name, steps, on_start)
  return {
    name = name,
    start = function(run)
      if on_start then
        on_start(run)
      end
      local left = steps
      return function()
        now = now + 0.001
        left = left - 1
        return left > 0 and E.MORE or E.DONE
      end
    end,
  }
end

-- The prepare job every test needs: something has to produce the manifest the
-- machine advances, and until the sweeps exist nothing does.
local function open_output(run)
  E.ensure_output_dirs(run.dir)
  run.manifest = E.new_manifest({
    theatre = "Caucasus",
    dcs_build = "2.9.29.27468",
    dcs_build_timestamp = "20250101-120000",
    terrain_fingerprint = { digest = "90c9cec8" },
    bounds_km = { sw = { -30, -45 }, ne = { 40, 25 } },
    grid = grid,
    omit_sea_tiles = true,
  })
end

local function new_run(edit)
  local fs = FakeFs.new()
  E.fs = fs
  local opts = {
    config = { output_dir = "C:/extract", frame_budget_ms = 5 },
    jobs = {
      prepare = { job("presweep", 1, open_output) },
      hook = { job("config", 1), job("water", 3) },
      mission = { job("surface", 2) },
    },
  }
  for k, v in pairs(edit or {}) do
    opts[k] = v
  end
  local run = E.new_run(opts)
  run.fs = fs
  return run
end

-- Frames until the run leaves `state`, or until `limit` frames have passed.
local function until_past(run, state, limit)
  local frames = 0
  while run.state == state and frames < (limit or 1000) do
    E.run_frame(run)
    frames = frames + 1
  end
  return frames
end

--------------------------------------------------------------------------------
T.group("phases run in order")
--------------------------------------------------------------------------------

local run = new_run()
T.eq("starts idle", run.state, E.STATE_IDLE)

local seen = { run.state }
for _ = 1, 200 do
  local state = E.run_frame(run)
  if seen[#seen] ~= state then
    seen[#seen + 1] = state
  end
  if run.state == E.STATE_DONE then
    break
  end
end

T.eq("idle to prepare to hook to mission to done",
  table.concat(seen, " "), "idle prepare hook mission done")

--------------------------------------------------------------------------------
T.group("done is the end of it")
--------------------------------------------------------------------------------

local frames_at_done = run.frames
for _ = 1, 100 do
  T.eq("stays done", E.run_frame(run), E.STATE_DONE)
end
T.eq("and takes no more frames", run.frames, frames_at_done)

--------------------------------------------------------------------------------
T.group("the manifest records the run")
--------------------------------------------------------------------------------

-- ADR 0013: one flag for the run and no record per pass. What each phase cost
-- is timing_ms, at finer grain than a pass, and which tiles exist is the
-- journal.
local manifest = run.manifest
T.eq("the run is complete", manifest.complete, true)
T.eq("and the key it replaced is gone", manifest.passes, nil)

T.eq("every job timed",
  table.concat({ manifest.timing_ms.presweep, manifest.timing_ms.config,
    manifest.timing_ms.water, manifest.timing_ms.surface }, ","),
  "1,1,3,2")

-- Nothing swept a tile, but the manifest must still carry the key as an array
-- rather than as the empty object an empty Lua table would encode to.
T.eq("tiles is an array", E.json(manifest.tiles), "[]")

--------------------------------------------------------------------------------
T.group("no pass can be switched off")
--------------------------------------------------------------------------------

-- ADR 0011: prepare, hook, mission, done is a walk and not a choice. The switch
-- that used to skip a pass is not a config field any more, and a config still
-- carrying it changes nothing about the order.
run = new_run({
  config = {
    output_dir = "C:/extract", frame_budget_ms = 5,
    passes = { hook = false, mission = false },
  },
})
until_past(run, E.STATE_IDLE, 5)
until_past(run, E.STATE_PREPARE, 5)
T.eq("prepare goes to the hook pass", run.state, E.STATE_HOOK)

-- These phases have no jobs registered in this test, so they cost a frame each
-- and finish. What is being asserted is that they were entered at all.
until_past(run, E.STATE_HOOK, 5)
T.eq("then the mission pass", run.state, E.STATE_MISSION)
T.eq("and the run is not complete part way through", run.manifest.complete, false)

--------------------------------------------------------------------------------
T.group("phases are logged")
--------------------------------------------------------------------------------

logged = {}
run = new_run()
for _ = 1, 20 do
  E.run_frame(run)
  if run.state == E.STATE_DONE then break end
end

local phases = {}
for i = 1, #logged do
  local state = logged[i]:match("^phase (%a+)$")
  if state then
    phases[#phases + 1] = state
  end
end
T.eq("one line per phase change",
  table.concat(phases, " "), "prepare hook mission done")

--------------------------------------------------------------------------------
T.group("job registration")
--------------------------------------------------------------------------------

T.raises("not a phase", function() E.add_job("teardown", job("x", 1)) end,
  "not a phase: teardown")

local added = E.add_job("hook", job("scratch", 1))
T.eq("appended to the phase", E.jobs.hook[#E.jobs.hook], added)
E.jobs.hook[#E.jobs.hook] = nil

T.done()
