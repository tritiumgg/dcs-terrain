-- Offline tests for the water and height sweeps: the bytes a tile holds, the
-- tiles that are left out, the journal, the skip set and what a resume does
-- with all of them.
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

local surface_calls, height_calls = 0, 0
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

-- Drives a job to its end, collecting the statuses.
local function sweep(job, run)
  local step, progress = job.start(run)
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

--------------------------------------------------------------------------------
T.group("the hook pass ends with water then height")
--------------------------------------------------------------------------------

T.eq("water is the third job", E.jobs.hook[3], E.water_job)
T.eq("and height the fourth", E.jobs.hook[4], E.height_job)
T.eq("named for their layers", E.water_job.name .. "," .. E.height_job.name, "water,height")

--------------------------------------------------------------------------------
T.group("water: one tile a step, the bytes, the set and the journal")
--------------------------------------------------------------------------------

surface_raises_at = { 4, 4 }
height_raises_at = { 5, 5 }
local fs = FakeFs.new()
-- A stale file from a killed run, for the tile that will be omitted.
fs.files["C:/extract/tiles/water/0_0.bin"] = "stale"
logged = {}
local run = new_run(fs)
local step, progress = E.water_job.start(run)
T.eq("the set starts empty", next(run.skip), nil)
T.eq("nothing done before the first step", select(1, progress()), 0)
T.eq("of four tiles", select(2, progress()), 4)

T.eq("the fill tile wants more", step(), E.MORE)
T.eq("and is not written", tile(fs, "water", 0, 0), nil)
T.eq("the stale file is gone", fs.files["C:/extract/tiles/water/0_0.bin"], nil)
T.eq("it is in the set as fill", run.skip["0_0"], "fill")
T.eq("no journal line for it", fs.files["C:/extract/tiles.jsonl"], nil)
T.eq("but it counts as done", select(1, progress()), 1)
T.eq("the log says so", log_has("water 0_0: fill throughout, omitted"), true)

T.eq("the coast wants more", step(), E.MORE)
T.eq("the coast's bytes", tile(fs, "water", 0, 1), bytes(
  254, 0, 1, 255,
  2, 0, 1, 255,
  2, 0, 1, 255,
  2, 0, 255, 255))
T.eq("the unknown string was logged", log_has("unrecognised surface string swamp, encoded 254"), true)
T.eq("not in the set", run.skip["0_1"], nil)
T.eq("its journal line has the range", fs.files["C:/extract/tiles.jsonl"],
  '{"layer":"water","max":254,"min":0,"path":"tiles/water/0_1.bin","tx":0,"tz":1}\n')
T.eq("and the run indexes it", run.done["water/0_1"] ~= nil, true)
T.eq("the log carries the counts", log_has("water 0_1: min 0 max 254, 5 nodata, 0 failed"), true)

