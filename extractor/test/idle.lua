-- Offline tests for the idle state.
--
-- Run from the repository root with a plain lua5.1.
--
-- Idle is where the hook spends most of a DCS session: it loads on every start
-- and DCS may sit at the main menu for hours before a map opens, so what is
-- being tested is mostly that a run does nothing, cheaply, until it should.

package.path = "extractor/?.lua;extractor/test/support/?.lua;" .. package.path

local T = require("testing")
local E = require("DcsTerrainExtract")
local FakeFs = require("fakefs")

E.now_iso = function() return "2026-09-04T09:12:44Z" end

-- Captured before the fake replaces it, because the last group tests the real
-- one against a plain interpreter with no DCS in it.
local real_terrain_id = E.terrain_id

local logged = {}
E.log = function(message) logged[#logged + 1] = message end

-- No terrain and no polls counted unless a test says otherwise.
local terrain, polls = nil, 0
E.terrain_id = function()
  polls = polls + 1
  return terrain
end

-- Idle reaches no job, so a run needs none to test it.
local function new_run()
  E.fs = FakeFs.new()
  return E.new_run({
    config = { output_dir = "C:/extract" },
    jobs = { prepare = {}, hook = {} },
  })
end

--------------------------------------------------------------------------------
T.group("idle waits for a terrain")
--------------------------------------------------------------------------------

terrain, polls = nil, 0
local run = new_run()
T.eq("starts idle", run.state, E.STATE_IDLE)

for _ = 1, 610 do E.run_frame(run) end
T.eq("still idle with no map open", run.state, E.STATE_IDLE)
T.eq("counted every idle frame", run.idle_frames, 610)
T.eq("no manifest", run.manifest, nil)

-- The poll is two DCS calls and idle can last hours, so it is not made every
-- frame: 610 frames is ten seconds of DCS and eleven polls.
T.eq("polled once every sixty frames", polls, 11)

--------------------------------------------------------------------------------
T.group("a terrain is picked up at the next poll")
--------------------------------------------------------------------------------

-- Polls land on idle frames 1, 61, 121 and so on, so the last one before here
-- was 601 and the next is 661.
terrain = "Caucasus"
for _ = 1, 50 do E.run_frame(run) end
T.eq("not polled between polls", run.state, E.STATE_IDLE)
T.eq("fifty frames later", run.idle_frames, 660)

T.eq("prepare on the next poll", E.run_frame(run), E.STATE_PREPARE)
T.eq("polled on idle frame 661", run.idle_frames, 661)
T.eq("theatre recorded", run.identity.theatre, "Caucasus")

-- Which callbacks fire where is a per-build measurement, so the frame count
-- reached in idle is the only evidence the hook has that it was given frames
-- at the main menu at all.
T.eq("idle frame count logged", logged[1],
  "terrain Caucasus after 661 idle frames")

-- Idle is behind it now, and the frames the phases take are not idle frames.
E.run_frame(run)
T.eq("no longer counting idle frames", run.idle_frames, 661)

--------------------------------------------------------------------------------
T.group("a map already open costs no idle frames")
--------------------------------------------------------------------------------

terrain, polls = "Caucasus", 0
run = new_run()
T.eq("first frame polls", E.run_frame(run), E.STATE_PREPARE)
T.eq("one idle frame", run.idle_frames, 1)
T.eq("one poll", polls, 1)

--------------------------------------------------------------------------------
T.group("the terrain seam answers nil with no DCS around it")
--------------------------------------------------------------------------------

-- The real seam, not the fake: these tests run in a plain interpreter, where
-- require("terrain") fails and the global is absent. A hook that raised here
-- would take the whole GameGUI state down on a machine where the module has
-- moved.
T.eq("no terrain module, no id", real_terrain_id(), nil)

local restore = { _G.terrain, package.loaded.terrain }
_G.terrain = { GetTerrainConfig = function(key) return key == "id" and "Syria" end }
package.loaded.terrain = _G.terrain
T.eq("reads the id from the module table", real_terrain_id(), "Syria")

-- Lua 5.1 require returns true, not the module, when a C module installs
-- itself as a global and returns nothing.
package.loaded.terrain = true
T.eq("falls back to the global", real_terrain_id(), "Syria")

_G.terrain = { GetTerrainConfig = function() error("no terrain loaded") end }
T.eq("a raising call is no id", real_terrain_id(), nil)

_G.terrain = { GetTerrainConfig = "not a function" }
T.eq("a module without the call is no id", real_terrain_id(), nil)

_G.terrain, package.loaded.terrain = restore[1], restore[2]

T.done()
