# `airfield_info` on every installed terrain

Does every airdrome answer? Do its runways equal the tables? How far are
the true courses from the geodesic, the widths from a brute-force scan, and
what does a request cost? Measured 2026-10-30 on DCS build 2.9.30.28738
(`autoupdate.cfg` timestamp `20261006-075422`), Windows 11 Pro 10.0.26200,
release build, the query pool capped at 5 threads, with other builds and
test runs sharing the machine: times are upper bounds. Cycles are the
process's CPU cycles over the same span, across every thread.

## Method

The opt-in test `every_airdrome_answers_on_every_installed_terrain` in
`query/airfield_info.rs`, run with `DCSTERRAIN_INSTALL` set and a scratch
cache. The install holds 9 terrain folders; Kola's has no `entry.lua`, so
8 terrains answer. Every airdrome of each is asked for by its id, or by its
`name_id` where it has none, cold and then warm, and again by its
`name_id`.

- **Runways:** the test fails unless each runway's designator, grid course,
  length and thresholds equal its table row, the table's course equals the
  grid bearing between its own ends to 1e-9°, and each threshold's true
  course is within 1e-5° of the initial azimuth of Vincenty's inverse
  geodesic on WGS84 towards the other threshold.
- **Width:** at each of the nine stations, against a scan across the runway
  every 2 cm out to 75 m each side, bridging any stretch off the paint
  narrower than 1 m as the operation does. The test fails if one finds an
  edge the other does not, or if they differ by more than 3 cm.
- **Elevation:** against the highest of points every meter along every
  centerline. The test fails if the elevation is under the scan's; with no
  runway, unless it equals the highest ground at the stands.
- **Designators:** each threshold's true course less ten times its number;
  the terrain's median of that is the declination the designators imply,
  and each threshold's departure from it is recorded. There is no magnetic
  model; this is a cross-check of the files, not of the tool.
- **Times:** the open, then each request cold and warm, and the width
  search alone.

## Algorithm

Courses and lengths are the tables'. The **true course** is the normal
section's azimuth in the threshold's local east-north plane, through
earth-centered coordinates on WGS84, the datum the terrain's transverse
Mercator projection is fitted on. It departs from the geodesic's as the
square of the distance, under 1e-5° within 100 km and so under 1e-8° over
a runway. The **grid course** is from DCS x; they differ by the meridian
convergence, up to 11.2° on GermanyCW.

The **width** is measured on the mesh's Runway material, the one surface
the decoder reads. Across the runway at nine stations, a tenth to nine
tenths along, each side is stepped every 1 m to the first point off the
paint, then every 0.25 m back over that meter to where the first stretch off
it of 1 m or more begins, then bisected to 1 cm; a stretch narrower than
1 m is a seam between triangles, where a point falls through to the ground
beneath. The 1 m step lands in every stretch that is not a seam, so the
coarse step skips none. Each edge is within 5 mm, the bisection's half
bracket, so a station's width within 1 cm; a stretch off the paint 0.75–1 m
wide may be judged either way. The runway's width is the median of its
stations. The paint is not a runway outline: taxiways, aprons and shoulders share its material, so a
station where one meets the runway reads wide, and some runways carry no
paint. The median keeps the width a runway shows at most of its stations.

The **elevation** is exact over the triangles: the highest end of any span
between the crossings of the triangles under each centerline, the top
triangle's where they overlap, as the sightline reads them.

## Results

Every airdrome on every terrain answered, and every check held.

| terrain | airdromes | no id | no runway | runways | true off geodesic, max ° | convergence max ° | widths | width p50 (min–max) m | width off scan max m | station spread p50 / max m | declination from designators ° | designator off p95 / max ° | stands |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Afghanistan | 26 | 0 | 0 | 27 | 5.2e-9 | 4.26 | 26 of 27 | 38.5 (20.5–76.3) | 0.020 | 2.69 / 49.97 | +0.6 | 11.1 / 25.2 | 1 381 |
| Caucasus | 21 | 0 | 0 | 21 | 5.0e-9 | 8.10 | 21 of 21 | 60.0 (49.0–82.5) | 0.023 | 0.80 / 25.99 | +4.6 | 29.0 / 175.7 | 900 |
| GermanyCW | 227 | 108 | 109 | 130 | 6.9e-9 | 11.22 | 102 of 130 | 51.4 (9.5–108.7) | 0.024 | 0.30 / 47.65 | −1.9 | 9.6 / 177.7 | 6 701 |
| MarianaIslands | 8 | 3 | 3 | 7 | 5.7e-9 | 0.52 | 0 of 7 | none | — | — | +4.9 | 7.2 / 7.2 | 264 |
| MarianaIslandsWWII | 11 | 0 | 0 | 11 | 8.9e-10 | 0.55 | 1 of 11 | 44.1 | 0.016 | 55.18 | +1.2 | 10.0 / 10.0 | 512 |
| Nevada | 17 | 8 | 0 | 26 | 4.4e-9 | 1.77 | 23 of 26 | 54.0 (15.7–92.8) | 0.023 | 2.41 / 33.77 | +15.0 | 5.2 / 173.2 | 632 |
| PersianGulf | 30 | 2 | 0 | 36 | 1.6e-8 | 2.19 | 34 of 36 | 61.5 (36.1–104.2) | 0.023 | 1.85 / 50.19 | +0.7 | 3.8 / 5.4 | 1 305 |
| Syria | 225 | 144 | 145 | 105 | 6.9e-9 | 3.99 | 102 of 105 | 50.1 (20.9–121.7) | 0.024 | 0.96 / 98.28 | +2.0 | 7.5 / 13.4 | 3 992 |

