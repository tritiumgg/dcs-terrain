-- DcsTerrainExtract: a DCS GameGUI hook that sweeps a loaded theatre and
-- writes an extract directory.
--
-- Lua 5.1, because that is what the DCS hook state runs: no string.pack, no
-- bit library, no LuaJIT, and file handles without seek. Anything this file
-- needs from a later Lua it builds by hand.
--
-- The file is loaded two ways. DCS loads it from Scripts/Hooks/ on every
-- start, where the hook environment exists and the state machine registers
-- itself. The offline tests load it from a plain interpreter, where none of
-- those globals exist; nothing registers, and the table returned at the
-- bottom is what they exercise. So every top-level statement here has to be
-- safe with no DCS around it.

local M = {}

local floor = math.floor
local ceil = math.ceil
local char = string.char
local format = string.format
local concat = table.concat
local sort = table.sort

local function is_finite(v)
  return type(v) == "number" and v == v and v ~= math.huge and v ~= -math.huge
end

--------------------------------------------------------------------------------
-- Encoders
--
-- A tile is raw samples with no header, little-endian, so these two functions
-- are the whole binary format. Everything else the hook writes is JSON.
--------------------------------------------------------------------------------

-- Samples clamp to +/- 32767 so that -32768 stays free to mean nodata. A
-- mountain that overflows has to read as a clipped mountain, not as a hole.
local I16_SAMPLE_MIN = -32767
local I16_SAMPLE_MAX = 32767
local I16_NODATA = -32768

local U8_MIN = 0
local U8_MAX = 255

-- Two's complement by hand, low byte first.
local function i16_bytes(v)
  if v < 0 then
    v = v + 65536
  end
  return char(v % 256, floor(v / 256))
end

-- The only way to write -32768, and deliberately not reachable through i16le:
-- a caller that means nodata says so, and a caller that means a sample cannot
-- produce one by accident.
M.I16_NODATA_BYTES = i16_bytes(I16_NODATA)

function M.i16le(v)
  if not is_finite(v) then
    error("i16le: not a finite number: " .. tostring(v), 2)
  end
  v = floor(v)
  if v < I16_SAMPLE_MIN then
    v = I16_SAMPLE_MIN
  elseif v > I16_SAMPLE_MAX then
    v = I16_SAMPLE_MAX
  end
  return i16_bytes(v)
end

function M.u8(v)
  if not is_finite(v) then
    error("u8: not a finite number: " .. tostring(v), 2)
  end
  v = floor(v)
  if v < U8_MIN then
    v = U8_MIN
  elseif v > U8_MAX then
    v = U8_MAX
  end
  return char(v)
end

--------------------------------------------------------------------------------
-- JSON
--------------------------------------------------------------------------------

-- A value no Lua table can equal, so a key DCS omitted can be carried through
-- a table and still written as null. Lua drops a nil value from a table, which
-- would silently turn a missing key into an absent one.
M.JSON_NULL = setmetatable({}, { __tostring = function() return "JSON_NULL" end })

-- Identity only, no behaviour: it marks a table that must be written as an
-- array even when it holds nothing. An empty list and an empty object are the
-- same Lua table, and DCS returns plenty of empty lists.
local ARRAY = {}

function M.as_array(t)
  return setmetatable(t, ARRAY)
end

local ESCAPES = { ['"'] = '\\"', ['\\'] = '\\\\' }
for i = 0, 31 do
  ESCAPES[char(i)] = format("\\u%04x", i)
end

local function escape(s)
  return (s:gsub('[%z\1-\31"\\]', ESCAPES))
end

-- Returns the element count when t is an array, nil when it is an object.
local function array_count(t)
  local n, max = 0, 0
  for k in pairs(t) do
    if type(k) ~= "number" or k < 1 or floor(k) ~= k then
      return nil
    end
    if k > max then
      max = k
    end
    n = n + 1
  end
  if n == 0 then
    return getmetatable(t) == ARRAY and 0 or nil
  end
  if max ~= n then
    return nil
  end
  return n
end

