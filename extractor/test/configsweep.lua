-- Offline tests for the config sweep: the water codes, the sample lattice,
-- the fill triple and the record config.json carries.
--
-- Run from the repository root with a plain lua5.1.
--
-- What the terrain answers is not tested here: that is checked live, table
-- against table, through the bridge. The job's own plumbing is driven over a
-- fake module answering the shapes the probe log measured, so that a
-- refusal, a failed call and a rewrite each do what they should.

package.path = "extractor/?.lua;extractor/test/support/?.lua;" .. package.path

local T = require("testing")
local E = require("DcsTerrainExtract")
local FakeFs = require("fakefs")

local logged = {}
E.log = function(message) logged[#logged + 1] = message end

local function log_count(text)
  local n = 0
  for i = 1, #logged do
    if logged[i]:find(text, 1, true) then
      n = n + 1
    end
  end
  return n
end

--------------------------------------------------------------------------------
T.group("water codes")
--------------------------------------------------------------------------------

T.eq("land", E.water_class("land"), 0)
T.eq("lake", E.water_class("lake"), 1)
T.eq("sea", E.water_class("sea"), 2)
T.eq("river", E.water_class("river"), 3)
T.eq("a string nobody has seen", E.water_class("swamp"), 254)
T.eq("logged once", log_count("unrecognised surface string swamp"), 1)
T.eq("the same string again", E.water_class("swamp"), 254)
T.eq("is not logged again", log_count("unrecognised surface string swamp"), 1)
T.eq("not a string at all", E.water_class(nil), 254)
T.eq("case matters, because the engine's strings are exact", E.water_class("Land"), 254)
T.eq("nodata is apart from unrecognised", E.WATER_NODATA ~= E.WATER_UNRECOGNISED, true)

--------------------------------------------------------------------------------
T.group("the lat/lon lattice is twenty points inside the grid")
--------------------------------------------------------------------------------

-- 10 000 m north by 20 000 m east, from (0, 0).
local grid = E.grid_from_rect({ min_x = 0, min_z = 0, max_x = 10000, max_z = 20000 }, 50, 256)
local points = E.latlon_lattice(grid)
T.eq("twenty", #points, 20)
T.eq("first row is an eighth of the way north", points[1].x, 1250)
T.eq("first column a tenth of the way east", points[1].z, 2000)
T.eq("columns run fastest", points[2].z, 6000)
T.eq("and stay on the row", points[2].x, 1250)
T.eq("last point north", points[20].x, 8750)
T.eq("last point east", points[20].z, 18000)

--------------------------------------------------------------------------------
T.group("the fill triple is read off three corners and must agree")
--------------------------------------------------------------------------------

local bounds = { min_x = -600000, min_z = -560000, max_x = 380000, max_z = 1130000 }
local at = E.fill_points(bounds)
T.eq("three points", #at, 3)
T.eq("south-west, out past the corner", at[1].x .. " " .. at[1].z, "-1100000 -1060000")
T.eq("north-east", at[2].x .. " " .. at[2].z, "880000 1630000")
T.eq("a third corner", at[3].x .. " " .. at[3].z, "880000 -1060000")

-- A land-fill theatre, as Caucasus reads, and a sea-fill one, as Syria does:
-- the triple is whatever was measured, and the code records the class.
local land = {
  { height = 5.000005, water = 0, seabed = 0 },
  { height = 5.000005, water = 0, seabed = 0 },
  { height = 5.000005, water = 0, seabed = 0 },
}
T.eq("land fill", E.json(E.fill_from_samples(land)), '{"height":5.0000049999999998,"seabed":0,"water":0}')
local sea = {
  { height = 0, water = 2, seabed = 100 },
  { height = 0, water = 2, seabed = 100 },
  { height = 0, water = 2, seabed = 100 },
}
T.eq("sea fill", E.json(E.fill_from_samples(sea)), '{"height":0,"seabed":100,"water":2}')

local drift = { land[1], land[2], { height = 5.000006, water = 0, seabed = 0 } }
T.eq("a height off by a millionth is disagreement", E.fill_from_samples(drift), nil)
local missing = { land[1], { height = 5.000005, water = 0 }, land[3] }
T.eq("a call that failed cannot agree", E.fill_from_samples(missing), nil)
T.eq("no samples, no triple", E.fill_from_samples({}), nil)

--------------------------------------------------------------------------------
T.group("the record carries every key the format names, and invents nothing")
--------------------------------------------------------------------------------

local record = E.config_record({
  id = "Caucasus",
  bounds_km = { sw = { -600, -560 }, ne = { 380, 1130 } },
  default_bullseye = { blue = { x = -291014, y = 617414 }, red = { x = 11557, y = 371700 } },
  sea_enabled = true,
  default_camera_km = { -355, 0.2, 618 },
  summer_time_delta = 4,
  shape = "FLAT",
  latlon_samples = { { x = 1, z = 2, lat = 42.5, lon = 41.25 } },
  fill = { height = 5.000005, water = 0, seabed = 0 },
  fill_samples = { { x = -1100000, z = -1060000, height = 5.000005, water = 0, seabed = 0 } },
})
T.eq("as written", E.json(record),
  '{"bounds_km":{"ne":[380,1130],"sw":[-600,-560]},'
  .. '"crs":null,'
  .. '"default_bullseye":{"blue":{"x":-291014,"z":617414},"red":{"x":11557,"z":371700}},'
  .. '"default_camera_km":[-355,0.20000000000000001,618],'
  .. '"fill":{"height":5.0000049999999998,"samples":[{"height":5.0000049999999998,"seabed":0,"water":0,"x":-1100000,"z":-1060000}],"seabed":0,"water":0},'
  .. '"id":"Caucasus",'
  .. '"latlon_samples":[{"lat":42.5,"lon":41.25,"x":1,"z":2}],'
  .. '"sea_enabled":true,"shape":"FLAT","summer_time_delta":4}')

-- A theatre that answers nothing for the optional keys, a bullseye of {0, 0}
-- as two theatres give, and a fill that did not agree.
local bare = E.config_record({
  bounds_km = { sw = { -300, -800 }, ne = { 1000, 800 } },
  default_bullseye = { blue = { x = 0, y = 0 }, red = { x = 0, y = 0 } },
  latlon_samples = {},
})
T.eq("nulls, not defaults", E.json(bare),
  '{"bounds_km":{"ne":[1000,800],"sw":[-300,-800]},"crs":null,'
  .. '"default_bullseye":{"blue":{"x":0,"z":0},"red":{"x":0,"z":0}},'
  .. '"default_camera_km":null,"fill":null,"id":null,"latlon_samples":[],'
  .. '"sea_enabled":null,"shape":null,"summer_time_delta":null}')

local odd = E.config_record({
  bounds_km = {}, default_bullseye = { blue = 7 }, default_camera_km = { 1, 2 },
})
T.eq("a bullseye that is not a point is null", E.json(odd.default_bullseye), '{"blue":null,"red":null}')
T.eq("a camera short a number is null", odd.default_camera_km, E.JSON_NULL)

local swept = E.config_record({ bounds_km = {}, presweep = { cell_km = 5, bits = "AA==" } })
T.eq("a pre-sweep record rides along", swept.presweep.cell_km, 5)
T.eq("and is absent, not null, without one", record.presweep, nil)

--------------------------------------------------------------------------------
T.group("a rewrite keeps the pre-sweep block already on disk")
--------------------------------------------------------------------------------

local existing = '{"id":"X","presweep":{"cell_km":5,"authored_cells":3}}'
local fresh = E.config_record({ bounds_km = {} })
E.keep_presweep(existing, fresh)
T.eq("kept", fresh.presweep.authored_cells, 3)

local own = E.config_record({ bounds_km = {}, presweep = { cell_km = 5, authored_cells = 9 } })
E.keep_presweep(existing, own)
T.eq("this run's own block wins", own.presweep.authored_cells, 9)

local none = E.config_record({ bounds_km = {} })
E.keep_presweep(nil, none)
T.eq("no file, nothing kept", none.presweep, nil)
E.keep_presweep("not json", none)
T.eq("a file that will not decode, nothing kept", none.presweep, nil)
E.keep_presweep('{"presweep":"5"}', none)
T.eq("a block that is not a table, nothing kept", none.presweep, nil)

--------------------------------------------------------------------------------
T.group("the job writes config.json and leaves the triple on the run")
--------------------------------------------------------------------------------

-- The measured shapes, answered by a fake standing where the module would
-- be. Outside the bounds every call answers the fill; inside, the lattice
-- points answer a lat/lon made of the point so a sample can be checked.
local calls = {}
local config = {
  id = "Synth", SW_bound = { -30, 0, -45 }, NE_bound = { 40, 0, 25 },
  defaultBullseye = { blue = { x = 1, y = 2 }, red = { x = 3, y = 4 } },
  seaEnabled = true, defaultcamera = { -3, 0.2, 6 }, SummerTimeDelta = 4,
}
local fake = {
  GetTerrainConfig = function(key) return config[key] end,
  getTerrainShpare = function() return "FLAT" end,
  convertMetersToLatLon = function(x, z) return x / 1000, z / 1000 end,
  GetHeight = function(x, z) calls[#calls + 1] = "h"; return 5.000005 end,
  GetSurfaceType = function(x, z) return "land" end,
  GetSurfaceHeightWithSeabed = function(x, z) return 5.000005, 0 end,
}
E.terrain_module = function() return fake end
E.terrain_bounds = function() return { min_x = -30000, min_z = -45000, max_x = 40000, max_z = 25000 } end

local fs = FakeFs.new()
E.fs = fs
E.ensure_output_dirs("C:/extract")
local run = E.new_run({ config = { output_dir = "C:/extract" } })
run.manifest = { grid = E.grid_from_rect({ min_x = 0, min_z = 0, max_x = 10000, max_z = 20000 }, 50, 256),
  bounds_km = { sw = { -30, -45 }, ne = { 40, 25 } } }

logged = {}
local step = E.config_job.start(run)
T.eq("one step and done", step(), E.DONE)
local written = E.decode(fs.files["C:/extract/config.json"])
T.eq("the id", written.id, "Synth")
T.eq("the shape", written.shape, "FLAT")
T.eq("twenty samples", #written.latlon_samples, 20)
T.eq("a sample is the point converted", written.latlon_samples[1].lat, 1.25)
T.eq("the triple on the file", written.fill.height, 5.000005)
T.eq("three fill samples", #written.fill.samples, 3)
T.eq("and on the run", run.fill.water, 0)
T.eq("three height calls, one per fill point", #calls, 3)
T.eq("the bullseye is x and z", written.default_bullseye.red.z, 4)
T.eq("the log carries the triple", log_count("fill height 5.000005 water 0 seabed 0"), 1)
T.eq("and the file", log_count("wrote config.json"), 1)

-- A pre-sweep block written once survives the rewrite a resumed run does.
fs.files["C:/extract/config.json"] = fs.files["C:/extract/config.json"]:gsub("}$", ',"presweep":{"cell_km":5}}')
run.presweep = false
E.config_job.start(run)()
T.eq("the pre-sweep block survives a rewrite", E.decode(fs.files["C:/extract/config.json"]).presweep.cell_km, 5)

-- A fill point answering something other than the others: no triple, said
-- so, and the run carries false rather than a wrong triple.
fake.GetHeight = function(x, z) return x < 0 and 5.000005 or 7 end
logged = {}
E.config_job.start(run)()
T.eq("the triple is not known", run.fill, false)
T.eq("the file says null", E.decode(fs.files["C:/extract/config.json"]).fill, E.JSON_NULL)
T.eq("and the log says why", log_count("fill samples disagree"), 1)
fake.GetHeight = function() return 5.000005 end

-- A call that raises is a log line and a null, never a raise out of the job.
fake.convertMetersToLatLon = function() error("no projection") end
fake.getTerrainShpare = nil
logged = {}
E.config_job.start(run)()
written = E.decode(fs.files["C:/extract/config.json"])
T.eq("a failed conversion is a null sample", written.latlon_samples[1].lat, E.JSON_NULL)
T.eq("logged with the message", log_count("terrain.convertMetersToLatLon failed: ") >= 1, true)
T.eq("a call that is not there is null", written.shape, E.JSON_NULL)
T.eq("and logged", log_count("terrain.getTerrainShpare is not a function"), 1)
T.eq("the triple is still read", run.fill.height, 5.000005)

-- No module is a refusal, and so is a file that will not land.
E.terrain_module = function() return nil end
run.refusal = nil
T.eq("no module refuses", E.config_job.start(run)(), E.REFUSED)
T.eq("saying so", run.refusal, "the terrain module is not loaded")
E.terrain_module = function() return fake end

fs.lose_bytes(1)
run.refusal = nil
T.eq("a short write refuses", E.config_job.start(run)(), E.REFUSED)
T.eq("naming the file", run.refusal:find("config.json cannot be written", 1, true), 1)

T.done()
