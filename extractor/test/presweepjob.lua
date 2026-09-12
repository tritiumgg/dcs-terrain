-- Offline tests for the pre-sweep job: the lattice walked one cell a step over
-- a fake theatre, the rectangle and the record it leaves on the run, and when
-- it measures nothing at all.
--
-- Run from the repository root with a plain lua5.1.
--
-- The fake theatre is closed-form: bumpy heights in one corner, a road
-- reachable from one other cell, flat everywhere else. What the real module
-- answers is checked live; here the walk, the rule's two arms, the counts and
-- the directory checks are what is under test.

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

--------------------------------------------------------------------------------
T.group("the prepare jobs are identity, presweep, grid")
--------------------------------------------------------------------------------

T.eq("three jobs", #E.jobs.prepare, 3)
T.eq("identity first", E.jobs.prepare[1].name, "identity")
T.eq("then the pre-sweep", E.jobs.prepare[2].name, "presweep")
T.eq("then the grid", E.jobs.prepare[3].name, "grid")

--------------------------------------------------------------------------------
T.group("a lattice walked one cell a step, both arms of the rule")
--------------------------------------------------------------------------------

-- 25 by 15 km at 5 km cells: 5 rows north, 3 columns east, 15 cells. Heights
-- alternate 0 and 1 every 10 m where x and z are both under 10 km, which is
-- the four cells of rows 0 and 1, columns 0 and 1, so every interior sample of
-- a line there is a breakpoint. A road answers only from inside cell (3, 1),
-- and nil, no road reachable, from anywhere else.
E.terrain_bounds = function()
  return { min_x = 0, min_z = 0, max_x = 25000, max_z = 15000 }
end
local height_calls, snap_calls = 0, 0
local fake = {
  GetTerrainConfig = function() return nil end,
  GetHeight = function(x, z)
    height_calls = height_calls + 1
    if x < 10000 and z < 10000 then
      return (x % 20 < 10) and 1 or 0
    end
    return 5
  end,
  getClosestPointOnRoads = function(kind, x, z)
    snap_calls = snap_calls + 1
    T.eq("the road kind", kind, "roads")
    if x >= 15000 and x < 20000 and z >= 5000 and z < 10000 then
      return 16000, 9000
    end
    return nil
  end,
}
E.terrain_module = function() return fake end

-- Heights swapped in below count their calls like the first.
local function counted(f)
  return function(x, z)
    height_calls = height_calls + 1
    return f(x, z)
  end
end

local fs = FakeFs.new()
E.fs = fs
local IDENTITY = {
  theatre = "Synth",
  dcs_build = "0.0.0.0",
  dcs_build_timestamp = "00000000-000000",
  terrain_fingerprint = { surface5 = { size = 4194304 } },
}
local function new_run(config)
  local run = E.new_run({ config = config or { output_dir = "C:/extract" } })
  for k, v in pairs(IDENTITY) do
    run.identity[k] = v
  end
  return run
end

local function sweep(run)
  local step, progress = E.presweep_job.start(run)
  local status, steps = E.MORE, 0
  while status == E.MORE and steps < 100 do
    status = step()
    steps = steps + 1
  end
  return status, steps, progress
end

logged = {}
local run = new_run()
local step, progress = E.presweep_job.start(run)
T.eq("nothing measured before the first step", select(1, progress()), 0)
T.eq("of the lattice's cells", select(2, progress()), 15)
T.eq("the first step wants more", step(), E.MORE)
T.eq("and counts one cell", select(1, progress()), 1)
T.eq("201 heights along the line", height_calls, 201)
T.eq("after asking for a road first", snap_calls, 1)
local status = E.MORE
for _ = 1, 14 do
  status = step()
end
T.eq("the fifteenth cell finishes", status, E.DONE)
T.eq("and done stays done", step(), E.DONE)
T.eq("every cell counted", select(1, progress()), 15)
T.eq("every cell but the road's read its line", height_calls, 14 * 201)
T.eq("every cell asked for a road", snap_calls, 15)

local rect = run.presweep_bounds
T.eq("the rectangle bounds rows 0 to 3 with the margin", rect.min_x, -10000)
T.eq("from the first row", rect.max_x, 30000)
T.eq("and columns 0 to 1", rect.min_z, -10000)
T.eq("with the margin east", rect.max_z, 20000)
local record = run.presweep
T.eq("five cells authored", record.authored_cells, 5)
T.eq("of fifteen", record.total_cells, 15)
T.eq("at 5 km", record.cell_km, 5)
T.eq("the breakpoint rule", record.breakpoint_min, 60)
T.eq("the road rule", record.road_max_m, 5000)
T.eq("and how far a road still lets breakpoints count", record.breakpoint_road_max_m, 25000)
T.eq("the bitmask, row by row", record.bits, E.base64("\192\192\000\064\000"))
T.eq("the log counts the cells", log_has("pre-sweep: 5 of 15 cells authored, 1 by a road within 5 km;"
  .. " 15 snaps, 0 cells cleared, 14 lines read"), true)
T.eq("and names the rectangle", log_has("authored rectangle x -10000..30000 z -10000..20000"), true)
T.eq("no failure is reported", log_has("failed"), false)

--------------------------------------------------------------------------------
T.group("a crop run measures nothing")
--------------------------------------------------------------------------------

height_calls, snap_calls = 0, 0
logged = {}
run = new_run({ output_dir = "C:/extract", crop = { x = 0, z = 0, radius_m = 5000 } })
T.eq("done in one step", E.presweep_job.start(run)(), E.DONE)
T.eq("no height read", height_calls, 0)
T.eq("no rectangle", run.presweep_bounds, false)
T.eq("no record", run.presweep, false)
T.eq("the log says why", log_has("crop given, no pre-sweep"), true)

--------------------------------------------------------------------------------
T.group("a directory that holds this theatre's extract is not measured again")
--------------------------------------------------------------------------------

local KEPT = { min_x = -5000, min_z = -5000, max_x = 35000, max_z = 25000 }
local function manifest(edit)
  local o = {
    theatre = IDENTITY.theatre,
    dcs_build = IDENTITY.dcs_build,
    dcs_build_timestamp = IDENTITY.dcs_build_timestamp,
    terrain_fingerprint = IDENTITY.terrain_fingerprint,
    bounds_km = { sw = { 0, 0 }, ne = { 25, 15 } },
    grid = E.grid_from_rect(KEPT, 50, 256),
    omit_sea_tiles = true,
    authored_bounds_m = KEPT,
    authored_bounds_source = "presweep",
  }
  for k, v in pairs(edit or {}) do
    o[k] = v
  end
  return E.new_manifest(o)
end

E.ensure_output_dirs("C:/extract")
E.write_manifest("C:/extract", manifest())
height_calls = 0
logged = {}
run = new_run()
T.eq("done in one step", E.presweep_job.start(run)(), E.DONE)
T.eq("no height read", height_calls, 0)
T.eq("the rectangle is the manifest's", E.json(run.presweep_bounds), E.json(KEPT))
T.eq("no new record, so the one on disk is kept", run.presweep, false)
T.eq("the log says where it came from", log_has("authored rectangle kept from the manifest x -5000..35000"), true)

-- A crop extract in the directory: a whole-map run would sweep the crop and
-- call it the map.
E.write_manifest("C:/extract", manifest({
  crop_m = { min_x = 0, min_z = 0, max_x = 5000, max_z = 5000 },
  authored_bounds_m = nil, authored_bounds_source = nil,
}))
run = new_run()
T.eq("a crop extract refuses", E.presweep_job.start(run)(), E.REFUSED)
T.eq("saying so", run.refusal:find("holds a crop extract", 1, true) ~= nil, true)
T.eq("without measuring", height_calls, 0)

-- Another theatre's extract: refused here, before the minute, with the grid
-- job's words.
E.write_manifest("C:/extract", manifest({ dcs_build = "0.0.0.1" }))
logged = {}
run = new_run()
T.eq("another build refuses", E.presweep_job.start(run)(), E.REFUSED)
T.eq("saying what differs", run.refusal,
  "the output directory holds another extract: dcs_build was 0.0.0.1, now 0.0.0.0")
T.eq("each problem logged on its own line", log_has("dcs_build was 0.0.0.1, now 0.0.0.0"), true)
T.eq("without measuring", height_calls, 0)

-- A journal with no manifest is the window between a rename and a rewrite,
-- and not a fresh directory.
fs.files["C:/extract/manifest.json"] = nil
E.append_tile("C:/extract", E.tile_entry("water", 0, 0, 2, 2))
run = new_run()
T.eq("a journal with no manifest refuses", E.presweep_job.start(run)(), E.REFUSED)
T.eq("naming the file", run.refusal,
  "the output directory holds another extract: tiles.jsonl is present and manifest.json is not")
T.eq("without measuring", height_calls, 0)
fs.files["C:/extract/tiles.jsonl"] = nil

--------------------------------------------------------------------------------
T.group("a far snap clears every cell nearer than what it found")
--------------------------------------------------------------------------------

-- Flat everywhere, and the nearest road point is always (40000, 12500), 15 km
-- north of the lattice, as a theatre with roads answers from anywhere. The
-- first cell's snap is 38 810 m away, so it clears every cell within
-- 13 810 m of its center, seven of them, which read their lines and no
-- more, being flat; by hand, (2, 2) snaps at 27 500 m and clears nothing,
-- and the six cells of rows 3 and 4 snap between 17 500 and 24 622 m,
-- inside the 25 km where breakpoints would count, so each reads its line.
-- Eight snaps, thirteen lines, nothing authored.
fake.GetHeight = counted(function() return 5 end)
fake.getClosestPointOnRoads = function()
  snap_calls = snap_calls + 1
  return 40000, 12500
end
snap_calls, height_calls = 0, 0
logged = {}
run = new_run()
T.eq("nothing is authored", sweep(run), E.REFUSED)
T.eq("eight snaps", snap_calls, 8)
T.eq("thirteen lines", height_calls, 13 * 201)

-- Rough everywhere, and the nearest road 138 km away: the first snap clears
-- the lattice and proves the theatre answers to 138 km. A cleared cell
-- whose road is provably within that is settled with nothing read; one
-- whose road may lie beyond it reads its line and, rough, asks, and each
-- such answer's disc settles cells after it. How many ask depends on the
-- geometry to the meter, so the checks are the properties: fewer than all,
-- one line per cleared cell that asked, and nothing authored, because rough
-- ground that far from every road is not built terrain.
fake.GetHeight = counted(function(x, z) return (x % 20 < 10) and 1 or 0 end)
fake.getClosestPointOnRoads = function()
  snap_calls = snap_calls + 1
  return 100000, 100000
end
snap_calls, height_calls = 0, 0
run = new_run()
T.eq("rough ground far from every road is not authored", sweep(run), E.REFUSED)
T.eq("not every cell asked", snap_calls < 15 and snap_calls > 1, true)
T.eq("each cleared cell that asked read its line first", height_calls, (snap_calls - 1) * 201)

-- Pagan: the theatre answers a far road from the sea south of an island and
-- nothing at all from the island, which has no road of its own. Rows 0 to 2
-- answer a road 100 km south, rows 3 and 4 answer nil, and only row 4 is
-- rough. The first cell's disc covers the lattice; the rough cells of row 4
-- ask anyway, hear nothing, and are authored by their lines.
fake.GetHeight = counted(function(x, z)
  if x >= 20000 then
    return (x % 20 < 10) and 1 or 0
  end
  return 5
end)
fake.getClosestPointOnRoads = function(kind, x, z)
  snap_calls = snap_calls + 1
  if x < 15000 then
    return -100000, 7500
  end
  return nil
end
snap_calls, height_calls = 0, 0
run = new_run()
T.eq("the island is authored", sweep(run), E.DONE)
T.eq("its three cells", run.presweep.authored_cells, 3)
T.eq("the rectangle is the island's row", run.presweep_bounds.min_x .. ".." .. run.presweep_bounds.max_x, "10000..35000")
T.eq("one snap cleared the lattice, and the three rough cells asked", snap_calls, 4)
T.eq("every cleared cell read its line", height_calls, 14 * 201)
fake.GetHeight = counted(function(x, z) return (x % 20 < 10) and 1 or 0 end)

-- Rough everywhere again, with the theatre's reach proven by the first
-- answer: the first cell hears of a road 1 000 km away, and every later cell
-- of a road 47 to 70 km away. The first answer's disc covers the lattice
-- and proves the theatre answers to 1 000 km; the second cell's road is not
-- provably within that from the first disc, so it reads its line and asks,
-- but its own answer's disc puts every other cell's road within 78 km, so
-- the thirteen left are settled with neither. Two snaps, one line.
local first = true
fake.getClosestPointOnRoads = function(kind, x, z)
  snap_calls = snap_calls + 1
  if first then
    first = false
    return 1000000, 12500
  end
  return 60000, 12500
end
snap_calls, height_calls = 0, 0
run = new_run()
T.eq("nothing is authored", sweep(run), E.REFUSED)
T.eq("two snaps", snap_calls, 2)
T.eq("one line", height_calls, 201)

-- The same rough ground with no road reachable at all: the line decides
-- alone, and every cell is authored, so an island keeps its detailed
-- ground.
fake.getClosestPointOnRoads = function()
  snap_calls = snap_calls + 1
  return nil
end
snap_calls, height_calls = 0, 0
run = new_run()
T.eq("no road reachable leaves it to the line", sweep(run), E.DONE)
T.eq("every cell", run.presweep.authored_cells, 15)
T.eq("every line read", height_calls, 15 * 201)

-- The bumpy corner with the road inside cell (3, 1) at (16000, 9000): every
-- cell whose center is within 5 km is authored by it without a line, the
-- four bumpy cells are within 25 km of it and are authored by their lines,
-- and the flat ring reads its lines for nothing. No snap answered beyond
-- 25 km, so nothing is cleared.
fake.GetHeight = counted(function(x, z)
  if x < 10000 and z < 10000 then
    return (x % 20 < 10) and 1 or 0
  end
  return 5
end)
fake.getClosestPointOnRoads = function()
  snap_calls = snap_calls + 1
  return 16000, 9000
end
snap_calls, height_calls = 0, 0
logged = {}
run = new_run()
T.eq("the sweep completes", sweep(run), E.DONE)
T.eq("four cells by the road and four by their lines", run.presweep.authored_cells, 8)
T.eq("the bitmask", run.presweep.bits, E.base64("\192\192\096\096\000"))
T.eq("fifteen snaps", snap_calls, 15)
T.eq("eleven lines", height_calls, 11 * 201)
T.eq("the log counts them", log_has("8 of 15 cells authored, 4 by a road within 5 km; 15 snaps, 0 cells cleared, 11 lines read"), true)
fake.getClosestPointOnRoads = function(kind, x, z)
  if x >= 15000 and x < 20000 and z >= 5000 and z < 10000 then
    return 16000, 9000
  end
  return nil
end

--------------------------------------------------------------------------------
T.group("what the theatre cannot answer")
--------------------------------------------------------------------------------

-- Flat everywhere and no road anywhere: nothing is authored, and that is a
-- refusal after the whole lattice, not a rectangle around nothing.
fake.GetHeight = function() return 5 end
fake.getClosestPointOnRoads = function() return nil end
logged = {}
run = new_run()
local got, steps = sweep(run)
T.eq("no authored cell refuses", got, E.REFUSED)
T.eq("after every cell", steps, 15)
T.eq("saying so", run.refusal, "the pre-sweep found no authored cell: nothing to extract")
T.eq("no rectangle", run.presweep_bounds, false)

-- A height call that raises is a missing sample, logged once with its
-- message and counted, never a raise out of the step. Here every call in the
-- bumpy corner raises, so those cells read flat and the road cell alone is
-- authored.
fake.GetHeight = counted(function(x, z)
  if x < 10000 and z < 10000 then
    error("no height here")
  end
  return 5
end)
fake.getClosestPointOnRoads = function(kind, x, z)
  if x >= 15000 and x < 20000 and z >= 5000 and z < 10000 then
    return 16000, 9000
  end
  error("no roads here")
end
logged = {}
height_calls = 0
run = new_run()
got, steps = sweep(run)
T.eq("the sweep completes", got, E.DONE)
T.eq("one cell authored, by its road", run.presweep.authored_cells, 1)
T.eq("without reading that cell's line", height_calls, 14 * 201)
T.eq("the first height failure carries its message", log_has("terrain.GetHeight failed at 2500 2500: "), true)
T.eq("the first snap failure too", log_has("terrain.getClosestPointOnRoads failed at 2500 2500: "), true)
T.eq("and the totals are one line", log_has("pre-sweep: 804 height samples and 14 road snaps failed"), true)

-- A snap that answers something other than two numbers is no road.
fake.GetHeight = function() return 5 end
fake.getClosestPointOnRoads = function() return "far", nil end
run = new_run()
T.eq("a snap that is not a point is no road", sweep(run), E.REFUSED)

-- No snap function at all: said once, and breakpoints decide alone.
fake.GetHeight = function(x, z) return (x % 20 < 10) and 1 or 0 end
fake.getClosestPointOnRoads = nil
logged = {}
run = new_run()
T.eq("the sweep completes without it", sweep(run), E.DONE)
T.eq("every cell is authored by its line", run.presweep.authored_cells, 15)
T.eq("said once", log_has("terrain.getClosestPointOnRoads is not a function"), true)

-- No module, no height function, no bounds: each is a refusal before a step.
E.terrain_module = function() return nil end
run = new_run()
T.eq("no module refuses", E.presweep_job.start(run)(), E.REFUSED)
T.eq("saying so", run.refusal, "the terrain module is not loaded")
E.terrain_module = function() return { GetTerrainConfig = function() end } end
run = new_run()
T.eq("no height function refuses", E.presweep_job.start(run)(), E.REFUSED)
T.eq("naming it", run.refusal, "terrain.GetHeight is not a function")
E.terrain_module = function() return fake end
E.terrain_bounds = function() return nil end
run = new_run()
T.eq("no bounds refuses", E.presweep_job.start(run)(), E.REFUSED)
T.eq("naming the bounds", run.refusal, "the theatre reports no bounds rectangle")

T.done()
