-- Offline tests for the pre-sweep lattice, its bounds derivation and its
-- base64 bitmask.
--
-- Run from the repository root with a plain lua5.1.
--
-- What makes a cell authored is not tested here: that is a terrain question
-- answered by GetHeight and getClosestPointOnRoads, and it is checked against
-- a running theatre. Everything below takes the answers as given and tests the
-- arithmetic around them.

package.path = "extractor/?.lua;extractor/test/support/?.lua;" .. package.path

local T = require("testing")
local E = require("DcsTerrainExtract")

--------------------------------------------------------------------------------
T.group("base64")
--------------------------------------------------------------------------------

-- RFC 4648's own vectors, which is where the padding cases live. The Rust side
-- has to read this byte for byte, and padding is the classic off-by-one.
T.eq("empty", E.base64(""), "")
T.eq("one byte", E.base64("f"), "Zg==")
T.eq("two bytes", E.base64("fo"), "Zm8=")
T.eq("three bytes", E.base64("foo"), "Zm9v")
T.eq("four bytes", E.base64("foob"), "Zm9vYg==")
T.eq("five bytes", E.base64("fooba"), "Zm9vYmE=")
T.eq("six bytes", E.base64("foobar"), "Zm9vYmFy")

-- A bitmask is bytes, not text: a zero byte is a row with nothing authored and
-- must not end the string or be skipped.
T.eq("zero bytes", E.base64("\000\000\000"), "AAAA")
T.eq("high bytes", E.base64("\255\255\255"), "////")
T.eq("a zero in the middle", E.base64("\000\016\131"), "ABCD")

T.raises("not a string", function() return E.base64(7) end, "not a string")

--------------------------------------------------------------------------------
T.group("lattice")
--------------------------------------------------------------------------------

-- 25 km by 15 km at 5 km cells: 5 rows north, 3 columns east.
local lattice = E.presweep_lattice({ min_x = 0, min_z = 0, max_x = 25000, max_z = 15000 }, 5)
T.eq("cell size in metres", lattice.cell_m, 5000)
T.eq("rows", lattice.rows, 5)
T.eq("columns", lattice.cols, 3)

-- A partial cell at the far edge is a whole lattice cell, the same way a
-- partial tile is a whole tile.
local ragged = E.presweep_lattice({ min_x = 0, min_z = 0, max_x = 26000, max_z = 15000 }, 5)
T.eq("a partial row still counts", ragged.rows, 6)

T.eq("first index", E.presweep_index(lattice, 0, 0), 1)
T.eq("last column of the first row", E.presweep_index(lattice, 0, 2), 3)
T.eq("first column of the second row", E.presweep_index(lattice, 1, 0), 4)
T.eq("last index", E.presweep_index(lattice, 4, 2), 15)

local cx, cz = E.presweep_centre(lattice, 0, 0)
T.eq("first centre x", cx, 2500)
T.eq("first centre z", cz, 2500)
local lx, lz = E.presweep_centre(lattice, 4, 2)
T.eq("last centre x", lx, 22500)
T.eq("last centre z", lz, 12500)

T.raises("a lattice needs a rectangle",
  function() return E.presweep_lattice(7, 5) end, "not a rectangle")
T.raises("a lattice needs a cell size",
  function() return E.presweep_lattice({ min_x = 0, min_z = 0, max_x = 10, max_z = 10 }, 0) end,
  "cell_km is not a positive number")

--------------------------------------------------------------------------------
T.group("bounds from authored cells")
--------------------------------------------------------------------------------

-- Two authored cells, at (1,1) and (3,2), so rows 1..3 and columns 1..2.
local authored = {}
authored[E.presweep_index(lattice, 1, 1)] = true
authored[E.presweep_index(lattice, 3, 2)] = true

-- The rectangle bounds the cells' squares: it starts at row 1's low edge and
-- ends at row 3's high edge, which is row 4's low edge. Bounding the centres
-- instead would lose 2 500 m at every side.
local bare = E.presweep_bounds(lattice, authored, 0)
T.eq("low edge of the first authored row", bare.min_x, 5000)
T.eq("high edge of the last authored row", bare.max_x, 20000)
T.eq("low edge of the first authored column", bare.min_z, 5000)
T.eq("high edge of the last authored column", bare.max_z, 15000)

