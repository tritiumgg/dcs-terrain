-- Offline tests for the scenery sweep's pure parts: the box lattice, which box
-- a point lies in, reading a chunk's answer, picking a footprint, and the line.
--
-- Run from the repository root with a plain lua5.1.
--
-- Nothing here touches DCS. What a real search answers is checked live; what
-- this file holds is that every point lands in exactly one box, the same box
-- on both sides of the transport, and that the lines are the encoder's bytes.

package.path = "extractor/?.lua;extractor/test/support/?.lua;" .. package.path

local T = require("testing")
local E = require("DcsTerrainExtract")

--------------------------------------------------------------------------------
T.group("the lattice is anchored on the grid origin and clipped to its far side")
--------------------------------------------------------------------------------

-- 5.3 x 2.1 km at 50 m: the lattice at 2 km is 3 x 2, its last row and
-- column cut short by the grid.
local GRID = { origin_x = 1000, origin_z = -2000, height = 106, width = 42,
  cell_size = 50, tile_size = 20 }
local plan = E.scenery_plan(GRID, 2000)
T.eq("three rows", plan.rows, 3)
T.eq("two columns", plan.cols, 2)
T.eq("six boxes", plan.total, 6)

local x0, z0, x1, z1 = E.scenery_box(plan, 1)
T.eq("box 1 starts at the origin", x0, 1000)
T.eq("on both axes", z0, -2000)
T.eq("and is a side long", x1, 3000)
T.eq("on both axes", z1, 0)
x0, z0, x1, z1 = E.scenery_box(plan, 2)
T.eq("box 2 is the next column", z0, 0)
T.eq("cut at the grid's edge", z1, 100)
T.eq("on the same row", x0, 1000)
x0, z0, x1, z1 = E.scenery_box(plan, 5)
T.eq("box 5 is the last row", x0, 5000)
T.eq("cut at the grid's edge", x1, 6300)

T.raises("a bad side raises", function() E.scenery_plan(GRID, 0) end, "side")
T.raises("so does no grid", function() E.scenery_plan(nil, 2000) end, "grid")

--------------------------------------------------------------------------------
T.group("a point lies in one box, lower edges in and upper edges out")
--------------------------------------------------------------------------------

T.eq("the origin is box 1", E.scenery_box_of(plan, 1000, -2000), 1)
T.eq("a lower x edge belongs to the row above it", E.scenery_box_of(plan, 3000, -2000), 3)
T.eq("a lower z edge to the next column", E.scenery_box_of(plan, 1000, 0), 2)
T.eq("just short of an edge is the box before", E.scenery_box_of(plan, 2999.9999999, -1), 1)
T.eq("the grid's far x side is off it", E.scenery_box_of(plan, 6300, 0), nil)
T.eq("and its far z side", E.scenery_box_of(plan, 2000, 100), nil)
T.eq("just inside both is the last box", E.scenery_box_of(plan, 6299.99, 99.99), 6)
T.eq("before the origin is off it", E.scenery_box_of(plan, 999.99, 0), nil)
T.eq("a NaN is off it", E.scenery_box_of(plan, 0 / 0, 0), nil)

-- Edges that are not round numbers: every box's own lower corner, carried
-- through %.17g as the chunk sees it, must land in that box and no other.
local ODD = { origin_x = 0.1, origin_z = -1234.567, height = 3000, width = 7,
  cell_size = 0.7, tile_size = 256 }
local odd = E.scenery_plan(ODD, 0.3)
local wrong = 0
for n = 1, odd.total do
  local bx, bz = E.scenery_box(odd, n)
  local cx = tonumber(string.format("%.17g", bx))
  local cz = tonumber(string.format("%.17g", bz))
  if E.scenery_box_of(odd, cx, cz) ~= n then
    wrong = wrong + 1
  end
end
T.eq("every box's corner lands in that box", wrong, 0)

--------------------------------------------------------------------------------
T.group("a chunk's answer is read whole or refused")
--------------------------------------------------------------------------------

local body = "80006261\tK_BUILDING_10_OPEN\t1500.5\t163.25\t-1999\n"
  .. "7\tMOST(ROAD)BIG_END\t2999.75\t0\t-0.5\n"
  .. "2 1"
