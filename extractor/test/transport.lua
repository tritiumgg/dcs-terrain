-- Offline tests for the server-state transport: the literals a source is
-- built from, the frame around an answer, and what the hook makes of every
-- way net.dostring_in can answer.
--
-- Run from the repository root with a plain lua5.1.
--
-- The fake net.dostring_in compiles the source and runs it in a table of its
-- own, the way the mission state is a Lua state of its own: a chunk that
-- leaned on a global of this file would fail here as it would in DCS. What
-- the real call answers is checked live.

package.path = "extractor/?.lua;extractor/test/support/?.lua;" .. package.path

local T = require("testing")
local E = require("DcsTerrainExtract")

-- The standard library the mission state has, and nothing of this file.
local server = {
  string = string, table = table, math = math, pcall = pcall, error = error,
  tostring = tostring, type = type,
}
local calls = {}

local function run_in_server(state, source)
  calls[#calls + 1] = state
  local chunk, err = loadstring(source)
  if not chunk then
    return err, false
  end
  setfenv(chunk, server)
  local ok, answer = pcall(chunk)
  if not ok then
    return tostring(answer), false
  end
  return answer, true
end

net = { dostring_in = run_in_server }

T.group("literals")

T.eq("an integer", E.server_literal(64), "64")
T.eq("a negative coordinate reads back exactly",
  tonumber(E.server_literal(-418619.1875 + 0.1)), -418619.1875 + 0.1)
T.eq("a third reads back exactly", tonumber(E.server_literal(1 / 3)), 1 / 3)
T.eq("past 2^31", E.server_literal(4294967296), "4294967296")
T.eq("a string is quoted", E.server_literal('a "b"\n\\'), '"a \\"b\\"\\\n\\\\"')
T.eq("a boolean", E.server_literal(false), "false")
T.raises("nan", function() E.server_literal(0 / 0) end, "not a finite number")
T.raises("inf", function() E.server_literal(math.huge) end, "not a finite number")
T.raises("a table", function() E.server_literal({}) end, "cannot inject a table")
T.raises("nil", function() E.server_literal(nil) end, "cannot inject a nil")

T.eq("a source", E.server_source("return %s + %s, %s", 1.5, 2, "x"),
  'return 1.5 + 2, "x"')

-- A string that tries to leave its literal stays inside it.
local hostile = '"); leaked = true; ("'
local body = E.server_call(E.server_source('local s = %s return #s .. ":" .. s', hostile))
T.eq("a hostile string round-trips as data", body, hostile)
T.eq("and ran nothing", server.leaked, nil)

T.group("frames")

T.eq("an empty body", E.unframe("0:"), "")
T.eq("a body with a colon in it", E.unframe("3:a:b"), "a:b")
T.eq("a body with a NUL in it", E.unframe("3:a\0b"), "a\0b")
local got, why = E.unframe("5:abc")
T.eq("a short body is refused", got, nil)
T.eq("and says so", why, "declared 5 bytes and 3 arrived")
got, why = E.unframe("2:abc")
T.eq("a long body is refused", got, nil)
T.eq("and says so too", why, "declared 2 bytes and 3 arrived")
got, why = E.unframe("abc")
T.eq("no frame is refused", got, nil)
T.eq("naming what came", why, 'no declared length in "abc"')

T.group("calls")

calls = {}
T.eq("a framed answer", E.server_call('return "4:abcd"'), "abcd")
T.eq("in the server state", calls[1], "server")

got, why = E.server_call('error("boom", 0)')
T.eq("a raise is refused", got, nil)
T.eq("with the message", why, "the chunk raised: boom")

got, why = E.server_call("return (")
T.eq("a chunk that does not compile is refused", got, nil)
T.eq("as a raise", why:sub(1, 17), "the chunk raised:")

got, why = E.server_call("return 12")
T.eq("a number is refused", got, nil)
T.eq("by type", why, "net.dostring_in returned a number")

-- The payload a transport cut short: the frame is what catches it.
got, why = E.server_call('return ("16384:" .. string.rep("1", 16384)):sub(1, 4096)')
T.eq("a truncated payload is refused", got, nil)
T.eq("with both counts", why, "declared 16384 bytes and 4090 arrived")

net.dostring_in = function() return nil end
got, why = E.server_call("return 1")
T.eq("a nil answer is refused", got, nil)
T.eq("by type too", why, "net.dostring_in returned a nil")

-- Without the boolean an error message still fails the frame.
net.dostring_in = function() return '[string "x"]:1: boom' end
got, why = E.server_call("return 1")
T.eq("an error with no boolean is refused", got, nil)
T.eq("by its frame", why:sub(1, 20), "no declared length i")

net.dostring_in = function() error("no such state") end
got, why = E.server_call("return 1")
T.eq("a transport that raises is refused", got, nil)
T.eq("and says where", why:sub(1, 24), "net.dostring_in raised: ")

net = nil
got, why = E.server_call("return 1")
T.eq("no net", got, nil)
T.eq("is not an error", why, "net.dostring_in is not available")

T.done()
