-- A widget library in a table, for the offline tests.
--
-- The real one is DCS's, and there is no DCS here. This stands in for M.gui and
-- records what was asked of it, which is enough to assert that a window was
-- built, that it was built in the right order, and -- the reason it exists --
-- that a run finishes identically when every call into it fails.
--
-- Two failure modes, because they are not the same thing. fail_every() raises on
-- every call, which is a library that is there and broken. no_library() answers
-- nil, which is a plain interpreter with no dxgui in it at all: nothing raises
-- and nothing is built.

local FakeGui = {}

local CLASSES = { "Window", "Panel", "Static" }

-- Every method the window calls. One that is not in this list is a nil index
-- rather than a silent no-op, so a typo shows up as a failure here rather than
-- as a widget that quietly does nothing. The controls branch extends both lists.
local METHODS = {
  "setVisible", "getVisible", "setSkin", "setBounds", "setDraggable",
  "setResizable", "insertWidget", "setText", "getText", "close",
}

function FakeGui.new()
  local gui = { calls = {}, made = {}, mode = "working" }

  local function refuse(what)
    if gui.mode == "failing" then
      error(what .. " refused", 0)
    end
  end

  -- Window.new is (x, y, w, h, text); every other class takes the text alone.
  local function make(class_name, ...)
    refuse(class_name .. ".new")
    local args = { ... }
    local text = (class_name == "Window") and args[5] or args[1]
    local widget = {
      class = class_name,
      text = text,
      visible = nil,
      skin = nil,
      children = {},
    }
    for i = 1, #METHODS do
      local name = METHODS[i]
      widget[name] = function(self, a, b, c, d)
        refuse(name)
        gui.calls[#gui.calls + 1] = class_name .. ":" .. name
        if name == "setText" then self.text = a end
        if name == "getText" then return self.text end
        if name == "setVisible" then self.visible = a end
        if name == "getVisible" then return self.visible end
        if name == "setSkin" then self.skin = a end
        if name == "insertWidget" then
          self.children[#self.children + 1] = a
        end
        if name == "close" then
          -- Hide, then fire the callback. Measured on 2.9.29.27468 rather than
          -- assumed, twice: a counter in onClose incremented after the window
          -- had already gone, for both a programmatic close and a real click on
          -- the title bar's X. The refusal depends on that order, so a DCS that
          -- reversed it would break the window and pass this fake.
          self.visible = false
          if self.onClose then self:onClose() end
        end
        return true
      end
    end
    gui.made[#gui.made + 1] = widget
    gui.calls[#gui.calls + 1] = class_name .. ".new"
    return widget
  end

  local classes = {}
  for i = 1, #CLASSES do
    local name = CLASSES[i]
    classes[name] = { new = function(...) return make(name, ...) end }
  end

  function gui.widget(name)
    refuse("widget")
    -- Recorded, so a caller that keeps asking for a class that is not there can
    -- be caught doing it.
    gui.calls[#gui.calls + 1] = "widget:" .. tostring(name)
    if gui.mode == "absent" then
      return nil
    end
    return classes[name]
  end

  function gui.skin(name)
    refuse("skin")
    if gui.mode == "absent" then
      return nil
    end
    return { skinData = { params = { name = name } } }
  end

  function gui.fail_every() gui.mode = "failing" end
  function gui.no_library() gui.mode = "absent" end

  -- The widget of a class, or nil. Every class the window builds is built once,
  -- so this is enough to reach any of them without counting positions.
  function gui.find(class_name)
    for i = 1, #gui.made do
      if gui.made[i].class == class_name then
        return gui.made[i]
      end
    end
    return nil
  end

  return gui
end

return FakeGui
