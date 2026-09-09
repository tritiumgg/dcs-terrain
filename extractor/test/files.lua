-- Offline tests for the file layer.
--
-- Run from the repository root with a plain lua5.1.
--
-- The fake file system in support/fakefs.lua is the reason the rest of X3 can
-- be tested at all: a manifest write, a journal append and a resume are all
-- file access, and none of them should need a disk to assert.

package.path = "extractor/?.lua;extractor/test/support/?.lua;" .. package.path

local T = require("testing")
local E = require("DcsTerrainExtract")

-- The fake file system lives in support/ because the journal and resume tests
-- drive the same code paths over it. Its strictness -- a rename that refuses
-- an existing destination, a remove that refuses a directory -- is what most
-- of the assertions below actually rest on.
local FakeFs = require("fakefs")
local new_fs = FakeFs.new

--------------------------------------------------------------------------------
T.group("the real seams answer nil with no lfs around them")
--------------------------------------------------------------------------------

-- Before the fake replaces them. These run in a plain interpreter, where the
-- lfs global is absent, and a hook that raised here would take the GameGUI
-- state down on a machine where the module has moved.
local nodir, nodir_err = E.fs.dir("anywhere")
T.eq("no directory listing", nodir, nil)
T.eq("with a message", type(nodir_err), "string")
T.eq("no current directory", E.fs.currentdir(), nil)
T.eq("no modification time", E.fs.modified("anywhere"), nil)

--------------------------------------------------------------------------------
T.group("the fake is strict")
--------------------------------------------------------------------------------

-- Asserted first, because every claim below about write_file rests on it.
local strict = new_fs()
strict.files["a"] = "1"
strict.files["b"] = "2"
T.eq("rename onto an existing name is refused", strict.rename("a", "b"), nil)
T.eq("and the source is untouched", strict.files["a"], "1")

-- And its handles report nothing, which is what DCS's own do. A fake that
-- returned true here would agree with the assumption that made every live write
-- fail while the tests stayed green (ADR 0016).
local handle = strict.open("h", "wb")
T.eq("write returns nothing", select("#", handle:write("x")), 0)
T.eq("close returns nothing", select("#", handle:close()), 0)
T.eq("but the bytes are there", strict.size("h"), 1)
T.eq("and an absent file has no size", strict.size("nowhere"), nil)

--------------------------------------------------------------------------------
T.group("join")
--------------------------------------------------------------------------------

T.eq("two parts", E.join("a", "b"), "a/b")
T.eq("does not double a slash", E.join("a/", "b"), "a/b")
T.eq("empty directory", E.join("", "b"), "b")
T.eq("no directory", E.join(nil, "b"), "b")

--------------------------------------------------------------------------------
T.group("write, read, append")
--------------------------------------------------------------------------------

local fs = new_fs()
E.fs = fs

T.eq("write reports success", E.write_file("out.json", "hello"), true)
T.eq("and reads back", E.read_file("out.json"), "hello")
T.eq("leaving no tmp behind", fs.files["out.json.tmp"], nil)

-- The case Windows refuses: the destination already exists. Without the remove
-- before the rename this fails and the tmp is orphaned.
T.eq("replaces an existing file", E.write_file("out.json", "second"), true)
T.eq("with the new content", E.read_file("out.json"), "second")
T.eq("still no tmp", fs.files["out.json.tmp"], nil)

-- Binary, because a tile is raw samples: a newline byte is a height, not a
-- line ending.
local raw = "\000\001\010\013\255\128"
T.eq("writes bytes", E.write_file("tile.bin", raw), true)
T.eq("and reads the same bytes", E.read_file("tile.bin"), raw)

T.eq("first append", E.append_file("tiles.jsonl", "one\n"), true)
T.eq("second append", E.append_file("tiles.jsonl", "two\n"), true)
T.eq("appends rather than truncates", E.read_file("tiles.jsonl"), "one\ntwo\n")
T.eq("append creates the file", E.read_file("tiles.jsonl") ~= nil, true)

local missing, err = E.read_file("absent.json")
T.eq("a missing file returns nil", missing, nil)
T.eq("with a message", type(err), "string")

