-- Offline tests for the config file the window owns.
--
-- Run from the repository root with a plain lua5.1.
--
-- The property that matters is the round trip. The window fills its controls
-- from this file and writes it back, so a value that does not survive being
-- written and read comes back at the next start as a problem the user did not
-- cause: a crop centre rounded on the way through the file moves the extract by
-- tens of metres, and nothing downstream can tell that from a crop they meant.

package.path = "extractor/?.lua;extractor/test/support/?.lua;" .. package.path

local T = require("testing")
local E = require("DcsTerrainExtract")
local FakeFs = require("fakefs")

local PATH = "C:/saved/Config/DcsTerrainExtract.lua"

local function with_fs()
  local fs = FakeFs.new()
  E.fs = fs
  return fs
end

-- Writes text straight into the fake, bypassing write_config, so a read can be
-- tested against a file write_config would have refused to produce.
local function given(text)
  local fs = with_fs()
  fs.files[PATH] = text
  return fs
end

--------------------------------------------------------------------------------
T.group("reading tells the four failures apart")
--------------------------------------------------------------------------------

-- Each of these asks something different of the user, which is why they are not
-- one message: no file is the ordinary state of a fresh install, a syntax error
-- has a line number to go to, and a missing `return` is a file that looks right.

with_fs()
local value, why = E.read_config(PATH)
T.eq("no file at all is nil", value, nil)
T.eq("and says so", why, PATH .. ": no such file")

given("return {")
value, why = E.read_config(PATH)
T.eq("a chunk that will not compile is nil", value, nil)
T.eq("and the message names the file", why:find(PATH, 1, true) ~= nil, true)

-- Indexing a nil local rather than calling error(), because error is a global
-- and the chunk has none -- which is the next group's point.
given("local t = nil\nreturn t.x")
value, why = E.read_config(PATH)
T.eq("a chunk that raises is nil", value, nil)
T.eq("and the message names the file", why:find(PATH, 1, true) ~= nil, true)
T.eq("and carries the raise", why:find("nil value", 1, true) ~= nil, true)

given("return 7")
value, why = E.read_config(PATH)
T.eq("a chunk returning a number is nil", value, nil)
T.eq("and says what it returned", why, PATH .. ": does not return a table: number")

given("local x = 1")
value, why = E.read_config(PATH)
T.eq("a chunk returning nothing is nil", value, nil)
T.eq("and calls that nil", why, PATH .. ": does not return a table: nil")

--------------------------------------------------------------------------------
T.group("the chunk runs with nothing in scope")
--------------------------------------------------------------------------------

-- A config is three values, not a program. This is the whole reason the chunk
-- gets an empty environment rather than the hook's own globals.

given("return { enabled = os.time() > 0 }")
value, why = E.read_config(PATH)
T.eq("os is not reachable", value, nil)
T.eq("and the raise says so", why:find("nil value", 1, true) ~= nil, true)

given("return { enabled = true, output_dir = tostring(1) }")
value, why = E.read_config(PATH)
T.eq("neither is tostring", value, nil)

given("return { enabled = true }")
value, why = E.read_config(PATH)
T.eq("a plain table still reads", type(value), "table")
T.eq("with its value", value.enabled, true)
T.eq("and no reason", why, nil)

--------------------------------------------------------------------------------
T.group("writing refuses what would not read back")
--------------------------------------------------------------------------------

-- Every refusal here is a message the window is already showing against the
-- control it belongs to, because both come from field_problem.

with_fs()
local ok, refused = E.write_config(PATH, { enabled = true })
T.eq("no output_dir is refused", ok, nil)
T.eq("with the field's own line", refused,
  "output_dir is not set, and there is no default for it")

ok, refused = E.write_config(PATH, { enabled = true, output_dir = "" })
T.eq("an empty output_dir is refused", ok, nil)
T.eq("with the field's own line", refused, "output_dir is not a non-empty string: ")

ok, refused = E.write_config(PATH,
  { enabled = true, output_dir = "C:/e", crop = { x = 1, z = 2 } })
T.eq("a crop missing its radius is refused", ok, nil)
T.eq("with the crop's own line", refused,
  "crop.radius_m is not a finite number: nil")

ok, refused = E.write_config(PATH, "not a table")
T.eq("a config that is not a table is refused", ok, nil)
T.eq("and says what it was", refused, "config is not a table: string")

-- enabled is written as `config.enabled == true`, which turns anything that is
-- not true into false. A caller holding 1 or "yes" would have the hook silently
-- switched off and no way to see why, so it is refused instead.
ok, refused = E.write_config(PATH, { enabled = 1, output_dir = "C:/e" })
T.eq("a truthy enabled that is not a boolean is refused", ok, nil)
T.eq("with the field's own line", refused, "enabled is not true or false: 1")

ok, refused = E.write_config(PATH, { enabled = "yes", output_dir = "C:/e" })
T.eq("and so is a string", ok, nil)
T.eq("with the field's own line", refused, "enabled is not true or false: yes")

local fs = with_fs()
E.write_config(PATH, { enabled = true })
T.eq("a refused write leaves no file", fs.files[PATH], nil)
T.eq("and no temporary either", fs.files[PATH .. ".tmp"], nil)

--------------------------------------------------------------------------------
T.group("a config survives the round trip")
--------------------------------------------------------------------------------

