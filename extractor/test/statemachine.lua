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
T.eq("starts in prepare", run.state, E.STATE_PREPARE)

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

T.eq("prepare to hook to done", table.concat(seen, " "), "prepare hook done")

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

-- There is no mission pass yet, and a pass that never ran keeps the false its
-- fresh manifest started with. That is already what the hook is asked to write
-- for a mission pass it does not run.
T.eq("mission pass incomplete", manifest.passes.mission.complete, false)
T.eq("and never started", manifest.passes.mission.started_at, E.JSON_NULL)

T.eq("every job timed",
  table.concat({ manifest.timing_ms.presweep, manifest.timing_ms.config,
    manifest.timing_ms.water }, ","),
  "1,1,3")

-- Nothing swept a tile, but the manifest must still carry the key as an array
-- rather than as the empty object an empty Lua table would encode to.
T.eq("tiles is an array", E.json(manifest.tiles), "[]")

--------------------------------------------------------------------------------
T.group("a disabled pass is skipped")
--------------------------------------------------------------------------------

run = new_run({
  config = {
    output_dir = "C:/extract", frame_budget_ms = 5,
    passes = { hook = false },
  },
})
until_past(run, E.STATE_PREPARE, 5)
T.eq("prepare straight to done", run.state, E.STATE_DONE)
T.eq("hook pass incomplete", run.manifest.passes.hook.complete, false)
T.eq("config never ran", run.manifest.timing_ms.config, nil)

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
T.eq("one line per phase change", table.concat(phases, " "), "prepare hook done")

--------------------------------------------------------------------------------
T.group("job registration")
--------------------------------------------------------------------------------

T.raises("not a phase", function() E.add_job("teardown", job("x", 1)) end,
  "not a phase: teardown")

local added = E.add_job("hook", job("scratch", 1))
T.eq("appended to the phase", E.jobs.hook[#E.jobs.hook], added)
E.jobs.hook[#E.jobs.hook] = nil

T.done()
