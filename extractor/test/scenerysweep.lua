-- Offline tests for the scenery sweep: the boxes it asks the server state for,
-- the objects each keeps, the footprints it attaches and the file it writes.
--
-- Run from the repository root with a plain lua5.1.
--
-- The server state is faked by compiling each chunk into a table holding a
-- world.searchObjects over a fixed list of objects, so the chunk the hook
-- sends is the chunk that runs. Like the real search, the fake answers
-- objects a little past the box's edges, which the chunk has to leave out.
-- What the real search and footprints answer is checked live.

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

-- 5 x 3 km at 50 m: boxes of 2 km make a lattice of 3 x 2, its last row and
-- column cut short. Box n is row floor((n - 1) / 2), column (n - 1) % 2.
local GRID = E.grid_from_rect({ min_x = 0, min_z = 0, max_x = 5000, max_z = 3000 }, 50, 20)
E.SCENERY_BOX_M = 2000

-- id, model, x, altitude, z, in the order the fake search answers them.
local OBJECTS = {
  { 101, "HOUSE_A", 100, 10, 100 },
  { 102, "HOUSE_B", 2000, 20, 0 },          -- on a lower x edge: box 3
  { 103, "WALL", 1999.5, 0, 2000 },         -- on a lower z edge: box 2
  { 104, "SHED", 4999.9, 5, 2999.9 },
  { 105, "PAST_X", 5000, 0, 100 },          -- the grid's far side: off it
  { 106, "PAST_Z", 100, 0, -0.5 },          -- before the origin: off it
  { 107, "TAB\tNAME", 300, 0, 300 },        -- dropped by the chunk
  { 108, "RAISES", 400, 0, 400 },           -- getTypeName raises
  { 109, "MOST(ROAD)BIG_END", 2500, 7, 2500 },
  { 110, 'Q"UOTE', 4500, 3, 500 },
  { 80006261, "K_BUILDING", 1000.25, 1789.0020751953125, 1500.75 },
  { 112, "HOUSE_C", 150, 0, 150 },
}

local function object(o)
  return {
    getName = function() return o[1] end,
    getTypeName = function()
      if o[2] == "RAISES" then error("no type") end
      return o[2]
    end,
    getPoint = function() return { x = o[3], y = o[4], z = o[5] } end,
  }
end

