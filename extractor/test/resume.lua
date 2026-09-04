-- Offline tests for resume.
--
-- Run from the repository root with a plain lua5.1.
--
-- The case that matters is a run killed part way through a sweep: the tiles
-- already written must not be swept again, and the record of what went wrong
-- before the kill must survive into the continued run.

package.path = "extractor/?.lua;extractor/test/support/?.lua;" .. package.path

local T = require("testing")
local E = require("DcsTerrainExtract")
local FakeFs = require("fakefs")

E.now_iso = function() return "2026-09-04T09:12:44Z" end

-- 1024 by 1024 cells at tile 256 is four tiles a side, so half the water layer
-- is eight tiles.
local grid = E.grid_from_rect({ min_x = 0, min_z = 0, max_x = 51200, max_z = 51200 }, 50, 256)

local function opts(edit)
  local o = {
    theatre = "Synth",
    dcs_build = "0.0.0.0",
    dcs_build_timestamp = "00000000-000000",
    terrain_fingerprint = { digest = "90c9cec8", surface5 = { size = 4194304 } },
    bounds_km = { sw = { -30, -45 }, ne = { 40, 25 } },
    grid = grid,
    omit_sea_tiles = true,
    authored_bounds_m = { min_x = 0, min_z = 0, max_x = 51200, max_z = 51200 },
    authored_bounds_source = "config",
  }
  for k, v in pairs(edit or {}) do
    o[k] = v
  end
  return o
end

local fs = FakeFs.new()
E.fs = fs
E.ensure_output_dirs("C:/extract")

local fresh = E.new_manifest(opts())

--------------------------------------------------------------------------------
T.group("resume with half the tiles journalled")
--------------------------------------------------------------------------------

-- What the interrupted run had recorded before it was killed.
fresh.passes.hook.started_at = "2026-09-04T09:12:44Z"
fresh.passes.hook.frames = 101884
fresh.timing_ms = { water = 98400 }
fresh.notes = E.as_array({ "height 2_2: GetHeight failed on 4 cells" })
E.write_manifest("C:/extract", fresh)

local rows, cols = E.tile_counts(grid)
T.eq("sixteen tiles", rows * cols, 16)

local written = 0
for tx, tz in E.each_tile(grid) do
  if written < 8 then
    E.append_tile("C:/extract", E.tile_entry("water", tx, tz, 0, 2))
    written = written + 1
  end
end
T.eq("half journalled", written, 8)

local state, problems = E.prepare_resume("C:/extract", opts())
T.eq("it resumes", state ~= nil, true)
T.eq("with no problems", problems, nil)
T.eq("and says so", state.resumed, true)
T.eq("nothing was cut short", state.partial_bytes, 0)

local done, todo = 0, 0
for tx, tz in E.each_tile(grid) do
  if state.done[E.tile_key("water", tx, tz)] then
    done = done + 1
  else
    todo = todo + 1
  end
end
T.eq("eight tiles are skipped", done, 8)
T.eq("eight are still to sweep", todo, 8)
T.eq("and the height layer is untouched", state.done[E.tile_key("height", 0, 0)], nil)

-- The four fields a resume cannot recompute. Losing notes is the one that
-- turns a recorded partial failure into an extract that looks clean.
T.eq("the original start time survives", state.manifest.extracted_at, "2026-09-04T09:12:44Z")
T.eq("the pass record survives", state.manifest.passes.hook.frames, 101884)
T.eq("the timings survive", state.manifest.timing_ms.water, 98400)
T.eq("the notes survive", E.json(state.manifest.notes),
  '["height 2_2: GetHeight failed on 4 cells"]')

T.eq("the grid comes from the manifest", state.grid.origin_x, 0)
T.eq("and so does the rectangle", state.authored_bounds_m.max_x, 51200)
T.eq("with its source", state.authored_bounds_source, "config")

-- The manifest's tile list is rebuilt from the journal, sorted.
T.eq("the tile list is the journal's", #E.manifest_tiles(state.entries), 8)

--------------------------------------------------------------------------------
T.group("resume refused")
--------------------------------------------------------------------------------

local function refusal(edit)
  local state, why = E.prepare_resume("C:/extract", opts(edit))
  if state then
    return "resumed"
  end
  return table.concat(why, " | ")
end

T.eq("another theatre", refusal({ theatre = "Caucasus" }), "theatre was Synth, now Caucasus")
T.eq("another build", refusal({ dcs_build = "0.0.0.1" }),
  "dcs_build was 0.0.0.0, now 0.0.0.1")
T.eq("another build timestamp", refusal({ dcs_build_timestamp = "20260902-093323" }),
  "dcs_build_timestamp was 00000000-000000, now 20260902-093323")
T.eq("omit_sea_tiles flipped", refusal({ omit_sea_tiles = false }),
  "omit_sea_tiles was true, now false")

-- The fingerprint is the one that catches a terrain rebuilt under an unchanged
-- DCS build, which is what makes re-running the pre-sweep unnecessary.
T.eq("a rebuilt terrain",
  refusal({ terrain_fingerprint = { digest = "deadbeef", surface5 = { size = 4194304 } } })
    :find("terrain_fingerprint was", 1, true) ~= nil, true)

-- Two problems at once are both reported, so one pass of the log names
-- everything the user has to fix rather than the first thing checked.
T.eq("both problems are reported",
  refusal({ theatre = "Caucasus", dcs_build = "0.0.0.1" }),
  "theatre was Synth, now Caucasus | dcs_build was 0.0.0.0, now 0.0.0.1")

-- The grid is only compared when a fresh one was computed. A resumed pre-sweep
-- run passes none, and must not be refused for it.
local moved = E.grid_from_rect({ min_x = 0, min_z = 0, max_x = 51200, max_z = 25600 }, 50, 256)
local with_grid = select(2, E.prepare_resume("C:/extract", opts({ grid = moved })))
T.eq("a moved grid refuses", #with_grid, 1)
T.eq("naming the grid", with_grid[1]:find("grid was", 1, true), 1)

local without = opts()
without.grid = nil
T.eq("no fresh grid, no grid check", E.prepare_resume("C:/extract", without) ~= nil, true)

--------------------------------------------------------------------------------
T.group("an empty and a half-written directory")
--------------------------------------------------------------------------------

local empty = FakeFs.new()
E.fs = empty
E.ensure_output_dirs("C:/new")
local first = E.prepare_resume("C:/new", opts())
T.eq("an empty directory is a fresh run", first.resumed, false)
T.eq("with nothing done", next(first.done), nil)

-- A journal with no manifest is the window between the manifest being renamed
-- aside and the new one landing. Treating it as fresh would re-sweep
-- everything and could leave two grids' tiles in one directory.
E.append_tile("C:/new", E.tile_entry("water", 0, 0, 2, 2))
local blocked, why = E.prepare_resume("C:/new", opts())
T.eq("a journal with no manifest refuses", blocked, nil)
T.eq("saying which file is missing", why[1], "tiles.jsonl is present and manifest.json is not")

T.done()
