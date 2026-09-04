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
-- ADR-0009: a crop run with no authored rectangle leaves both nil, and the
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
    bitmask = M.presweep_bitmask(lattice, authored),
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

return M
