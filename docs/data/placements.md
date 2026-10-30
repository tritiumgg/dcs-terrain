# `validate_placements` on every installed terrain

Is a placement on a building, on water and on a runway each reported with
what it stands on, does a random batch of every shape agree with a
brute-force check, and what does a placement cost? Measured 2026-10-30 on
DCS build 2.9.30.28738 (`autoupdate.cfg` timestamp `20261006-075422`),
Windows 11 Pro 10.0.26200, release build, the query pool capped at 5
threads, with other builds and test runs sharing the machine: times are
upper bounds. Cycles are the process's CPU cycles over the same span,
across every thread.

## Method

The opt-in test
`buildings_water_and_runways_are_reported_on_every_installed_terrain` in
`query/placements.rs`, run with `DCSTERRAIN_INSTALL` set and a scratch
cache, over the 8 terrains with an `entry.lua` (Kola's folder has none).

- **Building:** a building with a box over 4 m a side, found near random
  points; a point at its box's middle must report it by id.
- **Water:** random points in point mode until one is a lake, else a
  river, else the sea; a point there must report that class at that point.
  On Caucasus it must be a lake.
- **Runway:** a point at every runway's middle must report pavement at
  0 m from the nearest centerline, and at least one must name its own
  runway (crossing runways share a middle).
- **Batch:** 300 placements, a third near a structure, a third within 80 m
  of a runway's centerline, the rest anywhere; each a point, a box, a
  circle or a star-shaped polygon of 3 to 8 corners, 2 to 60 m. Each is
  checked against a brute force with no broad phase and no separating
  axes: every cell in a box a cell wider than the shape, kept when its area
  inside the shape (Sutherland–Hodgman clipping) or its distance from a
  disc says so, its ground read point by point; every object of the three
  pools and every runway triangle in a box 2 km wider, kept by its distance
  alone. Cells, water classes and counts, cells with no ground, pavement,
  the object count and, up to 100, the object ids must agree exactly.
- **Times:** the batch cold (first) and warm (again, the same answer
  asserted).

## Algorithm and bounds

- **Broad phase:** the scene file's own bounding tree per pool, and the
  mesh's runway index, over the shape's box widened by 500 m, then each
  candidate's box against the shape's box.
- **Narrow phase:** separating axes over each convex pair's side normals,
  with a flat polygon's line direction added; where every axis's gap is
  under the tolerance, their exact distance, which also decides every pair
  with a disc or a concave polygon. Two shapes meet within `TOUCH_M`, 1 µm.
  Each pair is tested in the placement's frame, coordinates under 10 km,
  where f64 rounding is under 2e-12 m, so the tolerance decides touching.
- **Coverage:** a cell is read when the shape reaches more than 1 µm into
  it, so every cell the shape overlaps is read, and a shape lying along a
  cell's edge does not take its neighbor. A cell is judged at its center,
  as the raster is: what is reported lies within one cell, 71 m at most,
  of the shape, and nothing is missed in a cell the shape reaches but water
  or a slope narrower than a cell between two centers.

## Results

Run with `DCSTERRAIN_PLACEMENTS=1000`. Every check passed on every terrain.

| terrain | building | water | runway named at its middle | rows not runways | middles unpaved | batch agrees | with slivers | issues | cold ms / Mcycles | warm ms / Mcycles |
|---|---|---|---|---|---|---|---|---|---|---|
| Afghanistan | `parthouseafghanistan_04` | lake | Bagram 21L-03R | 2 | 0 | 1000 of 1000 | 12 | 4289 | 1755 / 9014 | 1720 / 8749 |
| Caucasus | `home51_hr` | lake | Anapa 22-04 | 0 | 0 | 1000 of 1000 | 9 | 1691 | 1014 / 4824 | 1017 / 4833 |
| GermanyCW | `german_07` | lake | Adelsheim 5-23 | 6 | 0 | 1000 of 1000 | 5 | 3562 | 2486 / 13928 | 2514 / 14036 |
| MarianaIslands | `1-floor_house_5` | sea | Andersen_AFB 24L-06R | 0 | 0 | 1000 of 1000 | 5 | 7378 | 1721 / 8324 | 1741 / 8454 |
| MarianaIslandsWWII | `rocks_small` | river | Agana_Airfield 07-25 | 0 | 0 | 1000 of 1000 | 10 | 6187 | 1441 / 7081 | 1467 / 7173 |
| Nevada | `power_trans_line_03` | lake | BeattyAirport 16-34 | 0 | 0 | 1000 of 1000 | 3 | 2841 | 756 / 3705 | 760 / 3738 |
| PersianGulf | `fishingboat_01` | lake | Bandar_Abbas_airfield 03R-21L | 13 | 0 | 1000 of 1000 | 8 | 1572 | 1479 / 6957 | 1481 / 6973 |
| Syria | `israel_house_03` | lake | Abu_al-Duhur 27-09 | 1 | 2 | 1000 of 1000 | 3 | 2662 | 4375 / 9103 | 1464 / 7768 |

- **Classes, not placements:** the class rules call some models buildings
  that are not, such as a fishing boat, a power-line part and rocks on the
  Marianas. A placement reports what the scene places, whatever its class.
- **Rows not runways:** the tables list runway rows whose ends are stands,
  such as Abu Dhabi's `Stand03-Stand04`; there is no pavement under their
  middles, and a pavement issue may name one as the nearest centerline.
- **Middles unpaved:** two Syrian runways, Tel Nof 15-33 among them, are
  paved in the mesh only at their ends; the point's own ground there is
  plain, and no pavement is reported.
- **Slivers:** a shape reaching a cell by under 0.01 m², or a disc within a
  millimeter of a cell's corner, is left unjudged by the brute force; the
  cell count must lie between the two judgments, and the ground under it
  is not compared. Before the oracle clipped in each cell's own frame, a
  2.8 mm corner read as no area at 5e5 m from the origin; the operation,
  which tests in the shape's frame, had it right.

Time by step, warm, 1 000 placements, each step alone:

| terrain | objects pool ms / Mcycles (read per placement) | lines | trees | objects met, all pools | pavement |
|---|---|---|---|---|---|
| Afghanistan | 211 / 886 (528) | 299 / 1253 (1091) | 308 / 1289 (947) | 927 / 3881 | 91 / 382 |
| Caucasus | 362 / 1518 (54) | 63 / 264 (153) | 153 / 642 (284) | 624 / 2615 | 39 / 162 |
| GermanyCW | 138 / 577 (95) | 305 / 1278 (927) | 139 / 582 (186) | 659 / 2760 | 427 / 1787 |
| MarianaIslands | 97 / 406 (124) | 205 / 858 (952) | 556 / 2329 (2513) | 979 / 4102 | 157 / 659 |
| MarianaIslandsWWII | 40 / 169 (47) | 61 / 254 (274) | 595 / 2494 (2814) | 776 / 3251 | 127 / 532 |
| Nevada | 76 / 317 (166) | 226 / 948 (1101) | 56 / 233 (147) | 633 / 2653 | 3 / 13 |
| PersianGulf | 852 / 3568 (84) | 112 / 470 (229) | 104 / 434 (227) | 1105 / 4631 | 46 / 192 |
| Syria | 123 / 512 (128) | 202 / 845 (508) | 138 / 576 (271) | 505 / 2107 | 202 / 843 |

The rest of a request, 0.2 to 1.6 s, is the ground: each cell's point and
the slope over the tiles under scattered placements, which `point_info`
pays too.

## Optimization

Measured first, at 300 placements. The runway surface first read every
block under each placement's box, preparing blocks with no runway subset,
and placements scattered over the terrain turned the 32 MiB block budget
over: 1.8 s of MarianaIslands' 2.2 s warm. Two changes, each measured:

- Skipping blocks whose table of contents lists no runway subset: 2.2 s to
  1.9 s warm on MarianaIslands, 1.3 s to 1.2 s on GermanyCW.
- Reading the boxes block by block, each block prepared once for every box
  over it, as `points` does: MarianaIslands 1.9 s to 0.48 s,
  MarianaIslandsWWII 1.6 s to 0.51 s, GermanyCW 1.2 s to 1.0 s.

Answers were unchanged by both: the brute force agreed throughout.
