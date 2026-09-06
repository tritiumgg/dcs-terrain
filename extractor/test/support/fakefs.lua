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
      local done = false
      return {
        read = function()
          if done then
            return nil
          end
          done = true
          return data
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

  return fs
end

return FakeFs