-- A directory where the file should go. The tmp writes fine, the remove
-- cannot delete a directory, and the rename is what reports.
fs.mkdir("adir")
T.eq("a write onto a directory fails", E.write_file("adir", "x"), nil)
T.eq("and the tmp is left as evidence", fs.files["adir.tmp"], "x")
T.eq("an append onto a directory fails", E.append_file("adir", "x"), nil)

--------------------------------------------------------------------------------
T.group("a write is judged by what landed")
--------------------------------------------------------------------------------

-- The failure the size check exists for, and the one no return value can report
-- here: a write that loses bytes. On a real machine that is a full disk; from
-- inside the process it is silence and a short file.
local short = new_fs()
E.fs = short
short.lose_bytes(1)

local shorted, why = E.write_file("out.json", "hello")
T.eq("a short write fails", shorted, nil)
T.eq("saying how much landed", why:find("4 bytes of 5", 1, true) ~= nil, true)
T.eq("the name is not taken", short.files["out.json"], nil)
T.eq("and the tmp is left as evidence", short.files["out.json.tmp"], "hell")

local appended, awhy = E.append_file("tiles.jsonl", "one\n")
T.eq("a short append fails", appended, nil)
T.eq("saying what it expected", awhy:find("expected 4", 1, true) ~= nil, true)

-- The append check is against the growth, not against the whole file, so a good
-- append onto a file a bad one truncated still succeeds.
short.lose_bytes(0)
T.eq("a later append succeeds", E.append_file("tiles.jsonl", "two\n"), true)
T.eq("adding to what was there", E.read_file("tiles.jsonl"), "onetwo\n")

--------------------------------------------------------------------------------
T.group("a head is one counted read of the first bytes")
--------------------------------------------------------------------------------

local heads = new_fs()
E.fs = heads
heads.files["big.bin"] = string.rep("x", 100) .. "tail"

