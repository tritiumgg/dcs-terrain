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

-- Unstarted, for the tests that are about the state a run waits in.
local function new_stopped_run(edit)
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

local function new_run(edit)
  local run = new_stopped_run(edit)
  E.start(run)
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
T.group("the manifest records the pass")
--------------------------------------------------------------------------------

local manifest = run.manifest
T.eq("hook complete", manifest.passes.hook.complete, true)
T.eq("hook started", manifest.passes.hook.started_at, "2026-09-04T09:12:44Z")
T.eq("hook finished", manifest.passes.hook.finished_at, "2026-09-04T09:12:44Z")

-- Four one-millisecond steps of hook work fit one five-millisecond frame, so
-- the whole pass is one frame.
T.eq("hook pass took one frame", manifest.passes.hook.frames, 1)

T.eq("mission complete", manifest.passes.mission.complete, true)
T.eq("mission started", manifest.passes.mission.started_at, "2026-09-04T09:12:44Z")
T.eq("mission finished", manifest.passes.mission.finished_at, "2026-09-04T09:12:44Z")

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
T.eq("hook pass complete", run.manifest.passes.hook.complete, true)

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
-- idle is in the sequence because Start announces it the same way every other
-- state change is announced. It was absent before only because a run began
-- there, with nothing to announce.
T.eq("one line per phase change",
  table.concat(phases, " "), "idle prepare hook mission done")

--------------------------------------------------------------------------------
T.group("job registration")
--------------------------------------------------------------------------------

T.raises("not a phase", function() E.add_job("teardown", job("x", 1)) end,
  "not a phase: teardown")

