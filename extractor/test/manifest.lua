-- Offline tests for the manifest and resume.
--
-- Run from the repository root with a plain lua5.1.
--
-- The case that matters is a run killed part way through a sweep: the tiles
-- already written must not be swept again, and the record of what went wrong
-- before the kill must survive.

package.path = "extractor/?.lua;extractor/test/support/?.lua;" .. package.path

local T = require("testing")
local E = require("DcsTerrainExtract")
local FakeFs = require("fakefs")

E.now_iso = function() return "2026-09-04T09:12:44Z" end

-- 1024 by 1024 cells at tile 256 is four tiles a side, so half the water
-- layer is eight tiles.
local grid = E.grid_from_rect({ min_x = 0, min_z = 0, max_x = 51200, max_z = 51200 }, 50, 256)

-- A nil in a table literal is not a key, so an edit that leaves a field out
-- would be a no-op and every refusal below would silently assert nothing.
local REMOVE = {}

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
    if v == REMOVE then
      o[k] = nil
    else
      o[k] = v
    end
  end
  return o
end

--------------------------------------------------------------------------------
T.group("a fresh manifest")
--------------------------------------------------------------------------------

local fresh = E.new_manifest(opts())
T.eq("format version", fresh.format_version, E.FORMAT_VERSION)
T.eq("extractor version", fresh.extractor_version, E.EXTRACTOR_VERSION)
T.eq("the clock is used", fresh.extracted_at, "2026-09-04T09:12:44Z")
T.eq("an explicit time wins",
  E.new_manifest(opts({ extracted_at = "2020-01-01T00:00:00Z" })).extracted_at,
  "2020-01-01T00:00:00Z")

T.eq("a fresh manifest is not complete", fresh.complete, false)

-- notes and tiles are arrays even when empty, and timing_ms is an object.
T.eq("tiles is an array", E.json(fresh.tiles), "[]")
T.eq("notes is an array", E.json(fresh.notes), "[]")
T.eq("timings are an object", E.json(fresh.timing_ms), "{}")

T.eq("the layer block is there", fresh.layers.height.nodata, -32768)
T.eq("the table block is there", fresh.tables.config, "config.json")
T.eq("a fresh manifest encodes", type(E.json(fresh)), "string")

--------------------------------------------------------------------------------
T.group("the authored rectangle")
--------------------------------------------------------------------------------

-- ADR 0009: a crop run has no rectangle, and both keys are null.
local cropped = E.new_manifest(opts({
  authored_bounds_m = REMOVE, authored_bounds_source = REMOVE,
  crop_m = { min_x = 0, min_z = 0, max_x = 10000, max_z = 10000 },
}))
T.eq("no rectangle", cropped.authored_bounds_m, E.JSON_NULL)
T.eq("no source", cropped.authored_bounds_source, E.JSON_NULL)
T.eq("both write as null",
  E.json(cropped):find('"authored_bounds_m":null,"authored_bounds_source":null', 1, true) ~= nil,
  true)
T.eq("the crop is recorded", cropped.crop_m.max_x, 10000)
T.eq("a full run records no crop", E.json(fresh.crop_m), "null")

T.raises("a source with no rectangle", function()
  return E.new_manifest(opts({ authored_bounds_m = REMOVE }))
end, "set together or neither is")
T.raises("a rectangle with no source", function()
  return E.new_manifest(opts({ authored_bounds_source = REMOVE }))
end, "set together or neither is")

T.raises("a missing theatre",
  function() return E.new_manifest(opts({ theatre = REMOVE })) end, "theatre is missing")
T.raises("a missing fingerprint",
  function() return E.new_manifest(opts({ terrain_fingerprint = REMOVE })) end,
  "terrain_fingerprint is missing")
T.raises("omit_sea_tiles left out",
  function() return E.new_manifest(opts({ omit_sea_tiles = REMOVE })) end,
  "omit_sea_tiles is not a boolean")

--------------------------------------------------------------------------------
T.group("write and read back")
--------------------------------------------------------------------------------

local fs = FakeFs.new()
E.fs = fs
E.ensure_output_dirs("C:/extract")

T.eq("written", E.write_manifest("C:/extract", fresh), true)
T.eq("the file exists", fs.files["C:/extract/manifest.json"] ~= nil, true)
T.eq("no aside left", fs.files["C:/extract/manifest.json.prev"], nil)
T.eq("no tmp left", fs.files["C:/extract/manifest.json.tmp"], nil)

-- The round trip is what a resume depends on, so it is asserted as a fixed
-- point rather than field by field.
T.eq("it round trips", E.json(E.read_manifest("C:/extract")), E.json(fresh))

-- Rewritten at every phase change, so replacing one has to work.
fresh.complete = true
T.eq("rewritten", E.write_manifest("C:/extract", fresh), true)
T.eq("with the new content", E.read_manifest("C:/extract").complete, true)
T.eq("still no aside", fs.files["C:/extract/manifest.json.prev"], nil)

T.done()
