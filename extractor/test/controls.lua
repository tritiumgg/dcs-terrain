-- Offline tests for the conversion between what a control holds and a config.
--
-- Run from the repository root with a plain lua5.1.
--
-- A control holds a string and a config holds numbers, so something has to
-- convert, and that something is the only part of the window testable with no
-- widget in the process. Two properties are worth the file. A config has to
-- survive the trip out to the boxes and back unchanged, or a user who pressed
-- Start without touching anything would move their own crop. And text that will
-- not parse has to reach the existing checkers as itself, so that the message
-- names what is in the box rather than the nil it would otherwise become.

package.path = "extractor/?.lua;extractor/test/support/?.lua;" .. package.path

local T = require("testing")
local E = require("DcsTerrainExtract")

--------------------------------------------------------------------------------
T.group("a config becomes the strings a window shows")
--------------------------------------------------------------------------------

local text = E.control_text({ enabled = true, output_dir = "C:/extract" })
T.eq("the path is the path", text.output_dir, "C:/extract")
T.eq("no crop leaves the tick off", text.crop, false)
T.eq("and the centre blank", text.crop_x, "")
T.eq("and the other half of it", text.crop_z, "")
T.eq("and the radius", text.crop_radius_m, "")

text = E.control_text({
  enabled = true,
  output_dir = "C:/extract",
  crop = { x = -290000, z = 617000, radius_m = 5000 },
})
T.eq("a crop ticks the box", text.crop, true)
T.eq("the centre reads as it was typed", text.crop_x, "-290000")
T.eq("both halves of it", text.crop_z, "617000")
-- 5000 and not 5000.0000000000000, which is what an exact format would put in
-- front of somebody about to read the number back off the screen.
T.eq("and the radius is readable", text.crop_radius_m, "5000")

-- The other side of that trade: a value the readable format cannot carry falls
-- back to the exact one rather than losing a digit.
text = E.control_text({ crop = { x = 1 / 3, z = 0, radius_m = 1 } })
T.eq("a number needing every digit gets them", tonumber(text.crop_x), 1 / 3)
T.eq("and zero is still shown", text.crop_z, "0")

-- Total, because it is called on whatever the config file held.
text = E.control_text({})
T.eq("an empty config is blank", text.output_dir, "")
T.eq("with no crop", text.crop, false)

--------------------------------------------------------------------------------
T.group("the strings become a config again")
--------------------------------------------------------------------------------

local config = E.config_from_text({
  output_dir = "C:/extract",
  crop = false,
  crop_x = "1",
  crop_z = "2",
  crop_radius_m = "3",
})
T.eq("the window is on, so the config is", config.enabled, true)
T.eq("the path comes through", config.output_dir, "C:/extract")
-- Dropped rather than cleared: a user who unticks and ticks again still has
-- the three numbers they typed.
T.eq("an unticked crop is no crop", config.crop, nil)

config = E.config_from_text({
  output_dir = "  C:/extract  ",
  crop = true,
  crop_x = " -290000 ",
  crop_z = "617000",
  crop_radius_m = "5000",
})
T.eq("a typed path is trimmed", config.output_dir, "C:/extract")
T.eq("and so are the numbers", config.crop.x, -290000)
T.eq("the second one too", config.crop.z, 617000)
T.eq("and the radius", config.crop.radius_m, 5000)

config = E.config_from_text({ output_dir = "   ", crop = false })
T.eq("a blank box is a question not answered", config.output_dir, nil)

-- The point of the whole seam: text that will not parse arrives at the checkers
-- as itself. Turning it into nil here would report "nil" about a box the user
-- can see characters in.
config = E.config_from_text({
  crop = true,
  crop_x = "12abc",
  crop_z = "",
  crop_radius_m = "1e400",
})
T.eq("unparseable text stays text", config.crop.x, "12abc")
T.eq("a blank member is nil", config.crop.z, nil)
T.eq("and an overflow is the infinity it parsed to", config.crop.radius_m, math.huge)

--------------------------------------------------------------------------------
T.group("a config survives the trip to the boxes and back")
--------------------------------------------------------------------------------

-- Compared through the encoder, which sorts keys and writes every number at
-- seventeen digits, so a digit lost on the way through the boxes shows up in the
-- failure rather than hiding behind tostring.
local function round_trips(name, c)
  T.eq(name, E.json(E.config_from_text(E.control_text(c))), E.json(c))
end

round_trips("a path and no crop", { enabled = true, output_dir = "C:/extract" })
round_trips("a crop off the Caucasus map", {
  enabled = true,
  output_dir = "C:/extract",
  crop = { x = -290014.28571428, z = 617414.57142857, radius_m = 5000 },
})
-- Zero is the one a "blank means nothing" rule eats if it tests truthiness
-- rather than the text.
round_trips("a crop on the origin", {
  enabled = true,
  output_dir = "C:/extract",
  crop = { x = 0, z = 0, radius_m = 50 },
})

--------------------------------------------------------------------------------
T.group("a bad box is worded by the checker that already exists")
--------------------------------------------------------------------------------

-- Not a new message anywhere: the config the boxes describe goes through the
-- same validation the file does, and the line names what is in the box.
config = E.config_from_text({
  output_dir = "C:/extract",
  crop = true,
  crop_x = "12abc",
  crop_z = "2",
  crop_radius_m = "3",
})
local problem = E.field_problem("crop", config.crop)
T.eq("the message quotes the box", problem:find("12abc", 1, true) ~= nil, true)

local settings, problems, tags = E.validate_config(config)
T.eq("one line for the crop", #problems, 1)
T.eq("against the crop", tags[1], "crop")
T.eq("and it is the checker's own wording", problems[1], problem)
T.eq("the config is still usable", settings.enabled, true)
T.eq("with the bad crop dropped", settings.crop, nil)

-- A blank path is the one field with no default, and says so.
settings, problems, tags = E.validate_config(
  E.config_from_text({ output_dir = "", crop = false }))
T.eq("a blank path is one line", #problems, 1)
T.eq("against the path", tags[1], "output_dir")
T.eq("and there is nothing to fall back to", settings.output_dir, nil)

-- The crop reports the first thing wrong with it and stops, so a user with two
-- bad members fixes one and then sees the other. One line per field, counted.
settings, problems = E.validate_config(E.config_from_text({
  output_dir = "C:/extract",
  crop = true,
  crop_x = "no",
  crop_z = "2",
  crop_radius_m = "also no",
}))
T.eq("two bad crop members are still one line", #problems, 1)
T.eq("naming the first", problems[1]:find("crop.x", 1, true) ~= nil, true)

T.done()
