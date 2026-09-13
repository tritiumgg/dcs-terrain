-- Offline tests for the road sweeps' pure parts: the seed plan, the three line
-- kinds, reading a line back, merging, the discs and their rows, the
-- neighbour search and the pair rule.
--
-- Run from the repository root with a plain lua5.1.
--
-- Nothing here touches the router. What the real one answers is checked live;
-- what this file holds is that the numbering is a function of the plan alone,
-- that the lines are the encoder's bytes, and that every unordered pair comes
-- out exactly once.

package.path = "extractor/?.lua;extractor/test/support/?.lua;" .. package.path

local T = require("testing")
local E = require("DcsTerrainExtract")

--------------------------------------------------------------------------------
T.group("the plan numbers the lattice row-major, then airdromes, then towns")
--------------------------------------------------------------------------------

-- 5.3 x 2.1 km at 50 m: 106 x 42 cells, so the lattice at 1 km is 6 x 3 and
-- its last row and column fall past the grid.
local GRID = { origin_x = 1000, origin_z = -2000, height = 106, width = 42,
  cell_size = 50, tile_size = 20 }
local airdromes = { { id = 7, x = 1500, z = -1500 }, { id = 25, x = E.JSON_NULL, z = 0 } }
local towns = { { name = "A", x = 3000, z = -1900 }, { name = "B", x = 99999, z = 0 } }

local plan = E.seed_plan(GRID, 1000, airdromes, towns)
T.eq("six rows", plan.rows, 6)
T.eq("three columns", plan.cols, 3)
T.eq("eighteen lattice seeds", plan.lattice, 18)
T.eq("and four from the tables", plan.total, 22)

local x, z = E.seed_at(plan, 1)
T.eq("seed 1 is half a spacing in", x, 1500)
T.eq("on both axes", z, -1500)
x, z = E.seed_at(plan, 2)
T.eq("seed 2 is the next column", x, 1500)
T.eq("along z", z, -500)
x, z = E.seed_at(plan, 4)
T.eq("seed 4 is the next row", x, 2500)
T.eq("first column", z, -1500)
T.eq("the third column falls past the grid's 2.1 km", E.seed_at(plan, 3), nil)
T.eq("and so does the sixth row", E.seed_at(plan, 16), nil)
x = E.seed_at(plan, 19)
T.eq("seed 19 is the first airdrome", x, 1500)
T.eq("a null position places nothing", E.seed_at(plan, 20), nil)
x = E.seed_at(plan, 21)
T.eq("seed 21 is the first town", x, 3000)
T.eq("a town off the map places nothing", E.seed_at(plan, 22), nil)

-- Tile 0_0 is the first 20 x 20 cells, which holds seed 1 and the airdrome.
local skip = { ["0_0"] = "sea" }
T.eq("a seed in a skipped tile is not placed", E.seed_at(plan, 1, skip), nil)
T.eq("nor an airdrome in one", E.seed_at(plan, 19, skip), nil)
T.eq("a seed in another tile is", (E.seed_at(plan, 4, skip)), 2500)

T.eq("no tables is a lattice alone", E.seed_plan(GRID, 1000).total, 18)
T.raises("a bad spacing raises", function() E.seed_plan(GRID, 0) end, "spacing")

--------------------------------------------------------------------------------
T.group("the lines are the encoder's bytes")
--------------------------------------------------------------------------------

local function json_line(t)
  return E.json(t) .. "\n"
end

T.eq("a seed line", E.seed_line(12, 1500, -1500, 1450.25, -1400.5, 111.80339887498948),
  json_line({ kind = "seed", id = 12, x = 1500, z = -1500, snap_x = 1450.25,
    snap_z = -1400.5, snap_dist = 111.80339887498948 }))
T.eq("a seed line with no snap", E.seed_line(3, 0.1, -0.1),
  json_line({ kind = "seed", id = 3, x = 0.1, z = -0.1, snap_x = E.JSON_NULL,
    snap_z = E.JSON_NULL, snap_dist = E.JSON_NULL }))
T.eq("a nopath line", E.nopath_line(4, 9), json_line({ kind = "nopath", from = 4, to = 9 }))
local points = { { x = 1, y = 2 }, { x = 3.5, y = -4e-7 }, { x = 1e21, y = 0 } }
T.eq("a path line, y as z", E.path_line(4, 9, points),
  json_line({ kind = "path", from = 4, to = 9,
    points = E.as_array({ { 1, 2 }, { 3.5, -4e-7 }, { 1e21, 0 } }) }))
