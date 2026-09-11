-- Offline tests for the grid job: the rectangle an extract covers, the
-- manifest that records it, and what a run does with a directory that
-- already holds one.
--
-- Run from the repository root with a plain lua5.1.
--
-- The theatre here is a fake install laid out the way a real one is, so the
-- identity job runs for real ahead of the grid job and the two are tested as
-- the prepare phase the machine actually walks. The bounds are answered by
-- the seam, because they come from the terrain module and there is none.

package.path = "extractor/?.lua;extractor/test/support/?.lua;" .. package.path

local T = require("testing")
local E = require("DcsTerrainExtract")
local FakeFs = require("fakefs")

E.now_iso = function() return "2026-09-04T09:12:44Z" end
E.on_phase = function() end
E.on_frame = function() end

local logged = {}
E.log = function(message) logged[#logged + 1] = message end

local function log_has(text)
  for i = 1, #logged do
    if logged[i]:find(text, 1, true) then
      return true
    end
  end
  return false
end

E.terrain_id = function() return "Synth" end

-- 70 x 70 km of bounds, in the kilometers DCS gives and the meters ED makes
-- of them. A test that wants no bounds sets these to nil.
local bounds_km = { sw = { -30, -45 }, ne = { 40, 25 } }
local bounds_m = { min_x = -30000, min_z = -45000, max_x = 40000, max_z = 25000 }
E.terrain_bounds_km = function() return bounds_km end
E.terrain_bounds = function() return bounds_m end

local INSTALL = "C:/DCS"

local function header(payload)
  local bytes = { 2, 0, 0, 0, 48, 0, 0, 0 }
  for _ = 1, 8 do
    bytes[#bytes + 1] = payload % 256
    payload = math.floor(payload / 256)
  end
  return string.char(unpack(bytes))
end

local function build(version)
  return '{ "version": "' .. version .. '", "timestamp": "00000000-000000" }'
end

local function new_install()
  local fs = FakeFs.new()
  fs.cwd = "C:\\DCS"
  fs.files[INSTALL .. "/autoupdate.cfg"] = build("0.0.0.0")
  local synth = INSTALL .. "/Mods/terrains/Synth"
  fs.files[synth .. "/entry.lua"] = "theatre = {\n\t['id'] = \"Synth\";\n}\n"
  fs.files[synth .. "/Surface/Synth.surface5"] = header(1048576) .. string.rep("s", 100)
  fs.files[synth .. "/roads/Synth.rn4"] = header(2097152) .. string.rep("r", 100)
  fs.files[synth .. "/Scenes/Synth.scn5"] = header(524288) .. string.rep("c", 100)
  return fs
end

local CROP = { x = 0, z = 0, radius_m = 5000 }

-- A stopped run over the real prepare jobs, or over `prepare` when given, in
-- a fresh fake install unless one is handed in.
local function new_run(config, prepare, fs)
  fs = fs or new_install()
  E.fs = fs
  local run = E.new_run({
    config = config,
    jobs = { prepare = prepare or E.jobs.prepare, hook = {}, mission = {} },
  })
  run.fs = fs
  return run
end

-- Frames until the run stops or finishes.
local function drive(run)
  E.start(run)
  for _ = 1, 60 do
    E.run_frame(run)
    if run.state == E.STATE_DONE or run.state == E.STATE_STOPPED then
      break
    end
  end
  return run.state
end

local function manifest_on(fs, dir)
  local text = fs.files[(dir or "C:/extract") .. "/manifest.json"]
  return text and E.decode(text) or nil
end

--------------------------------------------------------------------------------
T.group("the real prepare jobs are identity, presweep, grid")
--------------------------------------------------------------------------------

T.eq("three jobs", #E.jobs.prepare, 3)
T.eq("identity first", E.jobs.prepare[1].name, "identity")
T.eq("then the pre-sweep", E.jobs.prepare[2].name, "presweep")
T.eq("then the grid", E.jobs.prepare[3].name, "grid")

--------------------------------------------------------------------------------
T.group("a crop run plans its grid from the box and knows no authored rectangle")
--------------------------------------------------------------------------------

local run = new_run({ output_dir = "C:/extract", crop = CROP })
T.eq("reaches done", drive(run), E.STATE_DONE)
T.eq("with a manifest on the run", run.manifest ~= nil, true)

local written = manifest_on(run.fs)
T.eq("and one on disk", written ~= nil, true)
T.eq("origin x is the box snapped to a cell", written.grid.origin_x, -5000)
T.eq("origin z likewise", written.grid.origin_z, -5000)
T.eq("200 cells north", written.grid.height, 200)
T.eq("200 cells east", written.grid.width, 200)
T.eq("at 50 m", written.grid.cell_size, 50)
T.eq("in tiles of 256", written.grid.tile_size, 256)
T.eq("the crop is the box", E.json(written.crop_m),
  '{"max_x":5000,"max_z":5000,"min_x":-5000,"min_z":-5000}')
-- ADR 0026: nothing is read from a theatre file, so a crop run has no
-- authored rectangle on any theatre, and ADR 0009 has both keys null.
T.eq("no authored rectangle", written.authored_bounds_m, E.JSON_NULL)
T.eq("and no source for one", written.authored_bounds_source, E.JSON_NULL)
T.eq("the bounds as DCS gave them", E.json(written.bounds_km),
  '{"ne":[40,25],"sw":[-30,-45]}')
T.eq("the theatre", written.theatre, "Synth")
T.eq("the build off the install", written.dcs_build, "0.0.0.0")
T.eq("the fingerprint off the install", written.terrain_fingerprint.rn4.payload_size, 2097152)
T.eq("no tiles yet", #written.tiles, 0)
T.eq("both prepare jobs are timed", written.timing_ms.grid ~= nil and written.timing_ms.identity ~= nil, true)
T.eq("nothing is journalled", next(run.done), nil)
T.eq("and the entries agree", #run.entries, 0)
T.eq("the tile directories exist", run.fs.files["C:/extract/tiles/water"], FakeFs.DIR)
T.eq("the log says the rectangle is unknown", log_has("authored rectangle unknown"), true)
T.eq("and names the crop", log_has("crop x -5000..5000 z -5000..5000"), true)

--------------------------------------------------------------------------------
T.group("a pre-sweep run plans its grid from what the pre-sweep found")
--------------------------------------------------------------------------------

-- The pre-sweep is a later task; here a job before the grid stands in for
-- it by leaving its rectangle on the run.
local RECT = { min_x = -20000, min_z = -30000, max_x = 20000, max_z = 10000 }
local function fake_presweep(run)
  run.presweep_bounds = RECT
  return function() return E.DONE end
end
logged = {}
run = new_run({ output_dir = "C:/extract" },
  { E.identity_job, { name = "presweep", start = fake_presweep }, E.grid_job })
T.eq("reaches done", drive(run), E.STATE_DONE)
written = manifest_on(run.fs)
T.eq("the rectangle is the pre-sweep's", E.json(written.authored_bounds_m), E.json(RECT))
T.eq("and says so", written.authored_bounds_source, "presweep")
T.eq("no crop", written.crop_m, E.JSON_NULL)
T.eq("the grid covers it", written.grid.height, 800)
T.eq("the log names the source", log_has("authored rectangle from the pre-sweep"), true)

--------------------------------------------------------------------------------
T.group("a whole-map run measures its rectangle once")
--------------------------------------------------------------------------------

-- The real pre-sweep over a fake theatre bumpy everywhere: every lattice cell
-- is authored, and the rectangle is the bounds grown by the margin. 70 by 70
-- km at 5 km cells is 14 by 14.
local real_module = E.terrain_module
local height_calls = 0
E.terrain_module = function()
  return {
    GetTerrainConfig = function() return nil end,
    GetHeight = function(x, z)
      height_calls = height_calls + 1
      return (x % 20 < 10) and 1 or 0
    end,
    getClosestPointOnRoads = function() return nil end,
  }
end
logged = {}
run = new_run({ output_dir = "C:/extract" })
T.eq("reaches done", drive(run), E.STATE_DONE)
written = manifest_on(run.fs)
T.eq("the rectangle is the bounds grown by the margin", E.json(written.authored_bounds_m),
  '{"max_x":50000,"max_z":35000,"min_x":-40000,"min_z":-55000}')
T.eq("from the pre-sweep", written.authored_bounds_source, "presweep")
T.eq("which was timed", written.timing_ms.presweep ~= nil, true)
T.eq("196 cells of 201 heights", height_calls, 196 * 201)

-- The same directory again, from a new process: the rectangle is the
-- manifest's, and nothing is measured.
height_calls = 0
logged = {}
run = new_run({ output_dir = "C:/extract" }, nil, run.fs)
T.eq("reaches done again", drive(run), E.STATE_DONE)
T.eq("without a height read", height_calls, 0)
T.eq("the log says the rectangle was kept", log_has("authored rectangle kept from the manifest"), true)
T.eq("and the run resumed", log_has("resuming C:/extract"), true)
E.terrain_module = real_module

--------------------------------------------------------------------------------
T.group("refusals, before anything is written")
--------------------------------------------------------------------------------

-- No crop, and no terrain module for the pre-sweep to measure with: the
-- pre-sweep refuses, and nothing is written.
logged = {}
run = new_run({ output_dir = "C:/extract" })
T.eq("no crop and no module stops the run", drive(run), E.STATE_STOPPED)
T.eq("at the pre-sweep", run.refusal, "the terrain module is not loaded")
T.eq("and nothing written", run.fs.files["C:/extract/manifest.json"], nil)
T.eq("not even the directory", run.fs.files["C:/extract"], nil)

-- A job list without the pre-sweep: the grid job has no rectangle at all.
run = new_run({ output_dir = "C:/extract" }, { E.identity_job, E.grid_job })
T.eq("no crop and no pre-sweep stops the run", drive(run), E.STATE_STOPPED)
T.eq("with the reason", run.refusal, "no crop was given and the pre-sweep found no rectangle")

E.terrain_bounds_km = function() return nil end
run = new_run({ output_dir = "C:/extract", crop = CROP })
T.eq("no bounds stops the run", drive(run), E.STATE_STOPPED)
T.eq("naming the bounds", run.refusal, "the theatre reports no bounds rectangle")
E.terrain_bounds_km = function() return bounds_km end

--------------------------------------------------------------------------------
T.group("a directory that holds an extract is resumed")
--------------------------------------------------------------------------------

-- A first run, then what a killed run leaves behind: two tiles in the
-- journal and a manifest carrying a sweep's timing and a note.
run = new_run({ output_dir = "C:/extract", crop = CROP })
drive(run)
local fs = run.fs
E.append_tile("C:/extract", E.tile_entry("water", 0, 0, 0, 2))
E.append_tile("C:/extract", E.tile_entry("water", 0, 1, 2, 2))
local before = manifest_on(fs)
before.timing_ms.water = 500
-- Seven milliseconds the last attempt spent reading the install. This one
-- spends a few microseconds on a fake, so the sum is at least the seven.
before.timing_ms.identity = 7
before.notes = E.as_array({ "water 0_1: nothing wrong, a note" })
E.write_manifest("C:/extract", before)

-- A new process: a run that has never seen the directory.
logged = {}
run = new_run({ output_dir = "C:/extract", crop = CROP }, nil, fs)
T.eq("reaches done", drive(run), E.STATE_DONE)
T.eq("the journal is indexed", run.done[E.tile_key("water", 0, 1)] ~= nil, true)
T.eq("both tiles", #run.entries, 2)
T.eq("the other layer is untouched", run.done[E.tile_key("height", 0, 0)], nil)
T.eq("the sweep's timing survives", run.timing_ms.water, 500)
T.eq("this attempt's identity time is added to the last one's",
  run.timing_ms.identity >= 7, true)
T.eq("the log says it resumed", log_has("resuming C:/extract: 2 tiles journalled"), true)

written = manifest_on(fs)
T.eq("the manifest lists the journal", #written.tiles, 2)
T.eq("the note survives", written.notes[1], "water 0_1: nothing wrong, a note")
T.eq("and the first start time", written.extracted_at, "2026-09-04T09:12:44Z")

-- The same process, Start again: the run already holds the directory's
-- timings, and must not add them onto themselves.
T.eq("started again", E.start(run), true)
for _ = 1, 60 do
  E.run_frame(run)
  if run.state == E.STATE_DONE then break end
end
T.eq("done again", run.state, E.STATE_DONE)
T.eq("the timing is not doubled", run.timing_ms.water, 500)

-- A partial journal line is reported and the tile it was for is not done.
fs.files["C:/extract/tiles.jsonl"] = fs.files["C:/extract/tiles.jsonl"] .. '{"layer":"water","tx":1'
logged = {}
run = new_run({ output_dir = "C:/extract", crop = CROP }, nil, fs)
T.eq("still resumes", drive(run), E.STATE_DONE)
T.eq("with two tiles", #run.entries, 2)
T.eq("and says what was cut short", log_has("bytes of a cut-short journal line"), true)

--------------------------------------------------------------------------------
T.group("a directory that holds another extract is refused")
--------------------------------------------------------------------------------

fs.files[INSTALL .. "/autoupdate.cfg"] = build("0.0.0.1")
logged = {}
run = new_run({ output_dir = "C:/extract", crop = CROP }, nil, fs)
T.eq("another build stops the run", drive(run), E.STATE_STOPPED)
T.eq("saying what differs",
  run.refusal, "the output directory holds another extract: dcs_build was 0.0.0.0, now 0.0.0.1")
T.eq("each problem is logged on its own line", log_has("dcs_build was 0.0.0.0, now 0.0.0.1"), true)
fs.files[INSTALL .. "/autoupdate.cfg"] = build("0.0.0.0")

run = new_run({ output_dir = "C:/extract", crop = { x = 1000, z = 0, radius_m = 5000 } }, nil, fs)
T.eq("a moved crop stops the run", drive(run), E.STATE_STOPPED)
T.eq("naming the grid", run.refusal:find("grid was", 1, true) ~= nil, true)

fs.files["C:/extract/manifest.json"] = nil
run = new_run({ output_dir = "C:/extract", crop = CROP }, nil, fs)
T.eq("a journal with no manifest stops the run", drive(run), E.STATE_STOPPED)
T.eq("saying which file is missing",
  run.refusal, "the output directory holds another extract: tiles.jsonl is present and manifest.json is not")

T.done()
