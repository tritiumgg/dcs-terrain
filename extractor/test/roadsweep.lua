-- Offline tests for the road sweeps as jobs: the seeds sweep over a fake
-- router, what it writes, what it skips, and how it picks the file back up.
--
-- Run from the repository root with a plain lua5.1.
--
-- The fake router has two straight roads, along x = 0 and along z = 0, and a
-- railway along x = 3000: the nearest road point is a projection, so every
-- answer can be worked out by hand. What the real router answers is checked
-- live.

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

-- The fake router. `axes` has both roads; `x0` only the one along x = 0, so
-- a point's road distance is its x, which is how the far case is built.
local mode = "axes"
local calls = { snap = 0, path = 0 }
local function nearest(kind, x, z)
  if kind == "railroads" then
    return 3000, z
  end
  if x > 100000 then
    return nil
  end
  if mode == "x0" or math.abs(x) <= math.abs(z) then
    return 0, z
  end
  return x, 0
end
local fake = {
  getClosestPointOnRoads = function(kind, x, z)
    calls.snap = calls.snap + 1
    return nearest(kind, x, z)
  end,
  findPathOnRoads = function()
    calls.path = calls.path + 1
    return nil
  end,
}
E.terrain_module = function() return fake end

local DIR = "C:/extract"
local FILE = DIR .. "/roads.jsonl"

-- 4 x 3 km at 50 m in 1 km tiles: twelve lattice seeds.
local GRID = { origin_x = -2000, origin_z = -1000, height = 80, width = 60,
  cell_size = 50, tile_size = 20 }
local AIRDROMES = { { id = 7, x = 10, z = 5 }, { id = 25, x = E.JSON_NULL, z = 0 } }
local TOWNS = { { name = "A", x = 60, z = 5 }, { name = "B", x = 99999, z = 0 } }

local function new_run(fs, grid)
  E.fs = fs
  E.ensure_output_dirs(DIR)
  local run = E.new_run({ config = { output_dir = DIR } })
  run.manifest = { grid = grid or GRID, passes = { hook = { complete = false } } }
  run.tables = { airdromes = AIRDROMES, towns = TOWNS }
  run.skip = { ["3_2"] = "sea" }
  return run
end