local line, why = E.path_line(1, 2, nil)
T.eq("nil is no line", line, nil)
T.eq("with the reason", why, "not a table: nil")
line, why = E.path_line(1, 2, {})
T.eq("no points is no line", line, nil)
T.eq("saying so", why, "no points")
line, why = E.path_line(1, 2, { { x = 1, y = 2 }, { x = 0 / 0, y = 1 } })
T.eq("a point that is not finite is no line", line, nil)
T.eq("naming the point", why, "point 2 is not finite")
line, why = E.path_line(1, 2, { { x = 1, y = 2 }, 5 })
T.eq("a point that is not a table is no line", line, nil)
T.eq("naming it", why, "point 2 is a number")

--------------------------------------------------------------------------------
T.group("a line read back gives its numbers in the order the writer took them")
--------------------------------------------------------------------------------

local kind, id, sx, sz, sd
kind, id, x, z, sx, sz, sd = E.parse_road_line(E.seed_line(12, 1500, -1500, 1450.25, -1400.5, 111.5))
T.eq("a seed", kind, "seed")
T.eq("its id", id, 12)
T.eq("its x", x, 1500)
T.eq("its z", z, -1500)
T.eq("its snap x", sx, 1450.25)
T.eq("its snap z", sz, -1400.5)
T.eq("its snap distance", sd, 111.5)
kind, id, x, z, sx, sz, sd = E.parse_road_line(E.seed_line(3, 0.1, -0.1))
T.eq("a null snap is nil", sx, nil)
T.eq("and so is its distance", sd, nil)
T.eq("but x is read", x, 0.1)
local from, to
kind, from, to = E.parse_road_line(E.path_line(4, 9, points))
T.eq("a path", kind, "path")
T.eq("from", from, 4)
T.eq("to", to, 9)
kind, from, to = E.parse_road_line(E.nopath_line(40, 90))
T.eq("a nopath", kind, "nopath")
T.eq("from", from, 40)
T.eq("to", to, 90)
kind, from = E.parse_road_line('{"kind":"other"}')
T.eq("an unknown kind is nil", kind, nil)
T.eq("with the kind beside it", from, "other")
T.eq("no kind is nil", (E.parse_road_line("garbage")), nil)

-- The seventeen digits survive the round trip, which is what makes the
-- merge replay exact.
local v = 0.1 + 0.2
kind, id, x = E.parse_road_line(E.seed_line(1, v, 0))
T.eq("a double round-trips exactly", x, v)

--------------------------------------------------------------------------------
T.group("a snap merges into the lowest kept id within 100 m, and never chains")
--------------------------------------------------------------------------------

local kept = E.kept_seeds(100)
T.eq("nothing kept, nothing to merge into", E.merge_target(kept, 0, 0), nil)
T.eq("the first seed is kept at index 1", E.keep_seed(kept, 5, 0, 0), 1)
T.eq("a snap 99 m away merges", E.merge_target(kept, 99, 0), 5)
T.eq("at 100 m it merges", E.merge_target(kept, 100, 0), 5)
T.eq("at 101 m it does not", E.merge_target(kept, 101, 0), nil)
T.eq("across a bucket edge it does", E.merge_target(kept, -60, 60), 5)
E.keep_seed(kept, 9, 150, 0)
T.eq("two kept within reach: the lower id", E.merge_target(kept, 75, 0), 5)
T.eq("a snap 130 m from the first and 20 m from the second: the second", E.merge_target(kept, 130, 0), 9)
-- A seed at 190 merges into 9 (40 m). Had it been kept, a seed at 280 would
-- chain onto it; it is not, so 280 is 130 m from 9 and stands alone.
T.eq("the chain candidate is not kept", E.merge_target(kept, 280, 0), nil)
T.eq("negative coordinates bucket too", E.keep_seed(kept, 12, -100000.5, -99999.5), 3)
T.eq("and are found", E.merge_target(kept, -100050, -100040), 12)
T.eq("three kept", kept.n, 3)
T.eq("with their ids in order", kept.id[3], 12)

--------------------------------------------------------------------------------
T.group("a far answer clears a disc, and a row of seeds is tested by spans")
--------------------------------------------------------------------------------

-- A disc that exactly reaches the next seed does not cover it, since the
-- edge is outside, so it is not kept.
local edge = E.discs(25000, 1000)
T.eq("26 km away reaches the next seed exactly: no disc", E.disc_add(edge, 0, 0, 26000), false)
T.eq("26.001 km away is one", E.disc_add(edge, 0, 0, 26001), true)
T.eq("that covers the next seed", E.disc_covers(edge, 1000, 0), true)