T.eq("the sea tile wants more", step(), E.MORE)
T.eq("sea throughout", tile(fs, "water", 1, 0), string.rep(bytes(2), 16))
T.eq("in the set as sea", run.skip["1_0"], "sea")
T.eq("journalled with min and max 2", #run.entries, 2)

T.eq("the land tile finishes", step(), E.DONE)
T.eq("a call that raised is nodata and counted failed", tile(fs, "water", 1, 1), bytes(
  255, 0, 0, 255,
  0, 0, 0, 255,
  0, 0, 0, 255,
  0, 0, 0, 255))
T.eq("said in the log", log_has("water 1_1: min 0 max 0, 5 nodata, 1 failed"), true)
T.eq("not in the set", run.skip["1_1"], nil)
T.eq("done stays done", step(), E.DONE)
T.eq("every tile counted", select(1, progress()), 4)
T.eq("three entries", #run.entries, 3)
T.eq("the totals line", log_has("water: 3 tiles written, 0 journalled, 0 swept again, 1 omitted, 0 skipped"), true)
T.eq("no temporary file is left", fs.files["C:/extract/tiles/water/1_1.bin.tmp"], nil)

--------------------------------------------------------------------------------
T.group("height: the rounding, the clamp, the fill test and the set")
--------------------------------------------------------------------------------

fs.files["C:/extract/tiles/height/1_0.bin"] = "stale"
logged = {}
height_calls = 0
step, progress = E.height_job.start(run)
T.eq("of four tiles", select(2, progress()), 4)
T.eq("the fill tile is skipped", step(), E.MORE)
T.eq("without a call", height_calls, 0)
T.eq("not written", tile(fs, "height", 0, 0), nil)
T.eq("the coast wants more", step(), E.MORE)
T.eq("the coast's heights, rounded, with the fill cell nodata", tile(fs, "height", 0, 1),
  i16(0, -2, 100) .. NODATA16
  .. i16(0, 1, 100) .. NODATA16
  .. i16(0, 1, 100) .. NODATA16
  .. i16(0, 12346) .. NODATA16 .. NODATA16)
T.eq("the sea tile is skipped", step(), E.MORE)
T.eq("and its stale file removed", fs.files["C:/extract/tiles/height/1_0.bin"], nil)
T.eq("the land tile finishes", step(), E.DONE)
T.eq("clamped each side, real 5 m land kept, the raise nodata", tile(fs, "height", 1, 1),
  i16(5, 32767, -32767) .. NODATA16
  .. i16(5) .. NODATA16 .. i16(5) .. NODATA16
  .. i16(5, 5, 5) .. NODATA16
  .. i16(5, 5, 5) .. NODATA16)
T.eq("the range is the clamped one", E.json(run.entries[#run.entries]),
  '{"layer":"height","max":32767,"min":-32767,"path":"tiles/height/1_1.bin","tx":1,"tz":1}')
T.eq("five entries over both layers", #run.entries, 5)
T.eq("the totals line", log_has("height: 2 tiles written, 0 journalled, 0 swept again, 0 omitted, 2 skipped"), true)

--------------------------------------------------------------------------------
T.group("a resume counts the journal as done and reads the set back")
--------------------------------------------------------------------------------

-- A new process over the same directory: the journal is indexed, the set is
-- gone, and the fill tile has no line to be found by.
local entries = E.load_journal("C:/extract")
local again = new_run(fs)
again.entries = entries
again.done = E.journal_index(entries)
surface_calls = 0
logged = {}
step, progress = E.water_job.start(again)
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
T.eq("the totals line", log_has("water: 0 tiles written, 3 journalled, 0 swept again, 1 omitted, 0 skipped"), true)

height_calls = 0
step, progress = E.height_job.start(again)
T.eq("two were done before a step", select(1, progress()), 2)
local statuses = {}
for _ = 1, 4 do
  statuses[#statuses + 1] = step()
end
T.eq("height walks four tiles and finishes", statuses[4], E.DONE)
T.eq("and no height was read", height_calls, 0)
T.eq("nothing new in the journal", #again.entries, 5)
T.eq("the bar is full", select(1, progress()), 4)

-- A journalled tile whose file is gone is swept and written again.
fs.files["C:/extract/tiles/water/0_1.bin"] = nil
local once_more = new_run(fs)
once_more.entries = E.load_journal("C:/extract")
once_more.done = E.journal_index(once_more.entries)
logged = {}
statuses = sweep(E.water_job, once_more)
T.eq("the sweep completes", statuses[#statuses], E.DONE)
T.eq("the file is back", tile(fs, "water", 0, 1) ~= nil, true)
T.eq("said in the log", log_has("water 0_1 is journalled but its file is not there or not the size: swept again"), true)
T.eq("counted", log_has("water: 1 tiles written, 2 journalled, 1 swept again, 1 omitted, 0 skipped"), true)
T.eq("with a second line for the tile", #once_more.entries, 6)
T.eq("that the manifest folds into one", #E.manifest_tiles(once_more.entries), 5)

--------------------------------------------------------------------------------
T.group("no triple: nothing is fill, and everything is written")
--------------------------------------------------------------------------------

surface_raises_at, height_raises_at = nil, nil
fs = FakeFs.new()
run = new_run(fs, false)
statuses = sweep(E.water_job, run)
T.eq("four steps", #statuses, 4)
T.eq("the fill tile is land", tile(fs, "water", 0, 0), string.rep(bytes(0), 16))
T.eq("and not in the set", run.skip["0_0"], nil)
T.eq("the sea tile still is", run.skip["1_0"], "sea")
statuses = sweep(E.height_job, run)
T.eq("the fill tile's heights are the fill height rounded", tile(fs, "height", 0, 0), string.rep(i16(5), 16))

--------------------------------------------------------------------------------
T.group("a tile that flips into the set keeps the other layer's journalled tile")
--------------------------------------------------------------------------------

-- The run above wrote every tile with no triple. Now the triple is known and
-- the fill tile's water file is gone, so water sweeps it again and finds it
-- fill. Its height tile has a journal line, and a line cannot be taken back,
-- so that tile stays, is not removed, and is counted once.
fs.files["C:/extract/tiles/water/0_0.bin"] = nil
local flipped = new_run(fs)
flipped.entries = E.load_journal("C:/extract")
flipped.done = E.journal_index(flipped.entries)
logged = {}
statuses = sweep(E.water_job, flipped)
T.eq("water finds the tile fill", flipped.skip["0_0"], "fill")
T.eq("and counts it once", log_has("water: 0 tiles written, 3 journalled, 1 swept again, 1 omitted, 0 skipped"), true)
step, progress = E.height_job.start(flipped)
T.eq("three height tiles done before a step", select(1, progress()), 3)
for i = 1, 4 do
  statuses[i] = step()
end
T.eq("finishes", statuses[4], E.DONE)
T.eq("the journalled height tile stays", tile(fs, "height", 0, 0) ~= nil, true)
T.eq("the bar is exactly full", select(1, progress()), 4)
T.eq("counted as journalled, not skipped", log_has("height: 0 tiles written, 3 journalled, 0 swept again, 0 omitted, 1 skipped"), true)
T.eq("no entry was added", #flipped.entries, 7)

--------------------------------------------------------------------------------
T.group("refusals")
--------------------------------------------------------------------------------

-- A short write: the fill tile writes nothing, so the first file is the
-- coast's, and the run stops there rather than leaving height to read the
-- tile as fill.
fs = FakeFs.new()
run = new_run(fs)
fs.lose_bytes(1)
statuses = sweep(E.water_job, run)
T.eq("the fill tile passes", statuses[1], E.MORE)
T.eq("the coast refuses", statuses[2], E.REFUSED)
T.eq("naming the tile", run.refusal:find("tiles/water/0_1.bin cannot be written", 1, true), 1)
T.eq("not journalled", fs.files["C:/extract/tiles.jsonl"], nil)
fs.lose_bytes(0)

run = new_run(FakeFs.new())
run.manifest = nil
T.eq("no grid refuses", E.water_job.start(run)(), E.REFUSED)
T.eq("saying so", run.refusal, "no grid was planned")

E.terrain_module = function() return { GetTerrainConfig = function() end } end
run = new_run(FakeFs.new())
T.eq("no surface function refuses", E.water_job.start(run)(), E.REFUSED)
T.eq("naming it", run.refusal, "terrain.GetSurfaceType is not a function")
T.eq("no height function refuses", E.height_job.start(run)(), E.REFUSED)
T.eq("naming it too", run.refusal, "terrain.GetHeight is not a function")
E.terrain_module = function() return nil end
T.eq("no module refuses", E.height_job.start(run)(), E.REFUSED)
E.terrain_module = function() return fake end

-- A sea-fill triple on the run but a module without the seabed call, which is
-- the one channel that tells that fill from real sea: nothing is fill, said
-- once, and the sweep goes on.
local partial = { GetTerrainConfig = fake.GetTerrainConfig, GetSurfaceType = fake.GetSurfaceType,
  GetHeight = fake.GetHeight }
E.terrain_module = function() return partial end
fs = FakeFs.new()
run = new_run(fs, { height = 0, water = 2, seabed = 100 })
logged = {}
statuses = sweep(E.water_job, run)
T.eq("the sweep completes", statuses[#statuses], E.DONE)
T.eq("the sea tile is written as sea", tile(fs, "water", 1, 0), string.rep(bytes(2), 16))
T.eq("and the log says why", log_has("no cell will be called fill"), true)

-- A land-fill triple needs no seabed call: its fill seabed is 0, and so is
-- real land's, so the same module tests fill with the two calls it has.
logged = {}
fs = FakeFs.new()
run = new_run(fs)
statuses = sweep(E.water_job, run)
T.eq("the fill tile is omitted without the seabed call", run.skip["0_0"], "fill")
T.eq("and nothing is said", log_has("no cell will be called fill"), false)
E.terrain_module = function() return fake end

T.done()
