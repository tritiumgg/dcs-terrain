-- Offline tests for the tile journal.
--
-- Run from the repository root with a plain lua5.1.
--
-- The journal is the only per-tile record. The manifest is rewritten at phase
-- changes and never per tile, so everything a resume knows about which tiles
-- exist comes from here.

package.path = "extractor/?.lua;extractor/test/support/?.lua;" .. package.path

local T = require("testing")
local E = require("DcsTerrainExtract")
local FakeFs = require("fakefs")

--------------------------------------------------------------------------------
T.group("an entry")
--------------------------------------------------------------------------------

local coastal = E.tile_entry("height", 4, 9, -3, 412)
T.eq("the entry", E.json(coastal),
  '{"layer":"height","max":412,"min":-3,"path":"tiles/height/4_9.bin","tx":4,"tz":9}')
T.eq("the path is derived, never passed in", coastal.path, "tiles/height/4_9.bin")

-- A tile whose every sample is nodata has no min and no max, and they are null
-- together rather than absent: an absent key cannot be told from an older
-- extractor that did not write one.
local empty = E.tile_entry("water", 0, 0, nil, nil)
T.eq("all nodata", E.json(empty),
  '{"layer":"water","max":null,"min":null,"path":"tiles/water/0_0.bin","tx":0,"tz":0}')

local flat = E.tile_entry("water", 1, 2, 2, 2)
T.eq("a tile of one value", E.json(flat),
  '{"layer":"water","max":2,"min":2,"path":"tiles/water/1_2.bin","tx":1,"tz":2}')

T.eq("a line ends in a newline", E.journal_line(coastal), E.json(coastal) .. "\n")

--------------------------------------------------------------------------------
T.group("entry refusals")
--------------------------------------------------------------------------------

local function entry(edit)
  local e = {
    layer = "height", tx = 4, tz = 9, path = "tiles/height/4_9.bin",
    min = -3, max = 412,
  }
  for k, v in pairs(edit) do
    e[k] = v
  end
  return e
end

T.raises("an unknown layer",
  function() return E.check_tile_entry(entry({ layer = "depth" })) end, "not a layer")
T.raises("a fractional address",
  function() return E.check_tile_entry(entry({ tx = 4.5 })) end, "is not a tile address")
T.raises("a negative address",
  function() return E.check_tile_entry(entry({ tz = -1 })) end, "is not a tile address")
-- Written out rather than through entry(), because a nil in a table literal is
-- not a key and the edit would be a no-op.
T.raises("a missing address", function()
  return E.check_tile_entry({ layer = "height", tz = 9, path = "tiles/height/4_9.bin",
    min = -3, max = 412 })
end, "is not a tile address")

-- This is the check that stops a line naming a file that does not exist, which
-- is the failure validation reports and cannot repair.
T.raises("a path that does not match the address",
  function() return E.check_tile_entry(entry({ path = "tiles/height/9_4.bin" })) end,
  "path is tiles/height/9_4.bin, not tiles/height/4_9.bin")
T.raises("a path from another layer",
  function() return E.check_tile_entry(entry({ path = "tiles/water/4_9.bin" })) end,
  "not tiles/height/4_9.bin")

T.raises("min null and max not",
  function() return E.check_tile_entry(entry({ min = E.JSON_NULL })) end,
  "null together or not at all")
T.raises("max null and min not",
  function() return E.check_tile_entry(entry({ max = E.JSON_NULL })) end,
  "null together or not at all")
T.raises("min above max",
  function() return E.check_tile_entry(entry({ min = 500 })) end, "is above max")
T.raises("a min that is not a number",
  function() return E.check_tile_entry(entry({ min = "low" })) end, "are not both numbers")
T.raises("not an entry at all",
  function() return E.check_tile_entry(7) end, "not an entry")

--------------------------------------------------------------------------------
T.group("parsing")
--------------------------------------------------------------------------------

local whole = E.journal_line(E.tile_entry("water", 0, 0, 2, 2))
  .. E.journal_line(E.tile_entry("water", 0, 1, 0, 3))
  .. E.journal_line(E.tile_entry("height", 0, 1, 5, 900))

