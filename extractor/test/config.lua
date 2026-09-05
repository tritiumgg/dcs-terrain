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
T.group("output_dir is the one field with no default")
--------------------------------------------------------------------------------

config, problems = E.validate_config({ enabled = true })
T.eq("a missing output_dir is one line",
  joined(problems), "output_dir is not set, and there is no default for it")
T.eq("and leaves nothing to run with", config.output_dir, nil)

-- A table's tostring holds its address, so cases are named by index: a test name
-- that changes between runs is one nobody can grep a log for.
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
T.group("the six that became constants")
--------------------------------------------------------------------------------

config = E.validate_config(good())

-- ADR 0011: these are constants, not fields, so they are absent from a
-- validated config however the file was written. The unknown-key group proves
-- the other half -- that writing one down is reported rather than ignored.
T.eq("cell_size is not a field", config.cell_size, nil)
T.eq("tile_size is not a field", config.tile_size, nil)
T.eq("omit_sea_tiles is not a field", config.omit_sea_tiles, nil)
T.eq("frame_budget_ms is not a field", config.frame_budget_ms, nil)
T.eq("road_seed_spacing is not a field", config.road_seed_spacing, nil)
T.eq("road_seed_neighbours is not a field", config.road_seed_neighbours, nil)

-- The constants they became are still readable, because the manifest records
-- them and the sweeps read them.
T.eq("cell_size is a constant", E.CELL_SIZE, 50)
T.eq("tile_size is a constant", E.TILE_SIZE, 256)
T.eq("omit_sea_tiles is a constant", E.OMIT_SEA_TILES, true)
T.eq("frame_budget_ms is a constant", E.FRAME_BUDGET_MS, 5)
T.eq("road_seed_spacing is a constant", E.ROAD_SEED_SPACING, 1000)
T.eq("road_seed_neighbours is a constant", E.ROAD_SEED_NEIGHBOURS, 4)

--------------------------------------------------------------------------------
T.group("the crop is a centre and a radius")
--------------------------------------------------------------------------------

-- Optional, so absent stays absent instead of gaining an area nobody asked for.
T.eq("an absent crop stays absent", config.crop, nil)

local CROPS = {
  { false, "crop is not a centre and a radius" },
  { 5, "crop is not a centre and a radius" },
  { { x = 0, z = 0 }, "crop.radius_m is not a finite" },
  { { x = 0, z = 0, radius_m = "5000" }, "crop.radius_m is not a finite" },
  { { x = 0, radius_m = 5000 }, "crop.z is not a finite" },
  { { x = 0, z = 0, radius_m = 5000, y = 0 },
    "crop has an unknown key: y" },
  { { x = 0, z = 0, radius_m = 0 }, "crop.radius_m is not a positive" },
  { { x = 0, z = 0, radius_m = -5000 }, "crop.radius_m is not a positive" },
  -- Every member wrong at once is still one line. The first bad member is
  -- enough to send the reader to the right place, and three lines would make
  -- the per-field count meaningless.
  { { x = "a", z = "b", radius_m = "c" }, "crop.x is not a finite" },
}

