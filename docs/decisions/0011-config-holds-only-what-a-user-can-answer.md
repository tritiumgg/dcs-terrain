# ADR 0011: The config table holds only what a user can answer

## Status

Accepted

## Context

**Affects:** `extractor-hook.md` "Config table", "Hook-pass sweeps" (the
`towns_lua`/`nodes_lua` sandbox and the `terrain_fingerprint` paragraph), and
"Mission-pass sweeps" (the helipad check); `extract-format.md` "Tables"
(`config.json.crs`) and "manifest.json" (`authored_bounds_source`); ADR 0010's
consequence keeping `allow_helipads`; `plan.md` X2a, X2b, X5, X8b, X10, X12, X13.

The frozen config table has sixteen fields:

```lua
return {
  enabled = true,
  output_dir = "C:/extracts/caucasus-50m",   -- created if absent
  cell_size = 50,                             -- metres; always 50, coarser bases are pack's choice
  tile_size = 256,
  frame_budget_ms = 5,                        -- work per onSimulationFrame
  crop_m = nil,                               -- or {min_x=, min_z=, max_x=, max_z=}
  authored_bounds_m = nil,                    -- nodesMapBorders, if known
  omit_sea_tiles = true,
  road_seed_spacing = 1000,
  road_seed_neighbours = 4,
  passes = { hook = true, mission = true },
  allow_helipads = false,                     -- run the scenery sweep with helipads present
  towns_lua = "C:/Program Files/Eagle Dynamics/DCS World/Mods/terrains/Caucasus/Map/towns.lua",
  nodes_lua = "C:/Program Files/Eagle Dynamics/DCS World/Mods/terrains/Caucasus/MissionGenerator/nodes.lua",
  crs = nil,                                  -- optional {proj4=..., lon_0=..., ...}
  terrain_dir = "Caucasus",                   -- directory under Mods/terrains/
}
```

It was written for a hand-edited Lua file, where asking the user for a path is
cheaper than deriving one. The configuration is moving into a window the user
drives from the Mission Editor, and a window has to name every control it shows.
Naming them exposes that most of these are not questions a user can answer.

Four are facts about the install, not choices. Measured on this install, DCS
2.9.29.27468 / 20260902-093323: `Mods/terrains/` holds `Afghanistan`, `Caucasus`,
`GermanyColdWar`, `Kola`, `MarianaIslands`, `MarianasWWII`, `Nevada`,
`PersianGulf`, `Sinai` and `Syria`, and each directory's `entry.lua` carries its
own theatre id — `Sinai/entry.lua` reads `['id'] = "SinaiMap"`, which is exactly
the mismatch the spec warns about when it says `terrain_dir` "is not always the
`id`, e.g. `SinaiMap` lives in `Sinai`". Matching `GetTerrainConfig("id")` against
those files resolves the directory with no lookup table and no user input.
`Sinai/Map/` and `Sinai/MissionGenerator/` are both present, so the two Lua paths
hang off the same answer. `authored_bounds_m` is `nodesMapBorders` or the
pre-sweep, which the spec already says.

Six more are values a user has no basis to pick. `cell_size` has exactly one
legal value: `extract-format.md` says "`cell_size` is 50 in every extract; a
coarser packed base is `pack`'s choice", and `design-and-facts.md` gives the
reason — "Do not multi-resolution the DCS sweep; multi-resolution the derived
products." `tile_size` has many legal values and no reason to move among them: it
is internal chunking, 128 KB per tile at 256. `omit_sea_tiles` has no useful
`false`, because omitting is lossless — `water` is `0 land, 1 lake, 2 sea, 3
river`, so a lake at altitude is `1` and is never omitted, and an omitted all-`2`
tile reconstructs exactly to height 0 and surface WATER. `road_seed_spacing` and
`road_seed_neighbours` were measured rather than chosen (X7a: 0.61 ms per 1 km
path, 1000 m seeds, 100 m merge); they do decide about twenty-five minutes of a
Caucasus run, but their effect on the road graph is not something a user can
predict from the numbers. `frame_budget_ms` changes how long a run takes and
never what it produces, and its effect is not even uniform: `queue_frame` always
runs at least one step, and a mission-pass chunk is about 40 ms and "exceeds
`frame_budget_ms` by design", so below 40 the surface sweep gets one chunk a frame
whatever the number says, while roads scale with it. `crs` is fitted from
`latlon_samples` at pack time by C10, so a value supplied here can only disagree
with the measurement.

