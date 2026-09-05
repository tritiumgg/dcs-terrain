-- Offline tests for the two log destinations.
--
-- Run from the repository root with a plain lua5.1.
--
-- The progress log is driven for real here, over the fake filesystem every
-- other test uses. What makes that possible is that the path is a value rather
-- than a function to swap: nil means log nowhere, so the other fourteen test
-- files reach these same code paths and touch no disk.
--
-- dcs.log is driven against a fake `log` global, set and restored the way
-- callbacks.lua does for `DCS`.

package.path = "extractor/?.lua;extractor/test/support/?.lua;" .. package.path

local T = require("testing")
local E = require("DcsTerrainExtract")
local FakeFs = require("fakefs")

E.now_iso = function() return "2026-09-04T09:12:44Z" end

local LOG = "C:/Saved Games/DCS/Logs/DcsTerrainExtract.log"

local function fresh_fs()
  local fs = FakeFs.new()
  E.fs = fs
  return fs
end

--------------------------------------------------------------------------------
T.group("no path means no file")
--------------------------------------------------------------------------------

local fs = fresh_fs()
E.log_path = nil
E.log("something happened")
T.eq("nothing was opened", next(fs.files), nil)

--------------------------------------------------------------------------------
T.group("a path means one appended line per call")
--------------------------------------------------------------------------------

fs = fresh_fs()
E.log_path = LOG
E.log("phase prepare")
T.eq("the file exists after the first line", fs.files[LOG] ~= nil, true)
T.eq("stamped, and ending in a bare LF",
  fs.files[LOG], "2026-09-04T09:12:44Z phase prepare\n")

E.log("phase hook")
T.eq("the second line appends rather than replacing", fs.files[LOG],
  "2026-09-04T09:12:44Z phase prepare\n2026-09-04T09:12:44Z phase hook\n")

-- Append mode across runs is the point: a resumed run continues the extract the
-- previous one began, so its lines belong after the earlier ones. Nothing in
-- the hook ever truncates this file.
E.log_path = nil
E.log_path = LOG
E.log("phase prepare")
T.eq("a second run keeps what the first wrote",
  select(2, fs.files[LOG]:gsub("\n", "\n")), 3)

-- Not a string is still one line: a logger that raised on a bad argument would
-- take the run down over something with no bearing on the extract.
E.log(42)
T.eq("a non-string is written, not raised on",
  fs.files[LOG]:find("2026-09-04T09:12:44Z 42\n", 1, true) ~= nil, true)

--------------------------------------------------------------------------------
T.group("dcs.log is silent without the global")
--------------------------------------------------------------------------------

local restore_log = _G.log
_G.log = nil
T.eq("no log global, no write", E.dcs_log("INFO", "phase prepare"), false)

_G.log = { INFO = 64 }
T.eq("a log global without write is the same", E.dcs_log("INFO", "x"), false)

--------------------------------------------------------------------------------
T.group("dcs.log takes a subsystem, a level and a message")
--------------------------------------------------------------------------------

local written = {}
_G.log = {
  INFO = 64,
  WARNING = 16,
  write = function(subsystem, level, message)
    written[#written + 1] = subsystem .. "|" .. tostring(level) .. "|" .. message
  end,
}

E.dcs_log("INFO", "phase hook")
T.eq("subsystem, level and message, in that order",
  written[1], "DcsTerrainExtract|64|phase hook")

E.dcs_log("WARNING", "output_dir is not set")
T.eq("the warning level is the other constant",
  written[2], "DcsTerrainExtract|16|output_dir is not set")

-- A level this file does not know is a caller's bug, not a user's, so it
-- raises where everything else here stays quiet.
T.raises("an unknown level raises",
  function() E.dcs_log("TRACE", "x") end, "not a level")

-- A log.write that throws must not take the run with it.
_G.log = { INFO = 64, write = function() error("boom") end }
T.eq("a failing write is swallowed", E.dcs_log("INFO", "x"), false)

--------------------------------------------------------------------------------
T.group("a warning goes to both destinations")
--------------------------------------------------------------------------------

fs = fresh_fs()
E.log_path = LOG
written = {}
_G.log = {
  WARNING = 16,
  write = function(subsystem, level, message)
    written[#written + 1] = subsystem .. "|" .. tostring(level) .. "|" .. message
  end,
}

E.warn("manifest write failed: disk full")
T.eq("the progress log has it", fs.files[LOG],
  "2026-09-04T09:12:44Z manifest write failed: disk full\n")
T.eq("and so does dcs.log, as a warning",
  written[1], "DcsTerrainExtract|16|manifest write failed: disk full")

--------------------------------------------------------------------------------
T.group("a phase change reaches both destinations, and a tile reaches one")
--------------------------------------------------------------------------------

fs = fresh_fs()
E.log_path = LOG
written = {}
_G.log = {
  INFO = 64,
  WARNING = 16,
  write = function(_, level, message)
    written[#written + 1] = tostring(level) .. "|" .. message
  end,
}

-- One job per phase, each finishing in a step, plus a line of its own so there
-- is per-tile-shaped traffic to check dcs.log does not carry.
local function job(name)
  return {
    name = name,
    start = function()
      return function()
        E.log(name .. " tile 0_0")
        return E.DONE
      end
    end,
  }
end

local run = E.new_run({
  config = { output_dir = "C:/extract", frame_budget_ms = 5 },
  jobs = { prepare = {}, hook = { job("water") }, mission = {} },
})
E.start(run)
E.terrain_id = function() return "Caucasus" end
for _ = 1, 20 do
  E.run_frame(run)
  if run.state == E.STATE_DONE then break end
end

T.eq("every phase change is one INFO line",
  table.concat(written, " "),
  "64|phase idle 64|phase prepare 64|phase hook 64|phase mission 64|phase done")

T.eq("the tile line reached the progress log",
  fs.files[LOG]:find("water tile 0_0", 1, true) ~= nil, true)
T.eq("and not dcs.log",
  table.concat(written, " "):find("tile", 1, true), nil)

--------------------------------------------------------------------------------
T.group("saved_games_dir is discovered, and absent without lfs")
--------------------------------------------------------------------------------

local restore_lfs = _G.lfs
_G.lfs = nil
T.eq("no lfs, no directory", E.saved_games_dir(), nil)

_G.lfs = { writedir = function() return "C:\\Users\\x\\Saved Games\\DCS\\" end }
T.eq("backslashes turned around, trailing separator kept",
  E.saved_games_dir(), "C:/Users/x/Saved Games/DCS/")

-- M.join handles the trailing separator, so the log path is a plain join.
T.eq("and the log path hangs off it",
  E.join(E.saved_games_dir(), E.LOG_NAME),
  "C:/Users/x/Saved Games/DCS/Logs/DcsTerrainExtract.log")

_G.lfs = { writedir = function() error("no writedir here") end }
T.eq("a writedir that throws is not a crash", E.saved_games_dir(), nil)

_G.lfs = { writedir = function() return 7 end }
T.eq("and neither is one that answers nonsense", E.saved_games_dir(), nil)

_G.lfs = restore_lfs
_G.log = restore_log
E.log_path = nil

T.done()
