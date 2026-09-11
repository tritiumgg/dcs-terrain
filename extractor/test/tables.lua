-- Offline tests for the table rows: what the seven tables' rows look like
-- when shaped from tables of the kinds the theatres have been measured to
-- hand back, and what happens to a shape nobody measured.
--
-- Run from the repository root with a plain lua5.1.
--
-- The fixtures are labelled by the shape they exercise rather than by a
-- theatre, because the rule under test is that every theatre gets the same
-- treatment: lists keyed from 0 and from 1, keys present on some entries and
-- absent on others, empty lists, a list with a gap, a key the format does not
-- name, and values that are not what they should be.

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

local NULL = E.JSON_NULL

--------------------------------------------------------------------------------
T.group("an airdrome row, from an entry shaped like a full one")
--------------------------------------------------------------------------------

-- Lists keyed from 0 (runways, runwayName, beacons) beside lists keyed from
-- 1 (radio, towers); shelters absent; a key the format does not name.
local full = {
  id = "KUTAISI", code = "UGKO", display_name = "Kutaisi", names = { en = "Kutaisi", [1] = "dropped" },
  class = "1", civilian = true, abandoned = false,
  reference_point = { x = -284887.375, y = 683858.71875 },
  reference_point_geo = { lat = 42.177616, lon = 42.481292 },
  runwayName = { [0] = "07-25" },
  runways = { [0] = { id = 1, name = "07-25" } },
  beacons = { [0] = { beaconId = "airfield25_0", runwayId = 1 }, [1] = { beaconId = "airfield25_1" } },
  radio = { "airfield25_0" },
  towers = { "externalId:1", "externalId:2" },
  warehouses = {},
  fueldepots = { "externalId:9" },
  roadnet = "./Mods/terrains/X/AirfieldsTaxiways/KUTAISI.rn4",
  roadnet5 = "./Mods/terrains/X/AirfieldsTaxiways/KUTAISI.rn5",
  zone = "X_terrain_32",
  projectors = {},
}

local row = E.airdrome_row(25, full)
T.eq("the row", E.json(row),
  '{"abandoned":false,"beacon_ids":["airfield25_0","airfield25_1"],"civilian":true,"class":"1",'
  .. '"code":"UGKO","display_name":"Kutaisi","fueldepots":["externalId:9"],"id":25,'
  .. '"lat":42.177616,"lon":42.481292000000003,"name_id":"KUTAISI","names":{"en":"Kutaisi"},'
  .. '"radio_ids":["airfield25_0"],"roadnet":"./Mods/terrains/X/AirfieldsTaxiways/KUTAISI.rn4",'
  .. '"roadnet5":"./Mods/terrains/X/AirfieldsTaxiways/KUTAISI.rn5","runway_names":["07-25"],'
  .. '"shelters":null,"towers":["externalId:1","externalId:2"],"warehouses":[],'
  .. '"x":-284887.375,"z":683858.71875}')
T.eq("the key the format does not name is not copied", row.zone, nil)

--------------------------------------------------------------------------------
T.group("an airdrome row, from an entry shaped like a bare one")
--------------------------------------------------------------------------------

-- Most keys absent, every list empty, no roadnet: a heliport or a strip on a
-- theatre where that is the norm.
local bare = { id = "STRIP", runwayName = {}, runways = {}, beacons = {}, projectors = {} }
row = E.airdrome_row(7, bare)
T.eq("absent keys are null, empty lists are empty", E.json(row),
  '{"abandoned":null,"beacon_ids":[],"civilian":null,"class":null,"code":null,"display_name":null,'
  .. '"fueldepots":null,"id":7,"lat":null,"lon":null,"name_id":"STRIP","names":null,'
  .. '"radio_ids":null,"roadnet":null,"roadnet5":null,"runway_names":[],"shelters":null,'
  .. '"towers":null,"warehouses":null,"x":null,"z":null}')

--------------------------------------------------------------------------------
T.group("a shape nobody measured is null with a line, not a raise")
--------------------------------------------------------------------------------