Two have stopped earning their place. `passes` switches off half the extract; it
was a mission-versus-hook distinction that ADR 0010 already reduced to "which Lua
state the calls run in", and both halves now run from the same bare editor map.
`allow_helipads` guards the scenery sweep against a placed heliport clearing every
scenery object within about 150 m, and ADR 0010's consequences kept it "for a user
who chooses to extract from inside a mission anyway".

## Decision

The config table holds three fields, and everything else is derived from the
install or is a constant in the hook.

`enabled` is the master switch, read before anything else, and keeps the meaning
the spec gives it: a boolean defaulting to false, and when it is not true the hook
does nothing at all — no window, no run, no log line. An absent config file is
therefore still a disabled hook.

`output_dir` is what a user sets, and the only field the window shows in its main
body: a required non-empty string, and the only field with no default.

`crop` is the third, and it changes form. The frozen field is `crop_m`, four
DCS-metre box coordinates, which nobody can type from knowing where they want to
extract. It becomes a centre and a radius — `{x, z, radius_m}` — because the
Mission Editor displays the X and Z under the cursor, so a user can read the two
numbers off the map they are looking at. `CLAUDE.md` already has the shape: "the
canonical form is an axis-aligned box in DCS metres … circles, polygons and
airfield distance bands are convenience inputs converted to a box". `M.crop_box`
converts, the grid is planned from the box, and the manifest records `crop_m` as
the box exactly as the frozen format says. A radius of `r` gives a square of side
`2r`, so X10's 10 × 10 km around Kutaisi is `radius_m = 5000`.

`terrain_dir` is derived by scanning `Mods/terrains/*/entry.lua` for the directory
whose `id` matches `GetTerrainConfig("id")`. `towns_lua` and `nodes_lua` are
`<terrain_dir>/Map/towns.lua` and `<terrain_dir>/MissionGenerator/nodes.lua`.
`authored_bounds_m` comes from `nodesMapBorders` or the pre-sweep.

`cell_size`, `tile_size`, `omit_sea_tiles`, `frame_budget_ms`,
`road_seed_spacing` and `road_seed_neighbours` become constants in the hook. The manifest records each of
them exactly as it did before — the format stays parametric and a reader still
gets told what it was built with. What goes away is the pretence that anyone
chooses them. A config still carrying one is reported as an unrecognised key like
any other.

`passes`, `allow_helipads` and `crs` are dropped. Both passes always run, the
helipad check is not written at all, and the projection is fitted at pack time.

The extractor's stated purpose is to be run from the Mission Editor with a map
open. Running it from inside a mission is not blocked and is not what it is for.

### Alternatives considered

**Keep `terrain_dir` as config with a derived default.** Rejected: it makes the
window show a control whose only correct value is the one already computed, and a
user who edits it can only break the fingerprint. The derivation either works, in
which case the field is noise, or it fails, in which case a wrong directory name
is not the fix — a theatre DCS did not install is not extractable either way.

**Keep the six as fields with validators.** Rejected, though it buys one thing: a
config saying `cell_size = 100` gets "cell_size is 100, and every extract is 50"
instead of the vaguer "cell_size is not a config field". That is a better message
for someone carrying a config over from the specified table, and it is not worth
six controls in the window whose only correct positions are the ones they already
have, or validators whose job is to reject every value but the default.

**Keep `omit_sea_tiles` because it changes the extract's size.** Rejected: `false`
produces a larger extract carrying identical information, so it is not a trade.
The reconstruction rule is exact, and the case that would make it lossy — a lake
above sea level — is `water` value `1` and is never omitted.

