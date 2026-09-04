-- Offline tests for the file layer.
--
-- Run from the repository root with a plain lua5.1.
--
-- The fake file system below is the reason the rest of X3 can be tested at
-- all: a manifest write, a journal append and a resume are all file access,
-- and none of them should need a disk to assert.

package.path = "extractor/?.lua;extractor/test/support/?.lua;" .. package.path

local T = require("testing")
local E = require("DcsTerrainExtract")

-- A file system in a table: paths are keys, file contents are strings, and a
-- directory is the DIR sentinel.
--
-- rename refuses an existing destination. Windows does; Linux replaces
-- silently. CI runs on Linux, so a fake that replaced silently would let the
-- remove step in write_file be deleted and stay green on every machine that
-- runs these tests. Modelling the strictest platform is what keeps that step
-- load-bearing.
local DIR = {}

local function new_fs()
  local files = {}
  local fs = { files = files, DIR = DIR }

  function fs.open(path, mode)
    if mode == "rb" then
      local data = files[path]
      if data == nil or data == DIR then
        return nil, path .. ": no such file"
      end
      local done = false
      return {
        read = function()
          if done then
            return nil
          end
          done = true
          return data
        end,
        close = function() return true end,
      }
    end
    if files[path] == DIR then
      return nil, path .. ": is a directory"
    end
    if mode == "wb" then
      files[path] = ""
    elseif mode == "ab" then
      files[path] = files[path] or ""
    else
      return nil, "unsupported mode " .. tostring(mode)
    end
    return {
      write = function(self, s)
        files[path] = files[path] .. s
        return self
      end,
      close = function() return true end,
    }
  end

  function fs.remove(path)
    if files[path] == nil then
      return nil, path .. ": no such file"
    end
    -- os.remove will not delete a directory out from under a rename, so
    -- neither does this. write_file ignores the result and lets the rename
    -- report, which is the path that has to work.
    if files[path] == DIR then
      return nil, path .. ": is a directory"
    end
    files[path] = nil
    return true
  end

  function fs.rename(from, to)
    if files[from] == nil then
      return nil, from .. ": no such file"
    end
    if files[to] ~= nil then
      return nil, to .. ": destination exists"
    end
    files[to] = files[from]
    files[from] = nil
    return true
  end

  function fs.mkdir(path)
    if files[path] ~= nil then
      return nil, path .. ": exists"
    end
    files[path] = DIR
    return true
  end

  function fs.is_dir(path)
    return files[path] == DIR
  end

  return fs
end

--------------------------------------------------------------------------------
T.group("the fake is strict")
--------------------------------------------------------------------------------

-- Asserted first, because every claim below about write_file rests on it.
local strict = new_fs()
strict.files["a"] = "1"
strict.files["b"] = "2"
T.eq("rename onto an existing name is refused", strict.rename("a", "b"), nil)
T.eq("and the source is untouched", strict.files["a"], "1")

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

T.done()