logged = {}
local odd = {
  id = 42, reference_point = "nowhere", reference_point_geo = { lat = "42" },
  runwayName = { [0] = "07-25", [2] = "16-34" },
  beacons = { [1] = "airfield3_0" },
  towers = { [1] = "a", [2] = function() end },
  names = "Odd",
}
row = E.airdrome_row(3, odd)
T.eq("a number where a string was is kept as a number", row.name_id, 42)
T.eq("a point that is not a table is null", row.x, NULL)
T.eq("a latitude that is a string is null", row.lat, NULL)
T.eq("a gapped list is null", row.runway_names, NULL)
T.eq("and says so", log_has("airdrome 3 runwayName: not a list, written null"), true)
T.eq("a beacon that is not a table has a null id", row.beacon_ids[1], NULL)
T.eq("a member the encoder cannot write is null", row.towers[2], NULL)
T.eq("names that are not a table are null", row.names, NULL)
T.eq("and the row still encodes", type(E.json(row)), "string")

--------------------------------------------------------------------------------
T.group("airdromes are rows in ascending id")
--------------------------------------------------------------------------------

logged = {}
local rows = E.airdrome_rows({ [25] = full, [7] = bare, [22] = bare, x = full, [9] = "not an entry" })
T.eq("three rows", #rows, 3)
T.eq("in id order", rows[1].id .. " " .. rows[2].id .. " " .. rows[3].id, "7 22 25")
T.eq("a key that is not a number is left out", log_has("airdrome x is not an entry"), true)
T.eq("an entry that is not a table is left out", log_has("airdrome 9 is not an entry"), true)
T.eq("no airdromes at all is an empty array", E.json(E.airdrome_rows({})), "[]")
T.eq("and so is nothing", E.json(E.airdrome_rows(nil)), "[]")

--------------------------------------------------------------------------------
T.group("runways and stands, from the lists the roadnet answers")
--------------------------------------------------------------------------------

local runways = E.runway_rows(25, {
  { course = -1.85, edge1name = "25", edge1x = -285000, edge1y = 683000, edge2name = "07", edge2x = -284000, edge2y = 685000 },
})
T.eq("one row, named from its edges", E.json(runways),
  '[{"airdrome_id":25,"course":-1.8500000000000001,"edge1_name":"25","edge1_x":-285000,"edge1_z":683000,'
  .. '"edge2_name":"07","edge2_x":-284000,"edge2_z":685000,"name":"25-07"}]')
T.eq("an empty list is no rows", E.json(E.runway_rows(25, {})), "[]")
T.eq("and so is none", E.json(E.runway_rows(25, nil)), "[]")
T.eq("an entry short an edge has no name", E.runway_rows(25, { { edge1name = "25" } })[1].name, NULL)

local stands = E.stand_rows(25, {
  { crossroad_index = 12, flag = 0, name = "A1", x = 1, y = 2,
    params = { SHELTER = 0, FOR_HELICOPTERS = 1, FOR_AIRPLANES = 1, WIDTH = 40, LENGTH = 50, HEIGHT = 15, EXTRA = 9 } },
  { crossroad_index = 13, name = "A2", x = 3, y = 4 },
})
T.eq("the params the format names, and no other", E.json(stands[1].params),
  '{"FOR_AIRPLANES":1,"FOR_HELICOPTERS":1,"HEIGHT":15,"LENGTH":50,"SHELTER":0,"WIDTH":40}')
T.eq("y is z", stands[1].z, 2)
T.eq("no params is null", stands[2].params, NULL)
T.eq("no flag is null", stands[2].flag, NULL)

--------------------------------------------------------------------------------
T.group("beacons, sorted by id, channel null where absent")
--------------------------------------------------------------------------------

local beacons = E.beacon_rows({
  b = { beaconId = "airfield12_1", callsign = "KT", display_name = "Kutaisi", type = 16408, frequency = 108900000,
    direction = 0, position = { -284000, 45, 683000 }, positionGeo = { latitude = 42.1, longitude = 42.4 },
    sceneObjects = { "t:1", "t:2" }, chartOffsetX = 1 },
  a = { beaconId = "airfield12_0", callsign = "KTS", type = 8, frequency = 395000, channel = 40,
    position = { 1, 2, 3 }, positionGeo = { latitude = 1, longitude = 2 }, sceneObjects = {} },
  c = "not a beacon",
})
T.eq("two rows", #beacons, 2)
T.eq("sorted on the id", beacons[1].beacon_id .. " " .. beacons[2].beacon_id, "airfield12_0 airfield12_1")
T.eq("the channel where there is one", beacons[1].channel, 40)
T.eq("null where there is not", beacons[2].channel, NULL)
T.eq("position is x, alt, z", beacons[2].x .. " " .. beacons[2].alt .. " " .. beacons[2].z, "-284000 45 683000")
T.eq("the geo position", beacons[2].lat, 42.1)
T.eq("scene objects", E.json(beacons[2].scene_objects), '["t:1","t:2"]')
T.eq("an empty scene list", E.json(beacons[1].scene_objects), "[]")
T.eq("a key the format does not name is not copied", beacons[2].chartOffsetX, nil)
T.eq("an entry that is not a table is left out", log_has("beacon c is not an entry"), true)

--------------------------------------------------------------------------------
T.group("radio, with the four bands and the first name of each callsign")
--------------------------------------------------------------------------------

local radio = E.radio_rows({
  { radioId = "airfield12_0",
    callsign = { { nato = { "Batumi", "Batumi" } }, { ussr = { "Druzhinnik", "Druzhinnik" } } },
    frequency = { [0] = { 0, 3750000 }, [1] = { 0, 38400000 }, [2] = { 0, 121000000 }, [3] = { 0, 250000000 } },
    role = { "ground", "tower", "approach" }, sceneObjects = { "t:9" } },
  { radioId = "airfield1_0", callsign = { { common = "Anapa" } }, frequency = { [2] = { 0, 121000000 } } },
})
-- Sorted as strings, where "12" comes before "1_": the order is stable and
-- not numeric, and that is all the file needs.
T.eq("sorted on the id", radio[1].radio_id .. " " .. radio[2].radio_id, "airfield12_0 airfield1_0")
T.eq("the bands", E.json(radio[1].frequencies_hz),
  '{"fm":38400000,"hf":3750000,"uhf":250000000,"vhf":121000000}')
T.eq("a band absent is null", E.json(radio[2].frequencies_hz),
  '{"fm":null,"hf":null,"uhf":null,"vhf":121000000}')
T.eq("callsigns by language, first name", E.json(radio[1].callsigns),
  '{"nato":"Batumi","ussr":"Druzhinnik"}')
T.eq("a callsign that is a bare string", E.json(radio[2].callsigns), '{"common":"Anapa"}')
T.eq("roles", E.json(radio[1].roles), '["ground","tower","approach"]')
T.eq("no roles is null", radio[2].roles, NULL)

--------------------------------------------------------------------------------
T.group("towns, sorted by name, positions through the conversion handed in")
--------------------------------------------------------------------------------

local function to_meters(lat, lon)
  if lat < 0 then
    return nil
  end
  return lat * 1000, lon * 1000
end
logged = {}
local towns = E.town_rows({
  ["POTI"] = { latitude = 42.157804, longitude = 41.677693, display_name = "POTI" },
  ["BATUMI"] = { latitude = 41.654059, longitude = 41.655372, display_name = "BATUMI" },
  ["SOUTH"] = { latitude = -1, longitude = 2, display_name = "SOUTH" },
  ["NOWHERE"] = { display_name = "NOWHERE" },
  [7] = { latitude = 1, longitude = 1 },
}, to_meters)
T.eq("four rows", #towns, 4)
T.eq("sorted by name", towns[1].name .. " " .. towns[2].name .. " " .. towns[3].name .. " " .. towns[4].name,
  "BATUMI NOWHERE POTI SOUTH")
T.eq("x from the conversion", towns[1].x, 41.654059 * 1000)
T.eq("z likewise", towns[1].z, 41.655372 * 1000)
T.eq("a position that did not convert is null", towns[4].x, NULL)
T.eq("and is logged", log_has("town SOUTH: position did not convert"), true)
T.eq("a town with no position keeps its name", towns[2].lat, NULL)
T.eq("a key that is not a name is left out", log_has("town 7 is not an entry"), true)

--------------------------------------------------------------------------------
T.group("nodes, positions from positional pairs, ids that are not the index")
--------------------------------------------------------------------------------

local nodes = E.node_rows({
  { id = 32, name = "31-SE-Plains-1", redPos = { -274514.25, 833228.5 }, bluePos = { -278971.5, 851457.25 },
    redTemplates = { "x" } },
  { id = 2, name = "2 SW-Rivers-1", redPos = { 1, 2 } },
})
T.eq("two rows", E.json(nodes),
  '[{"blue":{"x":-278971.5,"z":851457.25},"id":32,"name":"31-SE-Plains-1","red":{"x":-274514.25,"z":833228.5}},'
  .. '{"blue":null,"id":2,"name":"2 SW-Rivers-1","red":{"x":1,"z":2}}]')
logged = {}
T.eq("a gapped list is no rows", E.json(E.node_rows({ [1] = { id = 1 }, [3] = { id = 3 } })), "[]")
T.eq("and says so", log_has("missionNodes: not a list, written empty"), true)

--------------------------------------------------------------------------------
T.group("a theatre's Lua runs in a sandbox and gives up one table")
--------------------------------------------------------------------------------

-- The shape of the real file: a translator asked for by name and called on
-- every display name, then one global assignment.
local TOWNS = 'local gettext = require("i_18n")\nlocal       _ = gettext.translate\n\n'
  .. 'towns = {\n["POTI"] = { latitude = 42.157804, longitude = 41.677693, display_name = _("POTI")},\n}\n'
local towns_table = E.load_terrain_table(loadstring(TOWNS), "towns")
T.eq("the table", towns_table.POTI.display_name, "POTI")
T.eq("and nothing leaked into this environment", rawget(_G, "towns"), nil)

local NODES = "missionNodes = \n{\n    [2] = \n    {\n        id = 32,\n        redPos =  {-274514.28, 833228.57},\n    },\n}\n"
T.eq("the nodes", E.load_terrain_table(loadstring(NODES), "missionNodes")[2].id, 32)

local what, why = E.load_terrain_table(loadstring("towns = 7"), "towns")
T.eq("a global that is not a table is nothing", what, nil)
T.eq("with the reason", why, "the file sets no towns table")
what, why = E.load_terrain_table(loadstring("error('boom')"), "towns")
T.eq("a chunk that raises is nothing", what, nil)
T.eq("with its message", why:find("boom", 1, true) ~= nil, true)
what, why = E.load_terrain_table(loadstring("os.exit(3)"), "towns")
T.eq("a chunk that reaches outside the sandbox is nothing", what, nil)
what, why = E.load_terrain_table(nil, "towns")
T.eq("no chunk is nothing", what, nil)
T.eq("a data file may use the standard libraries",
  E.load_terrain_table(loadstring("towns = { n = math.floor(1.5), s = string.upper('a') }"), "towns").s, "A")

local fs = FakeFs.new()
E.fs = fs
fs.files["C:/DCS/Mods/terrains/X/map/towns.lua"] = TOWNS
T.eq("read off the file system", E.read_terrain_table("C:/DCS/Mods/terrains/X/map/towns.lua", "towns").POTI.latitude, 42.157804)
what, why = E.read_terrain_table("C:/DCS/Mods/terrains/X/map/missing.lua", "towns")
T.eq("a missing file is nothing", what, nil)
T.eq("with the path", why:find("missing.lua", 1, true) ~= nil, true)
fs.files["C:/DCS/Mods/terrains/X/map/broken.lua"] = "towns = {"
what, why = E.read_terrain_table("C:/DCS/Mods/terrains/X/map/broken.lua", "towns")
T.eq("a file that will not compile is nothing", what, nil)

-- The directory is matched without regard to case, and the file by name.
fs.files["C:/DCS/Mods/terrains/X/entry.lua"] = "x"
T.eq("the path is found through the directory as spelled",
  E.find_terrain_path("C:/DCS", "X", "Map", "towns.lua"), "Mods/terrains/X/map/towns.lua")
local none, err = E.find_terrain_path("C:/DCS", "X", "Map", "elsewhere.lua")
T.eq("a file that is not there is nothing", none, nil)
T.eq("saying which", err, "no elsewhere.lua under Mods/terrains/X/map")
none, err = E.find_terrain_path("C:/DCS", "X", "MissionGenerator", "nodes.lua")
T.eq("a directory that is not there is nothing", none, nil)
T.eq("saying which", err:find("no MissionGenerator directory", 1, true), 1)

T.done()