local added = E.add_job("hook", job("scratch", 1))
T.eq("appended to the phase", E.jobs.hook[#E.jobs.hook], added)
E.jobs.hook[#E.jobs.hook] = nil

--------------------------------------------------------------------------------
T.group("a run begins stopped and waits for Start")
--------------------------------------------------------------------------------

run = new_stopped_run()
T.eq("starts stopped", run.state, E.STATE_STOPPED)

-- A stopped frame is paid sixty times a second for as long as DCS sits there.
for _ = 1, 100 do E.run_frame(run) end
T.eq("still stopped after a hundred frames", run.state, E.STATE_STOPPED)
T.eq("and none of them counted", run.frames, 0)
T.eq("no terrain was polled for", run.idle_frames, 0)
T.eq("and nothing was written", next(run.fs.files), nil)

T.eq("Start says it started", E.start(run), true)
T.eq("and the run is looking for terrain", run.state, E.STATE_IDLE)

-- Pressing it twice is a user pressing it twice, not a bug worth raising over.
T.eq("Start again does nothing", E.start(run), false)
T.eq("and leaves the state alone", run.state, E.STATE_IDLE)

--------------------------------------------------------------------------------
T.group("Stop halts a run, whatever it was doing")
--------------------------------------------------------------------------------

run = new_stopped_run()
E.start(run)
until_past(run, E.STATE_IDLE, 5)
until_past(run, E.STATE_PREPARE, 5)
T.eq("sweeping", run.state, E.STATE_HOOK)

local stop_frames = run.frames
T.eq("Stop says it stopped", E.stop(run), true)
T.eq("and the run is stopped", run.state, E.STATE_STOPPED)
T.eq("the queue is dropped", run.queue, nil)
T.eq("the manifest is on disk", run.fs.files["C:/extract/manifest.json"] ~= nil, true)

for _ = 1, 50 do E.run_frame(run) end
T.eq("frames do nothing", run.state, E.STATE_STOPPED)
T.eq("and are not counted", run.frames, stop_frames)

T.eq("Stop again does nothing", E.stop(run), false)

-- A run stopped in idle has nothing to save, and save() saying so by returning
-- false is not something going wrong: there is no work to come back to.
run = new_stopped_run()
E.start(run)
E.terrain_id = function() return nil end
E.run_frame(run)
T.eq("waiting for terrain", run.state, E.STATE_IDLE)
T.eq("with no manifest", run.manifest, nil)
T.eq("Stop still stops", E.stop(run), true)
T.eq("and reaches stopped", run.state, E.STATE_STOPPED)
T.eq("having written nothing", run.fs.files["C:/extract/manifest.json"], nil)
E.terrain_id = function() return "Caucasus" end

run = new_stopped_run()
E.start(run)
for _ = 1, 40 do
  E.run_frame(run)
  if run.state == E.STATE_DONE then break end
end
T.eq("done", run.state, E.STATE_DONE)
T.eq("Stop does nothing to it", E.stop(run), false)
T.eq("and it stays done", run.state, E.STATE_DONE)

--------------------------------------------------------------------------------
T.group("Start after Stop goes back the way a restart would")
--------------------------------------------------------------------------------

-- One route into a run, on every Start and not only after a relaunch. The
-- journal makes it affordable: written tiles are skipped.
run = new_stopped_run()
E.start(run)
until_past(run, E.STATE_IDLE, 5)
until_past(run, E.STATE_PREPARE, 5)
T.eq("in the hook pass", run.state, E.STATE_HOOK)
E.stop(run)

logged = {}
T.eq("Start works from stopped again", E.start(run), true)
T.eq("and goes to idle, not back to the pass", run.state, E.STATE_IDLE)

until_past(run, E.STATE_IDLE, 5)
T.eq("then prepare, the same as a fresh run", run.state, E.STATE_PREPARE)

local resumed = {}
for i = 1, #logged do
  local state = logged[i]:match("^phase (%a+)$")
  if state then
    resumed[#resumed + 1] = state
  end
end
T.eq("announced as ordinary phase changes",
  table.concat(resumed, " "), "idle prepare")

-- Nothing is reset by a Stop: the manifest records the directory, not the try.
T.eq("frames carry across the stop", run.frames > 0, true)

-- And it has to finish, which is the point of restarting the pipeline.
for _ = 1, 60 do
  E.run_frame(run)
  if run.state == E.STATE_DONE then break end
end
T.eq("the restarted run finishes", run.state, E.STATE_DONE)

-- Stamped once, on the first entry: re-stamping would leave a finished pass
-- saying it started afterwards.
local stamps = 0
E.now_iso = function() stamps = stamps + 1 return string.format("T%02d", stamps) end
run = new_stopped_run()
E.start(run)
for _ = 1, 40 do
  E.run_frame(run)
  if run.state == E.STATE_DONE then break end
end
local first_started = run.manifest.passes.hook.started_at
local first_finished = run.manifest.passes.hook.finished_at
E.enter(run, E.STATE_HOOK)
T.eq("re-entering a pass keeps its first start",
  run.manifest.passes.hook.started_at, first_started)
T.eq("so it never starts after it finished",
  run.manifest.passes.hook.started_at < first_finished, true)
E.now_iso = function() return "2026-09-04T09:12:44Z" end
T.eq("both passes complete", run.manifest.passes.hook.complete, true)
T.eq("including the second", run.manifest.passes.mission.complete, true)

--------------------------------------------------------------------------------
T.group("the window is told about states and about frames")
--------------------------------------------------------------------------------

-- Two attachment points: a phase change is rare, and a frame is the tick a
-- window redraws on, arriving while the run is stopped and after it is done.

local seen_phases = {}
local frame_ticks = 0
E.on_phase = function(state) seen_phases[#seen_phases + 1] = state end
E.on_frame = function() frame_ticks = frame_ticks + 1 end

run = new_stopped_run()
local cb = E.callbacks(run)

for _ = 1, 5 do cb.onSimulationFrame() end
T.eq("the window ticks while the run is stopped", frame_ticks, 5)
T.eq("with no phase change to report", #seen_phases, 0)

E.start(run)
T.eq("Start is a phase change", seen_phases[1], E.STATE_IDLE)

for _ = 1, 40 do
  cb.onSimulationFrame()
  if run.state == E.STATE_DONE then break end
end
T.eq("every state was reported",
  table.concat(seen_phases, " "), "idle prepare hook mission done")

local ticks_at_done = frame_ticks
for _ = 1, 10 do cb.onSimulationFrame() end
T.eq("and the window still ticks once the run is done",
  frame_ticks, ticks_at_done + 10)


E.on_phase = function() end
E.on_frame = function() end

T.done()