-- The step a hair under the radius, so the numbers below stay round.
local discs = E.discs(25000, 999)
T.eq("an answer 25.9 km away clears less than a seed's step: no disc", E.disc_add(discs, 0, 0, 25900), false)
T.eq("26 km away is a 1 km disc", E.disc_add(discs, 0, 0, 26000), true)
T.eq("one disc", discs.n, 1)
T.eq("its radius is the answer less 25 km", discs.r[1], 1000)
T.eq("the center is covered", E.disc_covers(discs, 0, 0), true)
T.eq("999 m out is covered", E.disc_covers(discs, 999, 0), true)
T.eq("1000 m out is on the edge and not", E.disc_covers(discs, 1000, 0), false)

-- A second disc, radius 10 km at (0, 20000), and the row at x = 0.
E.disc_add(discs, 0, 20000, 35000)
local spans = E.disc_row(discs, 0)
T.eq("two spans", #spans, 4)
T.eq("the first from -1000", spans[1], -1000)
T.eq("to 1000", spans[2], 1000)
T.eq("the second from 10000", spans[3], 10000)
T.eq("to 30000", spans[4], 30000)
spans = E.disc_row(discs, 800)
T.eq("a row cutting the first disc off-center", spans[1], -600)
-- Overlapping spans merge: a third disc bridging the two.
E.disc_add(discs, 0, 5000, 31000)
spans = E.disc_row(discs, 0)
T.eq("one span once they overlap", #spans, 2)
T.eq("from -1000", spans[1], -1000)
T.eq("to 30000", spans[2], 30000)
T.eq("a row the discs do not reach is empty", #E.disc_row(discs, 50000), 0)

-- Walking a row: the cursor climbs with z and answers strictly inside.
local covered, cursor = E.span_covers({ -1000, 1000, 10000, 30000 }, -1000, 1)
T.eq("on the edge is not covered", covered, false)
covered, cursor = E.span_covers({ -1000, 1000, 10000, 30000 }, 0, cursor)
T.eq("inside is", covered, true)
covered, cursor = E.span_covers({ -1000, 1000, 10000, 30000 }, 5000, cursor)
T.eq("between spans is not", covered, false)
T.eq("and the cursor moved on", cursor, 3)
covered, cursor = E.span_covers({ -1000, 1000, 10000, 30000 }, 20000, cursor)
T.eq("inside the second is", covered, true)
covered, cursor = E.span_covers({ -1000, 1000, 10000, 30000 }, 40000, cursor)
T.eq("past the last is not", covered, false)
T.eq("no spans, nothing covered", (E.span_covers({}, 0, 1)), false)

-- The spans agree with the plain scan on random discs and rows.
math.randomseed(7)
local many = E.discs(25000, 1000)
for _ = 1, 40 do
  E.disc_add(many, math.random(-50000, 50000), math.random(-50000, 50000), math.random(26000, 60000))
end
local disagreements = 0
for _ = 1, 30 do
  local rx = math.random(-60000, 60000)
  local row = E.disc_row(many, rx)
  local c = 1
  for rz = -60000, 60000, 500 do
    local a
    a, c = E.span_covers(row, rz, c)
    if a ~= E.disc_covers(many, rx, rz) then
      disagreements = disagreements + 1
    end
  end
end
T.eq("spans and the scan agree everywhere", disagreements, 0)

--------------------------------------------------------------------------------
T.group("neighbours are the nearest kept seeds by snap, ties by id, capped")
--------------------------------------------------------------------------------

-- Kept seeds on a line, 1 km apart, with a tie and one far out.
kept = E.kept_seeds(100)
E.keep_seed(kept, 1, 0, 0)
E.keep_seed(kept, 2, 1000, 0)
E.keep_seed(kept, 3, 2000, 0)
E.keep_seed(kept, 4, 3000, 0)
E.keep_seed(kept, 5, -1000, 0)   -- ties with 2 for seed 1
E.keep_seed(kept, 6, 0, 2000)
E.keep_seed(kept, 7, 200000, 0)  -- beyond every cap
local index = E.neighbour_index(1000)
E.index_seeds(index, kept, 1, kept.n)
local out = {}
E.seed_neighbours(kept, index, 1, 4, 50000, out)
T.eq("nearest first, the tie broken by id", out[1], 2)
T.eq("then the other side of the tie", out[2], 5)
T.eq("then two away", out[3], 3)
T.eq("then the one two away on z", out[4], 6)
E.seed_neighbours(kept, index, 4, 4, 50000, out)
T.eq("from the end of the line", out[1], 3)
T.eq("then", out[2], 2)
T.eq("then", out[3], 1)
T.eq("then the tied ones by distance", out[4], 6)
E.seed_neighbours(kept, index, 7, 4, 50000, out)
T.eq("the far seed has nothing within the cap", out[1], 0)
T.eq("in any slot", out[4], 0)
E.seed_neighbours(kept, index, 1, 4, 1500, out)
T.eq("a tight cap keeps the nearest", out[1], 2)
T.eq("and its twin", out[2], 5)
T.eq("and zeroes the rest", out[3], 0)
E.seed_neighbours(kept, index, 6, 2, 50000, out)
T.eq("two asked for", out[1], 1)
T.eq("gives two", out[2], 2)

-- Against brute force on random points.
kept = E.kept_seeds(100)
for i = 1, 300 do
  E.keep_seed(kept, i, math.random(-20000, 20000), math.random(-20000, 20000))
end
index = E.neighbour_index(1000)
E.index_seeds(index, kept, 1, kept.n)
local wrong = 0
for k = 1, kept.n do
  E.seed_neighbours(kept, index, k, 4, 50000, out)
  local all = {}
  for j = 1, kept.n do
    if j ~= k then
      local ex, ez = kept.sx[j] - kept.sx[k], kept.sz[j] - kept.sz[k]
      all[#all + 1] = { d = ex * ex + ez * ez, j = j }
    end
  end
  table.sort(all, function(a, b) return a.d < b.d or (a.d == b.d and a.j < b.j) end)
  for s = 1, 4 do
    if out[s] ~= all[s].j then
      wrong = wrong + 1
    end
  end
end
T.eq("the search agrees with brute force", wrong, 0)

--------------------------------------------------------------------------------
T.group("every unordered pair is requested once, and requested by one side")
--------------------------------------------------------------------------------

-- Four kept seeds, two neighbours each: 1 lists 2,3; 2 lists 1,3; 3 lists
-- 4,1; 4 lists 3,0. Pairs: 1-2 (both list), 1-3 (both), 2-3 (2 only),
-- 3-4 (both).
local nbr = { 2, 3, 1, 3, 4, 1, 3, 0 }
T.eq("1 requests 2", E.pair_requested(nbr, 2, 1, 1), 2)
T.eq("1 requests 3", E.pair_requested(nbr, 2, 1, 2), 3)
T.eq("2 does not request 1, which requested it", E.pair_requested(nbr, 2, 2, 1), nil)
T.eq("2 requests 3, whose list has no 2", E.pair_requested(nbr, 2, 2, 2), 3)
T.eq("3 requests 4", E.pair_requested(nbr, 2, 3, 1), 4)
T.eq("3 does not request 1", E.pair_requested(nbr, 2, 3, 2), nil)
T.eq("4 does not request 3", E.pair_requested(nbr, 2, 4, 1), nil)
T.eq("an empty slot is nothing", E.pair_requested(nbr, 2, 4, 2), nil)

local seen = {}
local k, s, j = 0, 2, nil
local pairs_out = {}
while true do
  k, s, j = E.next_pair(nbr, 2, 4, k, s)
  if k == nil then break end
  local lo, hi = math.min(k, j), math.max(k, j)
  local key = lo .. "-" .. hi
  seen[key] = (seen[key] or 0) + 1
  pairs_out[#pairs_out + 1] = key
end
T.eq("four pairs", #pairs_out, 4)
T.eq("in cursor order", table.concat(pairs_out, " "), "1-2 1-3 2-3 3-4")
for key, n in pairs(seen) do
  T.eq("pair " .. key .. " once", n, 1)
end

-- Random neighbour sets: the walk yields exactly the set of unordered pairs
-- that appear in any list, each once.
for trial = 1, 20 do
  local n = 30
  local count = 4
  nbr = {}
  local want = {}
  for kk = 1, n do
    local used = { [kk] = true }
    for slot = 1, count do
      local jj
      if math.random() < 0.15 then
        jj = 0
      else
        repeat jj = math.random(1, n) until not used[jj]
        used[jj] = true
        local key = math.min(kk, jj) .. "-" .. math.max(kk, jj)
        want[key] = true
      end
      nbr[(kk - 1) * count + slot] = jj
    end
  end
  local got = {}
  local dup = 0
  k, s = 0, count
  while true do
    k, s, j = E.next_pair(nbr, count, n, k, s)
    if k == nil then break end
    local key = math.min(k, j) .. "-" .. math.max(k, j)
    if got[key] then dup = dup + 1 end
    got[key] = true
  end
  local missing, extra = 0, 0
  for key in pairs(want) do if not got[key] then missing = missing + 1 end end
  for key in pairs(got) do if not want[key] then extra = extra + 1 end end
  T.eq("trial " .. trial .. ": no pair twice", dup, 0)
  T.eq("trial " .. trial .. ": no pair missed", missing, 0)
  T.eq("trial " .. trial .. ": no pair invented", extra, 0)
end

T.done()
