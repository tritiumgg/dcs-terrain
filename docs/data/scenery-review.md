# Scenery review from screenshots

Every Caucasus model that could be a structure, photographed in DCS and judged
by eye, on 2.9.31.29434 (20261027-141234), 2026-11-01. It tests the class rules
against what the models look like, across the 8 terrains.

## Capture

Both calls work with a mission running and the F11 camera active. Neither has
a call site in ED's Lua, so this is measured, not read.

| Step | Call | State | Result |
|---|---|---|---|
| Move | `LoSetCameraPosition({x=fwd, y=up, z=right, p=pos})` | `export` | Holds in F11; `LoGetCameraPosition` reads it back on a later frame |
| Shoot | `DCS.makeScreenShot()`, no arguments | `hook` | Writes `Screen_<date>_<time>.png` to `Saved Games\DCS\Screenshots`, the file complete about 5 s later |

`right = fwd × up`. Both sides run from frame callbacks on a shared schedule of
model times: `LuaExportAfterNextFrame` moves, `onSimulationFrame` shoots.

- **The mission is started by hand.** ED's `MissionEditor/GameGUI.lua` marks
  `DCS.startMission` not implemented. One aircraft on the ramp, Fly, F11.
- **The view moves if the mouse moves while DCS has focus.** Hands off.
- **5 s a model works:** move, wait 3.5 s, shoot. Shots 3 s and 11 s after a
  move came out the same, and the user saw the models load in 1–2 s.
- **About 1 frame in 100 shows only water, cause unknown.** 3 of 291, and the
  HUD gave the same grid for the first one and the good frame after it. The
  camera position read back right, and the user watched loaded land turn to
  water, so it is not terrain still loading. A run of empty frames came just
  before the second, while this machine was reading all 8 catalogs, so another
  load on the machine may matter. Retake them.
- **Trees hide more than neighbors do.** Being clear of other boxes is not
  enough, because trees are a pool apart from the boxes: at 40° about a dozen
  models were hidden. Rank placements by `tree_density` and retake at 60° from
  another side.

## Coverage

241 of the 284 placed models were shot, one placement each, with 38 retakes
among 19 of them. Not shot: the 43 airfield markers, lights and
`blk_light_pole`, which their names and sizes settle.
`directional_approach_lights_group`, a row of lights by name, is `wall` by its
thin box and stays that way, unreviewed. 237 models have a label and 4 are left
to the user. Family was judged only where it changes whether a model settles.

## Heuristics

Scored on the 237 labels, rules alone, no overrides. Percent of models, then
the last column by objects.

| Rule added | building P / R | industrial P / R | misc P / R | Models right | Objects right |
|---|---|---|---|---|---|
| None | 66 / 100 | 100 / 76 | 96 / 35 | 75.1 | 89.9 |
| Slender: at most 7 m wide, 10 m or taller, is `misc` | 67 / 100 | 100 / 75 | 90 / 39 | 75.9 | 89.9 |
| Long: 5:1 or more and 30 m or longer is `misc` | 71 / 95 | 100 / 76 | 82 / 54 | 78.1 | 88.7 |
| **No family by name is `misc`** | **90 / 100** | **100 / 76** | **93 / 94** | **92.4** | **90.4** |

A model with no box, 0 x 0, is already `misc` by the 4 m² rule, so that rule
adds nothing. One of them is `nasos`, a pump jack, which is an override.

**Adopted: a `building` the family rules leave without a family is `misc`.**
Those rules already name bridges, cranes, masts and debris, and leave a flat box
no name tells without one too; such models were counting as built-up.

- **On Caucasus** it fires on 45 labeled models and is right on 41. The 4 wrong
  ones, `bashnja-stena`, `airbase_barrels_01`, `airbase_barrels_02` and
  `dok_s_new`, keep their overrides.
- **On all 8 terrains** it fires on 324 models. Only Caucasus has labels, so
  elsewhere this is a reading of names: about 10 of the 324 look like buildings
  caught by a name pattern, and the flat, unnamed models are rocks, sports cars
and a road crossover. The buildings are `2-floor_house_clay_walls` and `_2` (`wall`),
  `guam_memorial_hospital` (`memorial`), `mandalaybay` (`mandal`), `boat_house`,
  `monorail_parking_building`, `haifa_university_parking`, `vegas_parking_01`,
  `afb_buildings_01` and `breakwater_building`. Each terrain's review settles
  these with screenshots.

**Rejected: box shape.** Each candidate takes what look like real buildings
elsewhere, again by name:

- Slender: `transformer_booth_eu` and `german_rural_24` on GermanyCW,
  `parthouseafghanistan_04` and `_05` on Afghanistan, among 2.97 M objects there.
- Long: `bagram_apartment`, `dormitory`, `military_building_01`, `shop_3`,
  `saipan_int_terminal`, `greenhouse1`.

What the rule still misses on Caucasus is `komplex`, `komplex_big` and two radar
domes: nothing in a name or box tells those. `industrial` recall stays at 76 %
because `garage_a_new`, `ferma_a_new`, `tr_budka_new` and the like carry no
industrial name. Those are all overrides.

## What moved

Objects that changed class, by terrain, rule and overrides together.

| Terrain | Models | Objects | Of | `building` before → after | `misc` before → after |
|---|---|---|---|---|---|
| Afghanistan | 42 | 1 039 635 | 10 447 341 | 10 139 288 → 9 099 653 | 67 → 1 039 702 |
| Caucasus | 13 | 127 | 936 823 | 461 614 → 461 528 | 325 367 → 325 494 |
| GermanyCW | 98 | 59 736 | 8 687 758 | 7 828 128 → 7 768 392 | 190 079 → 249 815 |
| MarianaIslands | 18 | 11 778 | 191 999 | 157 476 → 145 698 | 26 330 → 38 108 |
| MarianaIslandsWWII | 12 | 8 051 | 51 008 | 47 900 → 39 849 | 1 729 → 9 780 |
| Nevada | 49 | 29 006 | 553 400 | 462 200 → 433 194 | 22 888 → 51 894 |
| PersianGulf | 55 | 271 966 | 2 941 637 | 2 285 042 → 2 013 076 | 311 522 → 583 488 |
| Syria | 122 | 441 068 | 6 328 726 | 5 471 009 → 5 029 941 | 604 538 → 1 045 606 |

Caucasus also moved 41 `industrial` objects to `misc`: lattice masts and
lighthouses, which are not plant. Places barely move: the rule touches only
models with no family, and only the override models lose one. `count_building`,
the built-up layers and the placement check's building figures do move. On
Afghanistan most of the change is debris, garbage and sand-block walls.

## What the review changed on Caucasus

Thirteen models changed class, and none of them among the 20 most placed: the
top of the catalog was already right. `korpus_a`, `_b`, `_a1`, `_d` and `korpus`
look more like offices than flats, but they settle either way, so their family
stands. Left to the user: `korpus_b1`, `korpus_c`, and `ukrytie` and
`air_closedguardhouse_03a`, which showed nothing in any frame.
