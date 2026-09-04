-- Offline tests for the encoders and the list normalisation.
--
-- Run from the repository root with a plain lua5.1.
--
-- Numbers here are built from locals rather than written as literals wherever
-- the sign of zero or an infinity matters. Lua 5.1 folds numeric literals at
-- compile time and shares the folded constants within a chunk, so a literal
-- -0.0 can print as 0 and a literal infinity can come back as a neighbouring
-- constant. Deriving the value at runtime is what makes the assertion about
-- the encoder rather than about the parser.

package.path = "extractor/?.lua;extractor/test/support/?.lua;" .. package.path

local T = require("testing")
local E = require("DcsTerrainExtract")

local zero = 0.0
local negative_zero = -zero
local nan = zero / zero
local inf = math.huge

--------------------------------------------------------------------------------
T.group("i16le")
--------------------------------------------------------------------------------

T.eq("zero", E.i16le(0), "\000\000")
T.eq("one", E.i16le(1), "\001\000")
T.eq("minus one", E.i16le(-1), "\255\255")
T.eq("low byte full", E.i16le(255), "\255\000")
T.eq("carry into high byte", E.i16le(256), "\000\001")
T.eq("negative carry", E.i16le(-256), "\000\255")

T.eq("maximum sample", E.i16le(32767), "\255\127")
T.eq("minimum sample", E.i16le(-32767), "\001\128")

-- The clamp is what keeps -32768 out of the sample range, so the two values
-- either side of it have to land on the sample extremes and not wrap.
T.eq("clamps above", E.i16le(32768), "\255\127")
T.eq("clamps below", E.i16le(-32768), "\001\128")
T.eq("clamps far above", E.i16le(1000000), "\255\127")
T.eq("clamps far below", E.i16le(-1000000), "\001\128")

-- Callers round before they encode; flooring here only guarantees that
-- string.char sees an integer.
T.eq("floors positive", E.i16le(5.7), "\005\000")
T.eq("floors negative", E.i16le(-5.7), "\250\255")

T.eq("nodata is out of reach of the clamp", E.I16_NODATA_BYTES, "\000\128")
T.eq("nodata is not any sample", E.I16_NODATA_BYTES ~= E.i16le(-32767), true)

T.raises("refuses nan", function() return E.i16le(nan) end, "not a finite number")
T.raises("refuses inf", function() return E.i16le(inf) end, "not a finite number")
T.raises("refuses -inf", function() return E.i16le(-inf) end, "not a finite number")
T.raises("refuses a string", function() return E.i16le("7") end, "not a finite number")
T.raises("refuses nil", function() return E.i16le(nil) end, "not a finite number")

--------------------------------------------------------------------------------
T.group("u8")
--------------------------------------------------------------------------------

T.eq("zero", E.u8(0), "\000")
T.eq("one", E.u8(1), "\001")
T.eq("high bit clear", E.u8(127), "\127")
T.eq("high bit set", E.u8(128), "\128")
T.eq("maximum", E.u8(255), "\255")

-- Every code the two u8 layers can carry, so a value the format names cannot
-- be lost to a clamp or a sign.
for _, code in ipairs({ 0, 1, 2, 3, 254, 255 }) do
  T.eq("water code " .. code, E.u8(code):byte(), code)
end
for _, code in ipairs({ 0, 1, 2, 3, 4, 5 }) do
  T.eq("surface code " .. code, E.u8(code):byte(), code)
end

T.eq("clamps above", E.u8(256), "\255")
T.eq("clamps far above", E.u8(1000000000), "\255")
T.eq("clamps below", E.u8(-1), "\000")
T.eq("floors", E.u8(3.9), "\003")

T.raises("refuses nan", function() return E.u8(nan) end, "not a finite number")
T.raises("refuses inf", function() return E.u8(inf) end, "not a finite number")
T.raises("refuses a string", function() return E.u8("7") end, "not a finite number")

--------------------------------------------------------------------------------
T.group("json numbers")
--------------------------------------------------------------------------------