local searches = {}
local world = {
  VolumeType = { SEGMENT = 0, BOX = 1, SPHERE = 2, PYRAMID = 3 },
  searchObjects = function(category, volume, handler)
    searches[#searches + 1] = volume
    if category ~= 5 or volume.id ~= 1 then
      error("not a scenery box search")
    end
    local lo, hi = volume.params.min, volume.params.max
    for _, o in ipairs(OBJECTS) do
      if o[3] >= lo.x - 20 and o[3] <= hi.x + 20 and o[5] >= lo.z - 20 and o[5] <= hi.z + 20 then
        if handler(object(o)) == false then
          return
        end
      end
    end
  end,
}

local server = {
  world = world, Object = { Category = { SCENERY = 5 } },
  string = string, table = table, pcall = pcall, error = error,
  tostring = tostring, type = type,
}
local server_keys = {}
for k in pairs(server) do server_keys[#server_keys + 1] = k end

local chunks = 0
local cut = nil
net = {
  dostring_in = function(state, source)
    chunks = chunks + 1
    if state ~= "server" then
      return "wrong state " .. tostring(state), false
    end
    local chunk, err = loadstring(source)
    if not chunk then
      return err, false
    end
    setfenv(chunk, server)
    local ok, reply = pcall(chunk)
    if not ok then
      return tostring(reply), false
    end
    if cut and chunks == cut then
      reply = reply:sub(1, #reply - 3)
    end
    return reply, true
  end,
}

-- Footprints: an object with an even id has one, behind another object's
-- entry; 109's call raises; the rest have none.
local footprint_calls = 0
local fake = {
  getObjectsAtMapPoint = function(x, z)
    footprint_calls = footprint_calls + 1
    for _, o in ipairs(OBJECTS) do
      if o[3] == x and o[5] == z then
        if o[1] == 109 then error("no footprint here") end
        if o[1] % 2 == 0 then
          return { { id = "999", radius = 1 }, { id = tostring(o[1]), sizeOBB = { 10, 20 },
            center = { x + 1, z + 2 }, rotation = 0.5, radius = 11 } }
        end
        return nil
      end
    end
    return nil
  end,
}
E.terrain_module = function() return fake end

local DIR = "C:/extract"
local FILE = DIR .. "/scenery.jsonl"

local function new_run(fs)
  E.fs = fs
  E.ensure_output_dirs(DIR)
  local run = E.new_run({ config = { output_dir = DIR } })
  run.manifest = { grid = GRID, passes = { hook = { complete = true }, mission = { complete = false } } }
  run.skip = { ["0_0"] = "sea" }
  return run
end

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
  return statuses, progress
end

-- The file as it should be, each line through the encoder, the objects in
-- box order and within a box in the order the search answered them.
local function line_for(o)
  local t = { id = o[1], model = o[2], x = o[3], alt = o[4], z = o[5],
    obb = E.JSON_NULL, radius = E.JSON_NULL }
  if o[1] % 2 == 0 then
    t.obb = { w = 10, d = 20, rotation = 0.5, cx = o[3] + 1, cz = o[5] + 2 }
    t.radius = 11
  end
  return E.json(t) .. "\n"
end
local BOXES = { { 101, 80006261, 112 }, { 103 }, { 102 }, { 109 }, { 110 }, { 104 } }
local BY_ID = {}
for _, o in ipairs(OBJECTS) do BY_ID[o[1]] = o end
local function expected(leave_out)
  local out = {}
  for n, ids in ipairs(BOXES) do
    if n ~= leave_out then
      for _, id in ipairs(ids) do out[#out + 1] = line_for(BY_ID[id]) end
    end
  end
  return table.concat(out)
end
-- 109 is odd, so its raising footprint call leaves it none either way.
local WHOLE = expected()

--------------------------------------------------------------------------------
T.group("the scenery sweep follows the surface sweep")
--------------------------------------------------------------------------------

T.eq("second in the mission pass", E.jobs.mission[2], E.scenery_job)
T.eq("named for its table", E.scenery_job.name, "scenery")

--------------------------------------------------------------------------------
T.group("a box a step, each object once, footprints attached")
--------------------------------------------------------------------------------

local fs = FakeFs.new()
logged = {}
local run = new_run(fs)
local statuses, progress = drive(E.scenery_job, run)
T.eq("done", statuses[#statuses], E.DONE)
T.eq("the file is every object in its box, in box order", fs.files[FILE], WHOLE)
T.eq("a search a box, the skipped tile's included", #searches, 6)
T.eq("each a column", searches[1].params.min.y, -1000)
T.eq("from below the ground to above it", searches[1].params.max.y, 10000)
T.eq("the last box is cut at the grid's x side", searches[6].params.max.x, 5000)
T.eq("and its z side", searches[6].params.max.z, 3000)
local done, total = progress()
T.eq("progress counts boxes", total, 6)
T.eq("all of them", done, 6)
T.eq("the totals", log_has("scenery: 6 boxes, 8 objects written, 4 with a footprint,"
  .. " 0 boxes refused, 1 dropped, 1 raised, 1 footprint calls failed"), true)
T.eq("the failed footprint call is logged once", log_has("terrain.getObjectsAtMapPoint failed at 2500 2500"), true)
local left = {}
for k in pairs(server) do left[#left + 1] = k end
T.eq("the chunks left no global behind", #left, #server_keys)

--------------------------------------------------------------------------------
T.group("a dense box is written a slice a step")
--------------------------------------------------------------------------------

E.SCENERY_SLICE = 1
searches = {}
fs = FakeFs.new()
run = new_run(fs)
local sliced = drive(E.scenery_job, run)
T.eq("the same file", fs.files[FILE], WHOLE)
T.eq("a step a box and a step a line, and the last", #sliced, 6 + 8 + 1)
E.SCENERY_SLICE = 4096

--------------------------------------------------------------------------------
T.group("a refused box writes nothing and the sweep goes on")
--------------------------------------------------------------------------------

chunks = 0
cut = 3
fs = FakeFs.new()
logged = {}
run = new_run(fs)
statuses = drive(E.scenery_job, run)
cut = nil
T.eq("done", statuses[#statuses], E.DONE)
T.eq("box 3's object is not there", fs.files[FILE], expected(3))
T.eq("the box is logged with its edges",
  log_has("scenery box 3 (x 2000..4000, z 0..2000) refused: declared"), true)
T.eq("and counted", log_has("1 boxes refused"), true)

--------------------------------------------------------------------------------
T.group("what the sweep refuses")
--------------------------------------------------------------------------------

E.terrain_module = function() return nil end
run = new_run(FakeFs.new())
T.eq("no terrain module refuses", E.scenery_job.start(run)(), E.REFUSED)
T.eq("saying so", run.refusal, "the terrain module is not loaded")
E.terrain_module = function() return {} end
run = new_run(FakeFs.new())
T.eq("no footprint call refuses", E.scenery_job.start(run)(), E.REFUSED)
T.eq("naming it", run.refusal, "terrain.getObjectsAtMapPoint is not a function")
E.terrain_module = function() return fake end

run = new_run(FakeFs.new())
run.manifest.grid = nil
T.eq("no grid refuses", E.scenery_job.start(run)(), E.REFUSED)

fs = FakeFs.new()
fs.lose_bytes(1)
run = new_run(fs)
statuses = drive(E.scenery_job, run)
T.eq("a short write refuses", statuses[#statuses], E.REFUSED)
T.eq("naming the file", run.refusal:find("scenery.jsonl cannot be written", 1, true), 1)

T.done()