-- The fake's handle honors the count, as a real one does. What the hook reads
-- from a six gigabyte file is sixteen bytes, and a fake that answered every
-- read with the whole file would let a read of "*a" pass for that.
local h = heads.open("big.bin", "rb")
T.eq("a counted read returns that many", #h:read(16), 16)
T.eq("and the next continues after them", h:read(4), "xxxx")
T.eq("the rest is the rest", #h:read("*a"), 84)
T.eq("a counted read at the end is nil", h:read(1), nil)
T.eq("the rest at the end is empty", h:read("*a"), "")

T.eq("read_head takes the first bytes", E.read_head("big.bin", 16), string.rep("x", 16))
T.eq("a short file gives what there is", E.read_head("big.bin", 1000), heads.files["big.bin"])
heads.files["empty.bin"] = ""
T.eq("an empty file gives an empty head", E.read_head("empty.bin", 16), "")
local nohead, herr = E.read_head("absent.bin", 16)
T.eq("an absent file is nil", nohead, nil)
T.eq("with a message", type(herr), "string")
T.eq("read_file still reads whole", E.read_file("big.bin"), heads.files["big.bin"])

--------------------------------------------------------------------------------
T.group("a directory lists its children")
--------------------------------------------------------------------------------

local tree = new_fs()
E.fs = tree
tree.mkdir("C:/DCS")
tree.mkdir("C:/DCS/Mods")
tree.mkdir("C:/DCS/Mods/terrains")
tree.mkdir("C:/DCS/Mods/terrains/Sinai")
tree.files["C:/DCS/Mods/terrains/Sinai/entry.lua"] = "x"
tree.files["C:/DCS/Mods/terrains/Sinai/surface/SinaiMap.surface5"] = "x"
tree.mkdir("C:/DCS/Mods/terrains/Caucasus")
tree.files["C:/DCS/autoupdate.cfg"] = "{}"

T.eq("children, sorted, files and directories alike",
  table.concat(tree.dir("C:/DCS/Mods/terrains"), " "), "Caucasus Sinai")
T.eq("one level only",
  table.concat(tree.dir("C:/DCS/Mods/terrains/Sinai"), " "), "entry.lua surface")
T.eq("a trailing slash is the same directory",
  table.concat(tree.dir("C:/DCS/"), " "), "Mods autoupdate.cfg")
T.eq("an empty directory is an empty list", #tree.dir("C:/DCS/Mods/terrains/Caucasus"), 0)
local nowhere, derr = tree.dir("C:/DCS/Mods/nowhere")
T.eq("not a directory is nil", nowhere, nil)
T.eq("with a message", type(derr), "string")

T.eq("no current directory until the test sets one", tree.currentdir(), nil)
tree.cwd = "C:\\DCS"
T.eq("then the one it set", tree.currentdir(), "C:\\DCS")

T.eq("a file with no time set reads as zero", tree.modified("C:/DCS/autoupdate.cfg"), 0)
tree.mtimes["C:/DCS/autoupdate.cfg"] = 1756800000
T.eq("a set time is reported", tree.modified("C:/DCS/autoupdate.cfg"), 1756800000)
T.eq("a directory has no time", tree.modified("C:/DCS/Mods"), nil)
T.eq("nor does an absent file", tree.modified("C:/DCS/nothing"), nil)

--------------------------------------------------------------------------------
T.group("mkdir_p")
--------------------------------------------------------------------------------

local dirs = new_fs()
E.fs = dirs

T.eq("absolute windows path", E.mkdir_p("C:/extracts/caucasus/tiles"), true)
T.eq("the drive is not created", dirs.files["C:"], nil)
T.eq("first level", dirs.is_dir("C:/extracts"), true)
T.eq("second level", dirs.is_dir("C:/extracts/caucasus"), true)
T.eq("leaf", dirs.is_dir("C:/extracts/caucasus/tiles"), true)
T.eq("running it again succeeds", E.mkdir_p("C:/extracts/caucasus/tiles"), true)

T.eq("absolute posix path", E.mkdir_p("/tmp/x/y"), true)
T.eq("the root is not created", dirs.files["/"], nil)
T.eq("posix first level", dirs.is_dir("/tmp"), true)
T.eq("posix leaf", dirs.is_dir("/tmp/x/y"), true)

T.eq("relative path", E.mkdir_p("a/b"), true)
T.eq("relative leaf", dirs.is_dir("a/b"), true)

-- A component that exists as a file, not a directory, cannot be stepped past.
dirs.files["blocker"] = "not a directory"
local failed, mkerr = E.mkdir_p("blocker/below")
T.eq("a file in the way fails", failed, nil)
T.eq("naming the component", mkerr:find("blocker", 1, true) ~= nil, true)

T.raises("refuses an empty path", function() return E.mkdir_p("") end, "not a path")
T.raises("refuses a non-string", function() return E.mkdir_p(7) end, "not a path")

--------------------------------------------------------------------------------
T.group("the output tree")
--------------------------------------------------------------------------------

local out = new_fs()
E.fs = out

T.eq("creates the tree", E.ensure_output_dirs("C:/extracts/caucasus"), true)
T.eq("the extract directory", out.is_dir("C:/extracts/caucasus"), true)
T.eq("the tiles directory", out.is_dir("C:/extracts/caucasus/tiles"), true)
T.eq("height", out.is_dir("C:/extracts/caucasus/tiles/height"), true)
T.eq("water", out.is_dir("C:/extracts/caucasus/tiles/water"), true)
-- The mission-pass layer too, so the mission pass has nowhere left to fail
-- before its first write.
T.eq("surface", out.is_dir("C:/extracts/caucasus/tiles/surface"), true)
T.eq("running it again succeeds", E.ensure_output_dirs("C:/extracts/caucasus"), true)

out.files["C:/blocked"] = "not a directory"
T.eq("a blocked output directory reports", E.ensure_output_dirs("C:/blocked/x"), nil)

-- A tile file with no journal line fails validation permanently, and a sweep
-- that decides to omit a tile is the case where nothing else would remove it.
out.files["C:/extracts/caucasus/tiles/height/4_9.bin"] = "stale"
T.eq("removes a tile", E.remove_tile("C:/extracts/caucasus", "height", 4, 9), true)
T.eq("and it is gone", out.files["C:/extracts/caucasus/tiles/height/4_9.bin"], nil)
-- Called unconditionally by a sweep that omits a tile, so absence is normal.
T.eq("removing an absent tile is fine",
  E.remove_tile("C:/extracts/caucasus", "height", 4, 9), true)
T.raises("but not an unknown layer",
  function() return E.remove_tile("C:/extracts/caucasus", "depth", 0, 0) end, "not a layer")

T.done()
