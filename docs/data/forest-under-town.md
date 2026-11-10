# Forest under town: Caucasus

Measured on 2.9.31.29434 (`autoupdate.cfg` timestamp 20261027-141234),
Windows 11, DCS not running, a release build of
`cargo test --release -p dcsterrain-core --lib forest_under_town_on_caucasus -- --nocapture`
with `DCSTERRAIN_INSTALL` set. No `.crypt` file was opened.

## Why

The ground cover is the first of the triangle's bits to match, and `town`
(64) is tested before `forest` (1024). A triangle carrying both reads as
`town`, so a placement on it found no forest in its `cover`. A field report
placed a helicopter pad near Bakhmaro, at x -321 588, z 675 397, a 40 m
circle; `validate_placements` answered it `ok`, and in game it stood in
dense forest.

## Method

Each point's ground at the center of every fifth cell (250 m) across the
terrain, and every 2 m within 40 m of the pad and every 10 m within 500 m.
Water is left out. Forest is the forest bit on land, whatever cover wins;
town is the cover `town`.

## Result

| sample | land points | forest | town | forest under town |
|---|---|---|---|---|
| every fifth cell | 11 156 004 | 10.94 % | 4.13 % | 0.10 % |
| pad, 40 m | 1 257 | 86.32 % | 13.68 % | 0.00 % |
| pad, 500 m | 7 845 | 62.82 % | 43.10 % | 5.91 % |

Town covers 0.9 % of the terrain's forest ground (0.10 % of 10.94 %), and
2.4 % of its town carries forest. The pad itself stands on forest town does
not cover: the cover class saw it, and `validate_placements` only listed
it. Round the pad, in Bakhmaro, 6 % of the ground within 500 m is
forest under town.

## What it decided

`validate_placements` raises `forest` by the forest bit, not the cover
class, so forest under town counts. Whether DCS draws trees on a triangle
carrying both bits has not been looked at in game; counting it is the
cautious side, an issue a caller can clear with `allow_forest`.
