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

return M
