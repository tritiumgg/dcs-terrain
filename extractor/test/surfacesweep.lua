-- Offline tests for the surface sweep: the bands it asks the server state
-- for, the bytes a tile holds, the tiles it leaves alone and what a refused
-- answer and a resume do.
--
-- Run from the repository root with a plain lua5.1.
--
-- The server state is faked by compiling each chunk into a table holding a
-- closed-form land.getSurfaceType, so the chunk the hook sends is the chunk
-- that runs. The grid is 7 by 7 cells at tiles of 4 and bands of 2 rows,
-- which puts a grid edge through three of the four tiles. What the real call
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

local GRID = E.grid_from_rect({ min_x = 0, min_z = 0, max_x = 350, max_z = 350 }, 50, 4)
T.eq("7 rows", GRID.height, 7)
T.eq("7 columns", GRID.width, 7)

-- Every cell answers (row + col) % 5 + 1, but for one that answers a value
-- outside the enum and one whose call raises. asked counts the cells.
local UNKNOWN, RAISES = { 0, 1 }, { 0, 2 }
local asked = 0
local function answer(row, col)
  if row == UNKNOWN[1] and col == UNKNOWN[2] then return 7 end
  return (row + col) % 5 + 1
end
local land = {
  getSurfaceType = function(p)
    asked = asked + 1
    local row, col = floor(p.x / 50), floor(p.y / 50)
    if row == RAISES[1] and col == RAISES[2] then
      error("no surface here")
    end
    return answer(row, col)
  end,
}

local server = {
  land = land, string = string, table = table, pcall = pcall, error = error,
}
local chunks = 0
local cut = nil
local function run_in_server(state, source)
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
end
net = { dostring_in = run_in_server }