local read, dropped = E.parse_journal(whole)
T.eq("three lines", #read, 3)
T.eq("nothing dropped", dropped, 0)
T.eq("in order", read[3].layer, "height")
T.eq("with their values", read[2].max, 3)

T.eq("an empty journal", #(E.parse_journal("")), 0)

-- A run killed between the tile rename and the journal append leaves a partial
-- line. The complete lines still count, the fragment does not, and the tile it
-- named is swept again.
local cut = whole:sub(1, #whole - 20)
local survivors, lost = E.parse_journal(cut)
T.eq("the complete lines survive", #survivors, 2)
-- The fragment is what is left of the third line, which is where the cut fell.
T.eq("the fragment is counted, not parsed", lost,
  #E.journal_line(E.tile_entry("height", 0, 1, 5, 900)) - 20)
T.eq("the last survivor is the second line", survivors[2].tz, 1)

-- A fragment that is not valid JSON still must not raise: that is the normal
-- shape of an interrupted run, not a corrupt journal.
local fragment = '{"layer":"height","tx":'
local kept, partial = E.parse_journal(whole .. fragment)
T.eq("a fragment of an object is dropped", #kept, 3)
T.eq("and counted", partial, #fragment)

-- A complete line that is corrupt is a different thing and does raise.
T.raises("a complete but corrupt line",
  function() return E.parse_journal('{"layer":"height"}\n') end, "is not a tile address")
T.raises("a complete but unparseable line",
  function() return E.parse_journal("not json\n") end, "json decode")

--------------------------------------------------------------------------------
T.group("index and manifest order")
--------------------------------------------------------------------------------

T.eq("a key", E.tile_key("height", 4, 9), "height/4_9")

-- A tile written before a resume and again after it has two lines. The second
-- describes the file actually on disk, so it wins.
local twice = {
  E.tile_entry("height", 0, 0, 1, 2),
  E.tile_entry("height", 0, 0, 10, 20),
}
local index = E.journal_index(twice)
T.eq("one entry for the tile", index["height/0_0"].min, 10)

local unordered = {
  E.tile_entry("water", 2, 0, 2, 2),
  E.tile_entry("height", 1, 3, 0, 9),
  E.tile_entry("water", 0, 10, 0, 2),
  E.tile_entry("height", 1, 3, 0, 9),
  E.tile_entry("water", 0, 2, 1, 2),
}
local ordered = E.manifest_tiles(unordered)
T.eq("the duplicate is gone", #ordered, 4)

local seen = {}
for i = 1, #ordered do
  seen[i] = E.tile_key(ordered[i].layer, ordered[i].tx, ordered[i].tz)
end
-- Layer, then tx, then tz. tz 2 before tz 10 because they are compared as
-- numbers; sorting the paths as strings would put 10 first.
T.eq("sorted by layer, tx, tz", table.concat(seen, " "),
  "height/1_3 water/0_2 water/0_10 water/2_0")

T.eq("an empty list is still an array", E.json(E.manifest_tiles({})), "[]")

--------------------------------------------------------------------------------
T.group("through the file layer")
--------------------------------------------------------------------------------

local fs = FakeFs.new()
E.fs = fs
E.ensure_output_dirs("C:/extract")

T.eq("no journal is a fresh run", #(E.load_journal("C:/extract")), 0)

T.eq("append", E.append_tile("C:/extract", E.tile_entry("water", 0, 0, 2, 2)), true)
T.eq("append again", E.append_tile("C:/extract", E.tile_entry("water", 0, 1, 0, 3)), true)
T.eq("the file is named tiles.jsonl", fs.files["C:/extract/tiles.jsonl"] ~= nil, true)

local loaded, leftover = E.load_journal("C:/extract")
T.eq("both come back", #loaded, 2)
T.eq("nothing partial", leftover, 0)
T.eq("with their addresses", E.tile_key(loaded[2].layer, loaded[2].tx, loaded[2].tz),
  "water/0_1")

-- A round trip through the encoder, the file and the decoder has to be the
-- same entry, because that is what a resume depends on.
T.eq("a nodata tile round trips", (function()
  E.append_tile("C:/extract", E.tile_entry("height", 3, 3, nil, nil))
  local back = E.load_journal("C:/extract")
  return E.json(back[#back])
end)(), E.json(E.tile_entry("height", 3, 3, nil, nil)))

T.raises("a bad entry never reaches the file",
  function() return E.append_tile("C:/extract", { layer = "depth", tx = 0, tz = 0 }) end,
  "not a layer")

T.done()
