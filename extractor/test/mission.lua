-- Offline tests for the server-state pass and for the manifest advancing
-- across a whole run.
--
-- Run from the repository root with a plain lua5.1.
--
-- ADR 0010: this pass reaches the mission-scripting state, and what those
-- calls need is loaded terrain rather than a running mission. So the gate is
-- the same terrain check idle polls with, made every frame, and there is no
-- waiting for a mission and no mission clock. It keeps the name "mission
-- pass" because `passes.mission` is a frozen manifest key.

package.path = "extractor/?.lua;extractor/test/support/?.lua;" .. package.path

local T = require("testing")
local E = require("DcsTerrainExtract")
local FakeFs = require("fakefs")

local now = 0
E.clock = function() return now end
E.now_iso = function() return "2026-09-04T09:12:44Z" end

local logged = {}
E.log = function(message) logged[#logged + 1] = message end

local terrain, polls = "Caucasus", 0
E.terrain_id = function()
  polls = polls + 1
  return terrain
end

local grid = E.grid_from_rect({ min_x = 0, min_z = 0, max_x = 51200, max_z = 51200 }, 50, 256)

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
      mission = { job("surface", 40) },
    },
  }
  for k, v in pairs(edit or {}) do
    opts[k] = v
  end
  local run = E.new_run(opts)
  run.fs = fs
  E.start(run)
  return run
end

local function until_past(run, state, limit)
  local frames = 0
  while run.state == state and frames < (limit or 1000) do
    E.run_frame(run)
    frames = frames + 1
  end
  return frames
end

-- Runs the phases up to the server-state pass, which is where the tests below
-- start.
local function up_to_mission(run)
  until_past(run, E.STATE_IDLE, 5)
  until_past(run, E.STATE_PREPARE, 5)
  until_past(run, E.STATE_HOOK, 5)
end

--------------------------------------------------------------------------------
T.group("the pass runs with a terrain and no mission")
--------------------------------------------------------------------------------

-- The measurement behind ADR 0010: with the Mission Editor open on Caucasus
-- and no mission running, land.getHeight, land.getSurfaceType and
-- world.searchObjects all answer correctly. Nothing here waits for one.
terrain = "Caucasus"
local run = new_run()
up_to_mission(run)
T.eq("in the pass", run.state, E.STATE_MISSION)

until_past(run, E.STATE_MISSION, 20)
T.eq("and it finished", run.state, E.STATE_DONE)
T.eq("surface swept", run.manifest.timing_ms.surface, 40)
T.eq("pass complete", run.manifest.passes.mission.complete, true)
T.eq("and stamped", run.manifest.passes.mission.finished_at, "2026-09-04T09:12:44Z")

--------------------------------------------------------------------------------
T.group("phases run in order through both passes")
--------------------------------------------------------------------------------

run = new_run()
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
T.group("no pass can be switched off")
--------------------------------------------------------------------------------

-- ADR 0011: the two passes are the two Lua states the sweeps call from, not a
-- choice a user was ever in a position to make usefully, so both always run. A
-- config carrying the switch that used to skip one changes nothing.
run = new_run({
  config = {
    output_dir = "C:/extract", frame_budget_ms = 5,
    passes = { hook = true, mission = false },
  },
})
up_to_mission(run)

T.eq("the mission pass ran anyway", run.state, E.STATE_MISSION)
T.eq("hook pass complete", run.manifest.passes.hook.complete, true)
T.eq("and the mission pass has started",
  run.manifest.passes.mission.started_at, "2026-09-04T09:12:44Z")

--------------------------------------------------------------------------------
T.group("terrain going away ends the pass")
--------------------------------------------------------------------------------

-- Terrain unloads when DCS returns to the main menu, and a server-state call
-- with no terrain under it crashes DCS. This pass runs for tens of minutes, so
-- the check is made every frame rather than once when the pass starts.
terrain = "Caucasus"
run = new_run()
up_to_mission(run)
T.eq("in the pass", run.state, E.STATE_MISSION)
T.eq("part way through it", run.manifest.timing_ms.surface, nil)

