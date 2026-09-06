-- Offline tests for what loading the hook does.
--
-- Run from the repository root with a plain lua5.1.
--
-- Silence is most of the behaviour and is most of what these assert. A hook in
-- Scripts/Hooks/ that nobody has enabled must write no log, build no window and
-- register no callback, because a DCS install carries the file long before its
-- owner wants a run out of it -- and the one thing worse than a hook that does
-- nothing is a hook that does something nobody asked for.

package.path = "extractor/?.lua;extractor/test/support/?.lua;" .. package.path

local T = require("testing")
local E = require("DcsTerrainExtract")
local FakeFs = require("fakefs")

-- Asserted before any fake is installed: requiring this file runs its bootstrap,
-- and every DCS global that reaches for is behind a seam answering nil without
-- one, so a plain interpreter loading the hook gets a module and nothing else.
T.group("loading the module outside DCS starts nothing")
T.eq("no run", E.run, false)
T.eq("and no log", E.log_path, nil)

local SAVED = "C:/saved/DCS"
local CONFIG = SAVED .. "/Config/DcsTerrainExtract.lua"
local LOG = SAVED .. "/Logs/DcsTerrainExtract.log"

local restore = { lfs = _G.lfs, DCS = _G.DCS, log = _G.log }

-- Backslashes and a trailing separator, which is what lfs.writedir hands back.
_G.lfs = { writedir = function() return "C:\\saved\\DCS\\" end }

local said = {}
_G.log = {
  INFO = 64,
  WARNING = 128,
  write = function(_, level, message)
    said[#said + 1] = (level == 128 and "W " or "I ") .. message
  end,
}

local registered = nil
_G.DCS = { setUserCallbacks = function(t) registered = t end }

local fs = nil

-- One load, from scratch. What a bootstrap writes into the module -- the log
-- path, the frame callback -- outlives the call, so a test that did not clear it
-- would be reading the previous one's.
local function boot(text)
  fs = FakeFs.new()
  E.fs = fs
  if text then fs.files[CONFIG] = text end
  E.log_path = nil
  E.on_frame = function() end
  registered = nil
  for i = #said, 1, -1 do said[i] = nil end
  return E.bootstrap()
end

local NO_FRAME = E.on_frame

local function files()
  local n = 0
  for _ in pairs(fs.files) do n = n + 1 end
  return n
end

--------------------------------------------------------------------------------
T.group("no config file at all is the ordinary state, and is silent")
--------------------------------------------------------------------------------

local run, why = boot(nil)
T.eq("nothing to do", run, false)
T.eq("and it says which nothing", why, "no config file")
T.eq("no log path", E.log_path, nil)
T.eq("no callbacks", registered, nil)
T.eq("nothing written anywhere", files(), 0)
T.eq("and nothing said to dcs.log", #said, 0)

--------------------------------------------------------------------------------
T.group("a config that will not load is a user who tried")
--------------------------------------------------------------------------------

-- So it is reported, and to dcs.log alone: whether the hook was enabled is
-- exactly what could not be read, and the progress log is something an enabled
-- run gets.
run, why = boot("return {")
T.eq("no run", run, false)
T.eq("one line", #said, 1)
T.eq("a warning", said[1]:sub(1, 2), "W ")
T.eq("naming the file", said[1]:find(CONFIG, 1, true) ~= nil, true)
T.eq("no progress log", E.log_path, nil)
T.eq("and none written", files(), 1)

--------------------------------------------------------------------------------
T.group("a config that says no is silent")
--------------------------------------------------------------------------------

run, why = boot("return { enabled = false }")
T.eq("nothing to do", run, false)
T.eq("and it says so", why, "not enabled")
T.eq("no log path", E.log_path, nil)
T.eq("no callbacks", registered, nil)
T.eq("only the config on disk", files(), 1)
T.eq("and nothing said", #said, 0)

-- A quoted boolean is the exception, and is why a disabled load reports anything
-- at all. `enabled = "true"` is not enabled, so nothing runs; without the line it
-- is silence, with no window and no clue what went wrong.
run = boot('return { enabled = "true" }')
T.eq("still nothing to do", run, false)
T.eq("but one line", #said, 1)
T.eq("naming the field", said[1]:find("enabled", 1, true) ~= nil, true)
T.eq("and still no progress log", E.log_path, nil)

--------------------------------------------------------------------------------
T.group("an enabled config registers a run")
--------------------------------------------------------------------------------

run = boot('return { enabled = true, output_dir = [[C:\\extract]] }')
T.eq("there is a run", type(run), "table")
T.eq("waiting to be started", run.state, E.STATE_STOPPED)
T.eq("with the directory it was given", run.dir, "C:/extract")
T.eq("the progress log has a path", E.log_path, LOG)
T.eq("and a line in it", fs.files[LOG]:find("hook loaded", 1, true) ~= nil, true)
T.eq("the window is attached", E.on_frame ~= NO_FRAME, true)
T.eq("the callbacks are registered", type(registered), "table")
T.eq("and drive frames", type(registered.onSimulationFrame), "function")
T.eq("with nothing warned about", #said, 0)

--------------------------------------------------------------------------------
T.group("a bad field costs one line in both places, and the run still starts")
--------------------------------------------------------------------------------

run = boot('return { enabled = true, output_dir = [[C:\\extract]], crop = 7 }')
T.eq("there is still a run", type(run), "table")
T.eq("one line to dcs.log", #said, 1)
T.eq("naming the field", said[1]:find("crop", 1, true) ~= nil, true)
T.eq("and the same line in the progress log",
  fs.files[LOG]:find("crop", 1, true) ~= nil, true)
T.eq("with no crop left in the config", run.config.crop, nil)

--------------------------------------------------------------------------------
T.group("no Saved Games under the process is not a failure")
--------------------------------------------------------------------------------

-- Which is every offline test, and every interpreter that is not DCS.
_G.lfs = nil
run, why = boot('return { enabled = true, output_dir = [[C:\\extract]] }')
T.eq("nothing to do", run, false)
T.eq("and it says why", why, "no Saved Games directory")
T.eq("no callbacks", registered, nil)
T.eq("and nothing said", #said, 0)

_G.lfs, _G.DCS, _G.log = restore.lfs, restore.DCS, restore.log

T.done()
