-- Offline tests for the tables sweep: seven files, one a step, and what the
-- sweep does when the theatre cannot give one of them.
--
-- Run from the repository root with a plain lua5.1.
--
-- The rows themselves are tested in tables.lua. Here the job is driven over a
-- fake module answering the measured shapes and a fake install holding the
-- two Lua files, so the plumbing -- order, progress, the file per step, the
-- empty table for what is missing -- is what is under test. What the real
-- module answers is checked live.

package.path = "extractor/?.lua;extractor/test/support/?.lua;" .. package.path

local T = require("testing")
local E = require("DcsTerrainExtract")
local FakeFs = require("fakefs")

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

--------------------------------------------------------------------------------
T.group("the hook pass is config then tables")
--------------------------------------------------------------------------------

T.eq("two jobs so far", #E.jobs.hook, 2)
T.eq("config first", E.jobs.hook[1].name, "config")
T.eq("then the tables", E.jobs.hook[2].name, "tables")

--------------------------------------------------------------------------------
T.group("seven files in order, and the rows kept for the road sweeps")
--------------------------------------------------------------------------------

local ROADNET = "./Mods/terrains/X/AirfieldsTaxiways/KUTAISI.rn4"
local airdromes = {
  [25] = { id = "KUTAISI", roadnet = ROADNET, reference_point = { x = 1, y = 2 } },
  [7] = { id = "STRIP" },
}
local roadnets_asked, stand_params = {}, nil
local fake = {
  GetTerrainConfig = function(key)
    if key == "Airdromes" then return airdromes end
  end,
  getRunwayList = function(roadnet)
    roadnets_asked[#roadnets_asked + 1] = roadnet
    return { { course = -1.85, edge1name = "25", edge1x = 1, edge1y = 2, edge2name = "07", edge2x = 3, edge2y = 4 } }
  end,
  getStandList = function(roadnet, params)
    stand_params = params
    return { { crossroad_index = 1, name = "A1", x = 1, y = 2, params = { SHELTER = 0 } } }
  end,
  getBeacons = function()
    return { { beaconId = "b1", position = { 1, 2, 3 }, positionGeo = { latitude = 1, longitude = 2 } } }
  end,
  getRadio = function()
    return { { radioId = "r1", frequency = { [0] = { 0, 3750000 } } } }
  end,
  convertLatLonToMeters = function(lat, lon) return lat * 1000, lon * 1000 end,
}
E.terrain_module = function() return fake end

local TOWNS = 'local gettext = require("i_18n")\nlocal _ = gettext.translate\n'
  .. 'towns = {\n["POTI"] = { latitude = 42.5, longitude = 41.25, display_name = _("POTI")},\n}\n'
local NODES = "missionNodes = {\n  { id = 32, name = \"n\", redPos = {1, 2}, bluePos = {3, 4} },\n}\n"

local function new_install()
  local fs = FakeFs.new()
  fs.cwd = "C:\\DCS"
  fs.files["C:/DCS/Mods/terrains/X/entry.lua"] = "x"
  fs.files["C:/DCS/Mods/terrains/X/map/towns.lua"] = TOWNS
  fs.files["C:/DCS/Mods/terrains/X/MissionGenerator/nodes.lua"] = NODES
  E.fs = fs
  E.ensure_output_dirs("C:/extract")
  return fs
end

local function new_run()
  local run = E.new_run({ config = { output_dir = "C:/extract" } })
  run.identity.terrain_dir = "X"
  return run
end

local function file(fs, name)
  return fs.files["C:/extract/" .. name]
end

-- Runs the sweep to its end and returns the statuses the steps answered.
local function sweep(run)
  local step, progress = E.tables_job.start(run)
  local statuses = {}
  for _ = 1, 10 do
    local status = step()
    statuses[#statuses + 1] = status
    if status ~= E.MORE then
      break
    end
  end
  return statuses, progress
end

local fs = new_install()
local run = new_run()
logged = {}
local step, progress = E.tables_job.start(run)
T.eq("nothing done at the start", select(1, progress()), 0)
T.eq("of seven", select(2, progress()), 7)
T.eq("airdromes first", step(), E.MORE)
T.eq("one done", select(1, progress()), 1)
T.eq("airdromes.json is there", file(fs, "airdromes.json") ~= nil, true)
T.eq("runways.json is not yet", file(fs, "runways.json"), nil)
for _ = 1, 5 do
  T.eq("more", step(), E.MORE)
end
T.eq("the seventh finishes", step(), E.DONE)
T.eq("seven done", select(1, progress()), 7)
T.eq("and done stays done", step(), E.DONE)

local written = {}
for i = 1, #E.TABLE_ORDER do
  local name = E.TABLE_ORDER[i] .. ".json"
  written[E.TABLE_ORDER[i]] = E.decode(file(fs, name))
  T.eq(name .. " is an array", type(written[E.TABLE_ORDER[i]]), "table")
end
T.eq("two airdromes", #written.airdromes, 2)
T.eq("in id order", written.airdromes[1].id, 7)
T.eq("one runway, from the one airdrome with a roadnet", #written.runways, 1)
T.eq("belonging to it", written.runways[1].airdrome_id, 25)
T.eq("the roadnet was asked as given", roadnets_asked[1], ROADNET)
T.eq("and only once", #roadnets_asked, 1)
T.eq("the stand parameters are the six", table.concat(stand_params, ","),
  "SHELTER,FOR_HELICOPTERS,FOR_AIRPLANES,WIDTH,LENGTH,HEIGHT")
T.eq("one stand", written.stands[1].name, "A1")
T.eq("one beacon", written.beacons[1].beacon_id, "b1")
T.eq("one radio", written.radio[1].frequencies_hz.hf, 3750000)
T.eq("one town, converted", written.towns[1].x, 42500)
T.eq("one node", written.nodes[1].id, 32)
T.eq("the airdrome rows are kept on the run", #run.tables.airdromes, 2)
T.eq("and the towns", run.tables.towns[1].name, "POTI")
T.eq("each file is logged with its count", log_has("wrote towns.json: 1 rows"), true)
T.eq("and the Lua files by path", log_has("read Mods/terrains/X/map/towns.lua"), true)

-- A second sweep over the same directory writes the same bytes.
local before = file(fs, "airdromes.json")
sweep(new_run())
T.eq("a rewrite is the same file", file(fs, "airdromes.json"), before)
T.eq("with no temporary left behind", file(fs, "airdromes.json.tmp"), nil)

--------------------------------------------------------------------------------
T.group("what the theatre cannot give is written empty, and the sweep goes on")
--------------------------------------------------------------------------------

fs = new_install()
-- The directory stays, as it would on a theatre that ships other map files;
-- the towns file alone is gone.
fs.files["C:/DCS/Mods/terrains/X/map/towns.lua"] = nil
fs.files["C:/DCS/Mods/terrains/X/map/other.lua"] = "x"
fake.getRadio = nil
local failing = ROADNET
fake.getRunwayList = function(roadnet)
  if roadnet == failing then error("no such file") end
  return {}
end
logged = {}
local statuses = sweep(new_run())
T.eq("the sweep finishes", statuses[#statuses], E.DONE)
T.eq("in seven steps", #statuses, 7)
T.eq("no towns file is no towns", file(fs, "towns.json"), "[]")
T.eq("and says which file", log_has("towns.lua not read, towns written empty: no towns.lua under Mods/terrains/X/map"), true)
T.eq("no getRadio is no radio", file(fs, "radio.json"), "[]")
T.eq("and says so", log_has("terrain.getRadio is not a function"), true)
T.eq("a runway list that raises is no rows for that airdrome", file(fs, "runways.json"), "[]")
T.eq("and says which", log_has("airdrome 25: getRunwayList gave nothing, no rows"), true)
T.eq("the airdromes themselves are still written", #E.decode(file(fs, "airdromes.json")), 2)

fs = new_install()
fs.files["C:/DCS/Mods/terrains/X/map/towns.lua"] = "towns = {"
logged = {}
sweep(new_run())
T.eq("a towns file that will not compile is no towns", file(fs, "towns.json"), "[]")
T.eq("and is named", log_has("towns.lua not read, towns written empty"), true)

fs = new_install()
fake.GetTerrainConfig = function() return nil end
logged = {}
sweep(new_run())
T.eq("no Airdromes is no airdromes", file(fs, "airdromes.json"), "[]")
T.eq("and no runways", file(fs, "runways.json"), "[]")
T.eq("and says so", log_has("Airdromes is not a table, airdromes written empty"), true)
fake.GetTerrainConfig = function(key) if key == "Airdromes" then return airdromes end end

-- A shaper that raised -- which none does on anything a theatre has handed
-- back, and which is the guard behind that -- costs its table, not the run.
fs = new_install()
local real_rows = E.airdrome_rows
E.airdrome_rows = function() error("a shape nobody foresaw") end
logged = {}
statuses = sweep(new_run())
E.airdrome_rows = real_rows
T.eq("the sweep still finishes", statuses[#statuses], E.DONE)
T.eq("the table is empty", file(fs, "airdromes.json"), "[]")
T.eq("and the reason is logged", log_has("airdromes could not be shaped, written empty: "), true)

--------------------------------------------------------------------------------
T.group("what stops the sweep: no module, and a file that will not land")
--------------------------------------------------------------------------------

E.terrain_module = function() return nil end
run = new_run()
T.eq("no module refuses", E.tables_job.start(run)(), E.REFUSED)
T.eq("saying so", run.refusal, "the terrain module is not loaded")
E.terrain_module = function() return fake end

fs = new_install()
fs.lose_bytes(1)
run = new_run()
statuses = sweep(run)
T.eq("a short write refuses at the first file", statuses[1], E.REFUSED)
T.eq("naming it", run.refusal:find("airdromes.json cannot be written", 1, true), 1)

T.done()