local records, dropped, raised = E.scenery_answer(body, plan, 1)
T.eq("two records", #records, 2)
T.eq("the id as its digits", records[1].id, "80006261")
T.eq("the model", records[2].model, "MOST(ROAD)BIG_END")
T.eq("x", records[1].x, 1500.5)
T.eq("altitude", records[1].alt, 163.25)
T.eq("z", records[2].z, -0.5)
T.eq("the dropped count", dropped, 2)
T.eq("the raised count", raised, 1)

records = E.scenery_answer("0 0", plan, 4)
T.eq("an empty box is no records", #records, 0)

local function refused(name, answer, box, want)
  local got, why = E.scenery_answer(answer, plan, box or 1)
  T.eq(name, got, nil)
  T.eq(name .. ", saying why", type(why) == "string" and why:find(want, 1, true) ~= nil, true)
end
refused("no counts", "1\tA\t1500\t0\t-1000\n", 1, "no counts")
refused("counts that are not two numbers", "x y", 1, "no counts")
refused("an object outside the box", "1\tA\t3000\t0\t-1000\n0 0", 1, "not in box 1")
refused("an object off the grid", "1\tA\t9000\t0\t-1000\n0 0", 1, "not in box 1")
refused("an id that is not digits", "1a\tA\t1500\t0\t-1000\n0 0", 1, "not a scenery record")
refused("an id with a leading zero", "01\tA\t1500\t0\t-1000\n0 0", 1, "whole number")
refused("a position that is not a number", "1\tA\tnan\t0\t-1000\n0 0", 1, "not finite")
refused("a missing field", "1\tA\t1500\t0\n0 0", 1, "not a scenery record")
refused("a model with a control character", "1\tA\1B\t1500\t0\t-1000\n0 0", 1, "not a scenery record")

--------------------------------------------------------------------------------
T.group("the footprint is the entry with the object's own id")
--------------------------------------------------------------------------------

local HIT = { id = "80006261", model = "k_building_10_open", radius = 39.616166273419,
  rotation = 2.9027869579643, sizeOBB = { 47.632614135742, 63.315849304199 },
  center = { 83452.6640625, 269621.28125 }, type = 65536 }
local obb, radius = E.scenery_footprint({ { id = "5", radius = 1 }, HIT }, "80006261")
T.eq("width is the first size", obb.w, 47.632614135742)
T.eq("depth the second", obb.d, 63.315849304199)
T.eq("center x the first", obb.cx, 83452.6640625)
T.eq("center z the second", obb.cz, 269621.28125)
T.eq("rotation as given", obb.rotation, 2.9027869579643)
T.eq("and the radius", radius, 39.616166273419)

obb, radius = E.scenery_footprint(nil, "1")
T.eq("no answer is no obb", obb, nil)
T.eq("and no radius", radius, nil)
obb, radius = E.scenery_footprint({ { id = "5", radius = 1 } }, "1")
T.eq("another object's entry is no obb", obb, nil)
T.eq("and no radius", radius, nil)
obb, radius = E.scenery_footprint({ { id = "1", radius = 2 } }, "1")
T.eq("an entry with no box is no obb", obb, nil)
T.eq("and keeps its radius", radius, 2)
obb, radius = E.scenery_footprint({ { id = "1", radius = 0 / 0, sizeOBB = { 1, 2 },
  center = { 3, 4 }, rotation = 0 } }, "1")
T.eq("a radius that is not finite is none", radius, nil)
T.eq("and the box stands", obb.cz, 4)
obb = E.scenery_footprint({ { id = "1", sizeOBB = { 1, 2 }, center = { 3 }, rotation = 0 } }, "1")
T.eq("a box missing a value is none", obb, nil)

--------------------------------------------------------------------------------
T.group("the line is the encoder's bytes, and reads back")
--------------------------------------------------------------------------------

local function json_line(t)
  return E.json(t) .. "\n"
end

local r = { id = "80006261", model = "K_BUILDING_10_OPEN", x = 83450.323852539062,
  alt = 1789.0020751953125, z = 269620.7236328125 }
obb, radius = E.scenery_footprint({ HIT }, r.id)
local line = E.scenery_line(r, obb, radius)
T.eq("a line with a footprint", line, json_line({ id = 80006261, model = r.model, x = r.x,
  alt = r.alt, z = r.z, radius = radius,
  obb = { w = obb.w, d = obb.d, rotation = obb.rotation, cx = obb.cx, cz = obb.cz } }))
T.eq("a line with none", E.scenery_line(r, nil, nil), json_line({ id = 80006261,
  model = r.model, x = r.x, alt = r.alt, z = r.z, radius = E.JSON_NULL, obb = E.JSON_NULL }))
local odd_model = { id = "3", model = 'A"B\\C', x = -0.1, alt = 1e-7, z = 1e21 }
T.eq("a model the encoder has to escape", E.scenery_line(odd_model, nil, 5), json_line({
  id = 3, model = 'A"B\\C', x = -0.1, alt = 1e-7, z = 1e21, radius = 5, obb = E.JSON_NULL }))

local id, x, z = E.parse_scenery_line(line:sub(1, -2))
T.eq("the id reads back", id, "80006261")
T.eq("x reads back exactly", x, r.x)
T.eq("z reads back exactly", z, r.z)
id, x, z = E.parse_scenery_line(E.scenery_line(odd_model, nil, 5):sub(1, -2))
T.eq("past an escaped model", x, -0.1)
T.eq("and z", z, 1e21)
T.eq("a line cut short is not a line", E.parse_scenery_line(line:sub(1, 60)), nil)
T.eq("nor a road line", E.parse_scenery_line(E.seed_line(1, 2, 3):sub(1, -2)), nil)

T.done()
