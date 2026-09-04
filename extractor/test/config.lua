-- Offline tests for config validation.
--
-- Run from the repository root with a plain lua5.1.
--
-- What is being tested is a counting property, not a wording one: every bad
-- field costs exactly one line and leaves its default behind. A field that
-- reported twice, or that reported once and left the bad value in the table,
-- would put a value the user did not choose into an hour-long sweep.

package.path = "extractor/?.lua;extractor/test/support/?.lua;" .. package.path

local T = require("testing")
local E = require("DcsTerrainExtract")

-- The smallest config that validates clean. Every case below is a copy of it
-- with one field changed, so a failure names one field and not a combination.
local function good()
  return {
    enabled = true,
    output_dir = "C:/extracts/caucasus-50m",
    omit_sea_tiles = true,
  }
end

local function with(name, value)
  local config = good()
  config[name] = value
  return config
end

-- T.eq compares with ==, so two tables are never equal. Problems are compared as
-- one joined string, which also makes a failure readable.
local function joined(problems)
  return table.concat(problems, "\n")
end

--------------------------------------------------------------------------------
T.group("a good config validates clean")
--------------------------------------------------------------------------------

local config, problems = E.validate_config(good())
T.eq("no problems", joined(problems), "")
T.eq("enabled", config.enabled, true)
T.eq("output_dir kept", config.output_dir, "C:/extracts/caucasus-50m")
T.eq("omit_sea_tiles kept", config.omit_sea_tiles, true)

-- false is a value and not an absence, so it must survive rather than be
-- replaced by the default the way a missing field is.
config, problems = E.validate_config(with("omit_sea_tiles", false))
T.eq("false survives", config.omit_sea_tiles, false)
T.eq("and is not a problem", joined(problems), "")

--------------------------------------------------------------------------------
T.group("a disabled hook validates nothing and says nothing")
--------------------------------------------------------------------------------

-- Not true means the hook does nothing at all, so there is nothing to check and
-- nobody to tell. An absent config file means the same thing, which is why an
-- absent `enabled` is not a problem either.
config, problems = E.validate_config({})
T.eq("an empty config is silent", joined(problems), "")
T.eq("and disabled", config.enabled, false)

config, problems = E.validate_config({ enabled = false, nonsense = 1 })
T.eq("a disabled config's other keys are not checked", joined(problems), "")

config, problems = E.validate_config({ enabled = "yes", nonsense = 1 })
T.eq("a non-boolean enabled is one problem",
  joined(problems), "enabled is not true or false: yes")
T.eq("and validation stops there", config.nonsense, nil)
T.eq("disabled", config.enabled, false)

config, problems = E.validate_config("not a table")
T.eq("a config that is not a table is one problem",
  joined(problems), "config is not a table: string")
T.eq("disabled", config.enabled, false)

--------------------------------------------------------------------------------
T.group("a bad value is one line and its default")
--------------------------------------------------------------------------------

-- A table's tostring holds its address, so cases are named by index: a test
-- name that changes between runs is one nobody can grep a log for.
local BOOLEANS = { "true", 1, 0, {} }
for i = 1, #BOOLEANS do
  config, problems = E.validate_config(with("omit_sea_tiles", BOOLEANS[i]))
  T.eq("omit_sea_tiles case " .. i .. ": one line", #problems, 1)
  T.eq("omit_sea_tiles case " .. i .. ": naming the field",
    problems[1]:find("omit_sea_tiles is not true or false", 1, true), 1)
  T.eq("omit_sea_tiles case " .. i .. ": default in its place",
    config.omit_sea_tiles, true)
end

--------------------------------------------------------------------------------
T.group("output_dir is the one field with no default")
--------------------------------------------------------------------------------

config, problems = E.validate_config({ enabled = true })
T.eq("a missing output_dir is one line",
  joined(problems), "output_dir is not set, and there is no default for it")
T.eq("and leaves nothing to run with", config.output_dir, nil)
T.eq("but the rest still defaults", config.omit_sea_tiles, true)

local BAD_PATHS = { "", 7, false, {} }
for i = 1, #BAD_PATHS do
  config, problems = E.validate_config(with("output_dir", BAD_PATHS[i]))
  T.eq("output_dir case " .. i .. ": one line", #problems, 1)
  T.eq("output_dir case " .. i .. ": nothing to run with",
    config.output_dir, nil)
end

-- The Windows trap. "C:\temp\new" in a double-quoted Lua string already holds a
-- tab and a newline by the time it gets here, so the value is not the path the
-- user typed and turning separators around cannot recover it.
config, problems = E.validate_config(with("output_dir", "C:\temp\new"))
T.eq("a path holding a tab from a backslash escape is one line", #problems, 1)
T.eq("and says what happened",
  problems[1]:find("backslash escape", 1, true) ~= nil, true)
T.eq("and leaves nothing to run with", config.output_dir, nil)

config, problems = E.validate_config(with("output_dir", "C:\\extracts\\caucasus"))
T.eq("backslashes become forward slashes",
  config.output_dir, "C:/extracts/caucasus")
T.eq("which is not a problem", joined(problems), "")

--------------------------------------------------------------------------------
T.group("a key that is not a config field is one line and is dropped")
--------------------------------------------------------------------------------

config, problems = E.validate_config(with("outputdir", "C:/typo"))
T.eq("one line", joined(problems), "outputdir is not a config field")
T.eq("and the key does not survive", config.outputdir, nil)

-- ADR 0011 under test: the fields that left the table are reported like any
-- other typo rather than quietly tolerated, so a config carried over from the
-- specified table tells its owner exactly which lines to delete.
local old = good()
old.passes = { hook = true, mission = true }
old.allow_helipads = false
old.crs = { proj4 = "+proj=tmerc" }
old.terrain_dir = "Caucasus"
old.towns_lua = "C:/x/towns.lua"
old.nodes_lua = "C:/x/nodes.lua"
old.authored_bounds_m = { min_x = 0, min_z = 0, max_x = 1, max_z = 1 }

-- Seven at once, which is also what shows that problems accumulate across
-- fields rather than stopping at the first.
config, problems = E.validate_config(old)
T.eq("every dropped field is reported, in a stable order", joined(problems),
  "allow_helipads is not a config field\n"
  .. "authored_bounds_m is not a config field\n"
  .. "crs is not a config field\n"
  .. "nodes_lua is not a config field\n"
  .. "passes is not a config field\n"
  .. "terrain_dir is not a config field\n"
  .. "towns_lua is not a config field")
T.eq("and the fields that stayed still came through",
  config.output_dir, "C:/extracts/caucasus-50m")

T.done()