local grown = E.presweep_bounds(lattice, authored, 10000)
T.eq("margin below x", grown.min_x, -5000)
T.eq("margin above x", grown.max_x, 30000)
T.eq("margin below z", grown.min_z, -5000)
T.eq("margin above z", grown.max_z, 25000)

-- The margin is allowed to leave the theatre. Outside is fill, the per-cell
-- test writes it nodata, and clipping would cost terrain at a theatre edge.
T.eq("the rectangle may start outside the lattice", grown.min_x < lattice.min_x, true)

local single = {}
single[E.presweep_index(lattice, 2, 1)] = true
local one = E.presweep_bounds(lattice, single, 0)
T.eq("one cell is one cell wide", one.max_x - one.min_x, 5000)
T.eq("and one cell deep", one.max_z - one.min_z, 5000)

T.raises("nothing authored",
  function() return E.presweep_bounds(lattice, {}, 10000) end, "no cell is authored")
T.raises("a margin that is not a distance",
  function() return E.presweep_bounds(lattice, authored, -1) end, "not a distance")

-- The rectangle feeds straight into the grid, which is the whole point of it.
local planned = E.plan_grid({
  cell_size = 50, tile_size = 256,
  presweep_bounds_m = E.presweep_bounds(lattice, authored, 10000),
})
T.eq("source is presweep", planned.authored_bounds_source, "presweep")
T.eq("and the grid starts there", planned.grid.origin_x, -5000)

--------------------------------------------------------------------------------
T.group("bitmask")
--------------------------------------------------------------------------------

-- Three columns is one byte a row, five rows is five bytes. Row 1 has column 1
-- set (64) and row 3 has column 2 set (32).
T.eq("bitmask", E.presweep_bitmask(lattice, authored), E.base64("\000\064\000\032\000"))
T.eq("bitmask, literally", E.presweep_bitmask(lattice, authored), "AEAAIAA=")
T.eq("nothing authored is all zero bytes",
  E.presweep_bitmask(lattice, {}), E.base64("\000\000\000\000\000"))

-- Nine columns is two bytes a row, so column 8 is the top bit of the second
-- byte rather than a ninth bit appended to the first. Padding each row to a
-- byte is what keeps a row addressable on its own.
local wide = E.presweep_lattice({ min_x = 0, min_z = 0, max_x = 5000, max_z = 45000 }, 5)
T.eq("nine columns", wide.cols, 9)
local ends = {}
ends[E.presweep_index(wide, 0, 0)] = true
ends[E.presweep_index(wide, 0, 8)] = true
T.eq("first and ninth column", E.presweep_bitmask(wide, ends), E.base64("\128\128"))
T.eq("first and ninth column, literally", E.presweep_bitmask(wide, ends), "gIA=")

local full = {}
for i = 1, 9 do
  full[i] = true
end
T.eq("every column set", E.presweep_bitmask(wide, full), E.base64("\255\128"))

--------------------------------------------------------------------------------
T.group("record")
--------------------------------------------------------------------------------

local record = E.presweep_record(lattice, authored, { breakpoint_min = 60, road_max_m = 5000 })
T.eq("cell size in kilometres", record.cell_km, 5)
T.eq("breakpoint threshold", record.breakpoint_min, 60)
T.eq("road distance", record.road_max_m, 5000)
T.eq("authored cells", record.authored_cells, 2)
T.eq("total cells", record.total_cells, 15)
-- The field is `bits`, the same name a base64 bitmask carries everywhere else
-- in the project, and the packing above is the same shape.
T.eq("bits", record.bits, "AEAAIAA=")

T.eq("the record encodes", E.json(record),
  '{"authored_cells":2,"bits":"AEAAIAA=","breakpoint_min":60,'
  .. '"cell_km":5,"road_max_m":5000,"total_cells":15}')

T.done()
