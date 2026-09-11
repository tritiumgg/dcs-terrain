-- Offline tests for the frame budget and the job queue.
--
-- Run from the repository root with a plain lua5.1.
--
-- The clock is replaced throughout, because the thing under test is how many
-- steps a frame runs, and against a real clock that answer is whatever the
-- machine happens to be doing. Here a step costs exactly what the test says it
-- costs.

package.path = "extractor/?.lua;extractor/test/support/?.lua;" .. package.path

local T = require("testing")
local E = require("DcsTerrainExtract")

-- Seconds, the unit os.clock returns. Tests move it by hand.
local now = 0
E.clock = function() return now end

local function advance_ms(ms)
  now = now + ms / 1000
end

--------------------------------------------------------------------------------
T.group("budget")
--------------------------------------------------------------------------------

now = 100
local spent = E.budget(5)
T.eq("unspent at once", spent(), false)
advance_ms(4.9)
T.eq("unspent just under", spent(), false)
advance_ms(0.1)
T.eq("spent at the limit", spent(), true)
advance_ms(1000)
T.eq("stays spent", spent(), true)

-- A budget of zero is spent before any work is attempted, which is exactly the
-- case queue_frame's first step exists for.
T.eq("zero is spent immediately", E.budget(0)(), true)

T.raises("negative budget", function() E.budget(-1) end,
  "frame_budget_ms is not a non-negative number")
T.raises("non-numeric budget", function() E.budget("5") end,
  "frame_budget_ms is not a non-negative number")

--------------------------------------------------------------------------------
T.group("job shape")
--------------------------------------------------------------------------------

T.raises("jobs is not a list", function() E.new_queue(nil) end,
  "jobs is not a list")
T.raises("job without a name", function()
  E.new_queue({ { start = function() end } })
end, "job 1 is not")
T.raises("job without a start", function()
  E.new_queue({ { name = "water" } })
end, "job 1 is not")

-- A step that returns nothing would read as "not finished" forever, so it is
-- caught at the step rather than left to hang the sweep.
T.raises("step returns nothing", function()
  local queue = E.new_queue({ { name = "water", start = function() return function() end end } })
  E.queue_frame(queue, nil, function() return true end)
end, "step returned nil")

T.raises("start returns no step", function()
  local queue = E.new_queue({ { name = "water", start = function() return 7 end } })
  E.queue_frame(queue, nil, function() return true end)
end, "start returned number")

--------------------------------------------------------------------------------
T.group("steps run until the budget is spent")
--------------------------------------------------------------------------------

-- Counts its steps and never finishes, so the only thing stopping a frame is
-- the budget.
local function endless(cost_ms, counter)
  return {
    name = "endless",
    start = function()
      return function()
        counter.steps = counter.steps + 1
        advance_ms(cost_ms)
        return E.MORE
      end
    end,
  }
end

-- The clock only ever moves forward from here: it is a wall clock, and a job
-- is timed across the frames it spans. Each frame takes a fresh budget from
-- wherever the clock has got to.
local counter = { steps = 0 }
local queue = E.new_queue({ endless(1, counter) })
T.eq("frame is not done", E.queue_frame(queue, nil, E.budget(5)), E.MORE)
T.eq("five one-ms steps in a five-ms frame", counter.steps, 5)

-- The step is charged after it runs, so a step that overruns the budget still
-- runs whole and the frame ends after it. The alternative -- refusing to start
-- a step that might overrun -- cannot be implemented, because the cost of a
-- DCS call is not known until it returns.
counter = { steps = 0 }
queue = E.new_queue({ endless(40, counter) })
T.eq("overrunning frame is not done", E.queue_frame(queue, nil, E.budget(5)), E.MORE)
T.eq("one step, budget blown", counter.steps, 1)

