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

Three more are numbers a user has no basis to pick. `cell_size` is fixed at 50 by
the extract format itself. `crs` is fitted from `latlon_samples` at pack time by
C10, so a value supplied here can only disagree with the measurement. A theatre
lives where DCS installed it, so no path among these is genuinely variable.

Two have stopped earning their place. `passes` switches off half the extract; it
was a mission-versus-hook distinction that ADR 0010 already reduced to "which Lua
state the calls run in", and both halves now run from the same bare editor map.
`allow_helipads` guards the scenery sweep against a placed heliport clearing every
scenery object within about 150 m, and ADR 0010's consequences kept it "for a user
who chooses to extract from inside a mission anyway".

## Decision

The config table holds nine fields in three tiers, and everything else is derived
from the install or is a constant in the hook.

`enabled` is the master switch, read before anything else, and keeps the meaning
the spec gives it: a boolean defaulting to false, and when it is not true the hook
does nothing at all — no window, no run, no log line. An absent config file is
therefore still a disabled hook.

`output_dir` and `omit_sea_tiles` are what a user sets. `output_dir` is a required
non-empty string and the only field with no default; `omit_sea_tiles` is a boolean
defaulting to true.

`crop_m`, `cell_size`, `tile_size`, `frame_budget_ms`, `road_seed_spacing` and
`road_seed_neighbours` are advanced: present in the file and in a separate section
of the window, defaulted when absent. `crop_m` stays because a crop is a
deliberate choice no theatre can supply and X10's acceptance run needs one.
`cell_size` is validated as exactly 50 rather than as a number, so a value the
format cannot carry is refused at the config rather than at `pack`.
`frame_budget_ms` stays because `onSimulationFrame` fires in the Mission Editor and
the slicing exists to keep it responsive.

`terrain_dir` is derived by scanning `Mods/terrains/*/entry.lua` for the directory
whose `id` matches `GetTerrainConfig("id")`. `towns_lua` and `nodes_lua` are
`<terrain_dir>/Map/towns.lua` and `<terrain_dir>/MissionGenerator/nodes.lua`.
`authored_bounds_m` comes from `nodesMapBorders` or the pre-sweep.

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

**Keep `allow_helipads` as a warning rather than dropping it.** Rejected on the
grounds that a warning nobody can act on is not worth the check: in the stated
workflow there is no mission, so no statics, so the guard never fires. Keeping the
check to describe a case the tool is not for costs code in X8b and a line in the
window for no reader.

**Keep `passes` because the manifest records it.** Rejected. The manifest key is
a separate question from the config switch, and a switch kept alive only to feed a
record that nothing reads is not a configuration option, it is a leftover.

**Move the advanced fields out of config into constants.** Rejected because X10's
done test is a 10 × 10 km crop around Kutaisi, and a crop that can only be set by
editing the hook is a rebuild for every test run.

## Consequences

The window has two controls a user reads and six behind an advanced section,
instead of sixteen fields in a file with no defaults visible.

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
it implements. X10 keeps `crop_m`. X13 is rewritten as the control window this
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
