-- Offline tests for the widget seam and its failure latch.
--
-- Run from the repository root with a plain lua5.1.
--
-- The property is one-directional: a widget call that fails costs the window and
-- nothing else. An extract that differed because the display broke would be
-- worse than no display.

package.path = "extractor/?.lua;extractor/test/support/?.lua;" .. package.path

local T = require("testing")
local E = require("DcsTerrainExtract")
local FakeFs = require("fakefs")

local now = 0
E.clock = function() return now end
E.now_iso = function() return "2026-09-04T09:12:44Z" end
E.log = function() end
E.terrain_id = function() return "Caucasus" end

local warned = {}
E.warn = function(message) warned[#warned + 1] = message end

-- Module state, so a group that did not clear it would test the previous one.
local function unlatch()
  E.ui_failed, E.ui_failure = false, nil
  for i = #warned, 1, -1 do warned[i] = nil end
end

local grid = E.grid_from_rect({ min_x = 0, min_z = 0, max_x = 51200, max_z = 51200 }, 50, 256)

local function job(name, steps, on_start)
  return {
    name = name,
    start = function(run)
      if on_start then on_start(run) end
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
  local run = E.new_run({
    config = { output_dir = "C:/extract", frame_budget_ms = 5 },
    jobs = {
      prepare = { job("presweep", 1, open_output) },
      hook = { job("water", 3) },
      mission = { job("surface", 2) },
    },
  })
  run.fs = E.fs
  return run
end

--------------------------------------------------------------------------------
T.group("a call that works is passed through")
--------------------------------------------------------------------------------

unlatch()
T.eq("the result comes back", E.ui(function(a, b) return a + b end, 2, 3), 5)
T.eq("with its arguments", E.ui(function(a) return a end, "x"), "x")
T.eq("nothing latched", E.ui_failed, false)

local obj = { seen = nil }
function obj:remember(v) self.seen = v return "ok" end
T.eq("a method answers too", E.ui_method(obj, "remember", 7), "ok")
T.eq("and got its self", obj.seen, 7)

--------------------------------------------------------------------------------
T.group("a call that fails latches, once")
--------------------------------------------------------------------------------

unlatch()
T.eq("a raise answers nil", E.ui(function() error("boom", 0) end), nil)
T.eq("and latches", E.ui_failed, true)
T.eq("naming what went wrong", E.ui_failure:find("boom", 1, true) ~= nil, true)
T.eq("and says so once", #warned, 1)

local called = false  -- a second line per failure would bury the first
for _ = 1, 100 do
  E.ui(function() called = true end)
end
T.eq("later calls do not run", called, false)
T.eq("and answer nil", E.ui(function() return 1 end), nil)
T.eq("and are silent", #warned, 1)

unlatch()
T.eq("something that is not a function is nil", E.ui("setText"), nil)
T.eq("and latches", E.ui_failed, true)
T.eq("saying what it got", E.ui_failure:find("string", 1, true) ~= nil, true)

unlatch()
T.eq("a method that raises is nil",
  E.ui_method({ go = function() error("nope", 0) end }, "go"), nil)
T.eq("and latches", E.ui_failed, true)
T.eq("naming the method", E.ui_failure:find("go", 1, true) ~= nil, true)

--------------------------------------------------------------------------------
T.group("a method on nothing is loud once and quiet after")
--------------------------------------------------------------------------------

-- Before the latch it is this file's own bug -- a class name typed wrong, and a
-- window skipping half its widgets would look built. After, it is a failed
-- constructor and there is nothing left to say.
unlatch()
T.eq("nil object answers nil", E.ui_method(nil, "setText", "x"), nil)
T.eq("and latches", E.ui_failed, true)
T.eq("naming the method", E.ui_failure:find("setText", 1, true) ~= nil, true)

local warnings_before = #warned
T.eq("a second one answers nil too", E.ui_method(nil, "setBounds", 1, 2), nil)
T.eq("and says nothing more", #warned, warnings_before)

--------------------------------------------------------------------------------
T.group("no widget library is not a failure")
--------------------------------------------------------------------------------

-- This interpreter, and any DCS whose widget library moved. The seam answers
-- nil, no window is built, nothing latches: nothing is broken.
unlatch()
T.eq("no class", E.gui.widget("Window"), nil)
T.eq("no skin", E.gui.skin("windowSkin"), nil)
T.eq("and nothing latched", E.ui_failed, false)
T.eq("and nothing was said", #warned, 0)

--------------------------------------------------------------------------------
T.group("a window that raises never reaches the run")
--------------------------------------------------------------------------------

-- What routing both attachment points through the latch buys: a run finishes and
-- writes its manifest with something attached that fails on every call.
unlatch()
local run = new_run()
local frame_attempts, phase_attempts = 0, 0
E.on_frame = function() frame_attempts = frame_attempts + 1 error("frame", 0) end
E.on_phase = function() phase_attempts = phase_attempts + 1 error("phase", 0) end

local cb = E.callbacks(run)
E.start(run)
for _ = 1, 200 do
  cb.onSimulationFrame()
  if run.state == E.STATE_DONE then break end
end

T.eq("the run finished", run.state, E.STATE_DONE)
T.eq("the window latched", E.ui_failed, true)
-- Start announces idle before the first frame, so the phase point fires and the
-- frame point never gets a turn. Counted apart: a total of one would also hold
-- if they ran in the other order.
T.eq("the phase point was tried once", phase_attempts, 1)
T.eq("and the frame point never, the latch being set", frame_attempts, 0)

-- A raise while reporting one must not climb out either.
E.ui_failed, E.ui_failure = false, nil
local real_warn = E.warn
E.warn = function() error("the log is broken too", 0) end
T.eq("a failure whose report fails is still nil", E.ui(function() error("x", 0) end), nil)
T.eq("and still latches", E.ui_failed, true)
E.warn = real_warn

-- The one that would have hurt: on_phase is announced from inside the phase
-- change and before the save, so an unguarded raise loses the manifest.
T.eq("the manifest was still written",
  run.fs.files["C:/extract/manifest.json"] ~= nil, true)
T.eq("and both passes completed", run.manifest.passes.hook.complete, true)
T.eq("including the second", run.manifest.passes.mission.complete, true)

E.on_frame = function() end
E.on_phase = function() end

T.done()
