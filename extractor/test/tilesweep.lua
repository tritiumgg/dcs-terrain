-- Offline tests for the tile sweep, water and height together: the bytes each
-- layer holds, the tiles that are left out, the journal, the skip set and
-- what a resume does with all of them.
--
-- Run from the repository root with a plain lua5.1.
--
-- The theatre is a closed-form fake over a 2 by 2 tile grid at four cells a
-- tile, small enough that every byte can be written down: one tile of fill,
-- one of sea, a coast with the cases the encoders have, and a land tile with
-- the heights that clamp and the two calls that raise. What the real module
-- answers is checked live.

package.path = "extractor/?.lua;extractor/test/support/?.lua;" .. package.path

local T = require("testing")
local E = require("DcsTerrainExtract")
local FakeFs = require("fakefs")

E.now_iso = function() return "2026-09-04T09:12:44Z" end

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

local floor = math.floor

-- 8 rows north by 7 columns east at 50 m, tiles of 4: tiles (0, 0) and
-- (1, 0) are whole, (0, 1) and (1, 1) have a column past the grid. Cells are
-- named by their row and column.
local GRID = E.grid_from_rect({ min_x = 0, min_z = 0, max_x = 400, max_z = 350 }, 50, 4)
T.eq("8 rows", GRID.height, 8)
T.eq("7 columns", GRID.width, 7)

-- Height, surface string and seabed per cell. Tile (0, 0) is the exact fill
-- triple; tile (1, 0) is sea; tile (0, 1) is a coast with an unknown surface
-- string, a fill cell inside it and heights on either side of a half; tile
-- (1, 1) is land at exactly 5 m, which is not the fill's 5.000005, with a
-- height each side of the encoder's range.
local function cell(x, z)
  local row, col = floor(x / 50), floor(z / 50)
  if row < 4 and col < 4 then return 5.000005, "land", 0 end
  if row >= 4 and col < 4 then return 0, "sea", 30 end
  if row < 4 then
    if row == 3 and col == 6 then return 5.000005, "land", 0 end
    if col == 4 then return 0, row == 0 and "swamp" or "sea", 12 end
    if col == 5 then return ({ -2.5, 0.5, 1.4999, 12345.6 })[row + 1], "land", 0 end
    return 100.4, "lake", 0
  end
  if row == 4 and col == 5 then return 40000, "land", 0 end
  if row == 4 and col == 6 then return -40000, "land", 0 end
  return 5, "land", 0
end

local surface_calls, height_calls, seabed_calls = 0, 0, 0
local surface_raises_at, height_raises_at = nil, nil
local fake = {
  GetTerrainConfig = function() return nil end,
  GetSurfaceType = function(x, z)
    surface_calls = surface_calls + 1
    if surface_raises_at and floor(x / 50) == surface_raises_at[1]
        and floor(z / 50) == surface_raises_at[2] then
      error("no surface here")
    end
    return (select(2, cell(x, z)))
  end,
  GetHeight = function(x, z)
    height_calls = height_calls + 1
    if height_raises_at and floor(x / 50) == height_raises_at[1]
        and floor(z / 50) == height_raises_at[2] then
      error("no height here")
    end
    return (cell(x, z))
  end,
  GetSurfaceHeightWithSeabed = function(x, z)
    seabed_calls = seabed_calls + 1
    local h, _, d = cell(x, z)
    return h, d
  end,
}
E.terrain_module = function() return fake end

local FILL = { height = 5.000005, water = 0, seabed = 0 }

local function new_run(fs, fill)
  E.fs = fs
  E.ensure_output_dirs("C:/extract")
  local run = E.new_run({ config = { output_dir = "C:/extract" } })
  run.manifest = { grid = GRID }
  run.fill = fill == nil and FILL or fill
  return run
end

-- A run over a directory another process wrote: the journal indexed, the
-- set gone.
local function resumed(fs, fill)
  local run = new_run(fs, fill)
  run.entries = E.load_journal("C:/extract")
  run.done = E.journal_index(run.entries)
  return run
end