-- JSON names are strings, so a numeric key is written as one. Sorting on the
-- written name rather than the key keeps a table with both kinds orderable and
-- the output stable, and two keys that write the same name are a bug worth
-- stopping for rather than a silently duplicated member.
local function object_entries(t)
  local entries = {}
  for k in pairs(t) do
    local kt = type(k)
    local name
    if kt == "string" then
      name = k
    elseif kt == "number" then
      if not is_finite(k) then
        error("json: object key is not a finite number: " .. tostring(k), 0)
      end
      name = format("%.17g", k)
    else
      error("json: object key is a " .. kt, 0)
    end
    entries[#entries + 1] = { key = k, name = name }
  end
  sort(entries, function(a, b) return a.name < b.name end)
  for i = 2, #entries do
    if entries[i].name == entries[i - 1].name then
      error('json: two keys write the same name: "' .. entries[i].name .. '"', 0)
    end
  end
  return entries
end

local function encode(v, out, seen)
  if v == M.JSON_NULL then
    out[#out + 1] = "null"
    return
  end

  local t = type(v)
  if t == "boolean" then
    out[#out + 1] = v and "true" or "false"
  elseif t == "number" then
    if not is_finite(v) then
      error("json: number is not finite: " .. tostring(v), 0)
    end
    -- 17 significant digits is what it takes for a double to survive the round
    -- trip through text, which is the point: the Rust side reads these back.
    out[#out + 1] = format("%.17g", v)
  elseif t == "string" then
    -- Bytes above 127 pass through untouched, so a UTF-8 airfield name stays
    -- UTF-8 rather than becoming escapes of its individual bytes.
    out[#out + 1] = '"' .. escape(v) .. '"'
  elseif t == "table" then
    if seen[v] then
      error("json: table contains itself", 0)
    end
    seen[v] = true
    local n = array_count(v)
    if n then
      out[#out + 1] = "["
      for i = 1, n do
        if i > 1 then
          out[#out + 1] = ","
        end
        encode(v[i], out, seen)
      end
      out[#out + 1] = "]"
    else
      local entries = object_entries(v)
      out[#out + 1] = "{"
      for i = 1, #entries do
        if i > 1 then
          out[#out + 1] = ","
        end
        out[#out + 1] = '"' .. escape(entries[i].name) .. '":'
        encode(v[entries[i].key], out, seen)
      end
      out[#out + 1] = "}"
    end
    seen[v] = nil
  elseif t == "nil" then
    error("json: nil is not a value; use JSON_NULL", 0)
  else
    error("json: cannot encode a " .. t, 0)
  end
end

-- Compact, no whitespace: one scenery record is one line of scenery.jsonl, and
-- there are about a million of them on Caucasus.
function M.json(value)
  local out = {}
  encode(value, out, {})
  return concat(out)
end

--------------------------------------------------------------------------------
-- JSON decoding
--
-- The hook reads back three things. manifest.json, to decide whether a run can
-- resume and to carry forward what a resume cannot recompute -- the notes, the
-- timings and the pass record of the run being continued. tiles.jsonl, to learn
-- which tiles are already written. And autoupdate.cfg, for the DCS build, which
-- is strict JSON ending in a newline: that is why trailing whitespace after the
-- top-level value is accepted and any other trailing content is not.
--
-- Object keys always come back as strings. M.json writes a numeric key as its
-- %.17g name, so a table keyed by number does not survive a round trip; nothing
-- the hook reads back is keyed that way.
--
-- Escapes above the basic multilingual plane are refused rather than decoded.
-- Everything here is either the hook's own output, whose only escapes are the
-- \u00XX the encoder writes for control characters, or ASCII from ED. A
-- surrogate pair would be the hardest arithmetic in the file with no caller.
--------------------------------------------------------------------------------

local DECODE_MAX_DEPTH = 64

local DECODE_ESCAPES = {
  ['"'] = '"', ['\\'] = '\\', ['/'] = '/',
  b = '\b', f = '\f', n = '\n', r = '\r', t = '\t',
}

-- Only ever called on the way to an error, so scanning from the start costs
-- nothing and a hand-edited manifest can be found by line.
local function decode_where(s, i)
  local line, last = 1, 0
  for at in s:sub(1, i):gmatch("()\n") do
    line = line + 1
    last = at
  end
  return line, i - last
end

local function decode_fail(s, i, message)
  local line, col = decode_where(s, i)
  error(format("json decode: %s at line %d column %d", message, line, col), 0)
end

local function skip_space(s, i)
  local _, stop = s:find("^[ \t\r\n]*", i)
  return stop + 1
end

local function utf8_bmp(cp)
  if cp < 0x80 then
    return char(cp)
  elseif cp < 0x800 then
    return char(0xC0 + floor(cp / 64), 0x80 + cp % 64)
  end
  return char(0xE0 + floor(cp / 4096), 0x80 + floor(cp / 64) % 64, 0x80 + cp % 64)
end

local function decode_string(s, i)
  i = i + 1
  local parts, n = {}, 0
  while true do
    local at = s:find('[%z\1-\31"\\]', i)
    if not at then
      decode_fail(s, i, "string is not terminated")
    end
    if at > i then
      n = n + 1
      parts[n] = s:sub(i, at - 1)
    end
    local c = s:sub(at, at)
    if c == '"' then
      return concat(parts), at + 1
    elseif c ~= "\\" then
      decode_fail(s, at, "a control character must be escaped")
    end
    local e = s:sub(at + 1, at + 1)
    local literal = DECODE_ESCAPES[e]
    if literal then
      n = n + 1
      parts[n] = literal
      i = at + 2
    elseif e == "u" then
      local hex = s:sub(at + 2, at + 5)
      if not hex:find("^%x%x%x%x$") then
        decode_fail(s, at, "\\u needs four hex digits")
      end
      local cp = tonumber(hex, 16)
      if cp >= 0xD800 and cp <= 0xDFFF then
        decode_fail(s, at, "surrogate escapes are not decoded")
      end
      n = n + 1
      parts[n] = utf8_bmp(cp)
      i = at + 6
    else
      decode_fail(s, at, "unknown escape \\" .. e)
    end
  end
end

local function decode_number(s, i)
  local from = i
  if s:sub(i, i) == "-" then
    i = i + 1
  end
  local a, b = s:find("^%d+", i)
  if not a then
    decode_fail(s, from, "a number needs a digit")
  end
  -- Refusing a leading zero keeps a hand-edited 007 from reading as 7.
  if b > a and s:sub(a, a) == "0" then
    decode_fail(s, from, "a number must not have a leading zero")
  end
  i = b + 1
  if s:sub(i, i) == "." then
    a, b = s:find("^%d+", i + 1)
    if not a then
      decode_fail(s, i, "a fraction needs a digit")
    end
    i = b + 1
  end
  local e = s:sub(i, i)
  if e == "e" or e == "E" then
    local j = i + 1
    local sign = s:sub(j, j)
    if sign == "+" or sign == "-" then
      j = j + 1
    end
    a, b = s:find("^%d+", j)
    if not a then
      decode_fail(s, i, "an exponent needs a digit")
    end
    i = b + 1
  end
  -- 1e999 reads as infinity, which the encoder then refuses to write. Stopping
  -- here is what makes a value that decodes always encodable again.
  local v = tonumber(s:sub(from, i - 1))
  if not is_finite(v) then
    decode_fail(s, from, "number is out of range")
  end
  return v, i
end

local decode_value

local function decode_object(s, i, depth)
  i = skip_space(s, i + 1)
  local out = {}
  if s:sub(i, i) == "}" then
    return out, i + 1
  end
  while true do
    if s:sub(i, i) ~= '"' then
      decode_fail(s, i, "expected a key")
    end
    local key, value
    key, i = decode_string(s, i)
    if out[key] ~= nil then
      decode_fail(s, i, 'two members named "' .. key .. '"')
    end
    i = skip_space(s, i)
    if s:sub(i, i) ~= ":" then
      decode_fail(s, i, "expected :")
    end
    i = skip_space(s, i + 1)
    value, i = decode_value(s, i, depth)
    out[key] = value
    i = skip_space(s, i)
    local c = s:sub(i, i)
    if c == "," then
      i = skip_space(s, i + 1)
    elseif c == "}" then
      return out, i + 1
    else
      decode_fail(s, i, "expected , or }")
    end
  end
end

local function decode_array(s, i, depth)
  i = skip_space(s, i + 1)
  local out, n = {}, 0
  if s:sub(i, i) == "]" then
    return M.as_array(out), i + 1
  end
  while true do
    local value
    value, i = decode_value(s, i, depth)
    n = n + 1
    out[n] = value
    i = skip_space(s, i)
    local c = s:sub(i, i)
    if c == "," then
      i = skip_space(s, i + 1)
    elseif c == "]" then
      return M.as_array(out), i + 1
    else
      decode_fail(s, i, "expected , or ]")
    end
  end
end

decode_value = function(s, i, depth)
  depth = depth + 1
  if depth > DECODE_MAX_DEPTH then
    decode_fail(s, i, "nested deeper than " .. DECODE_MAX_DEPTH)
  end
  local c = s:sub(i, i)
  if c == "{" then
    return decode_object(s, i, depth)
  elseif c == "[" then
    return decode_array(s, i, depth)
  elseif c == '"' then
    return decode_string(s, i)
  elseif c == "-" or (c >= "0" and c <= "9") then
    return decode_number(s, i)
  elseif s:sub(i, i + 3) == "true" then
    return true, i + 4
  elseif s:sub(i, i + 4) == "false" then
    return false, i + 5
  elseif s:sub(i, i + 3) == "null" then
    return M.JSON_NULL, i + 4
  elseif c == "" then
    decode_fail(s, i, "input ended early")
  end
  decode_fail(s, i, "unexpected " .. format("%q", c))
end

function M.decode(text)
  if type(text) ~= "string" then
    error("decode: not a string: " .. type(text), 2)
  end
  local i = skip_space(text, 1)
  local value
  value, i = decode_value(text, i, 0)
  i = skip_space(text, i)
  if i <= #text then
    decode_fail(text, i, "trailing content")
  end
  return value
end

--------------------------------------------------------------------------------
-- List normalisation
--
-- DCS keys some of its lists from 0 and some from 1, and the same field can
-- differ between two airdromes of one theatre. Normalising before encoding is
-- what keeps a field that is a list a JSON array everywhere, instead of an
-- array on one airdrome and an object keyed "0" on the next.
--------------------------------------------------------------------------------

-- Shallow by design: entries keep whatever shape they came with, and a nested
-- positional position stays the 1-based array DCS already returns.
--
-- nil in, nil out, so a caller can write normalise_list(t.runways) or
-- JSON_NULL, and tell a key DCS omitted from a list DCS returned empty.
function M.normalise_list(t)
  if t == nil then
    return nil
  end
  if type(t) ~= "table" then
    error("normalise_list: not a table: " .. type(t), 2)
  end

  local n, min, max = 0, nil, nil
  for k in pairs(t) do
    if type(k) ~= "number" or floor(k) ~= k then
      error("normalise_list: key is not an integer: " .. tostring(k), 2)
    end
    n = n + 1
    if min == nil or k < min then
      min = k
    end
    if max == nil or k > max then
      max = k
    end
  end

  if n == 0 then
    return M.as_array({})
  end
  if (min ~= 0 and min ~= 1) or max - min + 1 ~= n then
    error(format("normalise_list: keys %d to %d are not %d consecutive keys from 0 or 1",
      min, max, n), 2)
  end

  local out = {}
  for k = min, max do
    out[k - min + 1] = t[k]
  end
  return M.as_array(out)
end

--------------------------------------------------------------------------------
-- Format constants
--
-- The layer and table blocks the manifest carries, and the two version
-- strings beside them. Both blocks are handed out as fresh tables rather than
-- shared ones: a manifest owns its copy, and a caller that edits one must not
-- reach into every other manifest.
--------------------------------------------------------------------------------

M.FORMAT_VERSION = 1
M.EXTRACTOR_VERSION = "0.1.0"

local LAYER_SPECS = {
  { name = "height",  dtype = "i16", nodata = -32768, unit = "m",     pass = "hook" },
  { name = "water",   dtype = "u8",  nodata = 255,    unit = "class", pass = "hook" },
  { name = "surface", dtype = "u8",  nodata = 0,      unit = "enum",  pass = "mission" },
}

local LAYER_BY_NAME = {}
for i = 1, #LAYER_SPECS do
  LAYER_BY_NAME[LAYER_SPECS[i].name] = LAYER_SPECS[i]
end

-- nil for a name that is not a layer, which is how every caller that takes a
-- layer from outside checks one.
function M.layer(name)
  return LAYER_BY_NAME[name]
end

function M.layers()
  local out = {}
  for i = 1, #LAYER_SPECS do
    local spec = LAYER_SPECS[i]
    out[spec.name] = {
      dtype = spec.dtype, nodata = spec.nodata, unit = spec.unit, pass = spec.pass,
    }
  end
  return out
end

local TABLE_FILES = {
  config = "config.json", airdromes = "airdromes.json", runways = "runways.json",
  stands = "stands.json", beacons = "beacons.json", radio = "radio.json",
  towns = "towns.json", nodes = "nodes.json",
  roads = "roads.jsonl", railroads = "railroads.jsonl", scenery = "scenery.jsonl",
  scenery_models = "scenery_models.json",
}

function M.table_files()
  local out = {}
  for name, file in pairs(TABLE_FILES) do
    out[name] = file
  end
  return out
end

--------------------------------------------------------------------------------
-- Grid
--
-- The grid covers [origin_x, origin_x + height * cell_size) by [origin_z,
-- origin_z + width * cell_size). Rows run north with DCS x and columns run
-- east with DCS z, and every sample is taken at a cell center.
--------------------------------------------------------------------------------

local RECT_KEYS = { "min_x", "min_z", "max_x", "max_z" }

local function check_rect(rect, what)
  if type(rect) ~= "table" then
    error(format("%s: not a rectangle: %s", what, type(rect)), 3)
  end
  for i = 1, #RECT_KEYS do
    local key = RECT_KEYS[i]
    if not is_finite(rect[key]) then
      error(format("%s: %s is not a finite number: %s", what, key, tostring(rect[key])), 3)
    end
  end
  if rect.min_x >= rect.max_x or rect.min_z >= rect.max_z then
    error(format("%s: rectangle is empty", what), 3)
  end
  return rect
end

local function check_positive_integer(v, what, name)
  if not is_finite(v) or floor(v) ~= v or v <= 0 then
    error(format("%s: %s is not a positive integer: %s", what, name, tostring(v)), 3)
  end
  return v
end

-- Snaps the rectangle outward to a multiple of cell_size.
--
-- Outward and not to the nearest, so a cell the rectangle touches at all is
-- inside the grid. Rounding to the nearest drops the cell at each edge that
-- the rectangle only reaches partway into.
--
-- The extents are the difference of the two cell indices, not
-- ceil((max - min) / cell_size). Those are different functions: the second
-- measures the rectangle's own span, which misses the distance from the grid
-- origin to where the rectangle starts, so it can come out a cell short.
--
-- Nothing here needs an epsilon, and adding one would be the bug. Every
-- quantity is an exact double at theatre scale, and a correctly rounded
-- division whose exact quotient is representable is exact, so ceil(26400 / 50)
-- is 528 and never 528.000000001.
function M.grid_from_rect(rect, cell_size, tile_size)
  check_rect(rect, "grid_from_rect")
  check_positive_integer(cell_size, "grid_from_rect", "cell_size")
  check_positive_integer(tile_size, "grid_from_rect", "tile_size")

  local low_row = floor(rect.min_x / cell_size)
  local low_col = floor(rect.min_z / cell_size)
  local high_row = ceil(rect.max_x / cell_size)
  local high_col = ceil(rect.max_z / cell_size)

  return {
    cell_size = cell_size,
    origin_x = low_row * cell_size,
    origin_z = low_col * cell_size,
    height = high_row - low_row,
    width = high_col - low_col,
    tile_size = tile_size,
  }
end

-- Chooses the rectangle the grid covers and records where the authored
-- rectangle came from.
--
-- A crop wins over the authored rectangle, because the user asked for that
-- box. The authored rectangle is still recorded when there is one: it is what
-- tells a later reader which of those cells are terrain someone built rather
-- than the fill the engine returns outside it.
--
-- ADR 0009: a crop run with no authored rectangle leaves both nil, and the
-- manifest writes them as null. Nil here means unknown, never empty.
function M.plan_grid(opts)
  local authored, source = opts.authored_bounds_m, nil
  if authored then
    source = "config"
  elseif opts.presweep_bounds_m then
    authored = opts.presweep_bounds_m
    source = "presweep"
  end

  local rect = opts.crop_m or authored
  if not rect then
    error("plan_grid: no crop, authored bounds or pre-sweep rectangle", 2)
  end

  return {
    grid = M.grid_from_rect(rect, opts.cell_size, opts.tile_size),
    crop_m = opts.crop_m,
    authored_bounds_m = authored,
    authored_bounds_source = source,
  }
end

--------------------------------------------------------------------------------
-- Pre-sweep lattice
--
-- When neither a crop nor an authored rectangle is given, the authored
-- rectangle is measured: a coarse lattice over the theatre's bounds, each cell
-- tested for terrain someone built, and the bounding rectangle of the cells
-- that pass. This section is the lattice, the derivation and the record; what
-- makes a cell authored is a terrain question and belongs with the sweeps.
--
-- The lattice takes meters. The theatre's own bounds are kilometers, so the
-- caller multiplies, and the argument name says which unit it wanted.
--------------------------------------------------------------------------------

local function check_positive(v, what, name)
  if not is_finite(v) or v <= 0 then
    error(format("%s: %s is not a positive number: %s", what, name, tostring(v)), 3)
  end
  return v
end

function M.presweep_lattice(bounds_m, cell_km)
  check_rect(bounds_m, "presweep_lattice")
  check_positive(cell_km, "presweep_lattice", "cell_km")
  local cell_m = cell_km * 1000
  return {
    cell_m = cell_m,
    min_x = bounds_m.min_x,
    min_z = bounds_m.min_z,
    rows = ceil((bounds_m.max_x - bounds_m.min_x) / cell_m),
    cols = ceil((bounds_m.max_z - bounds_m.min_z) / cell_m),
  }
end

-- Row-major and 1-based, so the authored set is a plain Lua array and the
-- bitmask can walk it in the order it is written.
function M.presweep_index(lattice, row, col)
  return row * lattice.cols + col + 1
end

function M.presweep_center(lattice, row, col)
  return lattice.min_x + (row + 0.5) * lattice.cell_m,
         lattice.min_z + (col + 0.5) * lattice.cell_m
end

-- The bounding rectangle of the authored cells, grown by margin_m.
--
-- It bounds the cells' squares and not their centers, because a cell is
-- authored as a whole: bounding the centers would lose half a cell at each
-- edge, and a lattice cell is kilometers wide.
--
-- It is not clipped back to the theatre bounds. The margin can push it
-- outside, where the engine returns fill and the per-cell fill test writes
-- nodata anyway, so clipping would cost terrain at a theatre edge and buy
-- nothing.
function M.presweep_bounds(lattice, authored, margin_m)
  if not is_finite(margin_m) or margin_m < 0 then
    error("presweep_bounds: margin_m is not a distance: " .. tostring(margin_m), 2)
  end
  local min_row, max_row, min_col, max_col
  for row = 0, lattice.rows - 1 do
    for col = 0, lattice.cols - 1 do
      if authored[M.presweep_index(lattice, row, col)] then
        if min_row == nil or row < min_row then min_row = row end
        if max_row == nil or row > max_row then max_row = row end
        if min_col == nil or col < min_col then min_col = col end
        if max_col == nil or col > max_col then max_col = col end
      end
    end
  end
  if min_row == nil then
    error("presweep_bounds: no cell is authored", 2)
  end
  return {
    min_x = lattice.min_x + min_row * lattice.cell_m - margin_m,
    min_z = lattice.min_z + min_col * lattice.cell_m - margin_m,
    max_x = lattice.min_x + (max_row + 1) * lattice.cell_m + margin_m,
    max_z = lattice.min_z + (max_col + 1) * lattice.cell_m + margin_m,
  }
end

local B64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"

-- Base64 by arithmetic, because the hook state has no bit library.
function M.base64(s)
  if type(s) ~= "string" then
    error("base64: not a string: " .. type(s), 2)
  end
  local out, n = {}, 0
  local len = #s
  local i = 1
  while i <= len do
    local b1, b2, b3 = s:byte(i), s:byte(i + 1), s:byte(i + 2)
    local v = b1 * 65536 + (b2 or 0) * 256 + (b3 or 0)
    local c1 = floor(v / 262144) % 64
    local c2 = floor(v / 4096) % 64
    local c3 = floor(v / 64) % 64
    local c4 = v % 64
    local head = B64:sub(c1 + 1, c1 + 1) .. B64:sub(c2 + 1, c2 + 1)
    n = n + 1
    if b3 then
      out[n] = head .. B64:sub(c3 + 1, c3 + 1) .. B64:sub(c4 + 1, c4 + 1)
    elseif b2 then
      out[n] = head .. B64:sub(c3 + 1, c3 + 1) .. "="
    else
      out[n] = head .. "=="
    end
    i = i + 3
  end
  return concat(out)
end

local BIT_VALUE = { [0] = 128, [1] = 64, [2] = 32, [3] = 16, [4] = 8, [5] = 4, [6] = 2, [7] = 1 }

-- One bit per lattice cell, most significant bit first, each row padded to a
-- whole byte. Padding per row rather than packing the lattice end to end keeps
-- a row addressable on its own: row r starts at byte r * ceil(cols / 8).
--
-- This is the shape every base64 bitmask in the project has, and the field
-- holding one is called `bits` wherever it appears. A reader that learns to
-- unpack one has learned to unpack all of them.
function M.presweep_bitmask(lattice, authored)
  local stride = ceil(lattice.cols / 8)
  local bytes, n = {}, 0
  for row = 0, lattice.rows - 1 do
    for byte = 0, stride - 1 do
      local v = 0
      for bit = 0, 7 do
        local col = byte * 8 + bit
        if col < lattice.cols and authored[M.presweep_index(lattice, row, col)] then
          v = v + BIT_VALUE[bit]
        end
      end
      n = n + 1
      bytes[n] = char(v)
    end
  end
  return M.base64(concat(bytes))
end

-- The whole block config.json carries for a pre-sweep. Built here rather than
-- where config.json is written, so the lattice's indexing convention and the
-- record of it stay in one place.
function M.presweep_record(lattice, authored, opts)
  local total = lattice.rows * lattice.cols
  local count = 0
  for i = 1, total do
    if authored[i] then
      count = count + 1
    end
  end
  return {
    cell_km = lattice.cell_m / 1000,
    breakpoint_min = opts.breakpoint_min,
    road_max_m = opts.road_max_m,
    authored_cells = count,
    total_cells = total,
    bits = M.presweep_bitmask(lattice, authored),
  }
end

--------------------------------------------------------------------------------
-- Tiles
--
-- A tile is tile_size by tile_size cells. Tile (tx, tz) holds rows from
-- tx * tile_size north and columns from tz * tile_size east, and within a tile
-- the sample index is local_row * tile_size + local_col: row-major, columns
-- fastest. The last tile in each direction is a full tile, so the cells past
-- the grid edge are written as the layer's nodata rather than left out.
--------------------------------------------------------------------------------

function M.tile_counts(grid)
  return ceil(grid.height / grid.tile_size), ceil(grid.width / grid.tile_size)
end

-- tx outer, tz inner. Sequential access is what makes GetSurfaceType cheap, so
-- the order is stated once here rather than left to each sweep to choose.
function M.each_tile(grid)
  local high, wide = M.tile_counts(grid)
  local tx, tz = 0, -1
  return function()
    tz = tz + 1
    if tz >= wide then
      tz = 0
      tx = tx + 1
    end
    if tx >= high then
      return nil
    end
    return tx, tz
  end
end

function M.cell_center(grid, row, col)
  return grid.origin_x + (row + 0.5) * grid.cell_size,
         grid.origin_z + (col + 0.5) * grid.cell_size
end

function M.tile_first_cell(grid, tx, tz)
  return tx * grid.tile_size, tz * grid.tile_size
end

function M.cell_in_grid(grid, row, col)
  return row >= 0 and row < grid.height and col >= 0 and col < grid.width
end

function M.tile_sample_index(grid, local_row, local_col)
  return local_row * grid.tile_size + local_col
end

function M.tile_path(layer, tx, tz)
  if not LAYER_BY_NAME[layer] then
    error("tile_path: not a layer: " .. tostring(layer), 2)
  end
  return format("tiles/%s/%d_%d.bin", layer, tx, tz)
end

--------------------------------------------------------------------------------
-- Files
--
-- Every handle is opened binary. A tile is raw samples with no header, so a
-- 0x0A byte in a height sample would leave a text-mode handle as two bytes and
-- the file would fail its size check.
--
-- M.fs is the one seam the offline tests replace. Nothing above it touches io
-- or os directly, so a test can drive the whole write-and-resume path over a
-- table of strings and never need a disk.
--
-- A write is checked by the bytes that landed and never by what the call
-- returned, because in DCS a file handle's write and close return no values at
-- all -- on success and on failure alike (ADR 0016). os.rename and os.remove do
-- report, and are still checked on their result.
--------------------------------------------------------------------------------

M.fs = {}

function M.fs.open(path, mode)
  return io.open(path, mode)
end

function M.fs.remove(path)
  return os.remove(path)
end

function M.fs.rename(from, to)
  return os.rename(from, to)
end

-- lfs is a hook-state global, so it is fetched at the call and not at load:
-- this file is also loaded by a plain interpreter with no DCS around it.
function M.fs.mkdir(path)
  local lfs = rawget(_G, "lfs")
  if not lfs then
    return nil, "lfs is not available"
  end
  return lfs.mkdir(path)
end

function M.fs.is_dir(path)
  local lfs = rawget(_G, "lfs")
  if not lfs then
    return false
  end
  return lfs.attributes(path, "mode") == "directory"
end

-- The size on disk, or nil where there is no file. The only evidence a write
-- worked, which is why it is a seam rather than a call: a fake that can report
-- a short size is a fake that can express a full disk.
function M.fs.size(path)
  local lfs = rawget(_G, "lfs")
  if not lfs then
    return nil, "lfs is not available"
  end
  return lfs.attributes(path, "size")
end

-- When the file was last written, in whole seconds since the epoch, or nil
-- where there is no file. Whole seconds because that is what lfs reports;
-- anything comparing against it from outside has to truncate the same way.
function M.fs.modified(path)
  local lfs = rawget(_G, "lfs")
  if not lfs then
    return nil, "lfs is not available"
  end
  return lfs.attributes(path, "modification")
end

-- The directory DCS runs from, which is the install root. Discovered, never
-- recorded: no install path belongs in this file.
function M.fs.currentdir()
  local lfs = rawget(_G, "lfs")
  if not lfs or type(lfs.currentdir) ~= "function" then
    return nil, "lfs is not available"
  end
  local ok, dir = pcall(lfs.currentdir)
  if not ok or type(dir) ~= "string" or dir == "" then
    return nil, "lfs.currentdir gave no directory"
  end
  return dir
end

-- The names in a directory, sorted, without the two dot entries, or nil and a
-- message where there is no directory to list. lfs.dir raises on a path that
-- is not one rather than returning nil, so the walk is under pcall.
function M.fs.dir(path)
  local lfs = rawget(_G, "lfs")
  if not lfs or type(lfs.dir) ~= "function" then
    return nil, "lfs is not available"
  end
  local names = {}
  local ok, err = pcall(function()
    for name in lfs.dir(path) do
      if name ~= "." and name ~= ".." then
        names[#names + 1] = name
      end
    end
  end)
  if not ok then
    return nil, path .. ": " .. tostring(err)
  end
  sort(names)
  return names
end

function M.join(dir, name)
  if dir == nil or dir == "" then
    return name
  end
  if dir:sub(-1) == "/" then
    return dir .. name
  end
  return dir .. "/" .. name
end

function M.read_file(path)
  local f, err = M.fs.open(path, "rb")
  if not f then
    return nil, err or (path .. ": cannot open")
  end
  local data = f:read("*a")
  f:close()
  if not data then
    return nil, path .. ": read failed"
  end
  return data
end

-- The first n bytes of a file, fewer where the file is shorter, or nil and a
-- message where it cannot be opened. The handles here have no seek, so a head
-- is one counted read from a fresh handle and nothing else: the fingerprint
-- wants sixteen bytes of a six gigabyte file, and this is how it gets them
-- without reading the rest.
function M.read_head(path, n)
  local f, err = M.fs.open(path, "rb")
  if not f then
    return nil, err or (path .. ": cannot open")
  end
  local data = f:read(n)
  f:close()
  -- A counted read of an empty file is nil, and an empty head is not a failure.
  return data or ""
end

-- Writes whole, then renames, so a reader never sees half a file.
--
-- The destination is removed first because os.rename on Windows refuses an
-- existing destination, where on Linux it would replace one silently. That
-- leaves a window in which neither name exists, which is why a caller that
-- cannot afford the gap renames the old file aside itself rather than letting
-- this one remove it.
function M.write_file(path, data)
  local tmp = path .. ".tmp"
  local f, err = M.fs.open(tmp, "wb")
  if not f then
    return nil, err or (tmp .. ": cannot open")
  end
  f:write(data)
  -- Closed before the size is taken, because close is what flushes -- whatever
  -- it says about having done so.
  f:close()
  local size = M.fs.size(tmp)
  if size ~= #data then
    return nil, format("%s: %s bytes of %d landed", tmp, tostring(size), #data)
  end
  M.fs.remove(path)
  local renamed, rerr = M.fs.rename(tmp, path)
  if not renamed then
    return nil, rerr or (path .. ": rename failed")
  end
  return true
end

function M.append_file(path, data)
  -- Taken before the open, because opening for append creates the file: after
  -- it, an absent file and an empty one are the same zero.
  local before = M.fs.size(path) or 0
  local f, err = M.fs.open(path, "ab")
  if not f then
    return nil, err or (path .. ": cannot open")
  end
  f:write(data)
  f:close()
  local after = M.fs.size(path)
  if after ~= before + #data then
    return nil, format("%s: %s bytes, expected %d",
      path, tostring(after), before + #data)
  end
  return true
end

-- Creates every missing component of a path.
--
-- A drive letter is stepped over rather than created: output_dir is an
-- absolute Windows path and lfs.mkdir("C:") cannot succeed. A UNC path is not
-- handled and fails with the component it could not create. An existing
-- component is not a failure, so two callers reaching here for the same
-- directory both succeed.
function M.mkdir_p(path)
  if type(path) ~= "string" or path == "" then
    error("mkdir_p: not a path: " .. tostring(path), 2)
  end
  local made = path:match("^/*")
  for part in path:gmatch("[^/]+") do
    if made == "" or made:sub(-1) == "/" then
      made = made .. part
    else
      made = made .. "/" .. part
    end
    if not made:find("^%a:$") and not M.fs.is_dir(made) then
      local ok, err = M.fs.mkdir(made)
      if not ok and not M.fs.is_dir(made) then
        return nil, format("mkdir %s: %s", made, tostring(err))
      end
    end
  end
  return true
end

-- The whole tile tree, including the mission-pass layer. Making the surface
-- directory during the hook pass costs an empty directory and means the
-- mission pass has nowhere left to fail before its first write.
function M.ensure_output_dirs(dir)
  local ok, err = M.mkdir_p(dir)
  if not ok then
    return nil, err
  end
  for i = 1, #LAYER_SPECS do
    ok, err = M.mkdir_p(M.join(dir, "tiles/" .. LAYER_SPECS[i].name))
    if not ok then
      return nil, err
    end
  end
  return true
end

-- A tile file with no journal line fails validation, permanently.
--
-- The write order is file, rename, journal line, so a run killed between the
-- last two leaves one behind. That normally heals: the tile is not in the
-- journal, so the sweep writes it again. It does not heal when the second
-- sweep decides to omit the tile, because then nothing ever overwrites the
-- stale file and no line is ever written for it. So a sweep that omits a tile
-- removes it, whether or not it believes one is there.
function M.remove_tile(dir, layer, tx, tz)
  M.fs.remove(M.join(dir, M.tile_path(layer, tx, tz)))
  return true
end

--------------------------------------------------------------------------------
-- Tile journal
--
-- One line per tile, appended after the tile file is renamed into place. The
-- manifest is rewritten at phase changes and never per tile, so the journal is
-- what makes a killed run lose at most the tile it was writing.
--------------------------------------------------------------------------------

M.JOURNAL_NAME = "tiles.jsonl"

-- min and max are over the tile's samples that are not nodata, and are nil
-- together when every sample is nodata.
function M.tile_entry(layer, tx, tz, min, max)
  return M.check_tile_entry({
    layer = layer,
    tx = tx,
    tz = tz,
    path = M.tile_path(layer, tx, tz),
    min = min == nil and M.JSON_NULL or min,
    max = max == nil and M.JSON_NULL or max,
  })
end

local function is_index(v)
  return is_finite(v) and floor(v) == v and v >= 0
end

-- Checked on the way in and on the way back out, because the two failures it
-- catches are the ones validation reports later and cannot repair: a line
-- naming a file that does not exist, and a file with no line.
function M.check_tile_entry(entry)
  if type(entry) ~= "table" then
    error("check_tile_entry: not an entry: " .. type(entry), 2)
  end
  if not LAYER_BY_NAME[entry.layer] then
    error("check_tile_entry: not a layer: " .. tostring(entry.layer), 2)
  end
  if not is_index(entry.tx) or not is_index(entry.tz) then
    error(format("check_tile_entry: (%s, %s) is not a tile address",
      tostring(entry.tx), tostring(entry.tz)), 2)
  end
  local want = M.tile_path(entry.layer, entry.tx, entry.tz)
  if entry.path ~= want then
    error(format("check_tile_entry: path is %s, not %s", tostring(entry.path), want), 2)
  end
  local min_null = entry.min == M.JSON_NULL
  local max_null = entry.max == M.JSON_NULL
  if min_null ~= max_null then
    error("check_tile_entry: min and max are null together or not at all", 2)
  end
  if not min_null then
    if not is_finite(entry.min) or not is_finite(entry.max) then
      error(format("check_tile_entry: min %s and max %s are not both numbers",
        tostring(entry.min), tostring(entry.max)), 2)
    end
    if entry.min > entry.max then
      error(format("check_tile_entry: min %s is above max %s",
        tostring(entry.min), tostring(entry.max)), 2)
    end
  end
  return entry
end

function M.journal_line(entry)
  return M.json(M.check_tile_entry(entry)) .. "\n"
end

-- Only a line that ends in a newline counts, and the trailing bytes are
-- returned rather than parsed. A run killed between the tile rename and this
-- append leaves exactly that: a partial line, whose tile is swept again.
function M.parse_journal(text)
  local entries, n = {}, 0
  local at, len = 1, #text
  while true do
    local stop = text:find("\n", at, true)
    if not stop then
      return entries, len - at + 1
    end
    local line = text:sub(at, stop - 1)
    at = stop + 1
    if line ~= "" then
      n = n + 1
      entries[n] = M.check_tile_entry(M.decode(line))
    end
  end
end

function M.tile_key(layer, tx, tz)
  return format("%s/%d_%d", layer, tx, tz)
end

-- The last line for a tile wins. A tile written before a resume and written
-- again after it has two lines, and the second is the one describing the file
-- that is actually on disk.
function M.journal_index(entries)
  local index = {}
  for i = 1, #entries do
    local entry = entries[i]
    index[M.tile_key(entry.layer, entry.tx, entry.tz)] = entry
  end
  return index
end

-- Sorted, so two runs over the same theatre produce the same manifest bytes.
-- The format asks for no order; a stable one is what makes an extract diffable
-- and makes a byte-for-byte comparison against the Rust generator mean
-- anything.
function M.manifest_tiles(entries)
  local out, n = {}, 0
  for _, entry in pairs(M.journal_index(entries)) do
    n = n + 1
    out[n] = entry
  end
  sort(out, function(a, b)
    if a.layer ~= b.layer then
      return a.layer < b.layer
    end
    if a.tx ~= b.tx then
      return a.tx < b.tx
    end
    return a.tz < b.tz
  end)
  return M.as_array(out)
end

function M.append_tile(dir, entry)
  return M.append_file(M.join(dir, M.JOURNAL_NAME), M.journal_line(entry))
end

-- No journal is a fresh run. An unreadable one looks the same, which costs
-- nothing here: the tile writes that follow fail loudly on the same directory.
function M.load_journal(dir)
  local text = M.read_file(M.join(dir, M.JOURNAL_NAME))
  if not text then
    return {}, 0
  end
  return M.parse_journal(text)
end

--------------------------------------------------------------------------------
-- Manifest
--
-- Written whole at every phase change, and read back once: when a run finds an
-- output directory that already holds one.
--------------------------------------------------------------------------------

M.MANIFEST_NAME = "manifest.json"

-- Replaced in the tests, so a manifest can be compared against a fixed string.
function M.now_iso()
  return os.date("!%Y-%m-%dT%H:%M:%SZ")
end

local function or_null(v)
  if v == nil then
    return M.JSON_NULL
  end
  return v
end

local function new_pass()
  return {
    complete = false,
    started_at = M.JSON_NULL,
    finished_at = M.JSON_NULL,
    frames = 0,
  }
end

local MANIFEST_REQUIRED = {
  "theatre", "dcs_build", "dcs_build_timestamp", "terrain_fingerprint",
  "bounds_km", "grid",
}

function M.new_manifest(opts)
  for i = 1, #MANIFEST_REQUIRED do
    if opts[MANIFEST_REQUIRED[i]] == nil then
      error("new_manifest: " .. MANIFEST_REQUIRED[i] .. " is missing", 2)
    end
  end
  if type(opts.omit_sea_tiles) ~= "boolean" then
    error("new_manifest: omit_sea_tiles is not a boolean: "
      .. tostring(opts.omit_sea_tiles), 2)
  end
  -- ADR 0009: the authored rectangle and its source are known together or
  -- unknown together. A source naming where an absent rectangle came from, or
  -- a rectangle with no provenance, is a state no reader can interpret.
  if (opts.authored_bounds_m == nil) ~= (opts.authored_bounds_source == nil) then
    error("new_manifest: authored_bounds_m and authored_bounds_source are set"
      .. " together or neither is", 2)
  end

  return {
    format_version = M.FORMAT_VERSION,
    extractor_version = M.EXTRACTOR_VERSION,
    theatre = opts.theatre,
    dcs_build = opts.dcs_build,
    dcs_build_timestamp = opts.dcs_build_timestamp,
    terrain_fingerprint = opts.terrain_fingerprint,
    extracted_at = opts.extracted_at or M.now_iso(),
    bounds_km = opts.bounds_km,
    authored_bounds_m = or_null(opts.authored_bounds_m),
    authored_bounds_source = or_null(opts.authored_bounds_source),
    crop_m = or_null(opts.crop_m),
    grid = opts.grid,
    omit_sea_tiles = opts.omit_sea_tiles,
    layers = M.layers(),
    passes = { hook = new_pass(), mission = new_pass() },
    tiles = M.as_array({}),
    tables = M.table_files(),
    timing_ms = {},
    notes = M.as_array({}),
  }
end

-- The old manifest is renamed aside, and the aside removed only once the new
-- one is in place.
--
-- write_file removes the destination before renaming, which is right for a
-- tile and wrong here: it would leave a window with no manifest at all while
-- the journal already holds thousands of lines, and a run that started in that
-- window would have to refuse the directory or re-sweep the theatre.
function M.write_manifest(dir, manifest)
  local path = M.join(dir, M.MANIFEST_NAME)
  local aside = path .. ".prev"
  M.fs.remove(aside)
  M.fs.rename(path, aside)
  local ok, err = M.write_file(path, M.json(manifest))
  if not ok then
    M.fs.rename(aside, path)
    return nil, err
  end
  M.fs.remove(aside)
  return true
end

function M.read_manifest(dir)
  local text, err = M.read_file(M.join(dir, M.MANIFEST_NAME))
  if not text then
    return nil, err
  end
  local ok, value = pcall(M.decode, text)
  if not ok then
    return nil, value
  end
  return value
end

--------------------------------------------------------------------------------
-- Resume
--
-- A resume carries forward what it cannot recompute: when the run started, how
-- far each pass got, the accumulated timings, and the notes. The notes are the
-- ones that matter. A tile written with nodata cells because a DCS call threw
-- is recorded there, and dropping it turns a recorded partial failure into an
-- extract that looks clean.
--------------------------------------------------------------------------------

local function or_nil(v)
  if v == M.JSON_NULL then
    return nil
  end
  return v
end

local function differs(problems, key, was, now)
  if was ~= now then
    problems[#problems + 1] = format("%s was %s, now %s", key, tostring(was), tostring(now))
  end
end

-- Deep compare through the encoder: it sorts keys, so two tables that encode
-- to the same string hold the same data.
local function differs_deep(problems, key, was, now)
  local before = was ~= nil and M.json(was) or "absent"
  local after = now ~= nil and M.json(now) or "absent"
  if before ~= after then
    problems[#problems + 1] = format("%s was %s, now %s", key, before, after)
  end
end

-- The cheap half of the check, and the half that runs before the pre-sweep. A
-- pre-sweep is about a minute of frame-budgeted work on a large theatre, and
-- there is no sense paying it to find out the directory belongs to another
-- theatre.
function M.identity_problems(existing, opts)
  if type(existing) ~= "table" then
    return { "manifest is not an object" }
  end
  local problems = {}
  differs(problems, "format_version", existing.format_version, M.FORMAT_VERSION)
  differs(problems, "theatre", existing.theatre, opts.theatre)
  differs(problems, "dcs_build", existing.dcs_build, opts.dcs_build)
  differs(problems, "dcs_build_timestamp",
    existing.dcs_build_timestamp, opts.dcs_build_timestamp)
  differs(problems, "omit_sea_tiles", existing.omit_sea_tiles, opts.omit_sea_tiles)
  differs_deep(problems, "terrain_fingerprint",
    existing.terrain_fingerprint, opts.terrain_fingerprint)
  return problems
end

function M.grid_problems(existing, grid)
  local problems = {}
  differs_deep(problems, "grid", existing.grid, grid)
  return problems
end

-- Decides what a run does with an output directory that may already hold one.
-- Returns the resume state, or nil and one problem per line for the log.
--
-- A resumed run takes its grid and its authored rectangle from the manifest
-- rather than recomputing them. The fingerprint check already covers a terrain
-- rebuilt under the extract, and re-running a pre-sweep can shift the lattice
-- by a cell, move the rectangle derived from it, and refuse a half-finished
-- extract that was perfectly good. So the grid is compared only when a fresh
-- one was computed, which is the crop and config paths.
function M.prepare_resume(dir, opts)
  local existing = M.read_manifest(dir)
  local journal, partial = M.load_journal(dir)

  if not existing then
    -- No manifest and no journal is a fresh run. A journal without a manifest
    -- is not: the manifest is renamed aside and rewritten at every phase
    -- change, and reading that window as a fresh run would re-sweep everything
    -- and could leave two grids' tiles in one directory.
    if #journal > 0 or partial > 0 then
      return nil, { "tiles.jsonl is present and manifest.json is not" }
    end
    return { resumed = false, done = {}, entries = {}, partial_bytes = 0 }
  end

  local problems = M.identity_problems(existing, opts)
  if opts.grid then
    local from_grid = M.grid_problems(existing, opts.grid)
    for i = 1, #from_grid do
      problems[#problems + 1] = from_grid[i]
    end
  end
  if #problems > 0 then
    return nil, problems
  end

  return {
    resumed = true,
    manifest = existing,
    grid = existing.grid,
    authored_bounds_m = or_nil(existing.authored_bounds_m),
    authored_bounds_source = or_nil(existing.authored_bounds_source),
    done = M.journal_index(journal),
    entries = journal,
    partial_bytes = partial,
  }
end

--------------------------------------------------------------------------------
-- Frame budget
--
-- The hook gets one callback per simulation frame and has to give the frame
-- back. Work is therefore cut into steps, and a frame runs steps until the
-- budget is spent.
--
-- The budget is checked between steps and never inside one. A step is the
-- smallest thing the hook can stop after, and several of them cost more than
-- the whole budget on their own: one road path is about 0.61 ms, one
-- mission-pass chunk about 40 ms. A frame always runs at least one step, so a
-- budget smaller than a step still finishes the extract -- one step per frame,
-- slowly -- rather than deadlocking on a budget that is spent before any work
-- is attempted.
--------------------------------------------------------------------------------

-- Seam, like M.fs. The offline tests replace it with a clock they step by
-- hand, so what a budget test asserts does not depend on how fast the machine
-- running it is.
function M.clock()
  return os.clock()
end

function M.budget(budget_ms)
  if not is_finite(budget_ms) or budget_ms < 0 then
    error("budget: frame_budget_ms is not a non-negative number: "
      .. tostring(budget_ms), 2)
  end
  local started = M.clock()
  local limit = budget_ms / 1000
  return function()
    return (M.clock() - started) >= limit
  end
end

--------------------------------------------------------------------------------
-- Job queue
--
-- A job is one sweep: a name, and a `start` that is called once, when the job
-- first gets a frame, and returns the step function. The step returns M.MORE
-- while work remains, M.DONE when the sweep is finished, or M.REFUSED when the
-- job cannot go on and has left the reason in run.refusal; anything else
-- raises, because a step that returned nil by accident would otherwise read as
-- "not finished" and the sweep would never end.
--
-- A refusal ends the frame at once, so no job later in the queue starts
-- against work that did not happen. The queue only relays it: what a refusal
-- does to the run is the state machine's business.
--
-- Splitting `start` from the step is what lets a job be built against the run
-- it will sweep -- the grid, the skip set, the journal -- at the moment the
-- pass reaches it, rather than at load, when none of that exists yet.
--
-- The queue reports finished jobs in `finished` rather than writing anything
-- itself. What happens at the end of a sweep is the manifest's business, and
-- the queue does not know there is a manifest.
--------------------------------------------------------------------------------

M.MORE = "more"
M.DONE = "done"
M.REFUSED = "refused"

function M.new_queue(jobs)
  if type(jobs) ~= "table" then
    error("new_queue: jobs is not a list: " .. type(jobs), 2)
  end
  for i = 1, #jobs do
    local job = jobs[i]
    if type(job) ~= "table" or type(job.name) ~= "string"
      or type(job.start) ~= "function" then
      error(format("new_queue: job %d is not {name = string, start = function}", i), 2)
    end
  end
  return { jobs = jobs, index = 1, step = nil, started = nil, finished = {} }
end

-- Milliseconds, rounded, because that is what the manifest `timing_ms` holds.
local function elapsed_ms(started)
  return floor((M.clock() - started) * 1000 + 0.5)
end

-- Runs steps until the budget is spent, and returns M.MORE if the queue has
-- more jobs or M.DONE once every job has finished. `finished` is emptied at
-- the start of each frame, so a caller reads only the jobs this frame ended.
function M.queue_frame(queue, run, spent)
  queue.finished = {}
  repeat
    local job = queue.jobs[queue.index]
    if job == nil then
      return M.DONE
    end
    if queue.step == nil then
      queue.started = M.clock()
      queue.step = job.start(run)
      if type(queue.step) ~= "function" then
        error(format("job %s: start returned %s, not a step function",
          job.name, type(queue.step)), 0)
      end
    end
    local status = queue.step()
    if status == M.REFUSED then
      return M.REFUSED
    end
    if status == M.DONE then
      queue.finished[#queue.finished + 1] = {
        name = job.name, ms = elapsed_ms(queue.started),
      }
      queue.index = queue.index + 1
      queue.step = nil
      queue.started = nil
    elseif status ~= M.MORE then
      error(format("job %s: step returned %s, not M.MORE or M.DONE",
        job.name, tostring(status)), 0)
    end
  until spent()
  if queue.jobs[queue.index] == nil then
    return M.DONE
  end
  return M.MORE
end

--------------------------------------------------------------------------------
-- Config
--
-- What the user gets to decide, and what happens to a value they got wrong.
--
-- ADR 0012: validation never raises and always hands back a usable table. Each
-- bad field costs one line and takes its default, so no bad value can reach a
-- sweep and there is nothing to disable. output_dir is the exception, because
-- it is the one field with no default: a bad one leaves it nil, and a run
-- cannot start without somewhere to write.
--
-- The field list is data rather than a branch per field, because "one line per
-- problem" is then a property of the loop instead of something every branch has
-- to remember.
--
-- Every checker here returns nil for a good value and one message for a bad
-- one. Nothing raises. The grid's own checkers do, and should: a caller that
-- hands grid_from_rect a nil has a bug. A user who mistypes a config does not,
-- and the whole point of this section is to collect what went wrong rather than
-- stop at the first of it.
--------------------------------------------------------------------------------

-- ADR 0011: constants, not config. Each is still written into the manifest, so
-- the format stays parametric and a reader is told what the extract was built
-- with; what none of them is any more is a question put to the user.
--
-- The extract is always 50 m, because a coarser base is pack's choice and the
-- DCS sweep is deliberately not multi-resolution. tile_size is internal
-- chunking, 128 KB a tile. Omitting an all-sea tile is lossless, because water
-- 2 is sea and a lake at altitude is 1: an omitted tile reads back as height 0
-- and surface WATER exactly. The two road seed numbers were measured rather
-- than picked, and what moving them trades away is graph fidelity nothing
-- reports.
-- frame_budget_ms is here too, and for a reason worth writing down: it changes
-- how long a run takes and never what it produces, and its effect is not even
-- uniform. queue_frame always runs at least one step, and a server-state chunk
-- is about 40 ms, so under any sane budget that sweep gets one chunk a frame
-- whatever the number says, while roads scale with it. Nothing reports frame
-- cost to tune against either. If the trade ever matters it comes back as a
-- named mode with a measurement behind it, not as a millisecond number.
M.CELL_SIZE = 50
M.TILE_SIZE = 256
M.OMIT_SEA_TILES = true
M.FRAME_BUDGET_MS = 5
M.ROAD_SEED_SPACING = 1000
M.ROAD_SEED_NEIGHBOURS = 4

local function bad_boolean(v, name)
  if type(v) ~= "boolean" then
    return format("%s is not true or false: %s", name, tostring(v))
  end
end

-- The crop is a center and a radius, not the box the format records. A box is
-- four coordinates nobody can produce from knowing where they want to extract;
-- the Mission Editor shows the X and Z under the cursor, so a center read off
-- the map plus a radius is three numbers a user actually has.
local CROP_KEYS = { "x", "z", "radius_m" }

-- The smallest crop worth extracting: a 2 km box. The cell is 50 m, and the
-- derived layers want windows of 300 m and 2 km, so anything smaller packs
-- to cells with no usable layer over them (ADR 0019).
M.MIN_RADIUS_M = 1000
local CROP_BY_NAME = { x = true, z = true, radius_m = true }

-- One line for the whole crop, naming the first thing wrong with it, so a crop
-- with three bad members costs one line and not three. Anything else would make
-- "one line per bad field" a count nobody can rely on.
local function bad_crop(v, name)
  if type(v) ~= "table" then
    return format("%s is not a center and a radius: %s", name, tostring(v))
  end
  for i = 1, #CROP_KEYS do
    local key = CROP_KEYS[i]
    if not is_finite(v[key]) then
      return format("%s.%s is not a finite number: %s",
        name, key, tostring(v[key]))
    end
  end
  for key in pairs(v) do
    if not CROP_BY_NAME[key] then
      return format("%s has an unknown key: %s", name, tostring(key))
    end
  end
  if v.radius_m <= 0 then
    return format("%s.radius_m is not a positive number: %s",
      name, tostring(v.radius_m))
  end
  if v.radius_m < M.MIN_RADIUS_M then
    return format("%s.radius_m is under %d m: %s",
      name, M.MIN_RADIUS_M, tostring(v.radius_m))
  end
end

-- The box the grid is planned from and the manifest records. A radius of r is
-- half the side, so radius_m = 5000 is the 10 x 10 km crop X10 asks for.
--
-- Converted here rather than in validate_config, so a validated config keeps
-- the user's own vocabulary: the window fills its controls from the same table
-- the run starts from, and a center that had to be recovered from a box would
-- be a round trip waiting to lose a digit.
-- Why a crop's box reaches outside the theatre, or nil where it does not or
-- where there is nothing to check it against. The box is the center plus and
-- minus the radius on each axis, so one rule covers a radius too big for the
-- map and a center too near an edge: whichever edge the box crosses first is
-- named, with the number that crossed it, in whole meters.
--
-- The bounds are the theatre's raster rectangle, which every theatre
-- publishes. On Caucasus it is larger than the authored land, and a crop in
-- the sea outside the coast passes here and extracts sea, which is what it
-- asked for; the authored hull is only known for one theatre and costs a
-- pre-sweep to find for the rest (ADR 0019).
function M.crop_outside(crop, bounds)
  if crop == nil or bounds == nil then
    return nil
  end
  local box = M.crop_box(crop)
  local edges = {
    { box.min_x, bounds.min_x, "x", "below" },
    { box.min_z, bounds.min_z, "z", "below" },
    { box.max_x, bounds.max_x, "x", "above" },
    { box.max_z, bounds.max_z, "z", "above" },
  }
  for i = 1, #edges do
    local value, limit, axis, side = edges[i][1], edges[i][2], edges[i][3], edges[i][4]
    local crossed
    if side == "below" then
      crossed = value < limit
    else
      crossed = value > limit
    end
    if crossed then
      -- The edge first and the box's own number after the colon, because
      -- the screen keeps what is before the colon and the box already shows
      -- the center and the radius the number came from.
      return format("crop reaches past the map, %s %s %d: box edge %d",
        axis, side, floor(limit + 0.5), floor(value + 0.5))
    end
  end
  return nil
end

-- Why the output directory cannot be made, or nil where it can. The run makes
-- every missing component of the path, so the directory need not exist; its
-- root must, because a drive letter cannot be made and a share that is not
-- there fails at the first component (ADR 0020). One `lfs.attributes` on the
-- root, through the seam the tests fake. A path that fails the shape check
-- is not asked about, because the checker has already said what is wrong.
function M.drive_problem(path)
  if type(path) ~= "string" or path == "" then
    return nil
  end
  local root = path:match("^(%a:/)") or path:match("^(//[^/]+/[^/]+)")
  if root == nil or M.fs.is_dir(root) then
    return nil
  end
  -- The drive is the finding, so it stays in front of the colon.
  return format("output_dir's drive %s does not exist", root)
end

function M.crop_box(crop)
  if crop == nil then
    return nil
  end
  local bad = bad_crop(crop, "crop")
  if bad then
    error("crop_box: " .. bad, 2)
  end
  return {
    min_x = crop.x - crop.radius_m,
    min_z = crop.z - crop.radius_m,
    max_x = crop.x + crop.radius_m,
    max_z = crop.z + crop.radius_m,
  }
end

-- A control character in a path is almost always a Lua escape the user did not
-- mean to write: output_dir = "C:\temp\new" is not a path, because \t and \n are
-- escapes, so the value already holds a tab and a newline by the time it arrives.
-- Saying so is the difference between a baffling failure and a one-line fix.
--
-- "C:\Users\..." needs no check of its own: \U is not a valid escape, so the file
-- fails to load and the syntax error is reported instead.
local function bad_path(v, name)
  if type(v) ~= "string" or v == "" then
    return format("%s is not a non-empty string: %s", name, tostring(v))
  end
  if v:find("[%z\1-\31]") then
    return format("%s contains a control character, which is usually a "
      .. "backslash escape in a double-quoted path: %s", name, format("%q", v))
  end
  -- Absolute, as a drive letter or a UNC root, with either separator: the
  -- value is checked before it is normalised. A relative path would be made
  -- under whatever DCS's working directory happens to be, which is the
  -- install, and the install is never written into.
  if not (v:find("^%a:[/\\]") or v:find("^[/\\][/\\][^/\\]+[/\\][^/\\]+")) then
    return format("%s is not an absolute path: %s", name, v)
  end
  -- The characters Windows refuses in a path, and a colon anywhere but after
  -- the drive letter. Refused here rather than by the mkdir that would fail
  -- on them at prepare, so the box is told before a run is spent.
  if v:find('[<>"|?*]') or v:sub(3):find(":") then
    return format("%s has a character Windows forbids: %s", name, v)
  end
end

-- mkdir_p splits on "/" and steps over a drive letter only in a "/"-split
-- path, so a pasted C:\extracts\caucasus would otherwise become one directory
-- whose name holds colons and backslashes.
local function forward_slashes(v)
  return (v:gsub("\\", "/"))
end

-- name, the check, the default, and whether absent is allowed. enabled is not
-- in the list because it is read before the list is: it decides whether any of
-- the rest is looked at.
-- Two fields, and the window shows both. crop is optional rather than
-- defaulted, because a crop is a deliberate choice and there is no area to
-- invent for someone who did not ask for one.
-- kind is what a control is built from, not which checker runs. A checker is
-- not a control: two fields could share one and still need different widgets.
local CONFIG_FIELDS = {
  { name = "output_dir", kind = "path", check = bad_path,
    normalise = forward_slashes },
  { name = "crop", kind = "crop", check = bad_crop, optional = true },
}

local CONFIG_FIELD_BY_NAME = { enabled = true }
local CONFIG_SPEC_BY_NAME = {}
for i = 1, #CONFIG_FIELDS do
  CONFIG_FIELD_BY_NAME[CONFIG_FIELDS[i].name] = true
  CONFIG_SPEC_BY_NAME[CONFIG_FIELDS[i].name] = CONFIG_FIELDS[i]
end

-- One message for a field, or nil when the value is usable. Total: any name and
-- any value answer, and a name that is not a field says so.
--
-- This is the only place a config message is produced. Validating a whole table
-- and validating one control a user just typed into are the same question asked
-- about a different number of fields, and two implementations of it would be two
-- wordings to keep in step.
--
-- enabled is handled here rather than joining CONFIG_FIELDS, because it is read
-- before the list is: it decides whether the rest is looked at, and being absent
-- is not a problem -- an absent config file means a disabled hook, and that is
-- not an error either.
function M.field_problem(name, value)
  if name == "enabled" then
    if value == nil then
      return nil
    end
    return bad_boolean(value, "enabled")
  end

  local field = CONFIG_SPEC_BY_NAME[name]
  if not field then
    return format("%s is not a config field", tostring(name))
  end

  if value == nil then
    if field.optional or field.default ~= nil then
      return nil
    end
    return format("%s is not set, and there is no default for it", field.name)
  end

  return field.check(value, field.name)
end

-- The fields a window shows, in the order it shows them. Fresh tables per call,
-- like M.layers() and M.table_files(): a caller that edits one must not reach
-- into the spec every other caller reads.
function M.config_fields()
  local out = {}
  for i = 1, #CONFIG_FIELDS do
    local field = CONFIG_FIELDS[i]
    out[i] = {
      name = field.name,
      kind = field.kind,
      optional = field.optional or false,
    }
  end
  return out
end

-- Returns the config to run with, one line per problem, and the field each of
-- those lines belongs to.
--
-- The table comes back whatever went wrong, because the window that owns the
-- config file needs it to fill its controls and the run needs it to start. Two
-- code paths for those would be two chances to disagree about what a defaulted
-- field holds.
--
-- tags is parallel to problems: tags[i] is the field problems[i] belongs to, or
-- nil for a problem that belongs to no control -- an unrecognised key, or a
-- config that is not a table at all. Parallel arrays rather than a table keyed
-- by field, because "one line per problem" is the count everything here rests
-- on, and problems stays the one place it is counted.
--
-- tags therefore has holes, and # on a table with holes is undefined in Lua
-- 5.1. Walk it as `for i = 1, #problems`, never `for i = 1, #tags` and never
-- with ipairs.
--
-- Unknown keys are sorted before they are reported: pairs order is undefined,
-- and a problem list whose order changes between runs is a log nobody can diff.
function M.validate_config(config)
  local problems = {}
  local tags = {}
  local out = {}

  local function report(problem, field)
    problems[#problems + 1] = problem
    tags[#problems] = field
  end

  if type(config) ~= "table" then
    report(format("config is not a table: %s", type(config)))
    return { enabled = false }, problems, tags
  end

  -- enabled short-circuits. Not true means the hook does nothing at all, so
  -- there is nothing to validate and nobody to tell.
  local bad = M.field_problem("enabled", config.enabled)
  if bad then
    report(bad, "enabled")
  end
  if config.enabled ~= true then
    return { enabled = false }, problems, tags
  end
  out.enabled = true

  for i = 1, #CONFIG_FIELDS do
    local field = CONFIG_FIELDS[i]
    local value = config[field.name]
    local problem = M.field_problem(field.name, value)
    if problem then
      report(problem, field.name)
      out[field.name] = field.default
    elseif value == nil then
      out[field.name] = field.default
    elseif field.normalise then
      out[field.name] = field.normalise(value)
    else
      out[field.name] = value
    end
  end

  local unknown = {}
  for key in pairs(config) do
    if not CONFIG_FIELD_BY_NAME[key] then
      unknown[#unknown + 1] = tostring(key)
    end
  end
  sort(unknown)
  for i = 1, #unknown do
    report(M.field_problem(unknown[i]))
  end

  return out, problems, tags
end

--------------------------------------------------------------------------------
-- Controls
--
-- The config as a window shows it, and back. A control holds a string; a config
-- holds numbers, a table, and nil for a question nobody answered. These two
-- functions are that conversion and nothing else, which is what makes the whole
-- of it testable with no widget in the process -- and that matters more here
-- than anywhere else in this file, because a widget cannot be constructed
-- outside DCS at all.
--
-- They live here rather than beside the window because the window is where they
-- cannot be tested.
--
-- config_from_text produces no messages. A box that will not parse keeps its own
-- text, and the checkers above already reject a string and print it, so a box
-- holding 12abc comes back as "crop.x is not a finite number: 12abc". Turning it
-- into nil first would report "nil" about a box the user can plainly see
-- characters in; wording a message here would make a second place a config
-- problem is phrased, and then two wordings to keep in step.
--------------------------------------------------------------------------------

-- A box is typed by hand where the config file is written by this program, so
-- surrounding space is the user's and not a value. A trailing one is invisible
-- on screen, passes bad_path -- a space is not a control character -- and ends
-- up as a directory whose name ends in a space.
local function trim(s)
  if type(s) ~= "string" then
    return nil
  end
  return (s:gsub("^%s*(.-)%s*$", "%1"))
end

-- Blank is a question not answered, which is nil rather than "". The difference
-- is the whole of how an absent crop and an absent output_dir are told from a
-- bad one.
local function box_string(s)
  local text = trim(s)
  if text == nil or text == "" then
    return nil
  end
  return text
end

local function box_number(s)
  local text = box_string(s)
  if text == nil then
    return nil
  end
  return tonumber(text) or text
end

-- The shortest form that reads back as the same double. %.17g is exact and
-- unreadable -- a radius would sit in the box as 5000.0000000000000 -- and
-- %.14g, which is what tostring writes, moves a six-figure metre coordinate in
-- its last bits. A center that came out of the config file has to go back into
-- it unchanged, or a user who pressed Start without touching anything would have
-- moved their own crop.
--
-- Public because the window puts numbers into boxes from somewhere other than a
-- config too: a center read off the map has to be written the same way, or the
-- same digit would go missing by a different route.
function M.box_text(v)
  if v == nil then
    return ""
  end
  if not is_finite(v) then
    return tostring(v)
  end
  local short = format("%.14g", v)
  if tonumber(short) == v then
    return short
  end
  return format("%.17g", v)
end

-- What the controls hold for a config: four strings and the crop's tick.
function M.control_text(config)
  local crop = type(config) == "table" and config.crop or nil
  local has_crop = type(crop) == "table"
  return {
    output_dir = M.box_text(type(config) == "table" and config.output_dir or nil),
    crop = has_crop,
    crop_x = has_crop and M.box_text(crop.x) or "",
    crop_z = has_crop and M.box_text(crop.z) or "",
    crop_radius_m = has_crop and M.box_text(crop.radius_m) or "",
  }
end

-- The config those controls describe, whatever is in them.
--
-- enabled is true and is not read from anything, because the window only exists
-- when it is: a window writing enabled = false would be a control switching off
-- the only surface that can switch it back on.
--
-- An unticked crop drops the three boxes rather than clearing them, so a user
-- who unticks and ticks again still has what they typed. A ticked crop with a
-- blank box is a crop with a nil member, which is a problem and reads as one.
function M.config_from_text(values)
  local out = { enabled = true }
  out.output_dir = box_string(values.output_dir)
  if values.crop then
    out.crop = {
      x = box_number(values.crop_x),
      z = box_number(values.crop_z),
      radius_m = box_number(values.crop_radius_m),
    }
  end
  return out
end

--------------------------------------------------------------------------------
-- Config file
--
-- A Lua chunk in Saved Games returning a table. The window owns it: it fills its
-- controls from this file and writes it back at Start, which is the whole of
-- "the window is the only surface the config has" (ADR 0011).
--
-- Read through M.read_file and loadstring rather than dofile, so the M.fs seam
-- every other read already goes through covers this one too and the offline
-- tests need no disk. What the file is does not change: a chunk returning a
-- table.
--
-- The chunk runs with an empty environment. A config is three values, not a
-- program, and a file edited into calling os.execute should fail rather than
-- run. What that buys is exactly the globals: os, io, require, load and
-- getmetatable are all unreachable, so there is no route back to the hook's own
-- state.
--
-- It is not a sandbox. String methods come from the string metatable, which
-- setfenv does not touch, so ("x"):rep(1e9) still runs and still exhausts
-- memory. Closing that needs limits on the interpreter rather than on the
-- environment, and it buys little: the file lives in the user's own Saved Games,
-- and anything that can write there can drop a hook beside this one.
--------------------------------------------------------------------------------

M.CONFIG_NAME = "Config/DcsTerrainExtract.lua"

-- Where the config lives, or nil where there is no Saved Games under this
-- process -- which is every offline test, and is not an error.
function M.config_path()
  local dir = M.saved_games_dir()
  if not dir then
    return nil
  end
  return M.join(dir, M.CONFIG_NAME)
end

-- The table the file holds, or nil and one line saying why.
--
-- The failures are kept apart because they ask different things of the user: no
-- file at all is the ordinary state of a fresh install, a chunk that will not
-- compile is a typo with a line number, a chunk that raises got through the
-- parser and died anyway, and one returning a non-table is a file missing its
-- `return`.
function M.read_config(path)
  local text, err = M.read_file(path)
  if not text then
    return nil, err
  end

  -- The "@" prefix names the chunk as a file, so a syntax error reads as
  -- "<path>:12: unexpected symbol" instead of quoting the source back.
  local chunk, cerr = loadstring(text, "@" .. path)
  if not chunk then
    return nil, cerr or (path .. ": will not compile")
  end
  setfenv(chunk, {})

  local ok, value = pcall(chunk)
  if not ok then
    return nil, format("%s: %s", path, tostring(value))
  end
  if type(value) ~= "table" then
    return nil, format("%s: does not return a table: %s", path, type(value))
  end
  return value
end

-- Written above the table on every save, because the file is generated and a
-- reader who does not know that will edit it and lose the edit.
local CONFIG_HEADER = [[
-- Written by the DCS Terrain Extract window. Anything you add here by hand is
-- overwritten the next time you press Start -- set the values in the window.
--
-- enabled is the one exception. It is read before the window is built, so it
-- has to be set here once before there is a window to set anything in.

]]

-- Writes the config, or refuses with the one line saying which field stopped it.
--
-- Refusing rather than writing what it was given: this file is read back at the
-- next start, and a value that cannot survive the round trip -- a nil
-- output_dir, a crop missing its radius -- would come back as a problem the user
-- did not cause and cannot place. Every field goes through the same checker the
-- window shows a message from, so what is refused here is exactly what is
-- already red on screen.
function M.write_config(path, config)
  if type(config) ~= "table" then
    return nil, format("config is not a table: %s", type(config))
  end

  -- enabled is checked on its own because config_fields does not carry it: it
  -- is the master switch, read before there is a window to show a control in.
  -- Checking it anyway matters more here than anywhere else -- enabled is
  -- written as `config.enabled == true`, so a caller holding 1 or "yes" would
  -- otherwise have it silently written as false and the hook would not come
  -- back on the next start.
  local problem = M.field_problem("enabled", config.enabled)
  if problem then
    return nil, problem
  end

  local fields = M.config_fields()
  for i = 1, #fields do
    problem = M.field_problem(fields[i].name, config[fields[i].name])
    if problem then
      return nil, problem
    end
  end

  local out = { CONFIG_HEADER, "return {\n" }
  out[#out + 1] = format("  enabled = %s,\n", tostring(config.enabled == true))
  if config.output_dir ~= nil then
    out[#out + 1] = format("  output_dir = %q,\n", config.output_dir)
  end

  -- %.17g throughout, the same as the JSON encoder: a crop center is a
  -- six-figure metre coordinate, and %g would round it to six significant
  -- digits and move the crop by tens of meters on the way through the file.
  local crop = config.crop
  if crop ~= nil then
    out[#out + 1] = format("  crop = { x = %.17g, z = %.17g, radius_m = %.17g },\n",
      crop.x, crop.z, crop.radius_m)
  end
  out[#out + 1] = "}\n"

  return M.write_file(path, concat(out))
end

--------------------------------------------------------------------------------
-- State machine
--
-- idle -> prepare -> hook -> mission -> done. DCS gives the hook one callback
-- per simulation frame, and this is what a frame does.
--
-- What a phase contains is not decided here. Each phase is a list of jobs the
-- sweeps register, run in the order they registered, and the machine knows
-- only how to give them frames, time them, stamp the pass and save what they
-- finished. That is why a sweep can be added without touching this section,
-- and why this section can be tested without a sweep in it.
--------------------------------------------------------------------------------

-- stopped is where a run begins and waits. A run used to begin at idle and poll
-- for terrain the moment the hook loaded; the window puts a Start button in
-- front of that, so there has to be a state that costs nothing and does nothing
-- until somebody presses it (ADR 0014).
M.STATE_STOPPED = "stopped"
M.STATE_IDLE = "idle"
M.STATE_PREPARE = "prepare"
M.STATE_HOOK = "hook"
M.STATE_MISSION = "mission"
M.STATE_DONE = "done"

-- Frames between terrain polls in idle. The poll is two DCS calls and idle
-- lasts for as long as DCS sits at the main menu, which can be hours.
M.IDLE_POLL_FRAMES = 60

-- Only two of the six states are passes the manifest records.
local PASS_OF = { [M.STATE_HOOK] = "hook", [M.STATE_MISSION] = "mission" }

M.jobs = { prepare = {}, hook = {}, mission = {} }

function M.add_job(phase, job)
  local list = M.jobs[phase]
  if list == nil then
    error("add_job: not a phase: " .. tostring(phase), 2)
  end
  list[#list + 1] = job
  return job
end

-- Seam. The theatre id, or nil when no map is open: the module loads at the
-- main menu and answers nil there, so a non-nil id is what says the editor has
-- a map open or a mission is running.
--
-- The hook state spells the module table lowercase, terrain.GetTerrainConfig,
-- where the editor state spells it Terrain. Those are two entries in the DCS
-- symbol table and only the lowercase one is reachable from here.
--
-- The global is the fallback rather than an error because Lua 5.1 require
-- returns true, not the module, when a C module installs itself as a global
-- and returns nothing.
function M.terrain_id()
  local ok, mod = pcall(require, "terrain")
  if not ok then
    return nil
  end
  local terrain = type(mod) == "table" and mod or rawget(_G, "terrain")
  if type(terrain) ~= "table" or type(terrain.GetTerrainConfig) ~= "function" then
    return nil
  end
  local got, id = pcall(terrain.GetTerrainConfig, "id")
  if not got then
    return nil
  end
  return id
end

-- The theatre's bounds rectangle in meters, or nil with no terrain loaded or
-- a config that does not carry one. `SW_bound` and `NE_bound` are
-- `{x_km, 0, z_km}`; ED reads [1] as x and [3] as z and multiplies by a
-- thousand, and so does this.
function M.terrain_bounds()
  local ok, mod = pcall(require, "terrain")
  if not ok then
    return nil
  end
  local terrain = type(mod) == "table" and mod or rawget(_G, "terrain")
  if type(terrain) ~= "table" or type(terrain.GetTerrainConfig) ~= "function" then
    return nil
  end
  local got_sw, sw = pcall(terrain.GetTerrainConfig, "SW_bound")
  local got_ne, ne = pcall(terrain.GetTerrainConfig, "NE_bound")
  if not (got_sw and got_ne and type(sw) == "table" and type(ne) == "table") then
    return nil
  end
  local min_x, min_z, max_x, max_z = sw[1], sw[3], ne[1], ne[3]
  if not (is_finite(min_x) and is_finite(min_z)
      and is_finite(max_x) and is_finite(max_z)) then
    return nil
  end
  if min_x >= max_x or min_z >= max_z then
    return nil
  end
  return {
    min_x = min_x * 1000, min_z = min_z * 1000,
    max_x = max_x * 1000, max_z = max_z * 1000,
  }
end

--------------------------------------------------------------------------------
-- Logging
--
-- Two destinations. The progress log in Saved Games takes everything: a line
-- per tile, per phase change, per finished sweep and per failure. dcs.log takes
-- a phase change at INFO and anything the user has to act on at WARNING, and
-- nothing per tile -- at this hook's rate that would bury every other
-- subsystem's output.
--
-- ADR 0013: the path is a value rather than a function to swap. Nil means log
-- nowhere, which is a run with no Saved Games under it and is also every
-- offline test, so nothing here opens a file until something sets a path.
--
-- Append per line rather than a held handle. DCS is more often killed than
-- exited, and a buffered handle loses its tail in exactly the case where the
-- log is the only record of what the run was doing.
--------------------------------------------------------------------------------

M.log_path = nil

function M.log(message)
  if not M.log_path then
    return
  end
  M.append_file(M.log_path, M.now_iso() .. " " .. tostring(message) .. "\n")
end

-- Where the hook's own files live. Discovered, never recorded: no install or
-- Saved Games path belongs in this repository.
--
-- lfs is a hook-state global fetched at the call, like M.fs.mkdir does it, so
-- this file still loads under a plain interpreter. writedir() ends in a
-- separator already -- ED concatenates "Config/..." straight onto it -- and
-- M.join handles a trailing "/", so only the backslashes need turning around.
function M.saved_games_dir()
  local lfs = rawget(_G, "lfs")
  if not lfs or type(lfs.writedir) ~= "function" then
    return nil
  end
  local ok, dir = pcall(lfs.writedir)
  if not ok or type(dir) ~= "string" or dir == "" then
    return nil
  end
  return (dir:gsub("\\", "/"))
end

M.LOG_NAME = "Logs/DcsTerrainExtract.log"

-- The subsystem name dcs.log tags these lines with, so a reader can find them
-- among every other part of DCS writing to the same file.
M.DCS_LOG_SUBSYSTEM = "DcsTerrainExtract"

local DCS_LOG_LEVELS = { INFO = true, WARNING = true }

-- One line into dcs.log. Silent where there is no log global, which is every
-- offline test, and pcall'ed because a run must never fail on its own logging.
function M.dcs_log(level, message)
  if not DCS_LOG_LEVELS[level] then
    error("dcs_log: not a level: " .. tostring(level), 2)
  end
  local log = rawget(_G, "log")
  if type(log) ~= "table" or type(log.write) ~= "function" then
    return false
  end
  local ok = pcall(log.write, M.DCS_LOG_SUBSYSTEM, log[level], tostring(message))
  return ok
end

-- A problem the user has to act on: it goes to both destinations, because the
-- progress log is the run's own record and dcs.log is where somebody looks
-- when the hook appears to have done nothing.
function M.warn(message)
  M.log(message)
  M.dcs_log("WARNING", message)
end

function M.new_run(opts)
  opts = opts or {}
  local config = opts.config or {}
  local run = {
    state = M.STATE_STOPPED,
    config = config,
    jobs = opts.jobs or M.jobs,
    dir = config.output_dir,
    -- What was wrong with the config this run was made from, carried so the
    -- window can put each line under the field it belongs to. They are found
    -- before there is a window to show them in, and a warning in a log is not
    -- where somebody looking at an empty control will go looking for the
    -- reason it is empty.
    config_problems = opts.problems or {},
    config_tags = opts.tags or {},
    -- A constant, not config (ADR 0011). It stays a field on the run so a test
    -- can drive the budget by hand, which no config file could ever ask for.
    budget_ms = config.frame_budget_ms or M.FRAME_BUDGET_MS,
    -- Filled in by the prepare jobs, which is where the theatre, the build,
    -- the fingerprint and the bounds are read.
    identity = opts.identity or {},
    frames = 0,
    idle_frames = 0,
    phase_frames = 0,
    -- Accumulated on the run rather than in the manifest, because prepare
    -- times its own jobs before there is a manifest to put the timings in.
    timing_ms = {},
    entries = {},
    queue = nil,
    manifest = nil,
  }
  return run
end

-- Points a run at the settings the window now holds (ADR 0017).
--
-- Where the output directory and the crop are unchanged, only the config is
-- replaced and the run carries on where it left off, which is what makes a Stop
-- and a Start resume rather than restart.
--
-- Where either changed, everything the run accumulated goes. All of it
-- describes an output directory rather than a run: the manifest carries the
-- identity, the grid and the pass record, entries is the tile list rebuilt into
-- it at every save, and the timings are the work done in that directory. Kept
-- across a change, they would be written into the new directory by the save
-- that happens on entering prepare -- before any prepare job runs, so before
-- anything can notice they describe somewhere else, and a manifest naming tiles
-- that are not there fails validation permanently.
--
-- In place, never a fresh table: the callbacks closed over this run when the
-- bootstrap registered them, so a replacement would be invisible to the frames
-- that drive it.
--
-- Only legal from stopped or done. They are the two states with no queue, and
-- so the two in which no job is holding a directory it read at its own start.
-- It checks that itself rather than trusting whoever called it, and the reason
-- is where the raise would land: run_frame is called straight from
-- onSimulationFrame and is not under the window's latch, so a queue dropped
-- mid-pass would index a nil on the next frame and climb out into DCS's own
-- stack -- the one failure this file is built never to produce.
function M.retarget(run, config)
  if run.state ~= M.STATE_STOPPED and run.state ~= M.STATE_DONE then
    M.warn("the settings were not applied: a run in " .. tostring(run.state)
      .. " cannot be pointed somewhere else")
    return false
  end
  -- Through the encoder, which sorts keys, so two crops that encode the same
  -- hold the same numbers. Absent has to differ from present, and nil is not a
  -- value M.json takes.
  local function crop_text(crop)
    return crop ~= nil and M.json(crop) or "absent"
  end
  local same = run.config.output_dir == config.output_dir
    and crop_text(run.config.crop) == crop_text(config.crop)

  run.config = config
  run.dir = config.output_dir
  run.budget_ms = config.frame_budget_ms or M.FRAME_BUDGET_MS
  if same then
    return false
  end

  local fresh = M.new_run({ config = config, jobs = run.jobs })
  for key, value in pairs(fresh) do
    if key ~= "state" then
      run[key] = value
    end
  end
  -- pairs skips what new_run left nil, and those are exactly the fields that
  -- have to go: a manifest and a queue built for the old directory.
  run.manifest = nil
  run.queue = nil
  return true
end

-- Every manifest write goes through here, so the tile list and the timings are
-- never stale: the sweeps append to run.entries and the manifest copies are
-- rebuilt from the run.
function M.save(run)
  if not (run.manifest and run.dir) then
    return false
  end
  run.manifest.tiles = M.manifest_tiles(run.entries)
  run.manifest.timing_ms = run.timing_ms
  local ok, err = M.write_manifest(run.dir, run.manifest)
  if not ok then
    M.warn("manifest write failed: " .. tostring(err))
  end
  return ok and true or false
end

-- Overridden once prepare has a job the machine owns. Until then the phase is
-- whatever the sweeps registered.
function M.prepare_jobs(run)
  return run.jobs.prepare or {}
end

-- Overridden by the window, and a no-op until something does. Two of them,
-- because they answer different questions: on_phase is the run reaching a new
-- state, which is rare, and on_frame is the tick the window redraws on.
--
-- on_frame is called from the frame callback rather than from run_frame,
-- because run_frame does no work in stopped and returns immediately in done --
-- which between them are most of a session, and are exactly when a window still
-- has to be on screen and answering.
function M.on_phase(state) end
function M.on_frame(run) end

-- The one place a phase change is announced, so the window has one place to
-- attach rather than two calls to keep in step.
--
-- The window observes here rather than replacing this function, because the two
-- log lines are asserted as they stand and a window that took the function over
-- would take them with it.
local function phase_change(state)
  M.log("phase " .. state)
  M.dcs_log("INFO", "phase " .. state)
  M.ui(M.on_phase, state)
end

-- Moves the run into a state, and is the only place that does. A phase change
-- builds the queue for the phase it enters, logs to both destinations, and
-- saves the manifest, which with the per-sweep save is the whole of "the
-- manifest is rewritten at the end of each sweep and at every phase change".
function M.enter(run, state)
  run.state = state
  run.phase_frames = 0
  run.queue = nil

  local pass = PASS_OF[state]
  if pass then
    -- Stamped once, on the first entry into the pass, and left alone on any
    -- later one. A pass can be entered twice now: Stop during the second pass
    -- and Start again, and the first pass is re-entered with its work already
    -- done and journalled. Re-stamping would leave a manifest saying the pass
    -- started after it finished, which is the state a run killed between the
    -- two would be found in.
    --
    -- Both forms of unstamped are accepted. A fresh manifest holds JSON_NULL,
    -- and so does one decoded from disk, because the decoder reads null back as
    -- JSON_NULL rather than as nil.
    if run.manifest then
      local record = run.manifest.passes[pass]
      if record.started_at == nil or record.started_at == M.JSON_NULL then
        record.started_at = M.now_iso()
      end
    end
    -- A phase with nothing registered is legitimate: it is what every phase
    -- looks like before its sweeps are built.
    run.queue = M.new_queue(run.jobs[pass] or {})
  elseif state == M.STATE_PREPARE then
    run.queue = M.new_queue(M.prepare_jobs(run))
  end

  phase_change(state)
  M.save(run)
  return state
end

local function complete_pass(run, pass)
  if not run.manifest then
    return
  end
  local p = run.manifest.passes[pass]
  p.complete = true
  p.finished_at = M.now_iso()
end

-- ADR 0011: both passes always run, so this is a walk and not a choice. The two
-- passes are the two Lua states the sweeps call from, which is not something a
-- user was ever in a position to switch off usefully.
local function next_state(run)
  if run.state == M.STATE_PREPARE then
    return M.STATE_HOOK
  end
  if run.state == M.STATE_HOOK then
    return M.STATE_MISSION
  end
  return M.STATE_DONE
end

-- ADR 0010: what a server-state land or world call needs is loaded terrain,
-- not a running mission. Those calls answer correctly with the Mission Editor
-- open on a map, and crash DCS only when there is no terrain under them.
--
-- Checked every frame rather than once when the pass starts, because terrain
-- unloads when DCS returns to the main menu and this pass runs for tens of
-- minutes. The check is a package.loaded lookup and one C call.
local function terrain_loaded(run)
  if M.terrain_id() ~= nil then
    return true
  end
  M.warn("terrain unloaded during the " .. run.state .. " pass")
  return false
end

local function record_finished(run)
  local finished = run.queue.finished
  for i = 1, #finished do
    local job = finished[i]
    run.timing_ms[job.name] = (run.timing_ms[job.name] or 0) + job.ms
    M.log(format("%s finished in %d ms", job.name, job.ms))
  end
  if #finished > 0 then
    M.save(run)
  end
end

local function frame_idle(run)
  run.idle_frames = run.idle_frames + 1
  -- Poll on the first idle frame and every sixtieth after it, so a hook that
  -- loads with a map already open does not sit out a second of frames first.
  if (run.idle_frames - 1) % M.IDLE_POLL_FRAMES ~= 0 then
    return M.STATE_IDLE
  end
  local id = M.terrain_id()
  if id == nil then
    return M.STATE_IDLE
  end
  run.identity.theatre = id
  -- Which callbacks fire where is a per-build measurement, so the count
  -- reached in idle is the only evidence the hook has that it was given frames
  -- at the menu at all.
  M.log(format("terrain %s after %d idle frames", tostring(id), run.idle_frames))

  -- The first moment there is a theatre to hold the crop against. A config
  -- written at the main menu could not be checked when Start was pressed, so
  -- it is checked here, and a crop that reaches outside the map stops the run
  -- before prepare writes anything: the reason is kept on the run for the
  -- window to show and warned to the log (ADR 0019).
  local outside = M.crop_outside(run.config.crop, M.terrain_bounds())
  if outside then
    run.refusal = outside
    M.warn(outside)
    M.stop(run)
    return M.STATE_STOPPED
  end
  return M.enter(run, M.STATE_PREPARE)
end

local function frame_pass(run)
  run.phase_frames = run.phase_frames + 1
  local pass = PASS_OF[run.state]
  if pass and run.manifest then
    local p = run.manifest.passes[pass]
    p.frames = p.frames + 1
  end

  -- The sweeps of this pass reach the server state, so the run ends rather
  -- than calling into a terrain layer that is no longer there. The pass keeps
  -- the false `complete` it started with.
  if run.state == M.STATE_MISSION and not terrain_loaded(run) then
    return M.enter(run, M.STATE_DONE)
  end

  local status = M.queue_frame(run.queue, run, M.budget(run.budget_ms))
  record_finished(run)
  -- A job that cannot go on has left its reason on the run, the same place the
  -- crop check leaves one: the window shows it and the next Start clears it.
  -- The finished jobs were recorded first, because stop drops the queue they
  -- are reported in. A refusal with no reason is a bug in the job, and is
  -- still reported rather than left as a run that stopped for nothing.
  if status == M.REFUSED then
    if run.refusal == nil then
      run.refusal = "a job refused without saying why"
    end
    M.warn(run.refusal)
    M.stop(run)
    return M.STATE_STOPPED
  end
  if status == M.MORE then
    return run.state
  end
  if pass then
    complete_pass(run, pass)
  end
  return M.enter(run, next_state(run))
end

-- Leaves the stopped state and begins looking for terrain. Start always
-- re-enters idle, whether this is the first press or one after a Stop, so
-- there is one way back into a run rather than two (ADR 0014).
--
-- Nothing is reset. frames and the timings accumulate across a Stop, so what
-- the manifest records is the whole of the work done in this output directory
-- rather than the last attempt at it.
function M.start(run)
  -- From done as well as from stopped (ADR 0017). A finished run is not a
  -- running one, and refusing here made the window a single-use control: with
  -- no sweeps registered a run reaches done within a few frames of the first
  -- press, and both buttons were then inert until DCS was restarted. Restarted
  -- unchanged it resumes, finds every tile journalled, and returns to done --
  -- which looks like a button that did nothing, and is correct.
  if run.state ~= M.STATE_STOPPED and run.state ~= M.STATE_DONE then
    return false
  end
  -- Straight to idle rather than through enter, because idle is not a pass: it
  -- builds no queue and stamps nothing. It is still announced the same way, so
  -- the log carries one kind of line for a state change and not two, and a
  -- reader watching dcs.log sees the run begin.
  run.state = M.STATE_IDLE
  run.idle_frames = 0
  -- A refusal belongs to the attempt it stopped; this is the next one.
  run.refusal = nil
  phase_change(M.STATE_IDLE)
  return true
end

-- Halts the run and saves the manifest, so the tiles already written are found
-- again at the next Start.
--
-- A run stopped before prepare has no manifest to write, and M.save says so by
-- returning false. That is not a failure: there is nothing to resume to, and
-- the next Start recomputes it.
function M.stop(run)
  if run.state == M.STATE_STOPPED or run.state == M.STATE_DONE then
    return false
  end
  M.save(run)
  run.state = M.STATE_STOPPED
  run.queue = nil
  phase_change(M.STATE_STOPPED)
  return true
end

-- One simulation frame. Returns the state the run is in after it, which is the
-- same state on every frame but the ones that change phase.
function M.run_frame(run)
  -- Neither of these counts a frame. run.frames measures the work a run cost,
  -- and a hook sitting stopped at the main menu for an hour did none.
  if run.state == M.STATE_STOPPED then
    return M.STATE_STOPPED
  end
  if run.state == M.STATE_DONE then
    return M.STATE_DONE
  end
  run.frames = run.frames + 1
  if run.state == M.STATE_IDLE then
    return frame_idle(run)
  end
  return frame_pass(run)
end

--------------------------------------------------------------------------------
-- DCS callbacks
--
-- The four callbacks the run is driven by. onSimulationFrame is the whole
-- engine; the other three only tell the run things it cannot see from a frame.
--
-- Two of them only record that something happened, which is most of what the
-- progress log has to say about a run nobody is watching.
--
-- Nothing here registers itself. Registration needs a config, and reading one
-- is not built yet, so a DCS that loads this file gets a module and no run.
--------------------------------------------------------------------------------

-- Makes the next idle frame poll for a terrain rather than waiting out the
-- rest of the sixty.
local function poll_next_frame(run)
  run.idle_frames = 0
end

function M.callbacks(run)
  return {
    -- The window is ticked after the run, and on every frame rather than only
    -- the ones the run works on: it has to answer while the run is stopped and
    -- while it is done, which between them are most of a session.
    onSimulationFrame = function()
      M.run_frame(run)
      M.ui(M.on_frame, run)
    end,

    -- A mission has finished loading, so there is a terrain now whether or not
    -- the poll was due. No callback fires during a mission load, so this is
    -- also the first frame-adjacent event after one.
    onMissionLoadEnd = function()
      if run.state == M.STATE_IDLE then
        poll_next_frame(run)
      end
    end,

    -- A mission starting means a terrain too, so this ends the idle wait for
    -- the same reason a mission load does. It gates nothing: ADR 0010, the
    -- sweeps need terrain rather than a mission.
    onSimulationStart = function()
      M.log("simulation started")
      if run.state == M.STATE_IDLE then
        poll_next_frame(run)
      end
    end,

    -- Recorded and nothing more. A mission ending may or may not take the
    -- terrain with it -- back to the menu it goes, back to the editor it stays
    -- -- and the pass that cares tests for terrain on every frame rather than
    -- trusting an event to tell it.
    onSimulationStop = function()
      M.log("simulation stopped")
    end,
  }
end

-- Where a screen point falls on the Mission Editor's map, in DCS meters, or nil
-- when it is not on the map at all.
--
-- The editor's map lives in the gui state and this file runs in the hook state,
-- so the only way across is net.dostring_in, and the only thing that comes back
-- is a string.
--
-- Two questions, and both have to be asked. Whether the point is on the map is
-- not answered by getPointInMap, which sounds like it and is not: it tests
-- whether the coordinates land inside the theatre, so a click on the toolbar
-- above the map answers true. What does answer it is the widget painted at the
-- point -- if its root is the map's own window, the click was on the map.
-- Measured on 2.9.29.27468 with Caucasus in the editor: a click on the map gave
-- root == MapWindow.window, and one in the toolbar at y = 13 did not, while
-- getPointInMap called both of them inside.
--
-- Then getMapPoint converts, the same call getCurPosition uses. Formatted at
-- seventeen digits rather than tostring'd, because tostring is %.14g and a metre
-- coordinate reaches fourteen significant figures.
--
-- Everything is guarded and every failure is the same nil: no net is an offline
-- test, and no map is the main menu, where MapWindow has no view to ask.
local MAP_POINT_CHUNK = [[
local G = (type(Gui) == "table" and Gui.FindWidgetAtScreenPoint) and Gui
  or require("dxgui")
local ok, widget = pcall(G.FindWidgetAtScreenPoint, %d, %d)
if not ok or type(widget) ~= "userdata" then return "" end
local rooted, root = pcall(G.WidgetGetRoot, widget)
if not rooted then return "" end
local same, is_map = pcall(function()
  return root == MapWindow.window.widget
end)
if not same or not is_map then return "" end
local got, x, z = pcall(MapWindow.getMapPoint, %d, %d)
if not got or type(x) ~= "number" or type(z) ~= "number" then return "" end
return string.format("%%.17g %%.17g", x, z)
]]

function M.map_point_at(sx, sy)
  local net = rawget(_G, "net")
  if type(net) ~= "table" or type(net.dostring_in) ~= "function" then
    return nil
  end
  if not (is_finite(sx) and is_finite(sy)) then
    return nil
  end
  sx, sy = floor(sx), floor(sy)
  local ok, answer = pcall(net.dostring_in, "gui",
    format(MAP_POINT_CHUNK, sx, sy, sx, sy))
  if not ok or type(answer) ~= "string" then
    return nil
  end
  local mx, mz = answer:match("^(%S+) (%S+)$")
  local x, z = tonumber(mx or ""), tonumber(mz or "")
  if not (is_finite(x) and is_finite(z)) then
    return nil
  end
  return x, z
end

-- Returns nil and a reason where there is no DCS around this file, which is
-- every offline test and is not an error.
function M.register(run)
  local dcs = rawget(_G, "DCS")
  if type(dcs) ~= "table" or type(dcs.setUserCallbacks) ~= "function" then
    return nil, "DCS.setUserCallbacks is not available"
  end
  dcs.setUserCallbacks(M.callbacks(run))
  return true
end

--------------------------------------------------------------------------------
-- Widgets
--
-- The seam the window is built through, and what happens when a widget call
-- fails.
--
-- M.gui is the same shape as M.fs: everything DCS's widget library provides is
-- fetched here and nowhere else, at the call rather than at load, so this file
-- still loads under a plain interpreter with none of it around.
--
-- A failure switches the window off for the session and leaves the run alone.
-- The extract is the point; the window is how somebody watches it, and a
-- library that has started raising will raise again -- retrying it would put a
-- pcall and a traceback in the frame budget for as long as DCS is open (ADR
-- 0015).
--------------------------------------------------------------------------------

M.gui = {}

-- A widget class by name, or nil where the library is absent. Nil is not a
-- failure: it is what a plain interpreter answers, and the window simply is
-- not built there.
function M.gui.widget(name)
  local ok, class = pcall(require, name)
  if not ok or type(class) ~= "table" or type(class.new) ~= "function" then
    return nil
  end
  return class
end

-- `seen` holds what has been copied: a skin referring back to itself terminates
-- instead of running the stack out, and one referenced twice stays one table.
local function deep_copy(value, seen)
  if type(value) ~= "table" then
    return value
  end
  seen = seen or {}
  if seen[value] then
    return seen[value]
  end
  local out = {}
  seen[value] = out
  for k, v in pairs(value) do
    out[k] = deep_copy(v, seen)
  end
  return out
end

-- A named skin, deep-copied. The library hands back a fresh outer table but the
-- nested sub-skins are shared, so mutating one in place restyles the editor's
-- own dialogs for the rest of the session.
function M.gui.skin(name)
  local ok, skins = pcall(require, "Skin")
  if not ok or type(skins) ~= "table" or type(skins[name]) ~= "function" then
    return nil
  end
  local made, skin = pcall(skins[name])
  if not made or type(skin) ~= "table" then
    return nil
  end
  return deep_copy(skin)
end

-- The screen, in pixels, or nil where the library is absent or will not say.
-- Nil rather than a guess: the caller has a place to put the window without
-- one, and a guessed screen would put it somewhere on a screen it is not on.
function M.gui.screen_size()
  local ok, Gui = pcall(require, "dxgui")
  if not ok or type(Gui) ~= "table" or type(Gui.GetWindowSize) ~= "function" then
    return nil
  end
  local got, w, h = pcall(Gui.GetWindowSize)
  if not got or type(w) ~= "number" or type(h) ~= "number" or w <= 0 or h <= 0 then
    return nil
  end
  return w, h
end

-- Every mouse press anywhere in DCS, handed to fn(x, y, button).
--
-- Registered once and never removed: taking one off needs the same function
-- reference back, and the window has one handler for the life of the session
-- anyway. It is the callback's own business to do nothing when nothing has
-- asked it to listen, which is most of the time.
--
-- Gui or dxgui, whichever this build put it in -- the same fallback MarkPresets
-- uses, and for the same reason: the toolkit answers to both names depending on
-- the state.
function M.gui.on_mouse_down(fn)
  local lib = rawget(_G, "Gui")
  if type(lib) ~= "table" or type(lib.AddMouseCallback) ~= "function" then
    local ok, required = pcall(require, "dxgui")
    lib = ok and required or nil
  end
  if type(lib) ~= "table" or type(lib.AddMouseCallback) ~= "function" then
    return false
  end
  return pcall(lib.AddMouseCallback, "down", fn) and true or false
end

-- A value rather than a function to swap, so a test reads it the way it reads
-- any other state.
M.ui_failed = false
M.ui_failure = nil

-- One way, and reported once. A second line per frame would bury the log it is
-- trying to be useful in.
local function ui_fail(what, err)
  if M.ui_failed then
    return
  end
  M.ui_failed = true
  M.ui_failure = tostring(what) .. ": " .. tostring(err)
  -- Latched before it is reported, and the report guarded: M.log opens a file,
  -- so a raise here would climb out of the seam into the frame callback.
  pcall(M.warn, "the extract window has been switched off for this session "
    .. "after " .. M.ui_failure .. ". The run itself is unaffected.")
end

-- Calls fn under the latch. nil once the window has been abandoned, which is
-- what makes a chain of these collapse quietly rather than at the first index.
function M.ui(fn, ...)
  if M.ui_failed then
    return nil
  end
  if type(fn) ~= "function" then
    ui_fail("ui", "not a function: " .. type(fn))
    return nil
  end
  local ok, result = pcall(fn, ...)
  if not ok then
    ui_fail("ui", result)
    return nil
  end
  return result
end

-- Two entry points and not one, because a class constructor is a plain call and
-- everything afterwards is a method: one wrapper would pass the class as self.
--
-- A nil object once the latch is set is a constructor having failed, and says
-- nothing. Before it is set it is this file's own bug -- a name typed wrong --
-- and latches, because a window skipping half its widgets would look built.
function M.ui_method(obj, method, ...)
  if M.ui_failed then
    return nil
  end
  if obj == nil then
    ui_fail("ui_method", tostring(method) .. " on nothing")
    return nil
  end
  local ok, result = pcall(function(...)
    return obj[method](obj, ...)
  end, ...)
  if not ok then
    ui_fail("ui_method " .. tostring(method), result)
    return nil
  end
  return result
end
--------------------------------------------------------------------------------
-- Window
--
-- The chrome and the status line. The controls hang off this and arrive next.
--
-- Built on the first frame rather than at load, because a widget wants the
-- library warm and load is the one moment nothing else in DCS is ready.
--
-- Never closable: the close button fires the window's own onClose after the
-- native side has already hidden it, so re-asserting visibility there is what
-- refuses the close. Measured against the title bar's X, not inferred.
--------------------------------------------------------------------------------

M.WINDOW_TITLE = "DCS TERRAIN EXTRACT"

-- Above DCS's own chrome (ADR 0018). A window at the default zero is drawn
-- underneath the menu, the Mission Editor and the map view, which looks exactly
-- like a window that has been destroyed: it goes on answering every question put
-- to it, getVisible included, while nobody can see it. Raised, it stays up
-- across every screen DCS puts in front of the user and through a map click.
--
-- 10001 is the number ED uses for its own window that has to stay up, and there
-- is no map of this space to pick a smaller one from.
M.WINDOW_Z_ORDER = 10001

-- Hand-placed pixels. There is no layout engine here worth the indirection: the
-- window is one column of rows and the arithmetic is an addition per row.
--
-- h is where the window starts rather than where it ends. The rows decide the
-- height, and it is set once they have all been placed -- while the window is
-- still hidden, so nothing is seen resizing.
--
-- x and y are where the window opens when the screen cannot be measured. When
-- it can, the window opens centerd along the top, because the top-left corner
-- is the Mission Editor's own toolbar and a window raised above everything
-- (ADR 0018) would sit on it until it was dragged away.
--
-- The heights are the Mission Editor's: its own dialogs place a label and a box
-- at 20 pixels and a button at 24, and a control drawn taller than its skin
-- expects gets a stretched border rather than a bigger control.
--
-- w is set by the widest row, which is the tick and the pick button beside it;
-- everything else is stacked to fit under it rather than spread to fill it.
--
-- button is the pick button, whose label is four words; run_button is Start
-- and Stop, one word each, at the width the editor gives its own OK.
local WIN = {
  x = 60, y = 40, w = 360, h = 260,
  pad = 10, row = 20, button_h = 24, bar = 12, gap = 6, button = 130,
  run_button = 90, radius_box = 70,
  -- Two lines of the label font, measured with calcSize on a wrapped label.
  line_h = 32,
}

-- The Mission Editor's own skins, so the window is drawn with what the editor
-- draws itself with -- and the editor is not one set of skins but several,
-- so these are the ones its group panel is built from, read off that panel's
-- dialog file: the window with the 30-pixel header and the cyan title, the
-- gray labels, the dark boxes, the square ticks and the flat buttons. The bar
-- has no editor variant; the one here is what ED's own loading dialog draws
-- its bar with, and the stock `horzProgressBarSkin` is a pale green that
-- belongs to no screen the editor has.
local SKIN = {
  window = "windowSkinME",
  panel = "panelSkin",
  static = "staticSkin_ME",
  edit = "editBoxNew",
  check = "checkBoxSkin_MENew",
  button = "buttonSkin_MENew2",
  bar = "horzProgressBarStartDialogSkin",
}

-- The text in front of each field. Keyed by field name rather than built from
-- it: "output_dir" is what the config file calls it and not what a user should
-- have to read.
--
-- Upper case, because that is how the editor's own panels label a box: the
-- skin does not do it, the text does.
local FIELD_LABEL = {
  output_dir = "OUTPUT DIRECTORY",
  crop = "CROP TO A CENTER AND A RADIUS",
}

local CROP_LABEL = { crop_x = "X", crop_z = "Z", crop_radius_m = "RADIUS (M)" }
local CROP_ORDER = { "crop_x", "crop_z", "crop_radius_m" }
-- As wide as the label's text, so the boxes get the room.
local CROP_LABEL_W = { crop_x = 14, crop_z = 14, crop_radius_m = 68 }

-- What a control says when the cursor rests on it. The box is where the value
-- goes; the tooltip is the one place there is room to say what the value means.
local TOOLTIP = {
  output_dir = "The directory the extract is written to. "
    .. "Start again in the same directory to carry on from where it stopped.",
  crop = "Extract a circle around a center. Unticked, the whole theatre is swept.",
  crop_x = "The center's DCS x, in meters. North is positive.",
  crop_z = "The center's DCS z, in meters. East is positive.",
  crop_radius_m = "How far from the center to extract, in meters.",
  crop_pick = "Press, then click a point on the map to take it as the center.",
  start = "Save these settings and start, or carry on from where the run stopped.",
  stop = "Stop the run. Start carries on from where it stopped.",
}

-- The keyboard order between the boxes: the directory, then the center, then
-- the radius. The tick and the buttons are not in it, because Tab is for
-- moving between things that are typed into.
local TAB_ORDER = { "output_dir", "crop_x", "crop_z", "crop_radius_m" }

-- The buttons, left to right. A list rather than a placed widget each, so
-- adding one is an entry here and not another x to work out by hand.
local BUTTONS = {
  { name = "start", text = "START" },
  { name = "stop", text = "STOP" },
}

M.window = { built = false, root = nil, panel = nil, status = nil, bar = nil }

-- Skin, place, insert -- in that order, for every widget in the window. The
-- order is the one the chrome already used and the reason is the same: a widget
-- draws as soon as its parent has it, so it is finished before it is handed
-- over. An unskinned one draws nothing at all.
--
-- Where each widget was placed is kept, because the rows under the crop move
-- up when it is hidden and back down when it is shown, and their place in the
-- full layout is what they move from. Weak keys, so a window that is dropped
-- takes its bounds with it.
local PLACED = setmetatable({}, { __mode = "k" })

local function place(panel, widget, skin_name, x, y, w, h)
  M.ui_method(widget, "setSkin", M.ui(M.gui.skin, skin_name))
  M.ui_method(widget, "setBounds", x, y, w, h)
  M.ui_method(panel, "insertWidget", widget, -1)
  if widget ~= nil then
    PLACED[widget] = { x = x, y = y, w = w, h = h }
  end
  return widget
end

-- The window skin with its close button taken off. A window that refuses to
-- close should not offer to, and the refusal in onClose stays underneath as
-- the guard for a skin that draws the button anyway.
--
-- The path is the editor skin's own: the header is a sub-skin, and the button
-- is a parameter of it. Every hop is checked, because the skin is a deep copy
-- of whatever the library handed over, and a skin shaped differently is left
-- as it is rather than reached into.
local function without_close_button(skin)
  local data = type(skin) == "table" and skin.skinData
  local skins = type(data) == "table" and data.skins
  local header = type(skins) == "table" and skins.header
  local hdata = type(header) == "table" and header.skinData
  local params = type(hdata) == "table" and hdata.params
  if type(params) == "table" then
    params.hasCloseButton = false
  end
  return skin
end

-- A label skin with its text centered vertically. The editor's label skin
-- puts text at the top of the label, which is right for a label over a box
-- and wrong for one that shares a row with a button, whose skin centers its
-- own text: the two would sit a few pixels apart. The skin is this window's
-- copy, so the change reaches nothing else. Every state is walked, and one
-- without text alignment is left alone rather than given some.
local function vertically_centered(skin)
  local data = type(skin) == "table" and skin.skinData
  local states = type(data) == "table" and data.states
  if type(states) ~= "table" then
    return skin
  end
  for _, state in pairs(states) do
    local first = type(state) == "table" and state[1]
    local text = type(first) == "table" and first.text
    local align = type(text) == "table" and text.vertAlign
    if type(align) == "table" then
      align.type = "middle"
    end
  end
  return skin
end

-- Every control, in a fixed order. pairs order is undefined in 5.1, and a
-- window whose boxes filled in a different order each session could not be
-- tested for having filled them at all.
local CONTROL_ORDER =
  { "output_dir", "crop", "crop_x", "crop_z", "crop_radius_m" }

-- Clears every line, then writes one per problem against the field it belongs
-- to. Clearing first is what makes a problem the user has fixed disappear.
--
-- tags is parallel to problems and has holes, because a problem can belong to
-- no control at all -- an unrecognised key in the config file is one, and there
-- is no box on screen for a field that does not exist. So this walks problems,
-- never tags, and a tag with no line is skipped rather than indexed: reaching
-- setText through a nil would take the whole window down over a typo in a file.
-- The screen name of the field a problem starts with. A problem is phrased
-- once, in the config file's own words -- `output_dir`, `crop.x` -- because the
-- log reads it too and a log line has to be matched to a key in a file. Under a
-- box labelled "Output directory" those words are somebody else's, so the line
-- swaps the leading key for the label the box carries and changes nothing else.
-- Longest key first, so that `crop.x` is not read as `crop` followed by `.x`.
local SCREEN_NAME = {
  { "output_dir", "Output directory" },
  { "crop.radius_m", "Radius" },
  { "crop.x", "X" },
  { "crop.z", "Z" },
  { "crop", "Crop" },
}

-- Two problems carry an explanation after a comma that the log has room for
-- and the line beside the button has not. The screen keeps the finding and
-- drops the explanation; the log keeps both.
local SCREEN_TRIM = {
  ", and there is no default for it",
  ", which is usually a backslash escape in a double-quoted path",
}

-- A problem for the line beside the button: the field's label for its key,
-- the explanations above dropped, and the value the log echoes after the
-- colon dropped too -- the box the problem is about already shows it, and a
-- path or a number can be any length. What is left is the finding, as a
-- sentence. Every message is written so that what matters is before the
-- colon: the drive, the map edge.
function M.problem_for_screen(problem)
  for i = 1, #SCREEN_TRIM do
    local at = problem:find(SCREEN_TRIM[i], 1, true)
    if at then
      problem = problem:sub(1, at - 1) .. problem:sub(at + #SCREEN_TRIM[i])
    end
  end
  local colon = problem:find(": ", 1, true)
  if colon then
    problem = problem:sub(1, colon - 1)
  end
  if problem:sub(-1) ~= "." then
    problem = problem .. "."
  end
  for i = 1, #SCREEN_NAME do
    local key, label = SCREEN_NAME[i][1], SCREEN_NAME[i][2]
    if problem:sub(1, #key) == key then
      return label .. problem:sub(#key + 1)
    end
  end
  return problem
end

local function set_controls(config)
  local text = M.control_text(config or {})
  for i = 1, #CONTROL_ORDER do
    local name = CONTROL_ORDER[i]
    local widget = M.window.controls[name]
    if name == "crop" then
      M.ui_method(widget, "setState", text.crop)
    else
      M.ui_method(widget, "setText", text[name])
    end
  end
end

-- Closes up the space of whichever blocks are hidden. The rows under a hidden
-- block move up by its height and the window shrinks by the same, so a hidden
-- block costs no blank space. A row moves from where the full layout put it
-- rather than from where it is, so being laid out twice does not move it
-- twice; a row under both blocks moves by both. The window keeps the position
-- it has, which is wherever it was dragged to.
local function relayout()
  local w = M.window
  local crop_dy = w.crop_shown and 0 or w.crop_block_h
  local bar_dy = w.bar_shown and 0 or w.bar_block_h
  local shift = {}
  for i = 1, #w.below_crop do
    shift[w.below_crop[i]] = (shift[w.below_crop[i]] or 0) + crop_dy
  end
  for i = 1, #w.below_bar do
    shift[w.below_bar[i]] = (shift[w.below_bar[i]] or 0) + bar_dy
  end
  for widget, dy in pairs(shift) do
    local at = PLACED[widget]
    if at then
      M.ui_method(widget, "setBounds", at.x, at.y - dy, at.w, at.h)
    end
  end
  local dy = crop_dy + bar_dy
  local pos = M.ui(function()
    local x, y = w.root:getBounds()
    return { x = x, y = y }
  end)
  if pos then
    M.ui_method(w.root, "setBounds", pos.x, pos.y, w.frame.w, w.frame.h - dy)
  end
  M.ui_method(w.panel, "setBounds", 0, 0, w.client.w, w.client.h - dy)
end

-- The crop's boxes, their labels and the pick button, shown only while the tick
-- is on. An unticked crop has nothing to type into, and three empty boxes under
-- a tick that is off read as three things still to be filled in.
local function show_crop(on)
  local w = M.window
  for i = 1, #CROP_ORDER do
    M.ui_method(w.controls[CROP_ORDER[i]], "setVisible", on)
    M.ui_method(w.crop_labels[i], "setVisible", on)
  end
  M.ui_method(w.controls.crop_pick, "setVisible", on)
  w.crop_shown = on
  relayout()
end

-- The bar, shown only once there is progress on it. A bar standing at nothing
-- says nothing the line beside the button does not, and takes a row to say it.
local function show_bar(on)
  local w = M.window
  M.ui_method(w.bar, "setVisible", on)
  w.bar_shown = on
  relayout()
end

-- The tick's own state, read back rather than remembered: the widget library
-- has already toggled it by the time the change is reported, which is the
-- order ED's own handlers rely on.
local function crop_ticked()
  return M.ui_method(M.window.controls.crop, "getState") and true or false
end

local function crop_toggled()
  show_crop(crop_ticked())
end

-- What the controls hold, in the shape config_from_text takes, or nil where the
-- window failed part-way through being read.
--
-- Nil and not a partial answer, because every failure here reads as a legal
-- value rather than as an error. A getState that raises answers nil, and
-- `nil and true or false` is false -- which is an unticked crop, indistinguishable
-- from a user who does not want one. A getText that raises answers nil, which is
-- a blank box. So a widget library that starts raising during the read hands
-- back settings that look complete, validate clean, and are somebody else's: the
-- config file would be overwritten with the crop dropped and a full theatre
-- swept in its place, with the window dark and unable to say so.
--
-- The latch is the signal. It is false on entry, because a press cannot reach a
-- handler once it is set, so finding it true here means one of these calls set
-- it.
local function read_controls()
  local values = {}
  local controls = M.window.controls
  for i = 1, #CONTROL_ORDER do
    local name = CONTROL_ORDER[i]
    if name == "crop" then
      values.crop = M.ui_method(controls.crop, "getState") and true or false
    else
      values[name] = M.ui_method(controls[name], "getText")
    end
  end
  if M.ui_failed then
    return nil
  end
  return values
end

-- The line beside the button, which shows whatever was said last.
local function say(text)
  M.ui_method(M.window.message, "setText", text)
  -- Whatever this was, it was not an instruction; instruct sets that back.
  M.window.instructing = false
end

-- The first problem, on the same line. There is one line and the problems
-- come in the order the fields are shown, so the one shown is the first thing
-- to fix; fixing it and pressing Start brings the next. Every problem has
-- already gone to the log, which is where the whole list is. Nothing is
-- written when there is nothing wrong, so the instruction or the last press's
-- outcome stays where it was.
local function show_problems(problems)
  if problems == nil or problems[1] == nil then
    return
  end
  say(M.problem_for_screen(problems[1]))
end

-- A press arrives on DCS's own stack, called from inside the widget library, so
-- a raise in a handler lands where nothing this file wrote can catch it. Under
-- the latch, then, like every other widget call: a handler that fails switches
-- the window off for the session and leaves the run untouched, which is the
-- same one-directional failure the rest of the window already has.
--
-- It follows that a run under a dead window cannot be stopped from the window.
-- That is the accepted half of the same coin: the extract is the point, and a
-- run nobody can watch is still a run.
local function on_press(fn)
  return function()
    M.ui(fn)
  end
end

-- Start, and the order of it is the design.
--
-- The state is checked before anything is written, because a press during a run
-- that went on to write the file would leave the next DCS start using a
-- directory this run never used, with nothing on screen having said so.
--
-- Then the boxes are validated, and a problem stops it there: nothing is
-- written and nothing begins, so what is refused is exactly what is on the
-- lines above.
--
-- The file is written before the run is told anything, and a failure to write
-- it does not refuse the press. The extract is the point; the config file is
-- only how the settings come back next time, and a user who cannot save them
-- would rather have the run than the file. It is written from the validated
-- table rather than the boxes, and the boxes are then filled from that same
-- table, so what is on screen is what was saved -- a pasted path with
-- backslashes in it comes back with forward slashes.
local function start_pressed()
  local run = M.window.run
  if run == nil then
    return
  end
  if run.state ~= M.STATE_STOPPED and run.state ~= M.STATE_DONE then
    say("A run is already going. Stop it first.")
    return
  end

  -- Nothing is written and nothing begins when the window broke while being
  -- read. Saying so on the message line is not possible -- a latched window
  -- writes no text -- so the log is the only place it can go, and it has to go
  -- somewhere: the user pressed Start and is owed more than a line about the
  -- window having switched itself off.
  local values = read_controls()
  if values == nil then
    M.warn("Start was pressed, but the window failed while it was being read. "
      .. "Nothing was saved and no run was started.")
    return
  end

  local settings, problems = M.validate_config(M.config_from_text(values))
  if #problems > 0 then
    show_problems(problems)
    return
  end
  -- Against the disk: the drive has to be there, since the rest is made.
  local missing = M.drive_problem(settings.output_dir)
  if missing then
    show_problems({ missing })
    return
  end
  -- Against the map, where there is one. At the main menu there is nothing to
  -- check against, and the run does this check itself when the terrain
  -- appears, so nothing is lost by pressing Start there.
  local outside = M.crop_outside(settings.crop, M.terrain_bounds())
  if outside then
    show_problems({ outside })
    return
  end

  local path = M.config_path()
  local saved, why
  if path == nil then
    why = "there is no Saved Games directory to write it to"
  else
    saved, why = M.write_config(path, settings)
  end
  set_controls(settings)

  -- Nothing is said about a start that went well: the run moves on within a
  -- frame and the phase it moves into is the news. A save that failed is said,
  -- for the frame it lasts, and warned to the log, where it keeps.
  if not saved then
    say("Started. Settings not saved.")
    M.warn("could not save the config: " .. tostring(why))
  end

  -- Before the run is told to go, and after the file is written, so what is on
  -- disk and what the run is about to use are the same settings (ADR 0017).
  M.retarget(run, settings)
  M.start(run)
end

-- The crop center, off the map instead of out of the keyboard.
--
-- It ticks the crop as well as filling the two boxes. Reading a center off the
-- map is the deliberate act the tick is meant to record, and leaving it unticked
-- would let somebody press this, press Start, and sweep the whole theatre --
-- forty minutes on Caucasus -- having just told the window where they wanted to
-- extract. A ticked crop with no radius is refused on the crop's own line, which
-- is the right way to be told what is still missing.
-- Arms the next click on the map, and the button says which state it is in.
--
-- Arming rather than reading where the cursor happens to be: a button that took
-- the last hovered position needs the user to know that hovering comes before
-- pressing, and nothing on screen says so. Pressing, then clicking the point, is
-- the order somebody would guess.
--
-- Pressing it again disarms, because an armed handler with no way out would
-- leave the next click somewhere else doing something unexpected.
M.PICK_IDLE = "PICK ON MAP"
M.PICK_ARMED = "CLICK THE MAP..."

local function set_pick_label()
  M.ui_method(M.window.controls.crop_pick, "setText",
    M.window.arming and M.PICK_ARMED or M.PICK_IDLE)
end

local function pick_pressed()
  M.window.arming = not M.window.arming
  set_pick_label()
end

-- Every press in DCS arrives here. It does nothing at all unless the button
-- above has armed it, which is the usual case, and nothing when the press was
-- not on the map -- so a click on the toolbar, or on this window, neither takes
-- a coordinate nor disarms.
local function clicked(x, y)
  if not M.window.arming then
    return
  end
  local mx, mz = M.map_point_at(x, y)
  if mx == nil then
    return
  end
  -- A point off the edge of the theatre is not a center anybody wants, and
  -- the next Start would refuse it. Ignored like a click off the map view,
  -- with the pick left armed for the click that lands.
  local bounds = M.terrain_bounds()
  if bounds and (mx < bounds.min_x or mx > bounds.max_x
      or mz < bounds.min_z or mz > bounds.max_z) then
    return
  end
  M.window.arming = false
  set_pick_label()
  -- Whole meters, as the editor's own status bar shows the cursor. The point
  -- is the editor's answer for a pixel, and a pixel is tens of meters at any
  -- zoom the map is picked at, so the fraction is noise that would only make
  -- the box harder to read and to retype.
  M.ui_method(M.window.controls.crop_x, "setText", M.box_text(floor(mx + 0.5)))
  M.ui_method(M.window.controls.crop_z, "setText", M.box_text(floor(mz + 0.5)))
  M.ui_method(M.window.controls.crop, "setState", true)
  show_crop(true)
end

-- The seam the click arrives through, so a test can deliver one.
function M.on_map_click(x, y)
  M.ui(clicked, x, y)
end

local function stop_pressed()
  local run = M.window.run
  if run == nil then
    return
  end
  if M.stop(run) then
    say("Stopped. Start carries on from here.")
  else
    say("Nothing to stop.")
  end
end

function M.build_window()
  if M.window.built then
    return true
  end
  -- Asked once. A widget library that is not there will not turn up later, and
  -- this is called on every frame until it succeeds: a failed lookup costs
  -- about a tenth of a millisecond, so three of them a frame is a twentieth of
  -- the frame budget spent forever on an answer that cannot change.
  if M.window.unavailable then
    return false
  end
  -- Through the latch, like everything else: the seam reaches the widget
  -- library, and that is exactly the thing that might not be there.
  local Window = M.ui(M.gui.widget, "Window")
  local Panel = M.ui(M.gui.widget, "Panel")
  local Static = M.ui(M.gui.widget, "Static")
  local Bar = M.ui(M.gui.widget, "HorzProgressBar")
  local Edit = M.ui(M.gui.widget, "EditBox")
  local Check = M.ui(M.gui.widget, "CheckBox")
  local Push = M.ui(M.gui.widget, "Button")
  if not (Window and Panel and Static and Bar and Edit and Check and Push) then
    M.window.unavailable = true
    return false
  end

  -- Where the window is built. It is moved to the center of the screen once
  -- its size is known, below; this is only where it waits, hidden, until then.
  local wx, wy = WIN.x, WIN.y
  local root = M.ui(Window.new, wx, wy, WIN.w, WIN.h, M.WINDOW_TITLE)
  -- Hidden until it has been laid out. A widget with the right bounds and a
  -- true visibility flag still draws before its parent has recomputed, and a
  -- half-placed window flickering into the editor is worse than a late one.
  M.ui_method(root, "setVisible", false)
  M.ui_method(root, "setSkin", without_close_button(M.ui(M.gui.skin, SKIN.window)))
  M.ui_method(root, "setDraggable", true)
  M.ui_method(root, "setResizable", false)
  M.ui_method(root, "setZOrder", M.WINDOW_Z_ORDER)

  local panel = M.ui(Panel.new)
  M.ui_method(panel, "setSkin", M.ui(M.gui.skin, SKIN.panel))
  M.ui_method(panel, "setBounds", 0, 0, WIN.w, WIN.h)
  M.ui_method(root, "insertWidget", panel, -1)

  local inner = WIN.w - WIN.pad * 2
  local y = WIN.pad

  -- The settings first and the run under them, which is the order they are
  -- used in: fill the boxes, then press, then watch.
  --
  -- In the order config_fields hands them over, which is the order the config
  -- section says a window shows them in. A field added there appears here
  -- without this loop changing, so long as its kind has a shape below.
  local controls, crop_labels = {}, {}
  -- Two blocks come and go: the crop's, with the tick, and the bar's, with
  -- progress. The rows under each move up by its height while it is hidden,
  -- so each block has its height and the list of what is under it. The bar
  -- is last, so nothing is under it and hiding it only shortens the window;
  -- the list is kept so that a row added under it later moves like the rest.
  local below_crop, crop_block_h = {}, 0
  local below_bar, bar_block_h = {}, 0
  local fields = M.config_fields()
  for i = 1, #fields do
    local field = fields[i]
    local caption = FIELD_LABEL[field.name] or field.name

    if field.kind == "crop" then
      -- The tick is the field's own label, because what it switches on is the
      -- field: three boxes with no way to mean "no crop" would make an empty
      -- crop and a crop nobody asked for the same thing.
      controls[field.name] = place(panel, M.ui(Check.new, caption),
        SKIN.check, WIN.pad, y, inner, WIN.row)
      y = y + WIN.row + WIN.gap

      -- Everything from here to the crop's own line is the block the tick
      -- switches on. It is hidden with the tick off, and the rows under it
      -- move up into its place, so its height is what they move by.
      local block_top = y

      -- One row: label, box, label, box, label, box. Each label is as wide as
      -- its text and no wider, the radius box is the narrow one, and the two
      -- halves of the center share what is left: a radius is a few digits of
      -- meters, and a coordinate is a sign and six digits.
      --
      -- The labels are kept, because they are hidden and shown with their
      -- boxes.
      local labels_w = 0
      for c = 1, #CROP_ORDER do
        labels_w = labels_w + CROP_LABEL_W[CROP_ORDER[c]]
      end
      local wide = floor((inner - labels_w - WIN.radius_box - WIN.gap * 4) / 2)
      local x = WIN.pad
      for c = 1, #CROP_ORDER do
        local name = CROP_ORDER[c]
        local box = (name == "crop_radius_m") and WIN.radius_box or wide
        crop_labels[c] = place(panel, M.ui(Static.new, CROP_LABEL[name]),
          SKIN.static, x, y, CROP_LABEL_W[name], WIN.row)
        x = x + CROP_LABEL_W[name]
        controls[name] = place(panel, M.ui(Edit.new, ""), SKIN.edit,
          x, y, box, WIN.row)
        x = x + box + WIN.gap * 2
      end
      y = y + WIN.row + WIN.gap

      -- Under the two boxes it fills, at the left where they start. It ticks
      -- the crop as well, so it belongs to the crop rather than to the row of
      -- buttons that act on the run.
      --
      -- Nothing is written beside it: the button's own label changes while it
      -- is armed, the tooltip says the rest, and what is wrong with the crop
      -- goes on the line over the bar with every other problem.
      controls.crop_pick = place(panel, M.ui(Push.new, M.PICK_IDLE),
        SKIN.button, WIN.pad, y, WIN.button, WIN.button_h)
      y = y + WIN.button_h + WIN.gap

      crop_block_h = y - block_top
    else
      -- The label above the box, and the box the full width: a path is long,
      -- and beside its label it was the width of the window that was wrong.
      place(panel, M.ui(Static.new, caption), SKIN.static,
        WIN.pad, y, inner, WIN.row)
      y = y + WIN.row
      controls[field.name] = place(panel, M.ui(Edit.new, ""), SKIN.edit,
        WIN.pad, y, inner, WIN.row)
      y = y + WIN.row + WIN.gap
    end
  end

  -- The run: the one line and the one button, and under them the bar.
  --
  -- The line is the latest thing there is to say: the
  -- instruction at load, what is wrong with a field, what a press came to,
  -- which phase the run has moved into, and later the progress and errors of
  -- a sweep. Each replaces the one before, which is the whole of the rule. It
  -- is the button's height so the text sits level with the button's label,
  -- and it is as wide as what the button leaves, which is about forty-five
  -- characters -- the reason every line is short, and the reason the two
  -- problems with an explanation attached lose it for the screen.
  --
  -- The button for the next press is at the right, where the Mission Editor's
  -- own dialogs keep the button that acts on the whole of one. The two are
  -- placed on top of one another in that one slot, and only one is ever
  -- shown: Start while the run is stopped, Stop while it is going, because the
  -- other one would refuse the press anyway, and a button that cannot do
  -- anything is one more thing to read.
  --
  -- The line wraps, and the row is two lines tall with the button centered
  -- in it: a line that fits stays one line in the middle of the row, and a
  -- line that does not gets a second rather than a cut.
  local message = place(panel, M.ui(Static.new, ""), SKIN.static,
    WIN.pad, y, inner - WIN.run_button - WIN.gap, WIN.line_h)
  -- Skinned again with the text centered, so it sits level with the button's
  -- label rather than at the top of the row.
  M.ui_method(message, "setSkin",
    vertically_centered(M.ui(M.gui.skin, SKIN.static)))
  M.ui_method(message, "setWrapping", true)
  below_crop[#below_crop + 1] = message
  local buttons = {}
  local by = y + floor((WIN.line_h - WIN.button_h) / 2)
  for i = 1, #BUTTONS do
    local spec = BUTTONS[i]
    buttons[spec.name] = place(panel, M.ui(Push.new, spec.text), SKIN.button,
      WIN.pad + inner - WIN.run_button, by, WIN.run_button, WIN.button_h)
    below_crop[#below_crop + 1] = buttons[spec.name]
  end
  y = y + WIN.line_h

  -- The bar, a row under the button and the line, saying how far through the
  -- run is. On screen only once there is progress to show on it; until then
  -- the window ends at the button. Nothing is under it, so its block is the
  -- gap above it and itself.
  y = y + WIN.gap
  local bar = place(panel, M.ui(Bar.new), SKIN.bar,
    WIN.pad, y, inner, WIN.bar)
  M.ui_method(bar, "setRange", 0, 100)
  y = y + WIN.bar
  below_crop[#below_crop + 1] = bar
  bar_block_h = WIN.gap + WIN.bar

  -- What each control is for, where the cursor rests on it, and the order Tab
  -- moves between the boxes. Both are per widget and both are set once.
  for name, text in pairs(TOOLTIP) do
    M.ui_method(controls[name] or buttons[name], "setTooltipText", text)
  end
  for i = 1, #TAB_ORDER do
    M.ui_method(controls[TAB_ORDER[i]], "setTabOrder", i)
  end

  -- What the rows came to. Set while the window is still hidden, so the height
  -- is never seen changing, and taken from the same cursor that placed them, so
  -- a row added above cannot leave the last one hanging below the frame.
  -- The rows were placed in client coordinates, and a window's own bounds are
  -- the frame: a 400 x 200 window is granted a client area of 400 x 180, the
  -- other twenty pixels being the header. Laying rows out against the frame is
  -- what put the buttons off the bottom edge.
  --
  -- The inset is measured rather than assumed, because it belongs to the skin
  -- and not to this file: set the size to the content, ask what client area that
  -- bought, and grow the frame by the shortfall. Asked again afterwards, because
  -- the answer to the second setSize is the one the panel has to fill.
  -- Through M.ui rather than M.ui_method, and returning a table, because the
  -- seam hands back one value and getViewBounds answers four.
  local function client_of(window)
    return M.ui(function()
      local x, y, w, h = window:getViewBounds()
      return { x = x, y = y, w = w, h = h }
    end)
  end

  local content = y + WIN.pad
  local width, height = WIN.w, content
  M.ui_method(root, "setBounds", wx, wy, width, height)
  local view = client_of(root)
  -- A measurement of nothing is not a measurement. Zero or negative would drive
  -- a correction the size of the whole window, so it is left alone instead.
  if view and (view.w or 0) > 0 and (view.h or 0) > 0 then
    local dw, dh = width - view.w, content - view.h
    if dw ~= 0 or dh ~= 0 then
      width, height = width + dw, content + dh
      M.ui_method(root, "setBounds", wx, wy, width, height)
      -- What was asked for, where the second measurement fails -- never the
      -- first one. The frame has already grown by the inset, so falling back to
      -- the pre-correction reading would put an undersized panel inside a
      -- correctly sized window and clip the bottom row all over again, one call
      -- deeper than the bug this is here to fix.
      view = client_of(root) or { w = WIN.w, h = content }
    end
  end
  -- No client rectangle is a widget library that does not answer it, which the
  -- seam reports as nil. The frame is then the best measurement there is.
  M.ui_method(panel, "setBounds", 0, 0,
    (view and view.w) or WIN.w, (view and view.h) or content)

  -- The center of the screen, now that the frame has its size, and the fixed
  -- corner where the screen cannot be measured. The center rather than a
  -- corner because every corner is somebody's: the top-left is the Mission
  -- Editor's toolbar, and a window raised above everything (ADR 0018) would
  -- sit on it until it was dragged away. Through M.ui and returning a table,
  -- because the seam hands back one value and the screen is two.
  local screen = M.ui(function()
    local w, h = M.gui.screen_size()
    if w == nil then
      return nil
    end
    return { w = w, h = h }
  end)
  if screen then
    M.ui_method(root, "setBounds",
      floor((screen.w - width) / 2), floor((screen.h - height) / 2),
      width, height)
  end

  if M.ui_failed or root == nil then
    return false
  end

  -- Per instance, not on the class: the class is shared with every other window
  -- in the process, and this one is the only one that must not close.
  --
  -- It refuses only while the window is alive. Once the latch is set there is
  -- nothing left to show and nothing left updating it, so refusing would trap
  -- dead chrome carrying a stale line on somebody's screen -- and going through
  -- the seam is what makes that true, because a latched ui_method does nothing
  -- and the native hide stands.
  root.onClose = function(self)
    M.ui_method(self, "setVisible", true)
  end

  -- Per instance for the same reason, and the same shape: the widget library
  -- fires onChange on the widget itself, so a press is a field on the object
  -- rather than a callback registered somewhere.
  buttons.start.onChange = on_press(start_pressed)
  buttons.stop.onChange = on_press(stop_pressed)
  controls.crop_pick.onChange = on_press(pick_pressed)
  controls.crop.onChange = on_press(crop_toggled)

  -- Once, for the life of the session. The handler does nothing until the pick
  -- button arms it, so the cost of every other click in DCS is one comparison.
  M.ui(M.gui.on_mouse_down, function(x, y)
    M.on_map_click(x, y)
  end)

  M.ui_method(root, "setVisible", true)

  -- Asked again, because the check above covers the layout and nothing since:
  -- the callbacks, the mouse hook and the show itself can all latch. A window
  -- that failed on any of them is still hidden by the setVisible(false) it was
  -- laid out under, and calling it built would leave it invisible for the rest
  -- of the session with this function short-circuiting on every later frame.
  --
  -- Returning false costs nothing to retry: the next frame's first lookup goes
  -- through a latched M.ui, answers nil, and marks the library unavailable, so
  -- no second window is ever made.
  if M.ui_failed then
    return false
  end

  M.window.root, M.window.panel = root, panel
  M.window.bar, M.window.message = bar, message
  M.window.controls, M.window.crop_labels = controls, crop_labels
  M.window.below_crop, M.window.crop_block_h = below_crop, crop_block_h
  M.window.below_bar, M.window.bar_block_h = below_bar, bar_block_h
  -- Both blocks were placed and are on screen, and the first relayout goes
  -- from that rather than from a nil that would read as hidden.
  M.window.crop_shown, M.window.bar_shown = true, true
  M.window.frame = { w = width, h = height }
  M.window.client = { w = (view and view.w) or WIN.w, h = (view and view.h) or content }
  M.window.buttons = buttons
  M.window.built = true
  M.log("window built")
  return true
end

-- What the status line says: a pure function of the run and of whether there is
-- a terrain under it, so every line the window can show is reachable from a test
-- with no widget in the process.
--
-- The terrain question comes before the state because most of what a user needs
-- telling is that nothing will happen until a map is open: the hook loads at the
-- main menu, and DCS can sit there for hours. Done is the exception and is
-- tested first -- a finished run has something to report, and sending somebody
-- to the Mission Editor once the work is over is advice pointing at nothing.
M.STATUS_NO_TERRAIN = "Open a map in the Mission Editor."

local STATUS_OF = {
  [M.STATE_IDLE] = "Waiting for a map.",
  [M.STATE_PREPARE] = "Preparing.",
  [M.STATE_HOOK] = "Sweeping the terrain.",
  [M.STATE_MISSION] = "Sweeping the scenery.",
  [M.STATE_DONE] = "Finished.",
}

-- Nil for a stopped run: there is nothing to say about one. The line is then
-- left with what put the run there -- the instruction at load, or the Stop
-- press's own words -- rather than replacing either with "Stopped.", which a
-- map opening under a stopped run would otherwise write over the instruction.
function M.window_status(run)
  if run.state == M.STATE_DONE then
    return STATUS_OF[M.STATE_DONE]
  end
  if run.state == M.STATE_STOPPED then
    return nil
  end
  if M.terrain_id() == nil then
    return M.STATUS_NO_TERRAIN
  end
  -- The state's own name for a state with no line of its own. A state added
  -- later without one would otherwise reach setText as a nil and take the whole
  -- window down with it, which is a steep price for a missing sentence.
  return STATUS_OF[run.state] or tostring(run.state)
end

-- Where the bar stands, as a percentage.
--
-- By phase, because that is all anything here can know yet: a sweep cannot say
-- how much of its own work is done, so the bar moves at a phase change and
-- stands still in between. Prepare is left at zero rather than given a slice of
-- its own -- it is a handful of frames against tens of minutes, and a bar that
-- jumped before any terrain had been read would be describing nothing.
--
-- The two passes are given equal halves, which is wrong and is the honest kind
-- of wrong: the mission pass is not half the work, and nothing has measured
-- what it is. A fraction that knows costs a per-sweep measurement.
local PROGRESS_OF = {
  [M.STATE_STOPPED] = 0,
  [M.STATE_IDLE] = 0,
  [M.STATE_PREPARE] = 0,
  [M.STATE_HOOK] = 0,
  [M.STATE_MISSION] = 50,
  [M.STATE_DONE] = 100,
}

function M.window_progress(state)
  return PROGRESS_OF[state] or 0
end

-- Written only when it changed. The frame callback arrives about sixty times a
-- second and the line changes a handful of times in a run, so setting it every
-- frame is a relayout a frame for a string nobody could see change.
local function update_status(run)
  local text = M.window_status(run)
  if text == M.window.status_text then
    return
  end
  -- The phase the window has seen, which is not the text the line shows: a
  -- press can say something over a phase that has not changed, and a phase
  -- that has not changed must not say itself again over it -- at done, that
  -- wrote "Finished." over every refusal one frame after it was shown. Nil is
  -- seen too, so a Stop and a Start back into the same phase say it again.
  M.window.status_text = text
  if text ~= nil then
    say(text)
  end
end

-- What the line says before any press has had anything to say: the one thing
-- to do next. Without a map that is opening one, because nothing can run
-- until it is; then a fresh install has no directory, and that is the thing;
-- one with a config has only to be told where the button is.
M.INSTRUCTION_MAP = "Open a map in the Mission Editor."
M.INSTRUCTION_DIRECTORY = "Set an output directory, then press Start."
M.INSTRUCTION_START = "Press Start to begin."

function M.instruction(config, has_terrain)
  if not has_terrain then
    return M.INSTRUCTION_MAP
  end
  local dir = type(config) == "table" and config.output_dir
  if type(dir) ~= "string" or dir == "" then
    return M.INSTRUCTION_DIRECTORY
  end
  return M.INSTRUCTION_START
end

-- An instruction on the line, marked as one: an instruction is kept current
-- while the run is stopped and nothing else has been said -- a map opening
-- moves it on -- where anything else said stays until the next thing is.
local function instruct(text)
  say(text)
  M.window.instructing = true
  M.window.instruction_text = text
end

local function update_instruction(run)
  if run.state ~= M.STATE_STOPPED or not M.window.instructing then
    return
  end
  local text = M.instruction(run.config, M.terrain_id() ~= nil)
  if text ~= M.window.instruction_text then
    instruct(text)
  end
end

-- Puts a run's config on screen, once.
--
-- Once, because from then on the boxes are the user's. The frame callback
-- arrives about sixty times a second, and a refill per frame would take a
-- keystroke back out of the box before the next one could be typed. Start
-- writes them again, which is a different thing: that is the user asking.
local function fill_controls(run)
  if M.window.filled then
    return
  end
  set_controls(run.config)
  show_crop(crop_ticked())
  instruct(M.instruction(run.config, M.terrain_id() ~= nil))
  -- What was wrong with the config file, put where the user can act on it. This
  -- is the only moment those problems can be shown -- they were found before
  -- there was a window -- and until now the only record of them was a log
  -- nobody with a blank-looking crop would think to open.
  --
  -- All but one: a missing directory is what a fresh install has, and the
  -- instruction just written already says to set one. The checker's line for
  -- it would say the same thing as an error, so that problem is left to the
  -- instruction here, and the next one, if any, is shown. Start shows it
  -- like any other, because by then the user has asked.
  --
  -- A copy, because a drive problem is appended below and the run's own list
  -- is the record of what the file held.
  local problems = {}
  local from = run.config_problems or {}
  local first = 1
  if from[1] == M.field_problem("output_dir", nil) then
    first = 2
  end
  for i = first, #from do
    problems[#problems + 1] = from[i]
  end
  -- And one the checker cannot find, because it has no disk: a drive in the
  -- file that is not there.
  local missing = M.drive_problem(run.config.output_dir)
  if missing then
    problems[#problems + 1] = missing
  end
  show_problems(problems)
  M.window.filled = true
end

-- Same rule as the line above it, for the same reason.
local function update_progress(run)
  local value = M.window_progress(run.state)
  if value == M.window.bar_value then
    return
  end
  M.ui_method(M.window.bar, "setValue", value)
  M.window.bar_value = value
  -- Nil before the first frame, so the first frame settles it either way.
  local shown = value > 0
  if shown ~= M.window.bar_shown then
    show_bar(shown)
  end
end

-- Why the run stopped itself, where it did. The run cannot reach the line, so
-- it leaves the reason on itself and the window shows it once, when it
-- appears; Start clears it, and the line moves on with the next press.
local function update_refusal(run)
  if run.refusal == M.window.refusal_shown then
    return
  end
  if run.refusal ~= nil then
    say(M.problem_for_screen(run.refusal))
  end
  M.window.refusal_shown = run.refusal
end

-- Which controls answer, by whether the run is working. Written only when that
-- changed, for the same reason as the lines above it.
--
-- Working is everything between Start and Stop, the wait for a theatre
-- included: Start refuses while a run is going and retarget refuses to move
-- one, so a box that could be typed into then would take a value the run is
-- not going to use. Greying it says so before the typing rather than after.
--
-- The two buttons share one slot, and the one shown is the one that would not
-- refuse: Start while the run is stopped or done, Stop while it is going.
local function update_run_controls(run)
  local working = run.state ~= M.STATE_STOPPED and run.state ~= M.STATE_DONE
  if working == M.window.working then
    return
  end
  local controls = M.window.controls
  for i = 1, #CONTROL_ORDER do
    M.ui_method(controls[CONTROL_ORDER[i]], "setEnabled", not working)
  end
  M.ui_method(controls.crop_pick, "setEnabled", not working)
  M.ui_method(M.window.buttons.start, "setVisible", not working)
  M.ui_method(M.window.buttons.stop, "setVisible", working)
  M.window.working = working
end

-- Points on_frame at the window: build it, then say where the run has got to.
--
-- Build first because the window is built on a frame rather than at load, and
-- this is the frame. Nothing is written when it did not build, so a DCS whose
-- widget library is not there ticks a no-op rather than indexing a nil label.
function M.attach_window()
  M.on_frame = function(run)
    -- Where a press finds the run. This is the only caller of build_window, so
    -- no button can exist before a frame has carried one, and taking it from
    -- here rather than closing over it leaves one place the run comes from
    -- instead of two that can disagree.
    M.window.run = run
    if M.build_window() then
      fill_controls(run)
      update_instruction(run)
      update_status(run)
      update_progress(run)
      update_refusal(run)
      update_run_controls(run)
    end
  end
end

--------------------------------------------------------------------------------
-- Bootstrap
--
-- What DCS gets by loading this file from Scripts/Hooks/. Everything above is a
-- module; this is what turns one into a hook.
--
-- Silence is the default, and is most of the behaviour. A user who has not
-- written a config has not asked for anything, so an installed hook that is not
-- enabled writes no log, builds no window and registers no callback: it costs
-- one file read at start and nothing afterwards.
--------------------------------------------------------------------------------

-- Returns the registered run, or false and the reason there is nothing to do.
function M.bootstrap()
  local dir = M.saved_games_dir()
  if not dir then
    return false, "no Saved Games directory"
  end
  local path = M.join(dir, M.CONFIG_NAME)

  -- The bytes are asked for first because "no config file" and "a config file
  -- that will not load" come back from read_config looking the same and are not
  -- the same thing: the first is a fresh install and says nothing, the second is
  -- a user who tried and is owed the reason. It costs one extra read of a few
  -- hundred bytes, once, at load.
  if not M.read_file(path) then
    return false, "no config file"
  end

  local config, err = M.read_config(path)
  if not config then
    -- To dcs.log alone. The progress log has no path yet and cannot be given
    -- one, because whether the user enabled anything is exactly what could not
    -- be read.
    M.warn(err)
    return false, err
  end

  local settings, problems, tags = M.validate_config(config)
  if settings.enabled then
    M.log_path = M.join(dir, M.LOG_NAME)
    M.log("hook loaded")
  end
  -- Reported after the path is set, so an enabled run's problems reach both
  -- destinations. A disabled run's reach dcs.log alone, and that is the point of
  -- reporting them at all: `enabled = "true"` is a quoted boolean, and the hook
  -- doing nothing whatever about it would be silence with no window and no clue.
  for i = 1, #problems do
    M.warn(problems[i])
  end
  if not settings.enabled then
    return false, "not enabled"
  end

  -- The problems ride the run rather than being dropped here. warn has already
  -- put them in both logs; the window puts them under the control that caused
  -- them, which is the only form a user can act on without going to find a file.
  local run = M.new_run({ config = settings, problems = problems, tags = tags })
  M.attach_window()
  local ok, why = M.register(run)
  if not ok then
    M.warn("no callbacks registered: " .. tostring(why))
    return false, why
  end
  return run
end

-- The one top-level side effect in this file, and the only thing DCS causes by
-- loading it.
--
-- Under pcall because this runs inside DCS's own load of Scripts/Hooks, and what
-- a raise there costs the hooks loaded after this one has never been measured.
-- The honest options are to measure it or to not raise, and not raising is free.
--
-- Every DCS global it needs is reached through a seam that answers nil without
-- one, so a plain interpreter loading this file for the offline tests gets false
-- and no side effect at all.
local loaded, result = pcall(M.bootstrap)
if not loaded then
  M.dcs_log("WARNING", "hook did not load: " .. tostring(result))
end
M.run = loaded and result or false

return M
