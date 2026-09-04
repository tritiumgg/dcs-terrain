-- Offline tests for the DCS callbacks.
--
-- Run from the repository root with a plain lua5.1.
--
-- The callback table is the only part of the hook DCS itself calls, so this is
-- where a fake DCS goes. Beyond onSimulationFrame the callbacks do very
-- little: ADR 0010 took the gating off them, since the sweeps need loaded
-- terrain rather than a mission, so two of the three now only end the idle
-- wait early or write a line for the progress log.
--
-- Which of these four fire, and where, is a per-build measurement. Nothing
-- offline can settle that; it is settled by watching a real DCS.

package.path = "extractor/?.lua;extractor/test/support/?.lua;" .. package.path

local T = require("testing")
local E = require("DcsTerrainExtract")
local FakeFs = require("fakefs")

local now = 0
E.clock = function() return now end
E.now_iso = function() return "2026-09-04T09:12:44Z" end

local logged = {}
E.log = function(message) logged[#logged + 1] = message end

local terrain = nil
E.terrain_id = function() return terrain end

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

local function new_run()
  E.fs = FakeFs.new()
  return E.new_run({
    config = { output_dir = "C:/extract", frame_budget_ms = 5 },
    jobs = {
      prepare = { job("presweep", 1, open_output) },
      hook = { job("config", 1) },
      mission = { job("surface", 40) },
    },
  })
end

--------------------------------------------------------------------------------
T.group("onSimulationFrame drives the run")
--------------------------------------------------------------------------------

terrain = "Caucasus"
local run = new_run()
local cb = E.callbacks(run)

cb.onSimulationFrame()
T.eq("one frame, one phase", run.state, E.STATE_PREPARE)
T.eq("frame counted", run.frames, 1)

--------------------------------------------------------------------------------
T.group("a mission load ends the idle wait early")
--------------------------------------------------------------------------------

-- No callback fires during a mission load, so onMissionLoadEnd is the first
-- frame-adjacent event after one. Waiting out the rest of the sixty frames
-- would be a second of a terrain sitting there loaded.
terrain = nil
run = new_run()
cb = E.callbacks(run)

for _ = 1, 30 do cb.onSimulationFrame() end
T.eq("idle with no terrain", run.state, E.STATE_IDLE)
T.eq("thirty idle frames", run.idle_frames, 30)

terrain = "Caucasus"
cb.onSimulationFrame()
T.eq("still idle between polls", run.state, E.STATE_IDLE)

cb.onMissionLoadEnd()
T.eq("the next frame polls", cb.onSimulationFrame(), nil)
T.eq("and finds the terrain", run.state, E.STATE_PREPARE)

-- Past idle it is not the callback's business: a mission loading over a run
-- that is already sweeping must not reset anything.
local frames = run.frames
cb.onMissionLoadEnd()
T.eq("no effect once out of idle", run.state, E.STATE_PREPARE)
T.eq("and no frame taken", run.frames, frames)

--------------------------------------------------------------------------------
T.group("registration")
--------------------------------------------------------------------------------

-- These tests are a plain interpreter with no DCS in it, which is also what
-- the hook sees when it is loaded by one. It must not raise there.
local ok, why = E.register(new_run())
T.eq("nothing to register with", ok, nil)
T.eq("and it says so", why, "DCS.setUserCallbacks is not available")

local restore = _G.DCS
local registered = nil
_G.DCS = { setUserCallbacks = function(t) registered = t end }

run = new_run()
T.eq("registered", E.register(run), true)
T.eq("with four callbacks", type(registered.onSimulationFrame)
  .. type(registered.onMissionLoadEnd) .. type(registered.onSimulationStart)
  .. type(registered.onSimulationStop),
  "functionfunctionfunctionfunction")

terrain = "Caucasus"
registered.onSimulationFrame()
T.eq("and they drive the run", run.state, E.STATE_PREPARE)

_G.DCS = { setUserCallbacks = "not a function" }
T.eq("a DCS without the call registers nothing", E.register(new_run()), nil)

_G.DCS = restore

--------------------------------------------------------------------------------
T.group("a simulation start ends the idle wait and gates nothing")
--------------------------------------------------------------------------------

-- ADR 0010: the sweeps need terrain, not a mission, so a simulation starting
-- is only another way to learn a terrain is there. It admits no phase.
terrain = nil
run = new_run()
cb = E.callbacks(run)
for _ = 1, 30 do cb.onSimulationFrame() end
T.eq("idle with no terrain", run.state, E.STATE_IDLE)

terrain = "Caucasus"
logged = {}
cb.onSimulationStart()
T.eq("recorded", logged[1], "simulation started")
cb.onSimulationFrame()
T.eq("the next frame polls", run.state, E.STATE_PREPARE)

--------------------------------------------------------------------------------
T.group("a simulation stop is recorded and nothing else")
--------------------------------------------------------------------------------

-- A mission ending may or may not take the terrain with it: back to the menu
-- it goes, back to the editor it stays. The pass that cares tests for terrain
-- on every frame rather than trusting this event.
terrain = "Caucasus"
run = new_run()
cb = E.callbacks(run)
for _ = 1, 6 do cb.onSimulationFrame() end
T.eq("in the server-state pass", run.state, E.STATE_MISSION)

logged = {}
cb.onSimulationStop()
T.eq("recorded", logged[1], "simulation stopped")
T.eq("the pass carries on", run.state, E.STATE_MISSION)

cb.onSimulationFrame()
T.eq("and keeps sweeping", run.state, E.STATE_MISSION)

-- What does end it is the terrain going, which the frame check finds.
terrain = nil
cb.onSimulationFrame()
T.eq("terrain going ends the run", run.state, E.STATE_DONE)
T.eq("pass incomplete", run.manifest.passes.mission.complete, false)

T.done()