for i = 1, #CROPS do
  local value, wants = CROPS[i][1], CROPS[i][2]
  config, problems = E.validate_config(with("crop", value))
  T.eq("crop case " .. i .. ": one line", #problems, 1)
  T.eq("crop case " .. i .. ": naming what is wrong",
    problems[1]:find(wants, 1, true), 1)
  T.eq("crop case " .. i .. ": no crop left behind", config.crop, nil)
end

-- Kept in the user's own vocabulary rather than converted here, so the window
-- fills its controls from the same table the run starts from.
local crop = { x = -284887, z = 683859, radius_m = 5000 }
config, problems = E.validate_config(with("crop", crop))
T.eq("a good crop validates", joined(problems), "")
T.eq("and is kept as it was written", config.crop, crop)

--------------------------------------------------------------------------------
T.group("crop_box converts a centre and a radius to the box the grid wants")
--------------------------------------------------------------------------------

T.eq("no crop, no box", E.crop_box(nil), nil)

-- The radius is half the side, so 5000 is the 10 x 10 km crop X10 asks for
-- around the Kutaisi reference point.
local box = E.crop_box(crop)
T.eq("the box is the centre plus and minus the radius on both axes",
  E.json(box),
  E.json({ min_x = -289887, min_z = 678859, max_x = -279887, max_z = 688859 }))
T.eq("which is ten kilometres on a side", box.max_x - box.min_x, 10000)
T.eq("on both axes", box.max_z - box.min_z, 10000)

-- The box is what grid_from_rect takes, so the two have to fit without a shim
-- between them. 201 and not 200: the crop's edges do not land on cell
-- boundaries, and the grid snaps outward, so a cell the crop reaches partway
-- into is inside. A centre read off the map will almost never be a multiple of
-- 50, so this is the normal case rather than an edge one.
local grid = E.grid_from_rect(box, E.CELL_SIZE, E.TILE_SIZE)
T.eq("and the grid it plans covers the crop, snapped outward",
  grid.height .. "x" .. grid.width, "201x201")

-- A centre and radius that do land on cell boundaries give the exact extent,
-- which is what says the extra cell above is the snapping and not an off-by-one.
local aligned = E.crop_box({ x = 100000, z = 200000, radius_m = 5000 })
local exact = E.grid_from_rect(aligned, E.CELL_SIZE, E.TILE_SIZE)
T.eq("an aligned crop is exactly 200 cells square",
  exact.height .. "x" .. exact.width, "200x200")

T.raises("a bad crop raises rather than returning a wrong box",
  function() return E.crop_box({ x = 0, z = 0 }) end, "radius_m")

--------------------------------------------------------------------------------
T.group("a key that is not a config field is one line and is dropped")
--------------------------------------------------------------------------------

config, problems = E.validate_config(with("outputdir", "C:/typo"))
T.eq("one line", joined(problems), "outputdir is not a config field")
T.eq("and the key does not survive", config.outputdir, nil)

-- ADR 0011 under test: every field that left the table is reported like any
-- other typo rather than quietly tolerated, whether it was dropped outright, is
-- derived from the install now, or became a constant. A config carried over from
-- the specified sixteen tells its owner exactly which lines to delete.
local old = good()
old.passes = { hook = true, mission = true }
old.allow_helipads = false
old.crs = { proj4 = "+proj=tmerc" }
old.terrain_dir = "Caucasus"
old.towns_lua = "C:/x/towns.lua"
old.nodes_lua = "C:/x/nodes.lua"
old.authored_bounds_m = { min_x = 0, min_z = 0, max_x = 1, max_z = 1 }
old.cell_size = 50
old.tile_size = 256
old.omit_sea_tiles = true
old.frame_budget_ms = 5
old.road_seed_spacing = 1000
old.road_seed_neighbours = 4
old.crop_m = { min_x = 0, min_z = 0, max_x = 1, max_z = 1 }

-- Fourteen at once, which is also what shows that problems accumulate across
-- fields rather than stopping at the first. crop_m is in here because the crop
-- survives under a different name and a different shape: a centre and a radius,
-- not a box.
config, problems = E.validate_config(old)
T.eq("every field that left is reported, in a stable order", joined(problems),
  "allow_helipads is not a config field\n"
  .. "authored_bounds_m is not a config field\n"
  .. "cell_size is not a config field\n"
  .. "crop_m is not a config field\n"
  .. "crs is not a config field\n"
  .. "frame_budget_ms is not a config field\n"
  .. "nodes_lua is not a config field\n"
  .. "omit_sea_tiles is not a config field\n"
  .. "passes is not a config field\n"
  .. "road_seed_neighbours is not a config field\n"
  .. "road_seed_spacing is not a config field\n"
  .. "terrain_dir is not a config field\n"
  .. "tile_size is not a config field\n"
  .. "towns_lua is not a config field")
T.eq("and the fields that stayed still came through",
  config.output_dir, "C:/extracts/caucasus-50m")

--------------------------------------------------------------------------------
T.group("one field can be checked on its own")
--------------------------------------------------------------------------------

-- The window checks a control the moment it changes, and it must get the same
-- wording validate_config would have produced from the same value. Anything
-- else and the message on screen disagrees with the message in the log.

T.eq("a good path is nil", E.field_problem("output_dir", "C:/e"), nil)
T.eq("an empty one is the field's line",
  E.field_problem("output_dir", ""), "output_dir is not a non-empty string: ")
T.eq("an absent one says there is no default",
  E.field_problem("output_dir", nil),
  "output_dir is not set, and there is no default for it")

T.eq("an absent crop is nil, because a crop is optional",
  E.field_problem("crop", nil), nil)
T.eq("a good crop is nil",
  E.field_problem("crop", { x = 1, z = 2, radius_m = 3 }), nil)
T.eq("a crop missing a member is the crop's line",
  E.field_problem("crop", { x = 1, z = 2 }),
  "crop.radius_m is not a finite number: nil")

T.eq("an absent enabled is nil", E.field_problem("enabled", nil), nil)
T.eq("a false enabled is nil, because false is a value",
  E.field_problem("enabled", false), nil)
T.eq("a non-boolean enabled is a line",
  E.field_problem("enabled", "yes"), "enabled is not true or false: yes")

T.eq("a name that is not a field says so",
  E.field_problem("cell_size", 50), "cell_size is not a config field")

-- Every way a crop can be wrong, asked one field at a time. These already run
-- through here from validate_config, but the window asks the question this way
-- round -- one control, one value -- and that is the call being pinned.
T.eq("a crop that is not a table",
  E.field_problem("crop", 5), "crop is not a centre and a radius: 5")
T.eq("a crop missing x",
  E.field_problem("crop", { z = 2, radius_m = 3 }),
  "crop.x is not a finite number: nil")
T.eq("a crop missing z",
  E.field_problem("crop", { x = 1, radius_m = 3 }),
  "crop.z is not a finite number: nil")
T.eq("a crop with a member that is not a number",
  E.field_problem("crop", { x = "1", z = 2, radius_m = 3 }),
  "crop.x is not a finite number: 1")
T.eq("a crop with an unknown key",
  E.field_problem("crop", { x = 1, z = 2, radius_m = 3, extra = 4 }),
  "crop has an unknown key: extra")
T.eq("a crop with no area",
  E.field_problem("crop", { x = 1, z = 2, radius_m = 0 }),
  "crop.radius_m is not a positive number: 0")

-- A control character in a path, which is the message that quotes the value.
--
-- The tab comes back through %q as a tab, not as \9: Lua 5.1 escapes only the
-- quote, the backslash, a newline and a zero. So the quoted value is as
-- invisible as it was in the config, and the words are what tell the user what
-- they are looking at. That is the reason the message says "control character"
-- rather than showing one.
T.eq("a path holding a tab says which",
  E.field_problem("output_dir", "C:\tmp"),
  "output_dir contains a control character, which is usually a backslash "
  .. "escape in a double-quoted path: \"C:\tmp\"")

-- Total: it is public now, so it answers for any name rather than raising on
-- one the caller got wrong.
T.eq("a nil name is not a field", E.field_problem(nil, "x"),
  "nil is not a config field")
T.eq("a number name is not a field", E.field_problem(7, "x"),
  "7 is not a config field")

-- The same wording, reached both ways, across every field rather than one.
-- This is the assertion that would catch someone reintroducing a second copy of
-- a checker in validate_config, and one input would only catch it for one field.
local same_line_cases = {
  { "output_dir", "" },
  { "output_dir", nil },
  { "crop", 5 },
  { "crop", { x = 1, z = 2 } },
  { "enabled", "yes" },
}
for i = 1, #same_line_cases do
  local name, value = same_line_cases[i][1], same_line_cases[i][2]
  local config = good()
  config[name] = value
  if name == "output_dir" and value == nil then
    config.output_dir = nil
  end
  local _, both = E.validate_config(config)
  T.eq("both routes agree, case " .. i, E.field_problem(name, value), both[1])
end

--------------------------------------------------------------------------------
T.group("every problem names the field it belongs to")
--------------------------------------------------------------------------------

-- The window puts a message beside the control it is about, so it needs to know
-- which control. tags is parallel to problems and is read as
-- `for i = 1, #problems` -- never #tags, which has holes and is undefined.

local _, probs, tags = E.validate_config(good())
T.eq("a clean config tags nothing", #probs, 0)

_, probs, tags = E.validate_config(with("output_dir", ""))
T.eq("one problem", #probs, 1)
T.eq("tagged with its field", tags[1], "output_dir")

_, probs, tags = E.validate_config(with("crop", { x = 1, z = 2 }))
T.eq("a crop problem", #probs, 1)
T.eq("is tagged crop", tags[1], "crop")

_, probs, tags = E.validate_config({ enabled = "yes" })
T.eq("a bad enabled is one problem", #probs, 1)
T.eq("tagged enabled", tags[1], "enabled")

-- Two fields at once, so the pairing is shown to hold position by position
-- rather than by accident of there being one of each.
local both_bad = good()
both_bad.output_dir = ""
both_bad.crop = { x = 1, z = 2 }
_, probs, tags = E.validate_config(both_bad)
T.eq("two problems", #probs, 2)
T.eq("first is output_dir", tags[1], "output_dir")
T.eq("second is crop", tags[2], "crop")
T.eq("and they are in field order", probs[1]:sub(1, 10), "output_dir")

-- The holes. An unrecognised key belongs to no control, so there is nothing to
-- put a message beside and the tag is absent rather than invented.
_, probs, tags = E.validate_config(with("cell_size", 50))
T.eq("an unknown key is one problem", #probs, 1)
T.eq("belonging to no control", tags[1], nil)

_, probs, tags = E.validate_config("not a table")
T.eq("a config that is not a table is one problem", #probs, 1)
T.eq("belonging to no control either", tags[1], nil)

-- A hole followed by a tag, which is the arrangement that makes #tags wrong:
-- here #problems is 2 and the last tag is absent.
local mixed = good()
mixed.output_dir = ""
mixed.cell_size = 50
_, probs, tags = E.validate_config(mixed)
T.eq("two problems again", #probs, 2)
T.eq("the field one is tagged", tags[1], "output_dir")
T.eq("the unknown key is not", tags[2], nil)

-- Four problems with the holes interleaved rather than trailing, which is the
-- arrangement an index that drifted by one would still line up on by accident
-- when there are only two.
local four = { enabled = "yes", crop = { x = 1, z = 2 }, aaa = 1, zzz = 2 }
_, probs, tags = E.validate_config(four)
T.eq("a non-boolean enabled stops at one problem", #probs, 1)
T.eq("tagged enabled", tags[1], "enabled")

four.enabled = true
_, probs, tags = E.validate_config(four)
T.eq("four problems", #probs, 4)
T.eq("output_dir first, tagged", tags[1], "output_dir")
T.eq("crop second, tagged", tags[2], "crop")
T.eq("then the sorted unknown keys, untagged", tags[3], nil)
T.eq("both of them", tags[4], nil)
T.eq("and the lines are in that order", probs[3], "aaa is not a config field")
T.eq("sorted", probs[4], "zzz is not a config field")

--------------------------------------------------------------------------------
T.group("the window is told which controls to build")
--------------------------------------------------------------------------------

local fields = E.config_fields()
T.eq("two of them", #fields, 2)
T.eq("output_dir first", fields[1].name, "output_dir")
T.eq("and it is a path", fields[1].kind, "path")
T.eq("and it is required", fields[1].optional, false)
T.eq("crop second", fields[2].name, "crop")
T.eq("and it is a crop", fields[2].kind, "crop")
T.eq("and it is optional", fields[2].optional, true)

-- Fresh tables, like layers() and table_files(): a window that renamed a field
-- in the list it was handed must not rename it for the next caller.
local mine = E.config_fields()
mine[1].name = "clobbered"
T.eq("editing one does not reach the spec", E.config_fields()[1].name, "output_dir")

-- enabled is not among them. It is read before the list is, and there is no
-- control for it: a hook that is not enabled has no window to show one in.
for i = 1, #fields do
  T.eq("field " .. i .. " is not enabled", fields[i].name ~= "enabled", true)
end

T.done()