All 565 airdromes answered, 363 runways equal to their rows, and all 726
thresholds lie on the runway paint. The elevation never stood under the
meter scan; it rose over it by 2 mm at most (Syria), a crest between two
samples. Every beacon type on every terrain has a name.

- **No runway:** 257 airdromes, all but three of them on GermanyCW and
  Syria, are helipads and FARPs with stands and no runway. They answer no
  runways, a null position, and the highest ground at their stands.
- **No id:** 265 airdromes have none. They are named by `name_id` and
  answer no navaids or radio, which follow from the id.
- **Widths:** where a station spread is large, a taxiway or apron meets the
  runway at a few stations; the median is the runway's at the rest. The
  Mariana terrains paint few runways with the Runway material, so most have
  none.
- **Designators against the course:** three runways are named for their
  opposite ends, 173–178° off: Caucasus TbilisiMilitary 14-32, GermanyCW
  Barth 27-09 and Nevada MinaAirport_3Q0 13-31. Caucasus Gelendzhik 01-19
  (29°), Afghanistan Zaranj 01-19 (25°) and GermanyCW Landstuhl 31-13 (24°)
  are far off too. 142 of 726 thresholds are more than 5° off their
  terrain's median, where a designator rounds to 10°. Nevada's designators
  imply +15.0° of declination; the rest −1.9° to +4.9°.

### Cost

| terrain | open ms / Mcycles | cold p50 ms / Mcycles | cold max | warm p50 | warm max | width search, share of warm |
|---|---|---|---|---|---|---|
| Afghanistan | 628 / 2 503 | 8.0 / 36.8 | 16.2 / 77.1 | 2.6 / 11.0 | 11.1 / 46.7 | 96 % |
| Caucasus | 280 / 1 079 | 4.0 / 16.8 | 7.9 / 33.2 | 1.0 / 4.4 | 4.6 / 19.2 | 94 % |
| GermanyCW | 2 100 / 8 682 | 3.4 / 14.5 | 22.6 / 120.5 | 1.5 / 6.5 | 9.5 / 38.6 | 78 % |
| MarianaIslands | 217 / 825 | 16.5 / 92.3 | 108.9 / 599.1 | 5.1 / 21.7 | 86.8 / 376.9 | 67 % |
| MarianaIslandsWWII | 116 / 398 | 9.7 / 39.4 | 43.5 / 181.7 | 1.5 / 6.8 | 14.1 / 57.8 | 92 % |
| Nevada | 246 / 940 | 2.5 / 11.7 | 10.4 / 46.6 | 1.7 / 7.3 | 9.6 / 40.7 | 98 % |
| PersianGulf | 489 / 1 959 | 3.4 / 14.7 | 7.0 / 40.8 | 1.4 / 6.0 | 3.4 / 14.7 | 86 % |
| Syria | 1 329 / 5 455 | 0.8 / 3.2 | 21.2 / 102.4 | 0.6 / 2.5 | 17.7 / 74.3 | 77 % |

The open is the terrain's into an empty cache; the tables then take
0–7 ms. "Cold" is the first request on an airdrome, its mesh blocks read;
"warm" the same request again. The width search is most of a request, so
it was measured before each change to it. Stepping 0.25 m all the way to
150 m each side: warm p50 0.6–15.9 ms, worst 114 ms. A first pass to 40 m,
then on only where the paint went on: p50 0.6–13.3 ms. A 75 m reach, past
which the paint is an apron's: p50 0.6–6.9 ms, worst 141 ms
(MarianaIslands). The 1 m coarse step with a 0.25 m look back: the figures
above, every width unchanged. The worst cold request is over the 20 ms
target on GermanyCW, Syria and both Mariana terrains; warm, on
MarianaIslands alone.