logged = {}
terrain = nil
E.run_frame(run)
T.eq("the run is over", run.state, E.STATE_DONE)
T.eq("pass incomplete", run.manifest.passes.mission.complete, false)
T.eq("and it says why", logged[1], "terrain unloaded during the mission pass")

-- Nothing swept after the terrain went, so no chunk was sent into a state with
-- no terrain behind it.
T.eq("surface never finished", run.manifest.timing_ms.surface, nil)

-- The final manifest is on disk, not only in memory.
local saved = E.read_manifest(run.dir)
T.eq("hook pass complete on disk", saved.passes.hook.complete, true)
T.eq("mission pass incomplete on disk", saved.passes.mission.complete, false)

--------------------------------------------------------------------------------
T.group("the terrain check is made on every frame of the pass")
--------------------------------------------------------------------------------

terrain = "Caucasus"
run = new_run()
up_to_mission(run)
polls = 0
for _ = 1, 5 do E.run_frame(run) end
T.eq("one poll per frame", polls, 5)

--------------------------------------------------------------------------------
T.group("the manifest advances over ten thousand frames")
--------------------------------------------------------------------------------

-- The saved manifest is what a resume reads, so what matters is not that the
-- run object is right at the end but that every write to the file was right at
-- the time it was made. A run driven frame by frame must never write a
-- manifest that claims less than one already on disk claimed.
--
-- The sweeps here are 49 100 steps against a budget that fits five a frame, so
-- the run needs nearly all of the ten thousand frames driven below. What is
-- exercised is a hook budgeting a session of real work, not a handful of jobs
-- that fit in one frame each.
terrain = "Caucasus"
run = new_run({
  jobs = {
    prepare = { job("presweep", 2000, open_output) },
    hook = { job("config", 100), job("water", 20000), job("height", 15000) },
    mission = { job("surface", 12000) },
  },
})

local writes = 0
local save = E.save
E.save = function(r)
  local ok = save(r)
  if ok then
    writes = writes + 1
  end
  return ok
end

local highest, ordered, frames = 0, true, 0
for _ = 1, 10000 do
  E.run_frame(run)
  frames = frames + 1
  local saved_now = E.read_manifest(run.dir)
  if saved_now then
    local reached = 0
    if saved_now.passes.hook.started_at ~= E.JSON_NULL then reached = 1 end
    if saved_now.timing_ms.water then reached = 2 end
    if saved_now.passes.hook.complete then reached = 3 end
    if saved_now.passes.mission.started_at ~= E.JSON_NULL then reached = 4 end
    if saved_now.passes.mission.complete then reached = 5 end
    if reached < highest then
      ordered = false
    end
    highest = reached
  end
end
E.save = save

T.eq("the saved manifest only ever advances", ordered, true)
T.eq("it reached both passes complete", highest, 5)
T.eq("the run finished inside ten thousand frames", run.state, E.STATE_DONE)
T.eq("all ten thousand frames driven", frames, 10000)

-- Five sweeps and four phase changes, less the change into prepare, which
-- saves nothing because no job has made a manifest yet. Eight writes over
-- nearly two thousand frames is the point of the rule: the manifest is not
-- written per tile.
T.eq("one write per sweep and per phase change", writes, 8)

T.eq("every sweep timed",
  table.concat({ run.manifest.timing_ms.presweep, run.manifest.timing_ms.config,
    run.manifest.timing_ms.water, run.manifest.timing_ms.height,
    run.manifest.timing_ms.surface }, ","),
  "2000,100,20000,15000,12000")

-- Nothing is left half-renamed after ten thousand frames of rewriting it.
T.eq("no manifest left aside", run.fs.files["C:/extract/manifest.json.prev"], nil)

T.done()