-- The centre is the measured Caucasus reading the Mission Editor shows under
-- the cursor, kept to the digit it arrives with. %g would round it to six
-- significant figures and move the crop about forty metres east.
local CENTRE_X = -549428.57142857
local CENTRE_Z = 715714.28571429

with_fs()
local wrote = {
  enabled = true,
  output_dir = "C:/extracts/caucasus-50m",
  crop = { x = CENTRE_X, z = CENTRE_Z, radius_m = 5000 },
}
T.eq("the write succeeds", E.write_config(PATH, wrote), true)

local read = E.read_config(PATH)
T.eq("enabled round trips", read.enabled, true)
T.eq("output_dir round trips", read.output_dir, "C:/extracts/caucasus-50m")
T.eq("the crop centre x is exact", read.crop.x, CENTRE_X)
T.eq("the crop centre z is exact", read.crop.z, CENTRE_Z)
T.eq("the radius round trips", read.crop.radius_m, 5000)

-- And the written table is still what the run validates against, so the window
-- and the run cannot disagree about what was saved.
local validated, problems = E.validate_config(read)
T.eq("what was written validates clean", table.concat(problems, "\n"), "")
T.eq("with the same output_dir", validated.output_dir, "C:/extracts/caucasus-50m")

with_fs()
wrote = { enabled = true, output_dir = "C:/extracts/no-crop" }
E.write_config(PATH, wrote)
read = E.read_config(PATH)
T.eq("an absent crop stays absent", read.crop, nil)

-- Both settings of the switch, because the write is `config.enabled == true`
-- and a writer that hard-coded either one would round trip the other wrongly.
with_fs()
E.write_config(PATH, { enabled = false, output_dir = "C:/e" })
read = E.read_config(PATH)
T.eq("a false enabled round trips as false", read.enabled, false)

with_fs()
E.write_config(PATH, { enabled = nil, output_dir = "C:/e" })
read = E.read_config(PATH)
T.eq("an absent enabled is written as false", read.enabled, false)

-- Only the three fields are written. A config carrying a key from the older
-- sixteen-field table must not have it preserved into the next start, where it
-- would be reported as an unrecognised key the user never typed.
with_fs()
E.write_config(PATH,
  { enabled = true, output_dir = "C:/e", cell_size = 50, passes = { hook = true } })
read = E.read_config(PATH)
T.eq("an unknown key is dropped", read.cell_size, nil)
T.eq("whatever its shape", read.passes, nil)
local _, extra_problems = E.validate_config(read)
T.eq("so what was written still validates clean",
  table.concat(extra_problems, "\n"), "")

--------------------------------------------------------------------------------
T.group("a written file says it is written")
--------------------------------------------------------------------------------

-- A generated file a reader takes for a hand-edited one is a lost edit.

fs = with_fs()
E.write_config(PATH, { enabled = true, output_dir = "C:/e" })
local text = fs.files[PATH]
T.eq("it opens with a comment", text:sub(1, 2), "--")
T.eq("it says the window writes it", text:find("overwritten", 1, true) ~= nil, true)
T.eq("it says enabled is the exception", text:find("enabled", 1, true) ~= nil, true)
T.eq("and it ends in a newline", text:sub(-1), "\n")

-- The header is a comment block, so the return has to come after it and on its
-- own line, or the file is a comment and reads back as nothing.
T.eq("the return survives the header", text:find("\nreturn {\n", 1, true) ~= nil, true)

-- Written in the order a reader meets them in the window, which is also the
-- order the fields are declared in.
fs = with_fs()
E.write_config(PATH,
  { enabled = true, output_dir = "C:/e", crop = { x = 1, z = 2, radius_m = 3 } })
text = fs.files[PATH]
local at_enabled = text:find("enabled = ", 1, true)
local at_output = text:find("output_dir = ", 1, true)
local at_crop = text:find("crop = ", 1, true)
T.eq("enabled comes first", at_enabled < at_output, true)
T.eq("then output_dir", at_output < at_crop, true)

--------------------------------------------------------------------------------
T.group("a path in a double-quoted string survives")
--------------------------------------------------------------------------------

-- %q is what makes this true. A Windows path is the case that breaks a naive
-- writer, and it is the one every user will type.

with_fs()
E.write_config(PATH, { enabled = true, output_dir = "C:/a b/c'd" })
read = E.read_config(PATH)
T.eq("a space and a quote round trip", read.output_dir, "C:/a b/c'd")

with_fs()
E.write_config(PATH, { enabled = true, output_dir = "C:/a\\b" })
read = E.read_config(PATH)
T.eq("a backslash round trips", read.output_dir, "C:/a\\b")

--------------------------------------------------------------------------------
T.group("the path is discovered, never recorded")
--------------------------------------------------------------------------------

T.eq("the name is under Config", E.CONFIG_NAME, "Config/DcsTerrainExtract.lua")

local restore = _G.lfs
_G.lfs = nil
T.eq("no Saved Games, no path", E.config_path(), nil)

_G.lfs = { writedir = function() return "C:\\saved\\DCS\\" end }
T.eq("otherwise it hangs off writedir", E.config_path(),
  "C:/saved/DCS/Config/DcsTerrainExtract.lua")
_G.lfs = restore

T.done()
