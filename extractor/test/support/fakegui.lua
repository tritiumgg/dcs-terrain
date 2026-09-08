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

local CLASSES = {
  "Window", "Panel", "Static", "HorzProgressBar", "EditBox", "CheckBox",
  "Button",
}

-- Every method the window calls. One that is not in this list is a nil index
-- rather than a silent no-op, so a typo shows up as a failure here rather than
-- as a widget that quietly does nothing.
local METHODS = {
  "setVisible", "getVisible", "setSkin", "setBounds", "setDraggable",
  "setResizable", "insertWidget", "setText", "getText", "close",
  "setRange", "setValue", "setState", "getState", "getViewBounds",
}

-- What a window's frame costs it: DCS grants a client area shorter than the
-- window by the header, measured at 20 pixels on 2.9.29.27468. Rows are laid
-- out in client coordinates, so a window sized to its content is too small for
-- it and the last row falls off the bottom. Modelled here so the correction can
-- be tested, and named so the test does not repeat the number.
local HEADER = 20

function FakeGui.new()
  local gui = { calls = {}, made = {}, mode = "working" }

  -- Calls left before this library starts raising, or nil for one that does
  -- not. A library breaking part-way through a sequence is a different failure
  -- from one that was broken before the window was built, and the difference
  -- matters: a widget call that fails answers nil, and nil reads as a legal
  -- answer -- an unticked box, a blank field -- rather than as an error.
  local budget = nil

  local function refuse(what)
    if gui.mode == "failing" then
      error(what .. " refused", 0)
    end
    if budget then
      if budget <= 0 then
        error(what .. " refused", 0)
      end
      budget = budget - 1
    end
  end

  -- Window.new is (x, y, w, h, text) and a progress bar takes nothing; every
  -- other class takes the text alone.
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
      -- This widget's own calls, beside gui.calls which holds the window's.
      -- Several widgets of a class share a class name, so counting
      -- "Static:setText" across the window stopped meaning anything once there
      -- was more than one label in it.
      calls = {},
    }
    for i = 1, #METHODS do
      local name = METHODS[i]
      widget[name] = function(self, a, b, c, d)
        refuse(name)
        gui.calls[#gui.calls + 1] = class_name .. ":" .. name
        self.calls[#self.calls + 1] = name
        if name == "setText" then self.text = a end
        if name == "getText" then return self.text end
        if name == "setVisible" then self.visible = a end
        if name == "getVisible" then return self.visible end
        if name == "setSkin" then self.skin = a end
        if name == "setRange" then self.range = { a, b } end
        if name == "setValue" then self.value = a end
        -- The real one takes a boolean and stores 0 or 1; getState turns it
        -- back into a boolean, so a boolean is what a caller sees either way.
        if name == "setState" then self.state = a and true or false end
        if name == "getState" then return self.state and true or false end
        -- Kept so a row that runs off the bottom of the window can be caught
        -- without a screenshot.
        if name == "setBounds" then self.bounds = { a, b, c, d } end
        -- x, y, w, h of the client rectangle, inset by the header. A window
        -- with no bounds yet has no client area to report.
        if name == "getViewBounds" then
          if not self.bounds then return nil end
          return 0, HEADER, self.bounds[3], self.bounds[4] - HEADER
        end
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

  -- Let n more calls through, then raise on every one after them. For the
  -- library that breaks in the middle of something rather than before it.
  function gui.fail_after(n) budget = n end

  -- The first widget of a class, or nil. The window, the panel, the status line
  -- and the bar are each built once, so this reaches any of them without
  -- counting positions.
  function gui.find(class_name)
    for i = 1, #gui.made do
      if gui.made[i].class == class_name then
        return gui.made[i]
      end
    end
    return nil
  end

  -- A press. The native side fires a widget's own onChange and no Lua method
  -- does, which is why this is the test's handle on the event rather than
  -- another entry in METHODS: a window that could press its own buttons would
  -- be a shape the real library does not have.
  function gui.press(widget)
    if widget and widget.onChange then
      widget:onChange()
    end
  end

  -- How many times one widget was asked to do something. The window writes a
  -- label only when its text changed, and that is the property this counts.
  function gui.count(widget, method)
    if widget == nil then
      return 0
    end
    local n = 0
    for i = 1, #widget.calls do
      if widget.calls[i] == method then
        n = n + 1
      end
    end
    return n
  end

  -- Every widget of a class, in the order they were made. The controls are
  -- several of a kind, and counting them is how a missing row is caught without
  -- pinning a total that changes whenever a row is added.
  function gui.all(class_name)
    local out = {}
    for i = 1, #gui.made do
      if gui.made[i].class == class_name then
        out[#out + 1] = gui.made[i]
      end
    end
    return out
  end

  return gui
end

return FakeGui
