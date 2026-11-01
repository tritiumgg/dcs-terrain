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
- **A model with no box has no size to frame by.** From about 30 m, `ukrytie`
  and `air_closedguardhouse_03a` filled the frame with their grass roofs and
  looked like empty ground. From about 290 m each was plainly an aircraft
  shelter. Shoot no-box models from far out first.

## Coverage

241 of the 284 placed models were shot, one placement each, with 38 retakes
among 19 of them and 6 wide shots of the two shelters. Not shot: the 43 airfield markers, lights and
`blk_light_pole`, which their names and sizes settle.
`directional_approach_lights_group`, a row of lights by name, is `wall` by its
thin box and stays that way, unreviewed. All 241 have a label; the user judged
`korpus_b1` and `korpus_c`, which are hard to tell. Family was judged only where
it changes whether a model settles.

## Heuristics

Scored on the 241 labels, rules alone, no overrides. Percent of models, then
the last column by objects.

| Rule added | building P / R | industrial P / R | misc P / R | Models right | Objects right |
|---|---|---|---|---|---|
| None | 65 / 98 | 100 / 74 | 89 / 35 | 73.9 | 89.6 |
| Slender: at most 7 m wide, 10 m or taller, is `misc` | 67 / 98 | 100 / 72 | 84 / 39 | 74.7 | 89.6 |
| Long: 5:1 or more and 30 m or longer is `misc` | 70 / 93 | 100 / 74 | 79 / 54 | 76.8 | 88.4 |
| **No family by name is `misc`** | **88 / 98** | **100 / 74** | **90 / 94** | **90.9** | **90.1** |

A model with no box, 0 x 0, is already `misc` by the 4 m² rule, so that rule
adds nothing, and it is wrong for `nasos`, a pump jack, and the two shelters.

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
domes: nothing in a name or box tells those. `industrial` recall stays at 74 %
because `garage_a_new`, `ferma_a_new`, `korpus_b1` and the like carry no
industrial name. Those are all overrides.

## What moved

Objects that changed class, by terrain, rule and overrides together.

| Terrain | Models | Objects | Of | `building` before → after | `misc` before → after |
|---|---|---|---|---|---|
| Afghanistan | 42 | 1 039 635 | 10 447 341 | 10 139 288 → 9 099 653 | 67 → 1 039 702 |
| Caucasus | 17 | 2 123 | 936 823 | 461 614 → 460 212 | 325 367 → 325 154 |
| GermanyCW | 98 | 59 736 | 8 687 758 | 7 828 128 → 7 768 392 | 190 079 → 249 815 |
| MarianaIslands | 18 | 11 778 | 191 999 | 157 476 → 145 698 | 26 330 → 38 108 |
| MarianaIslandsWWII | 12 | 8 051 | 51 008 | 47 900 → 39 849 | 1 729 → 9 780 |
| Nevada | 49 | 29 006 | 553 400 | 462 200 → 433 194 | 22 888 → 51 894 |
| PersianGulf | 55 | 271 966 | 2 941 637 | 2 285 042 → 2 013 076 | 311 522 → 583 488 |
| Syria | 122 | 441 068 | 6 328 726 | 5 471 009 → 5 029 941 | 604 538 → 1 045 606 |

Caucasus also moved 41 `industrial` objects to `misc`, lattice masts and
lighthouses, which are not plant, and 1 656 `korpus_b1` and `korpus_c` objects
from `building` to `industrial`. Those keep settling, in the industrial family. Places barely move: the rule touches only
models with no family, and only the override models lose one. `count_building`,
the built-up layers and the placement check's building figures do move. On
Afghanistan most of the change is debris, garbage and sand-block walls.

## What the review changed on Caucasus

Seventeen models changed class, and none of them among the 20 most placed: the
top of the catalog was already right. `korpus_a`, `_b`, `_a1`, `_d` and `korpus`
look more like offices than flats, but they settle either way, so their family
stands.

## Models with no box

The scene file gives some models a 0 x 0 box. `ukrytie` and
`air_closedguardhouse_03a` are earth-covered aircraft shelters, 340 objects, now
`building` in the military family by the user's judgment. An override fixes the
class only: with no box a shelter has no footprint, so a placement or a clearing
sees it as a point and can sit on top of it. It counts as built-up and does not
settle.

The other 24 Caucasus models with no box are parked vehicles, trailers and
`nasos`, 1 493 objects. Models with no box, placed, by terrain:

| Terrain | Models | Objects | What they are, by name |
|---|---|---|---|
| Afghanistan | 14 | 59 | Stones, vehicles |
| Caucasus | 26 | 1 833 | Shelters, vehicles, trailers, `nasos` |
| GermanyCW | 8 | 164 | Cones, trees, an aircraft, `rsbn` |
| MarianaIslands | 0 | 0 | |
| MarianaIslandsWWII | 0 | 0 | |
| Nevada | 0 | 0 | |
| PersianGulf | 7 | 57 282 | Lights |
| Syria | 8 | 340 | Generators, aircraft, `surface` |

A large structure among them is a footprint the tool cannot see, so each
terrain's review shoots these from far out and says which are large.