-- Forward progress under a budget that is spent before the frame starts.
counter = { steps = 0 }
queue = E.new_queue({ endless(0, counter) })
T.eq("zero budget still steps", E.queue_frame(queue, nil, E.budget(0)), E.MORE)
T.eq("exactly one step", counter.steps, 1)

--------------------------------------------------------------------------------
T.group("jobs run in order and report their timings")
--------------------------------------------------------------------------------

-- Finishes after `steps` steps, each costing `cost_ms`, appending its name to
-- `order` when it starts.
local function counted(name, steps, cost_ms, order)
  return {
    name = name,
    start = function(run)
      order[#order + 1] = name .. ":" .. tostring(run)
      local left = steps
      return function()
        advance_ms(cost_ms)
        left = left - 1
        if left > 0 then
          return E.MORE
        end
        return E.DONE
      end
    end,
  }
end

local order = {}
queue = E.new_queue({
  counted("config", 1, 3, order),
  counted("water", 2, 4, order),
  counted("height", 1, 2, order),
})

T.eq("first frame has work left", E.queue_frame(queue, "run", E.budget(5)), E.MORE)
T.eq("config finished in frame one", #queue.finished, 1)
T.eq("config named", queue.finished[1].name, "config")
T.eq("config took three ms", queue.finished[1].ms, 3)

-- water started in the same frame: config finished 3 ms into a 5 ms budget, so
-- the frame took another step, and that step was water's first.
T.eq("water started in frame one", order[2], "water:run")

-- Frame two ends water and runs the whole of height, so two jobs finish in one
-- frame and each is timed over its own span, not the frame's.
T.eq("second frame drains the queue", E.queue_frame(queue, "run", E.budget(5)), E.DONE)
T.eq("two jobs finished in frame two", #queue.finished, 2)
T.eq("water named", queue.finished[1].name, "water")
T.eq("water took eight ms across two frames", queue.finished[1].ms, 8)
T.eq("height named", queue.finished[2].name, "height")
T.eq("height took two ms", queue.finished[2].ms, 2)

-- finished holds this frame's completions only, so a caller that writes the
-- manifest per frame does not rewrite a sweep it has already recorded.
T.eq("drained queue stays done", E.queue_frame(queue, "run", E.budget(5)), E.DONE)
T.eq("nothing finished again", #queue.finished, 0)

T.eq("three jobs started once each", #order, 3)
T.eq("in spec order",
  table.concat({ order[1], order[2], order[3] }, ","),
  "config:run,water:run,height:run")

--------------------------------------------------------------------------------
T.group("three small tables in one frame")
--------------------------------------------------------------------------------

order = {}
queue = E.new_queue({
  counted("beacons", 1, 1, order),
  counted("radio", 1, 1, order),
  counted("towns", 1, 1, order),
})
T.eq("one frame per table is not needed", E.queue_frame(queue, nil, E.budget(5)), E.DONE)
T.eq("all three reported", #queue.finished, 3)
T.eq("reported in order",
  table.concat({ queue.finished[1].name, queue.finished[2].name,
    queue.finished[3].name }, ","),
  "beacons,radio,towns")

--------------------------------------------------------------------------------
T.group("a step can refuse, and the frame ends there")
--------------------------------------------------------------------------------

-- A job that finds it cannot go on says so through its status. The queue does
-- not read the run -- it is handed nil here -- so what it proves is only that
-- the frame ends at the refusal and no later job is started against work that
-- did not happen.
local function refusing(name, order)
  return {
    name = name,
    start = function()
      order[#order + 1] = name
      return function()
        advance_ms(1)
        return E.REFUSED
      end
    end,
  }
end

order = {}
queue = E.new_queue({
  counted("build", 1, 1, order),
  refusing("terrain", order),
  counted("water", 1, 1, order),
})
T.eq("the frame reports the refusal", E.queue_frame(queue, nil, E.budget(5)), E.REFUSED)
T.eq("the job before it finished", #queue.finished, 1)
T.eq("and is the one reported", queue.finished[1].name, "build")
T.eq("the job after it never started", order[3], nil)
T.eq("two jobs started", #order, 2)

--------------------------------------------------------------------------------
T.group("an empty queue is done")
--------------------------------------------------------------------------------

queue = E.new_queue({})
T.eq("nothing to run", E.queue_frame(queue, nil, E.budget(5)), E.DONE)
T.eq("nothing reported", #queue.finished, 0)

--------------------------------------------------------------------------------
T.group("a job may say how far it has got")
--------------------------------------------------------------------------------

-- Finishes after `steps` steps and answers whatever `answer` returns when asked
-- for its progress, so the sanitising below can be fed anything.
local function counting(name, steps, answer)
  return {
    name = name,
    start = function()
      local left = steps
      return function()
        advance_ms(1)
        left = left - 1
        return left > 0 and E.MORE or E.DONE
      end, answer
    end,
  }
end

-- No running job, nothing to ask.
queue = E.new_queue({ counting("water", 3, function() return 1, 3 end) })
T.eq("before the first frame there is no count", E.queue_progress(queue), nil)

E.queue_frame(queue, nil, E.budget(1))
local done, total = E.queue_progress(queue)
T.eq("a running job answers", done .. "/" .. total, "1/3")

-- A job that returns only a step is the contract every earlier job was built
-- to, and it still runs, with no count.
queue = E.new_queue({ endless(1, { steps = 0 }) })
E.queue_frame(queue, nil, E.budget(1))
T.eq("a job without progress has no count", E.queue_progress(queue), nil)

-- The second value is checked like the first, because a typo here would
-- otherwise surface as a raise the first time somebody asked mid-sweep, on
-- DCS's own stack.
T.raises("a second value that is not a function raises", function()
  local q = E.new_queue({ { name = "water", start = function()
    return function() return E.DONE end, 7
  end } })
  E.queue_frame(q, nil, E.budget(5))
end, "job water: start returned number, not a progress function")

-- What a progress function may answer and still be counted: two finite
-- numbers with a positive total. Anything else is "cannot count", and done is
-- held inside the total, so no fraction above one can come out of here.
local function sanitised(d, t)
  local q = E.new_queue({ counting("x", 10, function() return d, t end) })
  E.queue_frame(q, nil, E.budget(0))
  local got_done, got_total = E.queue_progress(q)
  if got_done == nil then
    return "nil"
  end
  return got_done .. "/" .. got_total
end
T.eq("done above total is clamped to it", sanitised(7, 5), "5/5")
T.eq("done below zero is clamped to zero", sanitised(-1, 5), "0/5")
T.eq("a zero total cannot count", sanitised(0, 0), "nil")
T.eq("a negative total cannot count", sanitised(1, -5), "nil")
T.eq("a string cannot count", sanitised("3", 5), "nil")
T.eq("nothing answered cannot count", sanitised(nil, nil), "nil")
T.eq("an infinite total cannot count", sanitised(1, math.huge), "nil")
T.eq("a nan done cannot count", sanitised(0 / 0, 5), "nil")
T.eq("a fraction of a unit is fine", sanitised(2.5, 5), "2.5/5")

-- The frame in which a job finishes and its budget runs out before the next
-- starts: the count belongs to nobody. A progress function left over from the
-- finished job would answer total/total for a sweep that has not begun.
queue = E.new_queue({
  counting("water", 1, function() return 1, 1 end),
  counting("height", 5, function() return 0, 5 end),
})
T.eq("water finishes in one step", E.queue_frame(queue, nil, E.budget(1)), E.MORE)
T.eq("and the queue is between jobs", queue.step, nil)
T.eq("with no progress function held", queue.progress, nil)
T.eq("so there is no count", E.queue_progress(queue), nil)
E.queue_frame(queue, nil, E.budget(1))
done, total = E.queue_progress(queue)
T.eq("height's own count once it starts", done .. "/" .. total, "0/5")

T.done()
