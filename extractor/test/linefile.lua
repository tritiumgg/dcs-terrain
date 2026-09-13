-- Offline tests for reading a line file back in chunks, copying its head into
-- a repaired file, and recovering a swap that was cut short.
--
-- Run from the repository root with a plain lua5.1.
--
-- The chunk is a few bytes here so that lines straddle chunks, which is the
-- case a whole-file read would never exercise.

package.path = "extractor/?.lua;extractor/test/support/?.lua;" .. package.path

local T = require("testing")
local E = require("DcsTerrainExtract")
local FakeFs = require("fakefs")

local fs = FakeFs.new()
E.fs = fs
local PATH = "C:/extract/roads.jsonl"

local function lines_of(reader)
  local got = {}
  local steps = 0
  local status, tail
  repeat
    status, tail = reader.step(function(line) got[#got + 1] = line end)
    steps = steps + 1
  until status ~= E.MORE
  return got, status, tail, steps
end

--------------------------------------------------------------------------------
T.group("lines come out whole and in order, however the chunks fall")
--------------------------------------------------------------------------------

fs.files[PATH] = "first\nsecond line\n\nfourth\n"
local reader = E.line_reader(PATH, 7)
T.eq("nothing read yet", reader.read, 0)
T.eq("the size is known", reader.size, 26)
local got, status, tail, steps = lines_of(reader)
T.eq("four lines", #got, 4)
T.eq("the first", got[1], "first")
T.eq("the second straddled a chunk", got[2], "second line")
T.eq("an empty line is a line", got[3], "")
T.eq("the fourth", got[4], "fourth")
T.eq("done", status, E.DONE)
T.eq("with no cut-short tail", tail, 0)
T.eq("every byte read", reader.read, 26)
T.eq("in four chunks and one more to see the end", steps, 5)
T.eq("done stays done", (reader.step(function() end)), E.DONE)

fs.files[PATH] = "whole\ncut sho"
got, status, tail = lines_of(E.line_reader(PATH, 4))
T.eq("the whole line", #got, 1)
T.eq("is the only one", got[1], "whole")
T.eq("and the tail is counted", tail, 7)

fs.files[PATH] = ""
got, status, tail = lines_of(E.line_reader(PATH, 4))
T.eq("an empty file has no lines", #got, 0)
T.eq("and no tail", tail, 0)

local none, err = E.line_reader("C:/extract/none.jsonl")
T.eq("no file is no reader", none, nil)
T.eq("with the reason", err, "C:/extract/none.jsonl: no such file")

fs.files[PATH] = "a\nb\n"
reader = E.line_reader(PATH, 2)
reader.close()
T.eq("a closed reader refuses", (reader.step(function() end)), nil)

--------------------------------------------------------------------------------
T.group("the head is copied in chunks and swapped in with the original aside")
--------------------------------------------------------------------------------

local function run_copy(step)
  local statuses = {}
  repeat
    local s, e = step()
    statuses[#statuses + 1] = s or ("nil: " .. tostring(e))
  until s ~= E.MORE
  return statuses
end

fs.files[PATH] = "a\nbb\ncut"
fs.files[PATH .. ".tmp"] = "stale"
local statuses = run_copy(E.copy_head(PATH, 5, 2))
T.eq("three chunks then the swap", table.concat(statuses, " "), "more more more done")
T.eq("the file holds the head", fs.files[PATH], "a\nbb\n")
T.eq("no copy is left", fs.files[PATH .. ".tmp"], nil)
T.eq("no aside is left", fs.files[PATH .. ".old"], nil)

fs.files[PATH] = "cut"
statuses = run_copy(E.copy_head(PATH, 0, 2))
T.eq("keeping nothing is one step", table.concat(statuses, " "), "done")
T.eq("and an empty file", fs.files[PATH], "")

fs.files[PATH] = "ab"
local step = E.copy_head(PATH, 5, 2)
T.eq("the first chunk lands", step(), E.MORE)
local s, e = step()
T.eq("a file shorter than asked is a failure", s, nil)
T.eq("saying how short", e, "C:/extract/roads.jsonl: ended after 2 of 5 bytes")
T.eq("and the original is untouched", fs.files[PATH], "ab")

fs.files[PATH] = "a\nb\n"
fs.files[PATH .. ".tmp"] = nil
fs.lose_bytes(1)
s, e = E.copy_head(PATH, 2, 2)()
fs.lose_bytes(0)
T.eq("a short write fails", s, nil)
T.eq("naming the copy", e:find("roads.jsonl.tmp", 1, true) ~= nil, true)
T.eq("and the original is untouched", fs.files[PATH], "a\nb\n")

fs.files[PATH] = "a\nb\n"
fs.files[PATH .. ".old"] = "older"
fs.files[PATH .. ".tmp"] = nil
statuses = run_copy(E.copy_head(PATH, 2, 2))
T.eq("an earlier aside does not block the swap", statuses[#statuses], E.DONE)
T.eq("the file is the head", fs.files[PATH], "a\n")
T.eq("and the aside is gone", fs.files[PATH .. ".old"], nil)

T.eq("no file is no copy", (E.copy_head("C:/extract/none.jsonl", 1)), nil)

--------------------------------------------------------------------------------
T.group("a swap cut short is put back on the next look")
--------------------------------------------------------------------------------

fs.files[PATH] = nil
fs.files[PATH .. ".old"] = "a\n"
T.eq("an aside with no file is the file", E.recover_swap(PATH), "the file renamed back from its aside")
T.eq("renamed back", fs.files[PATH], "a\n")
T.eq("and gone", fs.files[PATH .. ".old"], nil)

fs.files[PATH .. ".old"] = "older"
T.eq("an aside beside the file is the swap's last step", E.recover_swap(PATH), "an aside removed")
T.eq("the file stays", fs.files[PATH], "a\n")
T.eq("the aside goes", fs.files[PATH .. ".old"], nil)

fs.files[PATH .. ".tmp"] = "stale"
T.eq("a copy alone is stale", E.recover_swap(PATH), "a stale copy removed")
T.eq("and removed", fs.files[PATH .. ".tmp"], nil)

T.eq("nothing to do is nil", E.recover_swap(PATH), nil)
T.eq("with the file as it was", fs.files[PATH], "a\n")

T.done()