-- Drives the job to its end, collecting the statuses.
local function sweep(run)
  local step, progress = E.water_height_job.start(run)
  local statuses = {}
  for _ = 1, 10 do
    statuses[#statuses + 1] = step()
    if statuses[#statuses] ~= E.MORE then
      break
    end
  end
  return statuses, progress
end

local function tile(fs, layer, tx, tz)
  return fs.files["C:/extract/" .. E.tile_path(layer, tx, tz)]
end

local function bytes(...)
  return string.char(...)
end

local function i16(...)
  local out = {}
  for i = 1, select("#", ...) do
    out[i] = E.i16le((select(i, ...)))
  end
  return table.concat(out)
end
local NODATA16 = E.I16_NODATA_BYTES

local function lines(fs)
  local out = {}
  for line in (fs.files["C:/extract/tiles.jsonl"] or ""):gmatch("[^\n]+") do
    out[#out + 1] = line
  end
  return out
end

--------------------------------------------------------------------------------
T.group("the hook pass ends with the one tile sweep")
--------------------------------------------------------------------------------

T.eq("three jobs", #E.jobs.hook, 3)
T.eq("the sweep is the third", E.jobs.hook[3], E.water_height_job)
T.eq("named for both layers", E.water_height_job.name, "water+height")

--------------------------------------------------------------------------------
T.group("one tile a step: both layers' bytes, the set and the journal")
--------------------------------------------------------------------------------

surface_raises_at = { 4, 4 }
height_raises_at = { 5, 5 }
local fs = FakeFs.new()
-- Stale files from a killed run, for the tile that will be omitted and the
-- layer that will be skipped.
fs.files["C:/extract/tiles/water/0_0.bin"] = "stale"
fs.files["C:/extract/tiles/height/0_0.bin"] = "stale"
fs.files["C:/extract/tiles/height/1_0.bin"] = "stale"
logged = {}
local run = new_run(fs)
local step, progress = E.water_height_job.start(run)
T.eq("the set starts empty", next(run.skip), nil)
T.eq("nothing done before the first step", select(1, progress()), 0)
T.eq("of four tiles", select(2, progress()), 4)

surface_calls, height_calls, seabed_calls = 0, 0, 0
T.eq("the fill tile wants more", step(), E.MORE)
T.eq("and is written in neither layer", tile(fs, "water", 0, 0) or tile(fs, "height", 0, 0), nil)
T.eq("both stale files are gone", fs.files["C:/extract/tiles/height/0_0.bin"], nil)
T.eq("it is in the set as fill", run.skip["0_0"], "fill")
T.eq("no journal line for it", fs.files["C:/extract/tiles.jsonl"], nil)
T.eq("but it counts as done", select(1, progress()), 1)
T.eq("sixteen surface reads", surface_calls, 16)
T.eq("sixteen height reads", height_calls, 16)
T.eq("and no seabed read on a land fill", seabed_calls, 0)
T.eq("the log says so", log_has("tile 0_0: fill throughout, omitted"), true)

T.eq("the coast wants more", step(), E.MORE)
T.eq("the coast's water", tile(fs, "water", 0, 1), bytes(
  254, 0, 1, 255,
  2, 0, 1, 255,
  2, 0, 1, 255,
  2, 0, 255, 255))
T.eq("the coast's heights, rounded, with the fill cell nodata", tile(fs, "height", 0, 1),
  i16(0, -2, 100) .. NODATA16
  .. i16(0, 1, 100) .. NODATA16
  .. i16(0, 1, 100) .. NODATA16
  .. i16(0, 12346) .. NODATA16 .. NODATA16)
T.eq("the unknown string was logged", log_has("unrecognised surface string swamp, encoded 254"), true)
T.eq("not in the set", run.skip["0_1"], nil)
local journal = lines(fs)
T.eq("two journal lines, water first", journal[1],
  '{"layer":"water","max":254,"min":0,"path":"tiles/water/0_1.bin","tx":0,"tz":1}')
T.eq("then height, with its range", journal[2],
  '{"layer":"height","max":12346,"min":-2,"path":"tiles/height/0_1.bin","tx":0,"tz":1}')
T.eq("the run indexes both", run.done["water/0_1"] ~= nil and run.done["height/0_1"] ~= nil, true)
T.eq("the log carries both ranges", log_has("tile 0_1: water 0..254, height -2..12346, 5 nodata, 0 failed"), true)

T.eq("the sea tile wants more", step(), E.MORE)
T.eq("sea throughout", tile(fs, "water", 1, 0), string.rep(bytes(2), 16))
T.eq("no height tile", tile(fs, "height", 1, 0), nil)
T.eq("its stale height file is gone", fs.files["C:/extract/tiles/height/1_0.bin"], nil)
T.eq("in the set as sea", run.skip["1_0"], "sea")
T.eq("three entries", #run.entries, 3)

T.eq("the land tile finishes", step(), E.DONE)
T.eq("a surface call that raised is nodata in water", tile(fs, "water", 1, 1), bytes(
  255, 0, 0, 255,
  0, 0, 0, 255,
  0, 0, 0, 255,
  0, 0, 0, 255))
T.eq("a height call that raised is nodata in height, the rest clamped", tile(fs, "height", 1, 1),
  i16(5, 32767, -32767) .. NODATA16
  .. i16(5) .. NODATA16 .. i16(5) .. NODATA16
  .. i16(5, 5, 5) .. NODATA16
  .. i16(5, 5, 5) .. NODATA16)
T.eq("the height range is the clamped one", E.json(run.entries[#run.entries]),
  '{"layer":"height","max":32767,"min":-32767,"path":"tiles/height/1_1.bin","tx":1,"tz":1}')
T.eq("both failures counted", log_has("tile 1_1: water 0..0, height -32767..32767, 4 nodata, 2 failed"), true)
T.eq("not in the set", run.skip["1_1"], nil)
T.eq("done stays done", step(), E.DONE)
T.eq("every tile counted", select(1, progress()), 4)
T.eq("five entries over both layers", #run.entries, 5)
T.eq("the totals line", log_has("water+height: 3 tiles written, 0 journalled, 0 swept again, 1 omitted as fill, 1 written for water only"), true)
T.eq("no temporary file is left", fs.files["C:/extract/tiles/height/1_1.bin.tmp"], nil)

--------------------------------------------------------------------------------
T.group("a resume counts the journal as done and reads the set back")
--------------------------------------------------------------------------------

local again = resumed(fs)
surface_calls = 0
logged = {}
step, progress = E.water_height_job.start(again)
T.eq("three tiles are done before a step", select(1, progress()), 3)
T.eq("the fill tile is swept again", step(), E.MORE)
T.eq("and is fill again", again.skip["0_0"], "fill")
T.eq("sixteen surface reads", surface_calls, 16)
T.eq("the coast is journalled", step(), E.MORE)
T.eq("the sea tile is journalled", step(), E.MORE)
T.eq("and read back into the set", again.skip["1_0"], "sea")
T.eq("the land tile is journalled, and done", step(), E.DONE)
T.eq("no other read", surface_calls, 16)
T.eq("the bar is full", select(1, progress()), 4)
T.eq("nothing new in the journal", #again.entries, 5)
T.eq("the totals line", log_has("water+height: 0 tiles written, 3 journalled, 0 swept again, 1 omitted as fill, 0 written for water only"), true)

-- A journalled tile whose water file is gone is swept and written again, in
-- both layers, since both have lines.
fs.files["C:/extract/tiles/water/0_1.bin"] = nil
local once_more = resumed(fs)
logged = {}
local statuses = sweep(once_more)
T.eq("the sweep completes", statuses[#statuses], E.DONE)
T.eq("the water file is back", tile(fs, "water", 0, 1) ~= nil, true)
T.eq("said in the log", log_has("tile 0_1 is journalled but a file is not there or not the size: swept again"), true)
T.eq("counted", log_has("water+height: 1 tiles written, 2 journalled, 1 swept again, 1 omitted as fill, 0 written for water only"), true)
T.eq("with two more lines", #once_more.entries, 7)
T.eq("that the manifest folds into five", #E.manifest_tiles(once_more.entries), 5)

-- A journalled height file gone, with its water tile in place: swept again
-- all the same, because the height line promises a file.
fs.files["C:/extract/tiles/height/1_1.bin"] = nil
local and_again = resumed(fs)
statuses = sweep(and_again)
T.eq("the height file is back", tile(fs, "height", 1, 1) ~= nil, true)
T.eq("nine lines now", #and_again.entries, 9)

--------------------------------------------------------------------------------
T.group("no triple: nothing is fill, and everything is written")
--------------------------------------------------------------------------------

surface_raises_at, height_raises_at = nil, nil
fs = FakeFs.new()
run = new_run(fs, false)
statuses = sweep(run)
T.eq("four steps", #statuses, 4)
T.eq("the fill tile is land", tile(fs, "water", 0, 0), string.rep(bytes(0), 16))
T.eq("its heights are the fill height rounded", tile(fs, "height", 0, 0), string.rep(i16(5), 16))
T.eq("and not in the set", run.skip["0_0"], nil)
T.eq("the sea tile still is", run.skip["1_0"], "sea")
T.eq("seven lines", #run.entries, 7)

--------------------------------------------------------------------------------
T.group("a tile that turns out fill keeps the files its lines promise")
--------------------------------------------------------------------------------

-- The run above wrote every tile with no triple. Now the triple is known and
-- the fill tile's water file is gone, so it is swept again and found fill.
-- Both its layers have journal lines, and a line cannot be taken back, so
-- both files are written, all nodata, and the tile still joins the set.
fs.files["C:/extract/tiles/water/0_0.bin"] = nil
local flipped = resumed(fs)
logged = {}
statuses = sweep(flipped)
T.eq("the tile is fill in the set", flipped.skip["0_0"], "fill")
T.eq("its water file is all nodata", tile(fs, "water", 0, 0), string.rep(bytes(255), 16))
T.eq("and its height file too", tile(fs, "height", 0, 0), string.rep(NODATA16, 16))
T.eq("both lines carry a null range", E.json(flipped.entries[#flipped.entries]),
  '{"layer":"height","max":null,"min":null,"path":"tiles/height/0_0.bin","tx":0,"tz":0}')
T.eq("counted as swept again, not omitted", log_has("water+height: 1 tiles written, 3 journalled, 1 swept again, 0 omitted as fill, 0 written for water only"), true)

--------------------------------------------------------------------------------
T.group("refusals")
--------------------------------------------------------------------------------

-- A short write: the fill tile writes nothing, so the first file is the
-- coast's water, and the run stops there.
fs = FakeFs.new()
run = new_run(fs)
fs.lose_bytes(1)
statuses = sweep(run)
T.eq("the fill tile passes", statuses[1], E.MORE)
T.eq("the coast refuses", statuses[2], E.REFUSED)
T.eq("naming the tile", run.refusal:find("tiles/water/0_1.bin cannot be written", 1, true), 1)
T.eq("not journalled", fs.files["C:/extract/tiles.jsonl"], nil)
fs.lose_bytes(0)

run = new_run(FakeFs.new())
run.manifest = nil
T.eq("no grid refuses", E.water_height_job.start(run)(), E.REFUSED)
T.eq("saying so", run.refusal, "no grid was planned")

E.terrain_module = function() return { GetTerrainConfig = function() end, GetHeight = fake.GetHeight } end
run = new_run(FakeFs.new())
T.eq("no surface function refuses", E.water_height_job.start(run)(), E.REFUSED)
T.eq("naming it", run.refusal, "terrain.GetSurfaceType is not a function")
E.terrain_module = function() return { GetTerrainConfig = function() end, GetSurfaceType = fake.GetSurfaceType } end
T.eq("no height function refuses", E.water_height_job.start(run)(), E.REFUSED)
T.eq("naming it too", run.refusal, "terrain.GetHeight is not a function")
E.terrain_module = function() return nil end
T.eq("no module refuses", E.water_height_job.start(run)(), E.REFUSED)
E.terrain_module = function() return fake end

--------------------------------------------------------------------------------
T.group("the seabed call is made where the fill is sea, and only there")
--------------------------------------------------------------------------------

-- A sea-fill triple: real sea shares its height of 0, so every sea cell pays
-- the seabed call, and none of the fake's answers the fill's depth, so
-- nothing is fill.
fs = FakeFs.new()
run = new_run(fs, { height = 0, water = 2, seabed = 100 })
seabed_calls = 0
statuses = sweep(run)
T.eq("the sweep completes", statuses[#statuses], E.DONE)
T.eq("the sea tile is written as sea", tile(fs, "water", 1, 0), string.rep(bytes(2), 16))
T.eq("one seabed read per sea cell", seabed_calls, 16 + 3)
T.eq("the land tile is written in both layers", tile(fs, "height", 0, 0), string.rep(i16(5), 16))

-- The same triple with a module without the seabed call: nothing is fill,
-- said once, and the sweep goes on.
local partial = { GetTerrainConfig = fake.GetTerrainConfig, GetSurfaceType = fake.GetSurfaceType,
  GetHeight = fake.GetHeight }
E.terrain_module = function() return partial end
fs = FakeFs.new()
run = new_run(fs, { height = 0, water = 2, seabed = 100 })
logged = {}
statuses = sweep(run)
T.eq("the sweep completes without it", statuses[#statuses], E.DONE)
T.eq("and the log says why", log_has("no cell will be called fill"), true)

-- A land-fill triple needs no seabed call: the same module tests fill with
-- the two calls it has.
logged = {}
fs = FakeFs.new()
run = new_run(fs)
statuses = sweep(run)
T.eq("the fill tile is omitted without the seabed call", run.skip["0_0"], "fill")
T.eq("and nothing is said", log_has("no cell will be called fill"), false)
E.terrain_module = function() return fake end

T.done()
