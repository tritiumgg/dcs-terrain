-- Offline tests for grid computation and tile addressing.
--
-- Run from the repository root with a plain lua5.1.
--
-- Two reference rectangles anchor this, and both are kept because each catches
-- something the other does not. Two more exist only to discriminate a wrong
-- formula that the reference ones agree with.

package.path = "extractor/?.lua;extractor/test/support/?.lua;" .. package.path

local T = require("testing")
local E = require("DcsTerrainExtract")
local S = require("synth_constants")

local zero = 0.0
local nan = zero / zero

--------------------------------------------------------------------------------
T.group("snapping")
--------------------------------------------------------------------------------

-- ADR 0007's Caucasus rectangle and the grid it publishes. This is the vector
-- that catches rounding to the nearest cell, which would give origin -418600
-- and a height of 8900, and an x/z swap, since the two extents differ.
local caucasus = E.grid_from_rect(
  { min_x = -418619.1875, min_z = 113728.15625, max_x = 26382.5, max_z = 943187.0625 },
  50, 256)
T.eq("caucasus origin x", caucasus.origin_x, -418650)
T.eq("caucasus origin z", caucasus.origin_z, 113700)
T.eq("caucasus height", caucasus.height, 8901)
T.eq("caucasus width", caucasus.width, 16590)

local caucasus_rows, caucasus_cols = E.tile_counts(caucasus)
T.eq("caucasus tile rows", caucasus_rows, 35)
T.eq("caucasus tile columns", caucasus_cols, 65)
T.eq("caucasus tiles", caucasus_rows * caucasus_cols, 2275)

-- The synthetic theatre's authored rectangle is inset a quarter cell on every
-- side, so snapping outward has to reproduce the grid origin exactly. This is
-- the vector that catches snapping inward, which would give -29950 and 1398.
--
-- It does not catch rounding to the nearest: a quarter-cell inset puts the
-- boundary at -599.75, which rounds and floors alike to -600. That is why the
-- Caucasus vector above stays.
local size_m = S.DEFAULT_SIZE_KM * 1000
local synth = E.grid_from_rect({
  min_x = S.ORIGIN_X_M + S.AUTHORED_INSET_M,
  min_z = S.ORIGIN_Z_M + S.AUTHORED_INSET_M,
  max_x = S.ORIGIN_X_M + size_m - S.AUTHORED_INSET_M,
  max_z = S.ORIGIN_Z_M + size_m - S.AUTHORED_INSET_M,
}, S.CELL_SIZE_M, S.TILE_SIZE)
T.eq("synth origin x", synth.origin_x, S.ORIGIN_X_M)
T.eq("synth origin z", synth.origin_z, S.ORIGIN_Z_M)
T.eq("synth height", synth.height, size_m / S.CELL_SIZE_M)
T.eq("synth width", synth.width, size_m / S.CELL_SIZE_M)

-- Neither reference rectangle separates the extent from the rectangle's own
-- span: ceil((max - min) / cell) gives 8901, 16590 and 1400 as well. This one
-- gives 1 under that formula and 2 under the right one.
local straddle = E.grid_from_rect({ min_x = 49, min_z = 49, max_x = 51, max_z = 51 }, 50, 256)
T.eq("straddling a boundary, origin", straddle.origin_x, 0)
T.eq("straddling a boundary, extent", straddle.height, 2)

-- A rectangle already on the grid must not gain a cell, which is what pins the
-- half-open convention: the far edge belongs to the next cell.
local aligned = E.grid_from_rect({ min_x = 0, min_z = 0, max_x = 100, max_z = 100 }, 50, 256)
T.eq("aligned origin", aligned.origin_x, 0)
T.eq("aligned extent", aligned.height, 2)

-- floor and ceil either side of zero, since a theatre's rectangle spans it.
local negative = E.grid_from_rect(
  { min_x = -100, min_z = -75, max_x = -50, max_z = -25 }, 50, 256)
T.eq("negative origin x", negative.origin_x, -100)
T.eq("negative origin z", negative.origin_z, -100)
T.eq("negative height", negative.height, 1)
T.eq("negative width", negative.width, 2)

local inside_one = E.grid_from_rect({ min_x = 10, min_z = 10, max_x = 20, max_z = 20 }, 50, 256)
T.eq("a rectangle inside one cell is one cell", inside_one.height, 1)

--------------------------------------------------------------------------------
T.group("bounds sources")
--------------------------------------------------------------------------------

local crop = { min_x = -285000, min_z = 683000, max_x = -275000, max_z = 693000 }
local authored = { min_x = -400000, min_z = 100000, max_x = 0, max_z = 900000 }
local presweep = { min_x = -390000, min_z = 110000, max_x = -10000, max_z = 890000 }

-- ADR 0009: a crop with no authored rectangle records both keys nil, and nil
-- means unknown rather than empty.
local by_crop = E.plan_grid({ cell_size = 50, tile_size = 256, crop_m = crop })
T.eq("the crop drives the grid", by_crop.grid.origin_x, -285000)
T.eq("the crop is recorded", by_crop.crop_m, crop)
T.eq("no authored rectangle", by_crop.authored_bounds_m, nil)
T.eq("and so no source", by_crop.authored_bounds_source, nil)

local by_config = E.plan_grid({ cell_size = 50, tile_size = 256, authored_bounds_m = authored })
T.eq("the authored rectangle drives the grid", by_config.grid.origin_x, -400000)
T.eq("source is config", by_config.authored_bounds_source, "config")
T.eq("no crop", by_config.crop_m, nil)

