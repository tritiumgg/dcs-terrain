-- Offline tests for the format constants the manifest carries.
--
-- Run from the repository root with a plain lua5.1.
--
-- The values are checked against synth_constants, which is the same reference
-- the Rust generator reads. That is what makes the two sides of the extract
-- format agree on a layer's nodata rather than each carrying its own copy.

package.path = "extractor/?.lua;extractor/test/support/?.lua;" .. package.path

local T = require("testing")
local E = require("DcsTerrainExtract")
local S = require("synth_constants")

--------------------------------------------------------------------------------
T.group("versions")
--------------------------------------------------------------------------------

T.eq("format version", E.FORMAT_VERSION, S.FORMAT_VERSION)
T.eq("extractor version", E.EXTRACTOR_VERSION, S.EXTRACTOR_VERSION)

--------------------------------------------------------------------------------
T.group("layers")
--------------------------------------------------------------------------------

-- The nodata values are the other half of the encoders' contract: i16le
-- refuses to produce -32768 so that only a caller meaning nodata can write
-- one, and this is where that number is written down.
T.eq("height nodata", E.layers().height.nodata, S.HEIGHT_NODATA)
T.eq("water nodata", E.layers().water.nodata, S.WATER_NODATA)
T.eq("surface nodata", E.layers().surface.nodata, S.SURFACE_NODATA)

T.eq("height is i16", E.layers().height.dtype, "i16")
T.eq("water is u8", E.layers().water.dtype, "u8")
T.eq("surface is u8", E.layers().surface.dtype, "u8")

T.eq("height is metres", E.layers().height.unit, "m")
T.eq("water is a class", E.layers().water.unit, "class")
T.eq("surface is an enum", E.layers().surface.unit, "enum")

-- Which pass writes a layer is what tells pack whether an absent tile means
-- omitted or not yet swept.
T.eq("height is a hook layer", E.layers().height.pass, "hook")
T.eq("water is a hook layer", E.layers().water.pass, "hook")
T.eq("surface is a mission layer", E.layers().surface.pass, "mission")

T.eq("the whole block", E.json(E.layers()),
  '{"height":{"dtype":"i16","nodata":-32768,"pass":"hook","unit":"m"},'
  .. '"surface":{"dtype":"u8","nodata":0,"pass":"mission","unit":"enum"},'
  .. '"water":{"dtype":"u8","nodata":255,"pass":"hook","unit":"class"}}')

T.eq("a known layer has a spec", E.layer("height").dtype, "i16")
T.eq("an unknown layer has none", E.layer("depth"), nil)
T.eq("nil is not a layer", E.layer(nil), nil)

--------------------------------------------------------------------------------
T.group("tables")
--------------------------------------------------------------------------------

T.eq("a json table", E.table_files().config, "config.json")
T.eq("the airdrome table", E.table_files().airdromes, "airdromes.json")
T.eq("a jsonl table", E.table_files().roads, "roads.jsonl")
T.eq("the mission-pass jsonl", E.table_files().scenery, "scenery.jsonl")
T.eq("the model catalogue", E.table_files().scenery_models, "scenery_models.json")

local count = 0
for _ in pairs(E.table_files()) do
  count = count + 1
end
T.eq("twelve tables", count, 12)

--------------------------------------------------------------------------------
T.group("fresh tables")
--------------------------------------------------------------------------------

-- A manifest owns its own blocks. Handing out the shared table would let one
-- manifest's edit reach every other manifest built in the same run.
T.eq("layers are not shared", E.layers() ~= E.layers(), true)
T.eq("table files are not shared", E.table_files() ~= E.table_files(), true)

local mine = E.layers()
mine.height.nodata = 0
T.eq("editing a copy leaves the source alone", E.layers().height.nodata, S.HEIGHT_NODATA)

T.done()
