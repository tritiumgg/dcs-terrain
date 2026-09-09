-- A file system in a table, for the offline tests.
--
-- Paths are keys, file contents are strings, and a directory is the DIR
-- sentinel. It is enough to drive the whole write-and-resume path -- a tile, a
-- journal append, a manifest rewrite -- without a disk.
--
-- Two places it is deliberately stricter than the host running the tests:
--
-- rename refuses an existing destination. Windows does; Linux replaces
-- silently. CI runs on Linux, so a fake that replaced silently would let the
-- remove step in write_file be deleted and stay green on every machine that
-- runs these tests.
--
-- remove refuses a directory, the way os.remove does, so a write onto a
-- directory reaches the rename and fails there rather than quietly deleting
-- the directory first.
--
-- Handles report nothing from write and close, which is what DCS's own io does
-- (ADR 0016). A fake that returned true would agree with the assumption that
-- caused the bug, so this one returns what the real thing returns and the code
-- has to check the size instead.

local DIR = {}

local FakeFs = { DIR = DIR }

function FakeFs.new()
  local files = {}
  local fs = { files = files, DIR = DIR }
  -- Bytes dropped from the end of every write, which is what a full disk looks
  -- like from inside the process: nothing is reported and only the size differs.
  local lose = 0

  function fs.open(path, mode)
    if mode == "rb" then
      local data = files[path]
      if data == nil or data == DIR then
        return nil, path .. ": no such file"
      end
      -- A position, so a counted read returns that many bytes and the next
      -- read continues after them, the way a real handle does. A fake that
      -- returned the whole file to every read would let a head read pass on
      -- a file it should have read sixteen bytes of.
      local pos = 0
      return {
        read = function(_, what)
          if type(what) == "number" then
            if pos >= #data then
              return nil
            end
            local chunk = data:sub(pos + 1, pos + what)
            pos = pos + #chunk
            return chunk
          end
          -- "*a", the only other form the hook uses: the rest, and an empty
          -- string at the end rather than nil, as real io answers.
          local rest = data:sub(pos + 1)
          pos = #data
          return rest
        end,
        close = function() end,
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
      write = function(_, s)
        if lose > 0 then s = s:sub(1, -1 - lose) end
        files[path] = files[path] .. s
      end,
      close = function() end,
    }
  end

  -- Every write from here on loses its last n bytes.
  function fs.lose_bytes(n) lose = n end

  function fs.size(path)
    local data = files[path]
    if data == nil or data == DIR then
      return nil
    end
    return #data
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

  -- The immediate children of a directory, sorted, read off the keys under
  -- it. A path with nothing under it and no DIR entry of its own is not a
  -- directory, which is how lfs.dir refuses one too.
  function fs.dir(path)
    local prefix = path
    if prefix:sub(-1) ~= "/" then
      prefix = prefix .. "/"
    end
    local seen, names = {}, {}
    for key in pairs(files) do
      if key:sub(1, #prefix) == prefix then
        local name = key:sub(#prefix + 1):match("^([^/]+)")
        if name and not seen[name] then
          seen[name] = true
          names[#names + 1] = name
        end
      end
    end
    if #names == 0 and files[path] ~= DIR then
      return nil, path .. ": not a directory"
    end
    table.sort(names)
    return names
  end

  -- What lfs.currentdir would answer; a test sets it. nil is no lfs at all,
  -- which is what a plain interpreter has.
  fs.cwd = nil
  function fs.currentdir()
    if fs.cwd == nil then
      return nil, "no current directory"
    end
    return fs.cwd
  end

  -- Modification times in seconds, set by the test per path. A file with no
  -- entry reads as written at zero rather than as absent, so a test sets only
  -- the times it asserts on.
  fs.mtimes = {}
  function fs.modified(path)
    local data = files[path]
    if data == nil or data == DIR then
      return nil
    end
    return fs.mtimes[path] or 0
  end

  return fs
end

return FakeFs
