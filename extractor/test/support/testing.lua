-- The whole framework an offline test gets.
--
-- Every .lua file directly under extractor/test/ is a test, run by a plain
-- lua5.1 with the repository root as the working directory, and its exit
-- status is the result. So a test requires this, calls the checks below, and
-- ends with done(), which is what turns a failure into a non-zero exit.
--
-- Nothing here knows about the hook. Fakes belong in the test that needs them.

local T = {}

local checks = 0
local failures = {}
local group = nil

-- Byte strings are most of what is compared here, and a tile sample or a
-- control-character escape is unreadable raw. Print them as escapes so a
-- failure says which byte moved.
local function show(v)
  local t = type(v)
  if t == "string" then
    local safe = v:gsub('[%z\1-\31\127-\255]', function(c)
      return string.format("\\x%02X", c:byte())
    end)
    return '"' .. safe .. '"'
  elseif t == "number" then
    -- Same 17 digits the encoder writes, so a failure on -0 or on the last
    -- digit of a double is visible rather than rounded away by tostring.
    return string.format("%.17g", v)
  end
  return tostring(v)
end

local function fail(name, message)
  failures[#failures + 1] = (group and (group .. ": ") or "") .. name .. "\n    " .. message
end

function T.group(name)
  group = name
end

-- Compares with ==, so two tables are equal only when they are the same table.
-- Everything here compares an encoded string or a number, which is the point:
-- the encoders are tested through their output, not through their internals.
function T.eq(name, got, want)
  checks = checks + 1
  if got ~= want then
    fail(name, "got  " .. show(got) .. "\n    want " .. show(want))
  end
end

-- Asserts that fn() raises, and that the message contains want as a plain
-- substring. The substring keeps the test off the exact wording while still
-- pinning which failure was raised.
function T.raises(name, fn, want)
  checks = checks + 1
  local ok, err = pcall(fn)
  if ok then
    fail(name, "returned " .. show(err) .. " instead of raising")
  elseif type(err) ~= "string" or not err:find(want, 1, true) then
    fail(name, "raised    " .. show(err) .. "\n    without  " .. show(want))
  end
end

function T.done()
  if #failures == 0 then
    print(string.format("ok  %d checks", checks))
    os.exit(0)
  end
  print(string.format("FAIL  %d of %d checks", #failures, checks))
  for i = 1, #failures do
    print("  " .. failures[i])
  end
  os.exit(1)
end

return T
