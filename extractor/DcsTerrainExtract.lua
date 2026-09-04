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
-- east with DCS z, and every sample is taken at a cell centre.
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
-- The lattice takes metres. The theatre's own bounds are kilometres, so the
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

function M.presweep_centre(lattice, row, col)
  return lattice.min_x + (row + 0.5) * lattice.cell_m,
         lattice.min_z + (col + 0.5) * lattice.cell_m
end

-- The bounding rectangle of the authored cells, grown by margin_m.
--
-- It bounds the cells' squares and not their centres, because a cell is
-- authored as a whole: bounding the centres would lose half a cell at each
-- edge, and a lattice cell is kilometres wide.
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

function M.cell_centre(grid, row, col)
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
  local written, werr = f:write(data)
  -- Checked separately from the write: a buffered write that fills the disk
  -- fails at the flush, which is here.
  local closed, cerr = f:close()
  if not written then
    return nil, werr or (tmp .. ": write failed")
  end
  if not closed then
    return nil, cerr or (tmp .. ": close failed")
  end
  M.fs.remove(path)
  local renamed, rerr = M.fs.rename(tmp, path)
  if not renamed then
    return nil, rerr or (path .. ": rename failed")
  end
  return true
end

function M.append_file(path, data)
  local f, err = M.fs.open(path, "ab")
  if not f then
    return nil, err or (path .. ": cannot open")
  end
  local written, werr = f:write(data)
  local closed, cerr = f:close()
  if not written then
    return nil, werr or (path .. ": write failed")
  end
  if not closed then
    return nil, cerr or (path .. ": close failed")
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
-- while work remains and M.DONE when the sweep is finished; anything else
-- raises, because a step that returned nil by accident would otherwise read as
-- "not finished" and the sweep would never end.
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

local function bad_boolean(v, name)
  if type(v) ~= "boolean" then
    return format("%s is not true or false: %s", name, tostring(v))
  end
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
local CONFIG_FIELDS = {
  { name = "output_dir", check = bad_path, normalise = forward_slashes },
  { name = "omit_sea_tiles", check = bad_boolean, default = true },
}

local CONFIG_FIELD_BY_NAME = { enabled = true }
for i = 1, #CONFIG_FIELDS do
  CONFIG_FIELD_BY_NAME[CONFIG_FIELDS[i].name] = true
end

-- Returns the config to run with and one line per problem.
--
-- The table comes back whatever went wrong, because the window that will own
-- the config file needs it to fill its controls and the run needs it to start.
-- Two code paths for those would be two chances to disagree about what a
-- defaulted field holds.
--
-- Unknown keys are sorted before they are reported: pairs order is undefined,
-- and a problem list whose order changes between runs is a log nobody can diff.
function M.validate_config(config)
  local problems = {}
  local out = {}

  if type(config) ~= "table" then
    return { enabled = false },
      { format("config is not a table: %s", type(config)) }
  end

  -- enabled short-circuits. Not true means the hook does nothing at all, so
  -- there is nothing to validate and nobody to tell. An absent enabled is a
  -- disabled hook and not a problem, because an absent config file means the
  -- same thing and that is not an error either.
  if config.enabled ~= nil then
    local bad = bad_boolean(config.enabled, "enabled")
    if bad then
      problems[#problems + 1] = bad
    end
  end
  if config.enabled ~= true then
    return { enabled = false }, problems
  end
  out.enabled = true

  for i = 1, #CONFIG_FIELDS do
    local field = CONFIG_FIELDS[i]
    local value = config[field.name]
    if value == nil then
      if not field.optional then
        if field.default == nil then
          problems[#problems + 1] =
            format("%s is not set, and there is no default for it", field.name)
        else
          out[field.name] = field.default
        end
      end
    else
      local bad = field.check(value, field.name)
      if bad then
        problems[#problems + 1] = bad
        out[field.name] = field.default
      elseif field.normalise then
        out[field.name] = field.normalise(value)
      else
        out[field.name] = value
      end
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
    problems[#problems + 1] = format("%s is not a config field", unknown[i])
  end

  return out, problems
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

M.STATE_IDLE = "idle"
M.STATE_PREPARE = "prepare"
M.STATE_HOOK = "hook"
M.STATE_MISSION = "mission"
M.STATE_DONE = "done"

M.DEFAULT_FRAME_BUDGET_MS = 5

-- Frames between terrain polls in idle. The poll is two DCS calls and idle
-- lasts for as long as DCS sits at the main menu, which can be hours.
M.IDLE_POLL_FRAMES = 60

-- Only two of the five states are passes the manifest records.
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

-- Seam, replaced by the progress log. A run that logs nowhere still runs,
-- which is what the offline tests want.
function M.log(message) end

function M.new_run(opts)
  opts = opts or {}
  local config = opts.config or {}
  local run = {
    state = M.STATE_IDLE,
    config = config,
    jobs = opts.jobs or M.jobs,
    dir = config.output_dir,
    budget_ms = config.frame_budget_ms or M.DEFAULT_FRAME_BUDGET_MS,
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
    M.log("manifest write failed: " .. tostring(err))
  end
  return ok and true or false
end

-- Overridden once prepare has a job the machine owns. Until then the phase is
-- whatever the sweeps registered.
function M.prepare_jobs(run)
  return run.jobs.prepare or {}
end

-- Moves the run into a state, and is the only place that does. A phase change
-- builds the queue for the phase it enters, stamps the pass, logs, and saves
-- the manifest, which with the per-sweep save is the whole of "the manifest is
-- rewritten at the end of each sweep and at every phase change".
function M.enter(run, state)
  run.state = state
  run.phase_frames = 0
  run.queue = nil

  local pass = PASS_OF[state]
  if pass then
    if run.manifest then
      run.manifest.passes[pass].started_at = M.now_iso()
    end
    -- A phase with nothing registered is legitimate: it is what every phase
    -- looks like before its sweeps are built.
    run.queue = M.new_queue(run.jobs[pass] or {})
  elseif state == M.STATE_PREPARE then
    run.queue = M.new_queue(M.prepare_jobs(run))
  end

  M.log("phase " .. state)
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
  M.log("terrain unloaded during the " .. run.state .. " pass")
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
  if status == M.MORE then
    return run.state
  end
  if pass then
    complete_pass(run, pass)
  end
  return M.enter(run, next_state(run))
end

-- One simulation frame. Returns the state the run is in after it, which is the
-- same state on every frame but the ones that change phase.
function M.run_frame(run)
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
    onSimulationFrame = function()
      M.run_frame(run)
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

return M