-- Drives a job to its end, or for `limit` steps, and returns the statuses.
local function drive(job, run, limit)
  local step, progress = job.start(run)
  local statuses = {}
  for _ = 1, limit or 100000 do
    local status = step()
    statuses[#statuses + 1] = status
    if status ~= E.MORE then
      break
    end
  end
  return statuses, progress, step
end

local function lines_of(text)
  local out = {}
  for line in (text or ""):gmatch("([^\n]*)\n") do
    out[#out + 1] = line
  end
  return out
end

local function ids_of(text)
  local ids = {}
  for _, line in ipairs(lines_of(text)) do
    local _, id = E.parse_road_line(line)
    ids[#ids + 1] = id
  end
  return table.concat(ids, " ")
end

local seeds = E.road_seeds_job("roads")

--------------------------------------------------------------------------------
T.group("the hook pass is config, tables, the tile sweep, then the roads")
--------------------------------------------------------------------------------

T.eq("seven jobs", #E.jobs.hook, 7)
T.eq("the road seeds after the tiles", E.jobs.hook[4].name, "roads:seeds")
T.eq("then the road paths", E.jobs.hook[5].name, "roads:paths")
T.eq("then the railroad seeds", E.jobs.hook[6].name, "railroads:seeds")
T.eq("then the railroad paths", E.jobs.hook[7].name, "railroads:paths")

--------------------------------------------------------------------------------
T.group("every placed seed gets a line with the router's answer, in plan order")
--------------------------------------------------------------------------------

local fs = FakeFs.new()
local run = new_run(fs)
calls.snap = 0
logged = {}
local statuses, progress, step = drive(seeds, run)
T.eq("the sweep finishes", statuses[#statuses], E.DONE)
T.eq("one call per placed seed", calls.snap, 13)
T.eq("one step per placed seed and one to finish", #statuses, 14)
local text = fs.files[FILE]
T.eq("thirteen lines", #lines_of(text), 13)
T.eq("ids in plan order, the skipped tile's seed and the two unplaced absent",
  ids_of(text), "1 2 3 4 5 6 7 8 9 10 11 13 15")
T.eq("no temporary left", fs.files[FILE .. ".tmp"], nil)
local first = lines_of(text)[1]
T.eq("the first line is the plan's first seed with its snap",
  first, (E.seed_line(1, -1500, -500, -1500, 0, 500):gsub("\n$", "")))
T.eq("the airdrome's line", lines_of(text)[12], (E.seed_line(13, 10, 5, 10, 0, 5):gsub("\n$", "")))
local state = run.roadnets.roads
T.eq("six kept: the merged ones share a snap with a lower id", state.kept.n, 6)
T.eq("kept ids", table.concat(state.kept.id, " "), "1 3 4 5 10 13")
T.eq("no pairs read back", state.pairs_read, 0)
T.eq("progress at the end is the whole plan", (progress()), 16)
T.eq("of sixteen positions", select(2, progress()), 16)
T.eq("the finish line counts it", log_count("roads: 13 seeds placed of 16 positions, 13 asked, 0 cleared by a disc, 6 kept, 7 merged, 0 with no road, 0 far, 0 failed, 0 read back"), 1)
T.eq("done stays done", step(), E.DONE)

--------------------------------------------------------------------------------
T.group("the file is the journal: a resumed sweep reads it back and asks nothing")
--------------------------------------------------------------------------------

local WHOLE = text
run = new_run(fs)
calls.snap = 0
logged = {}
statuses, progress = drive(seeds, run)
T.eq("finishes", statuses[#statuses], E.DONE)
T.eq("without a call", calls.snap, 0)
T.eq("the file is untouched", fs.files[FILE], WHOLE)
T.eq("the kept set is the same", table.concat(run.roadnets.roads.kept.id, " "), "1 3 4 5 10 13")
T.eq("and says what it read", log_count("roads.jsonl: 13 seed lines read back"), 1)

-- Stopped after seven seeds with the batch at four: four lines landed, three
-- were pending and are lost. The next start re-asks those three and the
-- rest, and the file comes out the same.
E.ROAD_LINE_BATCH = 4
fs = FakeFs.new()
run = new_run(fs)
calls.snap = 0
drive(seeds, run, 7)
T.eq("seven asked", calls.snap, 7)
T.eq("four lines landed", #lines_of(fs.files[FILE]), 4)
run = new_run(fs)
calls.snap = 0
statuses = drive(seeds, run)
T.eq("the rest are asked", calls.snap, 9)
T.eq("and the file equals a straight run's", fs.files[FILE], WHOLE)
T.eq("the batch flushes at the end", #lines_of(fs.files[FILE]), 13)

-- A cut-short last line is dropped and its seed asked again.
fs = FakeFs.new()
fs.files[FILE] = WHOLE:sub(1, -10)
run = new_run(fs)
calls.snap = 0
logged = {}
statuses = drive(seeds, run)
T.eq("finishes", statuses[#statuses], E.DONE)
T.eq("one seed asked again", calls.snap, 1)
T.eq("the file equals a straight run's", fs.files[FILE], WHOLE)
T.eq("no copy left", fs.files[FILE .. ".tmp"], nil)
T.eq("no aside left", fs.files[FILE .. ".old"], nil)
T.eq("and the drop is logged", log_count("a cut-short last line of"), 1)

-- Pair lines mean the seeds are complete, however many positions remain.
fs = FakeFs.new()
fs.files[FILE] = WHOLE .. E.nopath_line(1, 3) .. E.path_line(3, 4, { { x = 0, y = 1 } })
run = new_run(fs)
calls.snap = 0
logged = {}
statuses = drive(seeds, run)
T.eq("finishes", statuses[#statuses], E.DONE)
T.eq("asking nothing", calls.snap, 0)
T.eq("two pairs read", run.roadnets.roads.pairs_read, 2)
T.eq("the last from", run.roadnets.roads.last_from, 3)
T.eq("the last to", run.roadnets.roads.last_to, 4)
T.eq("logged", log_count("13 seed lines and 2 pair lines read back"), 1)

-- A swap cut short between its two renames is recovered before reading.
fs = FakeFs.new()
fs.files[FILE .. ".old"] = WHOLE
run = new_run(fs)
calls.snap = 0
statuses = drive(seeds, run)
T.eq("the aside is the file", fs.files[FILE], WHOLE)
T.eq("nothing asked", calls.snap, 0)

E.ROAD_LINE_BATCH = 64

--------------------------------------------------------------------------------
T.group("a file from another plan refuses, naming the file")
--------------------------------------------------------------------------------

fs = FakeFs.new()
fs.files[FILE] = E.seed_line(99, 0, 0, 0, 0, 0)
run = new_run(fs)
statuses = drive(seeds, run)
T.eq("refused", statuses[#statuses], E.REFUSED)
T.eq("with the reason", run.refusal,
  "roads.jsonl does not match this extract's seed plan (seed line 99 after seed 0 of 16): move it aside to sweep roads again")

fs = FakeFs.new()
fs.files[FILE] = E.seed_line(2, 0, 0, 0, 0, 0) .. E.seed_line(2, 0, 0, 0, 0, 0)
run = new_run(fs)
statuses = drive(seeds, run)
T.eq("an id that does not climb refuses", statuses[#statuses], E.REFUSED)

fs = FakeFs.new()
fs.files[FILE] = "not json\n"
run = new_run(fs)
statuses = drive(seeds, run)
T.eq("a line of another kind refuses", statuses[#statuses], E.REFUSED)
T.eq("saying so", run.refusal:find("a line that is not a seed", 1, true) ~= nil, true)

--------------------------------------------------------------------------------
T.group("a seed provably far from every road is neither asked nor written")
--------------------------------------------------------------------------------

-- 40 x 5 km with the road along x = 0: rows 0 to 24 lie within 25 km of it,
-- rows 25 on are far. The first far answer on a row clears what it can of
-- that row and the rows after.
mode = "x0"
local FAR_GRID = { origin_x = 0, origin_z = 0, height = 800, width = 100,
  cell_size = 50, tile_size = 20 }
fs = FakeFs.new()
run = new_run(fs, FAR_GRID)
run.tables = false
run.skip = {}
calls.snap = 0
logged = {}
statuses = drive(seeds, run)
T.eq("finishes", statuses[#statuses], E.DONE)
local far_lines = lines_of(fs.files[FILE])
local near, far_written = 0, 0
local far_ids = {}
for _, line in ipairs(far_lines) do
  local _, id, x = E.parse_road_line(line)
  if x <= 25000 then
    near = near + 1
  else
    far_written = far_written + 1
    far_ids[#far_ids + 1] = id
  end
end
T.eq("every near seed is written", near, 125)
T.eq("far seeds are asked only where no disc settles them", far_written < 30, true)
T.eq("and the calls are the lines", calls.snap, #far_lines)
T.eq("row 25 is at 25.5 km, too near for a disc, so all five are asked",
  table.concat(far_ids, " ", 1, 5), "126 127 128 129 130")
T.eq("row 26's first answer clears its neighbour", far_ids[6] .. " " .. far_ids[7], "131 133")
state = run.roadnets.roads
T.eq("the near seeds of a column share a snap: five kept", state.kept.n, 5)
T.eq("a far seed is not among them", state.kept.id[5] <= 125, true)
T.eq("the count says so", log_count("125 with no road, 0 far") == 0 and log_count(" far, 0 failed") == 1, true)
mode = "axes"

--------------------------------------------------------------------------------
T.group("what the router cannot answer, and what stops the sweep")
--------------------------------------------------------------------------------

-- A snap that raises is a seed with no road and one log line.
local raising = 0
fake.getClosestPointOnRoads = function(kind, x, z)
  calls.snap = calls.snap + 1
  if x == 500 then
    raising = raising + 1
    error("router down")
  end
  return nearest(kind, x, z)
end
fs = FakeFs.new()
run = new_run(fs)
logged = {}
statuses = drive(seeds, run)
T.eq("the sweep finishes", statuses[#statuses], E.DONE)
T.eq("three seeds raised", raising, 3)
T.eq("logged once", log_count("terrain.getClosestPointOnRoads(roads) failed at 500"), 1)
local raised = lines_of(fs.files[FILE])[7]
T.eq("written with no road", raised, (E.seed_line(7, 500, -500):gsub("\n$", "")))
T.eq("and counted as failed", log_count("3 with no road, 0 far, 3 failed"), 1)
fake.getClosestPointOnRoads = function(kind, x, z)
  calls.snap = calls.snap + 1
  return nearest(kind, x, z)
end

-- An answer of nil is a seed with no road, and a null snap.
fs = FakeFs.new()
run = new_run(fs)
run.tables = { airdromes = {}, towns = { { name = "FAR", x = 1900, z = 900 } } }
local real_nearest = nearest
nearest = function(kind, x, z)
  if x == 1900 then return nil end
  return real_nearest(kind, x, z)
end
statuses = drive(seeds, run)
nearest = real_nearest
T.eq("finishes", statuses[#statuses], E.DONE)
T.eq("the town has null fields", lines_of(fs.files[FILE])[12], (E.seed_line(13, 1900, 900):gsub("\n$", "")))
T.eq("and is not kept", run.roadnets.roads.kept.n, 5)

E.terrain_module = function() return nil end
run = new_run(FakeFs.new())
T.eq("no module refuses", seeds.start(run)(), E.REFUSED)
T.eq("saying so", run.refusal, "the terrain module is not loaded")
E.terrain_module = function() return fake end

local find_path = fake.findPathOnRoads
fake.findPathOnRoads = nil
run = new_run(FakeFs.new())
T.eq("no path call refuses before any seed is asked", seeds.start(run)(), E.REFUSED)
T.eq("naming it", run.refusal, "terrain.findPathOnRoads is not a function")
fake.findPathOnRoads = find_path

run = new_run(FakeFs.new())
run.manifest.grid = nil
T.eq("no grid refuses", seeds.start(run)(), E.REFUSED)

fs = FakeFs.new()
fs.lose_bytes(1)
run = new_run(fs)
statuses = drive(seeds, run)
T.eq("a short write refuses", statuses[#statuses], E.REFUSED)
T.eq("naming the file", run.refusal:find("roads.jsonl cannot be written", 1, true), 1)

fs = FakeFs.new()
run = new_run(fs)
run.manifest.passes.hook.complete = true
calls.snap = 0
logged = {}
statuses = drive(seeds, run)
T.eq("a complete hook pass is done at once", statuses[1], E.DONE)
T.eq("with no call", calls.snap, 0)
T.eq("and no file", fs.files[FILE], nil)
T.eq("saying why", log_count("roads: the hook pass is complete"), 1)

--------------------------------------------------------------------------------
T.group("the paths sweep asks each unordered pair of neighbours once, lower id first")
--------------------------------------------------------------------------------

-- The fake route: along one road when both snaps lie on it, else through
-- the crossing; nothing for a railway point off the railway.
fake.findPathOnRoads = function(kind, x1, z1, x2, z2)
  calls.path = calls.path + 1
  if kind == "railroads" then
    if x1 == 3000 and x2 == 3000 then
      return { { x = x1, y = z1 }, { x = x2, y = z2 } }
    end
    return nil
  end
  if (x1 == 0 and x2 == 0) or (z1 == 0 and z2 == 0) then
    return { { x = x1, y = z1 }, { x = x2, y = z2 } }
  end
  return { { x = x1, y = z1 }, { x = 0, y = 0 }, { x = x2, y = z2 } }
end
local paths = E.road_paths_job("roads")


local function pairs_in(text)
  local list, set = {}, {}
  for _, line in ipairs(lines_of(text)) do
    local k, from, to = E.parse_road_line(line)
    if k == "path" or k == "nopath" then
      local key = from .. "-" .. to
      list[#list + 1] = key
      set[key] = (set[key] or 0) + 1
    end
  end
  return list, set
end

fs = FakeFs.new()
run = new_run(fs)
drive(seeds, run)
calls.path = 0
logged = {}
statuses, progress, step = drive(paths, run)
T.eq("the sweep finishes", statuses[#statuses], E.DONE)
T.eq("fourteen calls", calls.path, 14)
local list, set = pairs_in(fs.files[FILE])
T.eq("fourteen pair lines", #list, 14)
T.eq("in cursor order, each from its lower id", table.concat(list, " "),
  "1-13 1-4 1-5 1-3 3-5 3-13 3-4 4-13 4-5 4-10 5-13 10-13 5-10 3-10")
for key, times in pairs(set) do
  T.eq("pair " .. key .. " once", times, 1)
end
local all = lines_of(fs.files[FILE])
T.eq("a path line carries the route, y as z", all[14],
  (E.path_line(1, 13, { { x = -1500, y = 0 }, { x = 10, y = 0 } }):gsub("\n$", "")))
T.eq("a route through the crossing", all[15],
  (E.path_line(1, 4, { { x = -1500, y = 0 }, { x = 0, y = 0 }, { x = 0, y = -500 } }):gsub("\n$", "")))
T.eq("the state is released", run.roadnets.roads, nil)
T.eq("progress is the pairs", (progress()), 14)
T.eq("of the pairs", select(2, progress()), 14)
T.eq("the finish line", log_count("roads: 14 pairs of 6 kept seeds, 14 paths with "), 1)
T.eq("and its tail", log_count(" points, 0 with no path, 0 failed, 0 not a polyline, 0 read back"), 1)
T.eq("done stays done", step(), E.DONE)
T.eq("a complete file, twenty-seven lines", #all, 27)

--------------------------------------------------------------------------------
T.group("a resumed paths sweep passes over the pairs the file holds")
--------------------------------------------------------------------------------

local WHOLE2 = fs.files[FILE]
run = new_run(fs)
calls.snap, calls.path = 0, 0
drive(seeds, run)
statuses = drive(paths, run)
T.eq("finishes", statuses[#statuses], E.DONE)
T.eq("with no call", calls.snap + calls.path, 0)
T.eq("and the file untouched", fs.files[FILE], WHOLE2)

-- Stopped after five route calls with the batch at four: four landed.
E.ROAD_LINE_BATCH = 4
fs = FakeFs.new()
run = new_run(fs)
drive(seeds, run)
calls.path = 0
step = paths.start(run)
while calls.path < 5 do
  step()
end
T.eq("four pair lines landed", #pairs_in(fs.files[FILE]), 4)
run = new_run(fs)
calls.path = 0
drive(seeds, run)
logged = {}
statuses, progress = drive(paths, run)
T.eq("finishes", statuses[#statuses], E.DONE)
T.eq("ten more calls", calls.path, 10)
T.eq("the file equals a straight run's", fs.files[FILE], WHOLE2)
T.eq("read back is counted", log_count("4 read back"), 1)
E.ROAD_LINE_BATCH = 64

-- A cut-short pair line is repaired by the seeds sweep and the pair asked again.
fs = FakeFs.new()
fs.files[FILE] = WHOLE2:sub(1, -8)
run = new_run(fs)
calls.path = 0
drive(seeds, run)
statuses = drive(paths, run)
T.eq("one pair asked again", calls.path, 1)
T.eq("the file equals a straight run's", fs.files[FILE], WHOLE2)

-- A last pair line the plan would not have produced there refuses.
fs = FakeFs.new()
fs.files[FILE] = WHOLE2:gsub('"from":3,"kind":"path","points":%b[],"to":10}\n$', '"from":3,"kind":"nopath","to":13}\n')
T.eq("the fixture changed the last line", fs.files[FILE] ~= WHOLE2, true)
run = new_run(fs)
drive(seeds, run)
statuses = drive(paths, run)
T.eq("refused", statuses[#statuses], E.REFUSED)
T.eq("naming the line", run.refusal,
  "roads.jsonl does not match this extract's seed plan (pair line 14 is 3-13 where the plan has 3-10): move it aside to sweep roads again")

fs = FakeFs.new()
fs.files[FILE] = WHOLE2 .. E.nopath_line(1, 2)
run = new_run(fs)
drive(seeds, run)
statuses = drive(paths, run)
T.eq("more pair lines than pairs refuses", statuses[#statuses], E.REFUSED)
T.eq("saying so", run.refusal:find("15 pair lines where the plan has 14 pairs", 1, true) ~= nil, true)

--------------------------------------------------------------------------------
T.group("the walk is sliced by the clock, and progress follows it")
--------------------------------------------------------------------------------

local now = 0
local real_clock = E.clock
E.clock = function()
  now = now + 0.003
  return now
end
fs = FakeFs.new()
run = new_run(fs)
drive(seeds, run)
step, progress = paths.start(run)
T.eq("nothing walked", (progress()), 0)
T.eq("of six kept", select(2, progress()), 6)
T.eq("the index is one step", step(), E.MORE)
T.eq("and one to see it done", step(), E.MORE)
step()
T.eq("one seed walked a step", (progress()), 1)
for _ = 1, 5 do step() end
T.eq("all six walked", (progress()), 6)
step()
T.eq("then the pairs", select(2, progress()), 14)
T.eq("none passed yet", (progress()), 0)
E.clock = real_clock

--------------------------------------------------------------------------------
T.group("what the router cannot route, and what stops the paths sweep")
--------------------------------------------------------------------------------

local good_path = fake.findPathOnRoads
fake.findPathOnRoads = function(kind, x1, z1, x2, z2)
  calls.path = calls.path + 1
  if x1 == -1500 then
    error("router down")
  end
  if z1 == 1500 then
    return { { x = 1, y = 2 }, { x = 0 / 0, y = 0 } }
  end
  return nil
end
fs = FakeFs.new()
run = new_run(fs)
drive(seeds, run)
logged = {}
statuses = drive(paths, run)
T.eq("finishes", statuses[#statuses], E.DONE)
list = pairs_in(fs.files[FILE])
T.eq("every pair has a line", #list, 14)
T.eq("all of them nopath", log_count("14 with no path, 4 failed, 3 not a polyline"), 1)
T.eq("the raise logged once", log_count("terrain.findPathOnRoads(roads) failed between 1 and 13: "), 1)
T.eq("the shape logged once", log_count("is not a polyline: point 2 is not finite"), 1)
fake.findPathOnRoads = good_path

run = new_run(FakeFs.new())
T.eq("no seeds sweep refuses", paths.start(run)(), E.REFUSED)
T.eq("saying so", run.refusal, "the roads seeds sweep has not run")

E.terrain_module = function() return nil end
run = new_run(FakeFs.new())
T.eq("no module refuses", paths.start(run)(), E.REFUSED)
E.terrain_module = function() return fake end

fs = FakeFs.new()
run = new_run(fs)
drive(seeds, run)
fs.lose_bytes(1)
statuses = drive(paths, run)
T.eq("a short write refuses", statuses[#statuses], E.REFUSED)
T.eq("naming the file", run.refusal:find("roads.jsonl cannot be written", 1, true), 1)

run = new_run(FakeFs.new())
run.manifest.passes.hook.complete = true
run.roadnets = { roads = {} }
calls.path = 0
T.eq("a complete hook pass is done at once", paths.start(run)(), E.DONE)
T.eq("with no call", calls.path, 0)
T.eq("and the state released", run.roadnets.roads, nil)

--------------------------------------------------------------------------------
T.group("railroads are the same two sweeps with the other word and their own file")
--------------------------------------------------------------------------------

local kinds_asked = {}
local plain_snap = fake.getClosestPointOnRoads
fake.getClosestPointOnRoads = function(kind, x, z)
  kinds_asked[kind] = (kinds_asked[kind] or 0) + 1
  return plain_snap(kind, x, z)
end
fs = FakeFs.new()
run = new_run(fs)
calls.path = 0
logged = {}
statuses = drive(E.road_seeds_job("railroads"), run)
T.eq("the seeds finish", statuses[#statuses], E.DONE)
T.eq("every snap asked for railroads", kinds_asked.railroads, 13)
T.eq("and none for roads", kinds_asked.roads, nil)
state = run.roadnets.railroads
T.eq("a column shares its railway point: four kept", state.kept.n, 4)
T.eq("the airdrome among them", state.kept.id[4], 13)
statuses = drive(E.road_paths_job("railroads"), run)
T.eq("the paths finish", statuses[#statuses], E.DONE)
T.eq("six pairs among four", calls.path, 6)
list = pairs_in(fs.files[DIR .. "/railroads.jsonl"])
T.eq("six pair lines", #list, 6)
T.eq("every pair a path along the railway", log_count("railroads: 6 pairs of 4 kept seeds, 6 paths with 12 points, 0 with no path"), 1)
T.eq("nothing written for roads", fs.files[FILE], nil)
T.eq("the railroad state released", run.roadnets.railroads, nil)
fake.getClosestPointOnRoads = plain_snap

T.done()