T.eq("zero", E.json(0), "0")
T.eq("integer", E.json(7), "7")
T.eq("negative integer", E.json(-7), "-7")
T.eq("half", E.json(0.5), "0.5")
T.eq("a tenth is not exact", E.json(0.1), "0.10000000000000001")
T.eq("negative zero keeps its sign", E.json(negative_zero), "-0")

-- What 17 significant digits are for: every one of these has to come back the
-- same double, because the Rust side parses what this writes.
local roundtrip = { 0.1, 5.000005, 1 / 3, 2 ^ 53, -418619.19, 943187.06, 1e-300, 1e300 }
for i = 1, #roundtrip do
  local v = roundtrip[i]
  T.eq("round trip " .. i, tonumber(E.json(v)), v)
end

T.raises("refuses nan", function() return E.json(nan) end, "not finite")
T.raises("refuses inf", function() return E.json(inf) end, "not finite")
T.raises("refuses -inf", function() return E.json(-inf) end, "not finite")
T.raises("refuses nan in a table", function() return E.json({ nan }) end, "not finite")

--------------------------------------------------------------------------------
T.group("json strings")
--------------------------------------------------------------------------------

T.eq("empty", E.json(""), '""')
T.eq("plain", E.json("Kutaisi"), '"Kutaisi"')
T.eq("quote", E.json('a"b'), '"a\\"b"')
T.eq("backslash", E.json("a\\b"), '"a\\\\b"')
T.eq("both", E.json('"\\'), '"\\"\\\\"')
T.eq("solidus is not escaped", E.json("Mods/terrains"), '"Mods/terrains"')

T.eq("nul", E.json("\000"), '"\\u0000"')
T.eq("tab", E.json("\009"), '"\\u0009"')
T.eq("newline", E.json("\010"), '"\\u000a"')
T.eq("carriage return", E.json("\013"), '"\\u000d"')
T.eq("last control character", E.json("\031"), '"\\u001f"')
T.eq("space is not a control character", E.json(" "), '" "')
T.eq("delete is not escaped", E.json("\127"), '"\127"')

-- UTF-8 goes through as bytes. Written as escapes so the test does not depend
-- on how this file itself is encoded: "Кутаиси" and "e-acute, euro".
local cyrillic = "\208\154\209\131\209\130\208\176\208\184\209\129\208\184"
T.eq("cyrillic", E.json(cyrillic), '"' .. cyrillic .. '"')
T.eq("multi-byte", E.json("\195\169\226\130\172"), '"\195\169\226\130\172"')
T.eq("escape beside utf-8", E.json(cyrillic .. '"'), '"' .. cyrillic .. '\\""')

--------------------------------------------------------------------------------
T.group("json scalars")
--------------------------------------------------------------------------------

T.eq("true", E.json(true), "true")
T.eq("false", E.json(false), "false")
T.eq("null sentinel", E.json(E.JSON_NULL), "null")
T.eq("null in an object", E.json({ shelters = E.JSON_NULL }), '{"shelters":null}')
T.eq("null in an array", E.json(E.as_array({ 1, E.JSON_NULL, 3 })), "[1,null,3]")

T.raises("refuses nil", function() return E.json(nil) end, "nil is not a value")
T.raises("refuses a function", function() return E.json(print) end, "cannot encode a function")

--------------------------------------------------------------------------------
T.group("json arrays and objects")
--------------------------------------------------------------------------------

T.eq("array of numbers", E.json({ 1, 2, 3 }), "[1,2,3]")
T.eq("array of strings", E.json({ "a", "b" }), '["a","b"]')
T.eq("one element", E.json({ 42 }), "[42]")
T.eq("nested arrays", E.json({ { 1 }, { 2, 3 } }), "[[1],[2,3]]")
T.eq("positional position", E.json({ -418619.19, 113728.16 }), "[-418619.19,113728.16]")

-- An empty table is an object unless it was tagged, which is the only thing
-- that separates a list DCS returned empty from a table with no members.
T.eq("empty object", E.json({}), "{}")
T.eq("empty array", E.json(E.as_array({})), "[]")
T.eq("tagging changes nothing when there are elements", E.json(E.as_array({ 1 })), "[1]")