-- The tile's bytes as they should be: nodata off the grid, at the unknown
-- value and at the call that raised, the enum everywhere else.
local function expected(tx, tz, refused_rows)
  local out = {}
  for lr = 0, 3 do
    for lc = 0, 3 do
      local row, col = tx * 4 + lr, tz * 4 + lc
      local v = 0
      if row < 7 and col < 7 and not (refused_rows and refused_rows[lr])
          and not (row == UNKNOWN[1] and col == UNKNOWN[2])
          and not (row == RAISES[1] and col == RAISES[2]) then
        v = answer(row, col)
      end
      out[#out + 1] = string.char(v)
    end
  end
  return table.concat(out)
end

local function new_run(fs)
  E.fs = fs
  E.ensure_output_dirs("C:/extract")
  local run = E.new_run({ config = { output_dir = "C:/extract" } })
  run.manifest = { grid = GRID }
  run.skip = { ["1_1"] = "sea" }
  return run
end

local function tile(fs, tx, tz)
  return fs.files["C:/extract/" .. E.tile_path("surface", tx, tz)]
end

local function lines(fs)
  local out = {}
  for line in (fs.files["C:/extract/tiles.jsonl"] or ""):gmatch("[^\n]+") do
    out[#out + 1] = line
  end
  return out
end

E.SURFACE_CHUNK_ROWS = 2

--------------------------------------------------------------------------------
T.group("the surface sweep is the mission pass's first job")
--------------------------------------------------------------------------------

T.eq("first", E.jobs.mission[1], E.surface_job)
T.eq("named for its layer", E.surface_job.name, "surface")

--------------------------------------------------------------------------------
T.group("a band a step, a tile when its last band is in")
--------------------------------------------------------------------------------

local fs = FakeFs.new()
logged = {}
local run = new_run(fs)
local step, progress = E.surface_job.start(run)
T.eq("the sea tile counts as done from the start", select(1, progress()), 1)
T.eq("of four", select(2, progress()), 4)

chunks, asked = 0, 0
T.eq("the first band wants more", step(), E.MORE)
T.eq("one chunk", chunks, 1)
-- Two rows of four, and the row that raised at its third cell redone.
T.eq("of two rows of four, one of them twice", asked, 8 + 3)
T.eq("and nothing written yet", tile(fs, 0, 0), nil)
T.eq("the second band wants more", step(), E.MORE)
T.eq("tile 0_0 is written", tile(fs, 0, 0), expected(0, 0))
T.eq("and counted", select(1, progress()), 2)
T.eq("the unknown value was counted", log_has("tile 0_0: surface 1..5, 2 nodata, 1 unrecognised, 1 failed"), true)
local journal = lines(fs)
T.eq("journalled", journal[1],
  '{"layer":"surface","max":5,"min":1,"path":"tiles/surface/0_0.bin","tx":0,"tz":0}')

chunks, asked = 0, 0
step()
step()
T.eq("tile 0_1 has three columns and a pad", tile(fs, 0, 1), expected(0, 1))
T.eq("only the three are asked", asked, 12)

chunks, asked = 0, 0
step()
step()
T.eq("tile 1_0 has three rows and a nodata row", tile(fs, 1, 0), expected(1, 0))
T.eq("only three rows are asked", asked, 12)
T.eq("in two chunks", chunks, 2)

chunks = 0
T.eq("the sea tile is a step", step(), E.MORE)
T.eq("with no chunk", chunks, 0)
T.eq("and no file", tile(fs, 1, 1), nil)
T.eq("then the sweep is done", step(), E.DONE)
T.eq("all four counted", select(1, progress()), 4)
T.eq("three lines", #lines(fs), 3)
T.eq("the totals", log_has("surface: 3 tiles written, 0 journalled, 1 left out, 0 bands refused"), true)

--------------------------------------------------------------------------------
T.group("a refused answer is a band of nodata, and the tile is still written")
--------------------------------------------------------------------------------

fs = FakeFs.new()
logged = {}
run = new_run(fs)
step = E.surface_job.start(run)
chunks, cut = 0, 2
step()
step()
cut = nil
T.eq("the second band's rows are nodata", tile(fs, 0, 0), expected(0, 0, { [2] = true, [3] = true }))
T.eq("the refusal is logged with both counts",
  log_has("surface 0_0 rows 2..3 refused: declared 8 bytes and 5 arrived"), true)
T.eq("and in the tile's line", log_has("1 bands refused"), true)
T.eq("journalled all the same", #lines(fs), 1)

-- A chunk that raises in the server state is refused the same way.
local real = land.getSurfaceType
server.land = nil
logged = {}
step()
step()
server.land = land
T.eq("a raising chunk is nodata", tile(fs, 0, 1), expected(0, 1, { [0] = true, [1] = true, [2] = true, [3] = true }))
T.eq("and says why", log_has("surface 0_1 rows 0..1 refused: the chunk raised:"), true)
land.getSurfaceType = real

--------------------------------------------------------------------------------
T.group("a resume sweeps only what is not on disk")
--------------------------------------------------------------------------------

fs = FakeFs.new()
run = new_run(fs)
step = E.surface_job.start(run)
for _ = 1, 10 do
  if step() ~= E.MORE then break end
end
fs.files["C:/extract/" .. E.tile_path("surface", 1, 0)] = "short"

logged = {}
run = new_run(fs)
run.entries = E.load_journal("C:/extract")
run.done = E.journal_index(run.entries)
step, progress = E.surface_job.start(run)
T.eq("every tile counts as done", select(1, progress()), 4)
chunks = 0
step()
step()
T.eq("the journalled tiles take no chunk", chunks, 0)
step()
T.eq("the short one is swept again", chunks, 1)
T.eq("and counted back out", select(1, progress()), 3)
T.eq("and logged", log_has("tile 1_0 is journalled for surface but its file"), true)
step()
T.eq("its file is whole again", tile(fs, 1, 0), expected(1, 0))
step()
T.eq("done", step(), E.DONE)
T.eq("with the counts", log_has("surface: 1 tiles written, 2 journalled, 1 left out"), true)

--------------------------------------------------------------------------------
T.group("the chunk leaves no global behind")
--------------------------------------------------------------------------------

local names = {}
for k in pairs(server) do
  names[#names + 1] = k
end
table.sort(names)
T.eq("only what the fake put there", table.concat(names, " "), "error land pcall string table")

T.done()