**Keep the two road seed numbers, because they decide half the run time.**
Rejected as the closest call here. Coarser seeds really would shorten a Caucasus
run by much of its twenty-five roads minutes. But X7a measured these values, and
what a user would be trading away is road-graph fidelity that nothing reports and
that they cannot predict from the numbers. If a faster run turns out to be worth
having, the honest form is a named coarse mode with its own measurement, not two
raw numbers in a window.

**Keep `allow_helipads` as a warning rather than dropping it.** Rejected on the
grounds that a warning nobody can act on is not worth the check: in the stated
workflow there is no mission, so no statics, so the guard never fires. Keeping the
check to describe a case the tool is not for costs code in X8b and a line in the
window for no reader.

**Keep `passes` because the manifest records it.** Rejected. The manifest key is
a separate question from the config switch, and a switch kept alive only to feed a
record that nothing reads is not a configuration option, it is a leftover.

**Move the crop into a constant too.** Rejected: it is the one remaining field
that changes what the extract *is* rather than how fast it appears, and X10's done
test is a 10 × 10 km crop around Kutaisi.

**Keep `frame_budget_ms` because it has a describable trade.** Rejected, and this
was the last field to fall. Being able to describe a trade is not the same as a
user being able to make one: nothing in the tool reports frame cost or a projected
finish to tune against, the effect is not uniform across sweeps, and the stated
design target is "the extraction finishing while the user does something else",
which makes the responsiveness half of the trade largely hypothetical for a
forty-minute run. If it ever does matter, the honest form is a named two-state
control — keep the editor usable, or run flat out — with a measurement behind each
state, the same shape as the coarse road mode above.

**Keep the crop as a box.** Rejected: a box is what the format records, not what a
user can supply. A centre and a radius is two numbers the Mission Editor shows
under the cursor plus one the user chooses, and the conversion is four lines.

## Consequences

The window has one control a user reads, one more for the crop, and no advanced
section at all — against sixteen fields in a file with no defaults visible.

**The crop's config form no longer matches the manifest's.** `crop` goes in as a
centre and a radius and `crop_m` comes out as a box, so a reader comparing the two
files sees different shapes for the same idea. The conversion is one function with
its own tests, and the alternative was a field nobody could fill in.

X13 can improve on typing two coordinates by letting the user click the map, which
is a stretch goal rather than a requirement — the readout the Mission Editor
already shows is enough to make the field usable without it.

**A user extracting from inside a mission with a heliport static or FARP placed
gets scenery with a hole about 150 m across, and is not told.** That is the cost of
dropping the guard, and it is accepted because the stated workflow is the editor,
where no statics exist. It reverses ADR 0010's consequence line keeping
`allow_helipads`; that clause no longer holds.

A theatre that ships map helipads is better off: the old default skipped its
scenery sweep entirely, and only a config edit could turn it back on.

`M.pass_enabled` and its two callers in `next_state` are deleted, and
`prepare → hook → mission → done` becomes unconditional.

**Done tests that change.** X2b gains the `terrain_dir` derivation and the
entry-file scan, so its fingerprint no longer depends on a config value. X5 gains
the `towns_lua` and `nodes_lua` derivation. X8b loses the helipad check from what
it implements. X10 states its crop as a centre and a radius: `radius_m = 5000` around Kutaisi. X13 is rewritten as the control window this
decision is made for, and X12 with it, since the bar it fills in is the window's.
X2a's own done test changes too, but with the separate decision about what
validation does with a bad value rather than with this one.

**Frozen text that now reads false:** the config table in `extractor-hook.md`, the
`terrain_dir` clause in its `terrain_fingerprint` paragraph, the `allow_helipads`
sentence in its scenery sweep, and `config.json.crs` in `extract-format.md`
"Tables" so far as it says the extractor was given the parameters — it is fitted at
pack time and the extractor writes null.

**Provisional in one respect.** The `entry.lua` scan is measured against the ten
theatres on this install at 2.9.29.27468. A theatre whose `entry.lua` does not set
`id`, or sets it in a form the pattern does not match, would reopen the question of
a fallback; the fallback would be to try the id as a directory name before failing,
not to restore the config field.