-- A gap means it is not a list, and guessing would drop the far element.
T.eq("a hole makes it an object", E.json({ [1] = 1, [3] = 3 }), '{"1":1,"3":3}')
T.eq("keys from 0 are an object", E.json({ [0] = "a", [1] = "b" }), '{"0":"a","1":"b"}')

T.eq("keys are sorted", E.json({ b = 2, a = 1, c = 3 }), '{"a":1,"b":2,"c":3}')
T.eq("sorted by byte", E.json({ a = 1, A = 2 }), '{"A":2,"a":1}')
T.eq("escaped key", E.json({ ['a"b'] = 1 }), '{"a\\"b":1}')
T.eq("nested object", E.json({ grid = { cell_size = 50, tile_size = 256 } }),
  '{"grid":{"cell_size":50,"tile_size":256}}')
T.eq("object in an array", E.json({ { id = 1 }, { id = 2 } }), '[{"id":1},{"id":2}]')

T.raises("refuses a boolean key", function() return E.json({ [true] = 1 }) end, "object key is a boolean")
T.raises("refuses a table key", function() return E.json({ [{}] = 1 }) end, "object key is a table")
T.raises("refuses two keys with one name",
  function() return E.json({ [10] = 1, ["10"] = 2 }) end, "the same name")

local loop = {}
loop.self = loop
T.raises("refuses a cycle", function() return E.json(loop) end, "contains itself")

--------------------------------------------------------------------------------
T.group("normalise_list")
--------------------------------------------------------------------------------

-- The case this exists for: an airdrome whose runwayName DCS keys from 0.
T.eq("keyed from 0", E.json(E.normalise_list({ [0] = "08-26", [1] = "12-30" })),
  '["08-26","12-30"]')
T.eq("keyed from 1", E.json(E.normalise_list({ [1] = "a", [2] = "b" })), '["a","b"]')
T.eq("single entry from 0", E.json(E.normalise_list({ [0] = "a" })), '["a"]')
T.eq("single entry from 1", E.json(E.normalise_list({ [1] = "a" })), '["a"]')

-- Syria has 145 airdromes whose runways table is empty; every one has to write
-- as a list and not as an object.
T.eq("empty stays a list", E.json(E.normalise_list({})), "[]")
T.eq("empty list inside an airdrome",
  E.json({ runways = E.normalise_list({}), name = "Kutaisi" }),
  '{"name":"Kutaisi","runways":[]}')

-- Ascending key order, whatever order pairs happens to hand them back.
local scrambled = {}
for k = 9, 0, -1 do
  scrambled[k] = k * 10
end
T.eq("ascending order", E.json(E.normalise_list(scrambled)), "[0,10,20,30,40,50,60,70,80,90]")

-- Shallow: an entry keeps its identity, so a nested positional position is the
-- same table afterwards and is not copied key by key.
local position = { -418619.19, 113728.16 }
local normalised = E.normalise_list({ [0] = { id = 1, pos = position } })
T.eq("entries are not copied", normalised[1].pos, position)
T.eq("nested position still encodes", E.json(normalised),
  '[{"id":1,"pos":[-418619.19,113728.16]}]')

T.eq("nil in, nil out", E.normalise_list(nil), nil)

T.raises("refuses a gap", function() return E.normalise_list({ [0] = 1, [2] = 2 }) end,
  "consecutive keys from 0 or 1")
T.raises("refuses a start at 2", function() return E.normalise_list({ [2] = 1, [3] = 2 }) end,
  "consecutive keys from 0 or 1")
T.raises("refuses a negative key", function() return E.normalise_list({ [-1] = 1, [0] = 2 }) end,
  "consecutive keys from 0 or 1")
T.raises("refuses a string key", function() return E.normalise_list({ name = 1 }) end,
  "key is not an integer")
T.raises("refuses a fractional key", function() return E.normalise_list({ [1.5] = 1 }) end,
  "key is not an integer")
T.raises("refuses a non-table", function() return E.normalise_list("a") end, "not a table")

T.done()