local by_presweep = E.plan_grid({ cell_size = 50, tile_size = 256, presweep_bounds_m = presweep })
T.eq("the pre-sweep rectangle drives the grid", by_presweep.grid.origin_x, -390000)
T.eq("source is presweep", by_presweep.authored_bounds_source, "presweep")
T.eq("and it is recorded as the authored rectangle", by_presweep.authored_bounds_m, presweep)

-- Both given: the crop cuts the grid, and the rectangle is still recorded,
-- because it is what says which of those cells are authored terrain.
local by_both = E.plan_grid({
  cell_size = 50, tile_size = 256, crop_m = crop, authored_bounds_m = authored })
T.eq("the crop wins", by_both.grid.origin_x, -285000)
T.eq("the rectangle survives", by_both.authored_bounds_m, authored)
T.eq("with its source", by_both.authored_bounds_source, "config")

local config_wins = E.plan_grid({
  cell_size = 50, tile_size = 256, authored_bounds_m = authored, presweep_bounds_m = presweep })
T.eq("config beats a pre-sweep", config_wins.authored_bounds_source, "config")
T.eq("and its rectangle is the one used", config_wins.grid.origin_x, -400000)

--------------------------------------------------------------------------------
T.group("refusals")
--------------------------------------------------------------------------------

local box = { min_x = 0, min_z = 0, max_x = 10, max_z = 10 }

T.raises("nothing to work from",
  function() return E.plan_grid({ cell_size = 50, tile_size = 256 }) end,
  "no crop, authored bounds or pre-sweep")
T.raises("min above max",
  function() return E.grid_from_rect({ min_x = 10, min_z = 0, max_x = 0, max_z = 10 }, 50, 256) end,
  "rectangle is empty")
T.raises("zero width",
  function() return E.grid_from_rect({ min_x = 0, min_z = 0, max_x = 0, max_z = 10 }, 50, 256) end,
  "rectangle is empty")
T.raises("a bound that is not a number",
  function() return E.grid_from_rect({ min_x = nan, min_z = 0, max_x = 10, max_z = 10 }, 50, 256) end,
  "min_x is not a finite number")
T.raises("a missing bound",
  function() return E.grid_from_rect({ min_x = 0, min_z = 0, max_x = 10 }, 50, 256) end,
  "max_z is not a finite number")
T.raises("not a rectangle", function() return E.grid_from_rect(7, 50, 256) end, "not a rectangle")
T.raises("zero cell size", function() return E.grid_from_rect(box, 0, 256) end,
  "cell_size is not a positive integer")
T.raises("fractional cell size", function() return E.grid_from_rect(box, 12.5, 256) end,
  "cell_size is not a positive integer")
T.raises("missing tile size", function() return E.grid_from_rect(box, 50, nil) end,
  "tile_size is not a positive integer")

--------------------------------------------------------------------------------
T.group("addressing")
--------------------------------------------------------------------------------

-- 1400 cells at tile 256 is six tiles reaching 1536, so the last tile row and
-- column stand partly outside the grid. That is the case a sweep has to fill
-- with nodata, and the case cell_in_grid exists to answer.
local rows, cols = E.tile_counts(synth)
T.eq("tile rows", rows, 6)
T.eq("tile columns", cols, 6)

local first_x, first_z = E.cell_centre(synth, 0, 0)
T.eq("first cell centre x", first_x, S.ORIGIN_X_M + 25)
T.eq("first cell centre z", first_z, S.ORIGIN_Z_M + 25)
local last_x, last_z = E.cell_centre(synth, synth.height - 1, synth.width - 1)
T.eq("last cell centre x", last_x, S.ORIGIN_X_M + size_m - 25)
T.eq("last cell centre z", last_z, S.ORIGIN_Z_M + size_m - 25)

local row0, col0 = E.tile_first_cell(synth, 2, 3)
T.eq("first row of a tile", row0, 512)
T.eq("first column of a tile", col0, 768)

T.eq("the last cell is inside", E.cell_in_grid(synth, 1399, 1399), true)
T.eq("one row past is outside", E.cell_in_grid(synth, 1400, 0), false)
T.eq("one column past is outside", E.cell_in_grid(synth, 0, 1400), false)
T.eq("negative is outside", E.cell_in_grid(synth, -1, 0), false)
T.eq("the last tile starts inside", E.cell_in_grid(synth, 1280, 1280), true)
T.eq("and ends outside", E.cell_in_grid(synth, 1535, 1535), false)

T.eq("columns are fastest", E.tile_sample_index(synth, 0, 1), 1)
T.eq("a row is tile_size apart", E.tile_sample_index(synth, 1, 0), 256)
T.eq("the last sample", E.tile_sample_index(synth, 255, 255), 65535)

T.eq("a tile path", E.tile_path("height", 4, 9), "tiles/height/4_9.bin")
T.eq("origin tile", E.tile_path("water", 0, 0), "tiles/water/0_0.bin")
T.raises("an unknown layer", function() return E.tile_path("depth", 0, 0) end, "not a layer")

-- tx outer, tz inner, every tile once. Sized so the order is readable.
local small = E.grid_from_rect({ min_x = 0, min_z = 0, max_x = 100, max_z = 150 }, 50, 1)
local visited = {}
for tx, tz in E.each_tile(small) do
  visited[#visited + 1] = tx .. "," .. tz
end
T.eq("tx outer and tz inner", table.concat(visited, " "), "0,0 0,1 0,2 1,0 1,1 1,2")

T.done()
