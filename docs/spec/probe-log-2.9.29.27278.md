# Terrain probe log, DCS 2.9.29.27278, Caucasus

Single player, mission running, bridge `dcs_eval_in`. `hook` means the bridge's
own state with `require("terrain")`; `server` means the mission-scripting
state. All coordinates are DCS metres, x north, z east. Every figure below is
one machine's measurement on 2026-09-02.

## Terrain lifecycle

At the main menu `require("terrain")` loads and every function is present, but
`GetTerrainConfig("id")` returns nil. With the Mission Editor open on Caucasus
it returns `"Caucasus"`, before any mission runs. The editor is enough to load
a terrain for hook-side extraction.

## GetTerrainConfig from the hook

| Key | Value |
|---|---|
| `id` | `Caucasus` |
| `SW_bound` | `{-600, 0, -560}` (km) |
| `NE_bound` | `{380, 0, 1130}` (km) |
| `defaultBullseye` | `{blue={x=-291014,y=617414}, red={x=11557,y=371700}}` (`y` is z) |
| `seaEnabled` | `true` |
| `defaultcamera` | `{-355, 0.2, 618}` (km) |
| `SummerTimeDelta` | `4` |
| `defaultDateTime` | nil |
| `standDescriptionVersion` | nil |
| `Airdromes` | 21 entries |
| `getTerrainShpare()` | `FLAT` |

The bounds rectangle is 980 × 1690 km. The authored area from
`entry.lua` `nodesMapBorders` is x −418.6..26.4 km, z 113.7..943.2 km, so the
bounds describe the raster, not the authored hull. Only the Caucasus
`nodesMapBorders` bounds the authored area; the other theatres' values
(`MissionGenerator/nodesMap.lua`) do not, and the pre-sweep finds the
hull ("Plan measurement V0").

Airdrome sample: Kutaisi key 25 (`id` field `"KUTAISI"`, `code` `UGKO`)
`reference_point` (−284887, 683859), Batumi key 22 (−355811, 617386),
Sochi-Adler key 18 (−164496, 462219). The table key is the numeric
airdrome id and equals `Airbase:getID()` in the mission state (Kutaisi
25, Batumi 22); the entry's `id` field is a string. `towers`,
`warehouses`, `fueldepots` and `shelters` are arrays of
`"externalId:NNN"` strings. 21 entries, none `abandoned`. `roadnet` is
`./Mods/terrains/Caucasus/AirfieldsTaxiways/<Name>.rn4`.

## Height and surface, hook

`terrain.GetHeight(x, z)` returns a number.

| Loop | ms/call |
|---|---|
| 20 000 random points across the authored area | 0.0018 |
| 20 000 sequential points 0.5 m apart | 0.00045 |

`terrain.GetSurfaceType(x, z)` returns a **string**, not the `land.SurfaceType`
integer. Values seen over 60 000 samples: `land`, `sea`, `lake`, `river`. No
`road` or `runway` string appeared on a 20 km line through Kutaisi airfield,
so the hook classification is water-body only; road and runway come from
`land.getSurfaceType` in the mission state.

| Loop | ms/call |
|---|---|
| 40 000 random | 0.113 |
| 20 000 sequential 1 m apart | 0.00065 |

Random access is 170× slower than sequential for surface type; sweep
row-major.

`terrain.GetSurfaceHeightWithSeabed(x, z)` returns two numbers: land gives
`(height, 0)`; sea gives `(0, depth)` with depth positive, e.g. `(0, 63.48)`
off Batumi. This separates in-bounds sea from the out-of-bounds fill.

## Scenery

`terrain.getObjectsAtMapPoint(x, z)` is a point-in-footprint test: nil unless
the point lies inside an object's oriented box. 0.004 ms/call. A hit returns
an array of tables with `id` (a string of digits), `model` (lowercase type name),
`type` (65536, 65537 and 131072 seen; over 1 km² of Kutaisi city 203,
374 and 15 hits), `radius`, `rotation` (rad), and four positional arrays
`sizeOBB` (`[1]` w, `[2]` d), `center` (`[1]` x, `[2]` z), `boxMin`,
`boxMax`. Example: `concrete_wall_01` sizeOBB `{0.08, 4}`;
`sklad_new` sizeOBB `{41.4, 43.3}`; `school_a` id 70741507 center
`{-273176.5, 700435.4}` sizeOBB `{52.0, 52.3}` rotation 0.183.

A 5 m lattice over 1 km² of Kutaisi city (40 401 calls, 0.2 s) found 106
distinct objects across 30 models, all buildings, poles and wire.

`world.searchObjects(Object.Category.SCENERY, {id=world.VolumeType.SPHERE,
params={point=vec3, radius=r}}, handler, acc)` in `server` enumerates every
scenery object in the volume. Handler signature `(object, acc)`, return true
to continue. `getTypeName()` gives the uppercase model name; `getDesc()` has
`life`, `_origin`, `category`, `typeName`, `displayName`; `getLife()` 400 on a
warehouse. The object has no `getID` method; `getName()` returns the
numeric id as a Lua number, and for `SCHOOL_A` at (−273177, 700435) it
is 70741507, the same digits `terrain.getObjectsAtMapPoint` returns as
the string `id` for that building (`"70741507" == 70741507` is false
in Lua; compare `tostring` of both). `getPoint().y` is the ground
height (163.4 m).
`getCategory()` is 5 (`Object.Category.SCENERY`); `getDesc().category`
is 4.

| Centre | Radius | Objects | Models | Time |
|---|---|---|---|---|
| Kutaisi city | 500 m | 130 | 29 | <1 ms |
| Kutaisi city | 5 km | 6 354 | 99 | 21 ms |
| Kutaisi city | 20 km | 50 196 | 130 | 134 ms |
| Sochi hills (43.55 N 39.95 E) | 500 m | 0 | 0 | <1 ms |
| Sochi hills | 5 km | 824 | 38 | 5 ms |

The whole theatre's scenery is a few seconds of `searchObjects` calls.

**Trees are not scenery objects.** Two wooded hill sites (Sochi hills, Sataplia
near Kutaisi) return zero objects within 500 m by both methods, while the 5 km
sphere around the same Sochi point returns only houses, shops and light poles.
`BLK_LIGHT_POLE` is the most common model everywhere (14 960 of 50 196 in
20 km).

## getIP

`land.getIP({x, y=h+200, z}, {x=0, y=-1, z=0})` returns a vec3 at exactly the
`land.getHeight` value at the same point, in woods, in a city and at a
warehouse's own origin (162.95 m at the SKLAD_NEW centre, equal to ground).
The ray hits terrain only. A third length argument is accepted and changes
nothing. No canopy or structure height is reachable this way.

## land.profile

`land.profile(vec3, vec3)` returns an array of `{x, y, z}`:

| Segment | Points | Time | Gap min / median / max | max ǀy − getHeightǀ |
|---|---|---|---|---|
| 9.8 km hills | 127 | 5 ms | 1.8 / 59 / 258 m | 1.28 m |
| 2.0 km hills | 30 | <1 ms | 8 / 48 / 212 m | 1.88 m |
| 200 m hills | 3 | <1 ms | 63 / 102 / 102 m | 0.002 m |
| 9.9 km plain | 129 | 2 ms | 0.16 / 62 / 288 m | 1.46 m |

The first point is 20 m from the requested start, spacing is irregular, and
heights differ from `getHeight` by up to 1.9 m, so the points are crossings of
some coarser mesh, not the height posts. Not useful for the sweep: 200
`terrain.GetHeight` calls over the same 10 km cost 0.1 ms.

## Roads, hook

`terrain.getClosestPointOnRoads("roads", x, z)` returns `x, z`; 58 m snap from
Kutaisi city centre. `"railroads"` returns the nearest rail point (1.3 km).
0.26 ms/call on random points in a 60 km box.

`terrain.findPathOnRoads("roads", x1, z1, x2, z2)` from Kutaisi city to
Kutaisi airfield: 161 points `{x, y}` (`y` is z), 23 257 m, vertex spacing
4.7 m to 5 230 m, 16 ms. Endpoints are snapped: first point equals the
`getClosestPointOnRoads` snap of the start. `"railroads"` between the same
points returned nil.

## Visibility

`terrain.isVisible(x1, alt1, z1, x2, alt2, z2)` against an offline
straight-line check sampling `GetHeight` every 10 m, 300 random pairs 0.5 to
15 km apart at 2 m AGL in the Sochi hills: **300 of 300 agree**. DCS call
0.14 ms; offline 0.16 ms in Lua.

Buildings do not block it: across a 41 × 43 m warehouse in Kutaisi, points at
±60, ±100, ±200 m along one axis at 0.5, 2, 5 and 20 m AGL all report
visible. Terrain only.

## FARP siting example, live

20 × 20 km box around Kutaisi airfield, 100 m cells, footprint 120 × 80 m
tested at two orientations, criteria: surface `land`, height span across the
footprint ≤ 2.5 m, 200 m to 3 km from a road, ≥ 3 km from the airfield.
40 401 cells in 4.9 s from the hook (6 height, 1 surface, 1 road call per
surviving cell). Rejections: 1 220 not land, 3 170 too steep, 2 721 inside
the airfield exclusion, 7 205 outside the road band; 26 085 kept. Top 8 after
2 km non-maximum suppression, ranked by span then road distance:

| # | x | z | lat | lon | h (m) | span (m) | road (m) | airfield (m) |
|---|---|---|---|---|---|---|---|---|
| 1 | −286787 | 677859 | 42.16669 | 42.40710 | 35 | 0.01 | 233 | 6 294 |
| 2 | −275487 | 684259 | 42.26072 | 42.49869 | 76 | 0.13 | 218 | 9 409 |
| 3 | −287787 | 684759 | 42.15096 | 42.48816 | 40 | 0.12 | 233 | 3 036 |
| 4 | −283387 | 679459 | 42.19532 | 42.43074 | 40 | 0.15 | 211 | 4 649 |
| 5 | −287487 | 675859 | 42.16245 | 42.38229 | 33 | 0.16 | 206 | 8 412 |
| 6 | −294387 | 676359 | 42.10064 | 42.37913 | 40 | 0.14 | 235 | 12 104 |
| 7 | −282187 | 693859 | 42.19157 | 42.60435 | 83 | 0.17 | 222 | 10 358 |
| 8 | −280487 | 688559 | 42.21200 | 42.54335 | 74 | 0.15 | 257 | 6 438 |

The Rioni plain is flat, so slope discriminates little here. Second pass in
`server` (20 ms for all eight): scenery objects within 100 m and 500 m, and
ground exposure = fraction of 36 observers (12 bearings × 3, 6, 10 km, 2 m
AGL) that see the site at 2 m AGL.

| # | scenery 100 m | scenery 500 m | dominant models in 500 m | ground exposure |
|---|---|---|---|---|
| 1 | 0 | 65 | light poles 33, houses | 0.72 |
| 2 | 0 | 0 | — | 0.42 |
| 3 | 14 | 132 | light poles 34, houses | 0.78 |
| 4 | 0 | 46 | light poles 13, houses | 0.42 |
| 5 | 0 | 16 | poles, hangars, garages | 0.69 |
| 6 | 9 | 172 | light poles 46, houses | 0.44 |
| 7 | 0 | 65 | poles, chemical tanks, hangars | 0.78 |
| 8 | 1 | 138 | poles, chemical tanks, warehouses | 0.56 |

Candidate 2 is clear of scenery and among the least exposed; candidate 4 is
equally low-exposure with a village 500 m away. A designer would take 2 or 4
and reject 3 (buildings inside the footprint radius) and 7 (fully exposed).

## SAM siting example, live

Asset: Kutaisi airfield. Targets: the airfield centre plus 24 points on a
5 km ring, all at 100 m AGL. Candidates: 300 m lattice, 4 to 15 km from the
airfield, surface `land`, road within 500 m. Score: fraction of targets
visible from a 6 m mast, then fraction of 7 points on a 10 km western arc at
100 m AGL that cannot see the site. 1 979 candidates × 32 LOS calls in 0.9 s
from the hook.

| # | x | z | lat | lon | h (m) | road (m) | from asset (km) | coverage | masked from W |
|---|---|---|---|---|---|---|---|---|---|
| 1 | −293887 | 682059 | 42.09946 | 42.44780 | 48 | 472 | 9.2 | 1.00 | 1.00 |
| 2 | −294487 | 674859 | 42.10123 | 42.36110 | 47 | 274 | 13.2 | 1.00 | 0.71 |
| 3 | −294487 | 690459 | 42.08576 | 42.54717 | 77 | 435 | 11.6 | 1.00 | 0.71 |
| 4 | −277987 | 673059 | 42.24962 | 42.36141 | 64 | 131 | 12.8 | 1.00 | 0.43 |
| 5 | −286987 | 671259 | 42.17141 | 42.32800 | 32 | 147 | 12.8 | 1.00 | 0.43 |

Every candidate on the plain sees the whole 100 m AGL ring, so masking is the
discriminator. Site 1's western masking is real: the profile at bearing 270
rises to 108 m at 2 km from the 48 m site, above the sightline to a 137 m
target at 10 km; at 225 a 369 m ridge at 6 km does the same.

Unmasking point: an ingress at 100 m AGL from 60 km due west of the airfield
is first seen by site 1 at 7.8 km from the airfield.

## Sizing figures, server state

| Call | Result |
|---|---|
| `land.getSurfaceType` 20 001 sequential 1 m through Kutaisi airfield | LAND 19 156, WATER 557, ROAD 90, RUNWAY 198; 0.00075 ms/call |
| `land.getSurfaceType` 20 000 scattered | 0.078 ms/call |
| `land.getHeight` 20 000 sequential | 0.00045 ms/call, equal to the hook |
| `world.searchObjects` SCENERY sphere 150 km at map centre | 306 009 objects, 0.72 s |
| `world.searchObjects` SCENERY sphere 600 km at map centre | **919 641 objects, 1.34 s** — the whole theatre |

## Airfield, beacon and radio tables, hook

`getRunwayList("./Mods/terrains/Caucasus/AirfieldsTaxiways/KUTAISI.rn4")`:
one entry `{course=-1.85, edge1name="25", edge1x, edge1y, edge2name="07",
edge2x, edge2y}`; `getRunwayHeading` on the same file returns −1.8501 (rad).

`getStandList(roadnet, {"SHELTER","FOR_HELICOPTERS","FOR_AIRPLANES","WIDTH",
"LENGTH","HEIGHT"})`: 58 entries `{crossroad_index, flag, name, x, y,
params={FOR_AIRPLANES, FOR_HELICOPTERS, HEIGHT, LENGTH, SHELTER, WIDTH}}`.

`getBeacons()`: 164 entries `{beaconId, callsign, direction, display_name,
frequency (Hz), position={[1]=x, [2]=alt, [3]=z}, positionGeo={latitude,
longitude}, sceneObjects={"t:NNN"}, type, channel?}`; `beaconId` is a
string (`airfield12_0`); `channel` is present on 27 entries; `type`
values over the 164: 8 (64), 16408 (21), 16424 (21), 16640 (13), 16896
(13), 4 (6), 33024 (6), 33280 (6), 1 (5), 128 (4), 4136 (3), 4104 (2),
not the small type numbers (1 to 8) in older community notes alone.
`sceneObjects` uses the same `t:NNN` instance ids as
`notInstances.lua`.

`getRadio()`: 21 entries `{radioId, callsign, frequency={[0..3]={0, Hz}},
role={"ground","tower","approach"}, sceneObjects}`; `radioId` is a
string (`airfield12_0`); `callsign` is an array of `{<lang> = {name,
name}}` tables (`{{common = {"Anapa", "Anapa"}}}`, `{{nato = {"Batumi",
"Batumi"}}, {ussr = {"Druzhinnik", "Druzhinnik"}}}`); `frequency`
indices 0..3 hold 3.75, 38.4, 121 and 250 MHz at Anapa, i.e. HF, FM,
VHF, UHF.

## Projection check, Caucasus

The transverse Mercator row for Caucasus (`lon_0 = 33`, `k_0 = 0.9996`,
WGS84, easting offset 99517, northing offset 4998115, i.e. PROJ
`+proj=tmerc +lat_0=0 +lon_0=33 +k_0=0.9996 +x_0=-99517 +y_0=-4998115
+datum=WGS84 +units=m`, forward emits `(z, x)`) was checked against
eight live pairs from this session: three `convertLatLonToMeters` /
`coord.LLtoLO` inputs and five `convertMetersToLatLon` outputs. Maximum
error 0.5 m in either axis, which is the rounding of five-decimal
lat/lon. The row is correct on 2.9.29.27278. The other six theatre rows
and the fill sentinels were not re-measured in this session.

| lat | lon | DCS (x, z) | projected (x, z) | error x, z (m) |
|---|---|---|---|---|
| 42.26709 | 42.69685 | −272918, 700542 | −272918, 700542 | 0.0, 0.0 |
| 42.31000 | 42.67000 | −268401, 697778 | −268401, 697778 | −0.4, −0.2 |
| 43.55000 | 39.95000 | −152699, 461956 | −152699, 461956 | +0.2, +0.5 |
| 42.16669 | 42.40710 | −286787, 677859 | −286787, 677859 | −0.3, −0.3 |
| 42.10064 | 42.37913 | −294387, 676359 | −294387, 676359 | −0.2, +0.1 |
| 42.19157 | 42.60435 | −282187, 693859 | −282187, 693859 | −0.3, −0.4 |
| 42.08576 | 42.54717 | −294487, 690459 | −294487, 690459 | +0.3, −0.3 |
| 42.24962 | 42.36141 | −277987, 673059 | −277987, 673059 | −0.1, −0.3 |

## Scenery clearing by spawned statics

Mission state, `coalition.addStaticObject(country.id.RUSSIA, {name, type,
category, x, y=z, heading=0, dead=false})`, Kutaisi city. Counts are
`world.searchObjects` SCENERY spheres, radius as stated, centred at ground
height.

| Static | Site | Before | After | Nearest survivor |
|---|---|---|---|---|
| `FARP` / `Heliports` | A (−273278, 700392), on a warehouse | 18 in 120 m | 0 | 159 m (light pole) |
| `FARP_SINGLE_01` / `Heliports` | D (−272918, 700542), city centre | 9 in 150 m | 2 | 166 m |
| `FARP_SINGLE_01` / `Heliports` | C (−272700, 700700), sparse | 4 in 200 m, nearest 177 m | 4 | 177 m |
| `Hangar A` / `Fortifications` | B (−273000, 700100), housing | 18 in 120 m | 18 | — |

Rings around site A after the FARP: 0 objects to 150 m, 11 at 150–200 m,
16 at 200–250 m, then normal density. One removed object (a house at
167 m by centre) sat outside 150 m, so removal is by object extent
against a clearing zone, not by centre distance. Findings:

- Heliport-category statics clear scenery in about a 150 m radius,
  regardless of pad size. A building static clears nothing.
- Removal is immediate at spawn: the count changed within the same
  chunk that spawned the object.
- `terrain.getObjectsAtMapPoint` in the hook returns nil at the cleared
  warehouse's centre afterwards, so the terrain module and the mission
  state share one scene. An extract's scenery pass must run in a mission
  with no heliports placed.
- `StaticObject.destroy` on the FARP returns without error, `isExist`
  stays true (a FARP is an airbase), and the cleared scenery does not
  return.
- The spawned `Hangar A` appears under `Object.Category.STATIC`, the FARP
  does not.

## Projection, bounds and fill for every installed theatre

Mission Editor open on each map, no mission, hook state. Per map: `id`,
`SW_bound`/`NE_bound`, `defaultBullseye`, 20 `convertMetersToLatLon`
samples on a 4 × 5 lattice over the central 60 % of the bounds rectangle,
then `GetHeight`, `GetSurfaceType` and `GetSurfaceHeightWithSeabed` at
three points 500 km outside the rectangle. The samples are in
`projection-samples/<id>.txt` (`x z lat lon`) with `fit.py`, which fits a
transverse Mercator on WGS84 with `k_0 = 0.9996` and integer `lon_0`, then
refits `lon_0`, `k_0` and both offsets freely to confirm.

| `id` | `lon_0` | easting offset | northing offset | residual | SW_bound (km) | NE_bound (km) | fill height / surface / seabed | airdromes |
|---|---|---|---|---|---|---|---|---|
| `Caucasus` | 33 | 99517 | 4998115 | 0.5 m (8 live pairs) | −600, −560 | 380, 1130 | not re-measured | 21 |
| `SinaiMap` | 33 | −169222 | 3325313 | 0.000 m | −500, −280 | 500, 560 | 0 / `sea` / 100 | 56 |
| `Afghanistan` | 63 | 300150 | 3759657 | 0.000 m | −1180, −534 | 532, 757 | 5.000005 / `land` / 0 | 29 |
| `Syria` | 39 | −282801 | 3879866 | 0.000 m | −460, −400 | 380, 500 | 0 / `sea` / 100 | 225 |
| `PersianGulf` | 57 | −75756 | 2894933 | 0.000 m | −460, −900 | 800, 800 | 10.00001 / `land` / 0 | 30 |
| `GermanyCW` | 21 | −35427.62 | 6061633.128 | 0.000 m | −600, −1100 | 200, −300 | 5.000005 / `land` / 0 | 227 |
| `MarianaIslands` | 147 | −238418 | 1491840 | 0.000 m | −300, −800 | 1000, 800 | 0 / `sea` / 100 | 8 |
| `MarianaIslandsWWII` | 147 | −238418 | 1491840 | 0.000 m | −300, −500 | 1000, 500 | 0 / `sea` / 100 | 11 |

Offsets are as DCS applies them: PROJ `+x_0` is the negated easting offset
and `+y_0` the negated northing offset, and the forward direction emits
`(z_dcs, x_dcs)`. The free fit returned `k_0 = 0.9996000` and integer
`lon_0` on every map, so the model is exact, not approximate. Every fill
value equals the one recorded on 2.9.28; the sea-fill maps report seabed
100 outside the map, which is the third channel that separates fill from
in-bounds sea (in-bounds Black Sea off Batumi reads 63 to 96).

`defaultBullseye` is `{0, 0}` for both coalitions on `PersianGulf` and
`MarianaIslands`; a consumer cannot treat it as always meaningful.
`seaEnabled` is true and `getTerrainShpare()` is `FLAT` on all eight.

## Spec-review measurements, 2026-09-02

Same build, same machine, mission running (phase `sim`), taken while
reviewing the specs.

Hook state: `_VERSION` is `Lua 5.1`; `string.pack`, `bit` and `jit` are
nil. `io.open` works on install files, but the file handle's metatable
has only `close`, `flush`, `lines`, `read`, `write` (no `seek`);
`lfs.attributes(path, "size")` returns 6293651208 for
`Caucasus.surface5`; `f:read(1048576)` returns 1 MiB in 9.0 ms;
`lfs.currentdir()` is the install root and `lfs.writedir()` is Saved
Games; `loadfile`, `setfenv`, `os.clock`, `log.write`,
`DCS.setUserCallbacks`, `DCS.getModelTime`, `net.dostring_in` are
present. A loop of 2 000 000 iterations doing two table lookups, a
`math.floor` and three modulos runs in 115 ms (about 17 million
iterations per second).

`net.dostring_in("server", src)` returns two values: the chunk's return
value as a string and a boolean. `return 1, 2` gives `("1", true)`;
`error('boom')` gives the error message and `false`. A 65 536-byte
return takes under 0.1 ms and a 3 000 000-byte return 18 ms.

A full 256 × 256 `land.getSurfaceType` tile at 50 m around Kutaisi,
row-major, one chunk: 65 536 bytes in 147 ms. A 20 km
`world.searchObjects` sphere at Kutaisi airfield serialised to one line
per object (`getName`, `getTypeName`, `getPoint`): 3 175 900 bytes in
525 ms.

`terrain.GetHeight(−284887, 683859)` (Kutaisi reference point) is
45.010047; `land.getHeight` at the same point is 45.010048;
`GetSurfaceType` is `land`, `GetSurfaceHeightWithSeabed` is (45.01, 0),
`land.getSurfaceType` is 5 (RUNWAY).

`land.SurfaceType` is `{LAND=1, SHALLOW_WATER=2, WATER=3, ROAD=4,
RUNWAY=5}`; `Object.Category` is `{VOID=0, UNIT=1, WEAPON=2, STATIC=3,
BASE=4, SCENERY=5, CARGO=6}`; `world.VolumeType` is `{SEGMENT=0, BOX=1,
SPHERE=2, PYRAMID=3}`; `Airbase.Category` is `{AIRDROME=0, HELIPAD=1,
SHIP=2}`. `world.getAirbases()` returns 21 airbases, no helipads, and
`Airbase:getID()` equals the `Airdromes` table key (Batumi 22, Kutaisi
25).

`getRunwayHeading(Kutaisi.rn4)` is −1.8500649, equal to
`getRunwayList(...)[1].course`.

Fill inside the authored rectangle: a 1 km lattice over
`nodesMapBorders` (368 905 points, 6.6 s) reads exactly 5.000005 with
surface `land` at 11 843 points, all along the rectangle's edges (the
first hits are the column x = −418 119 for every z), and reads within
0.5 m of 5 m without being exact fill at 500 points. Outside the
rectangle but inside the bounds rectangle (x −600 000..−420 000,
z −560 000..100 000, 118 800 points) 113 780 are exact fill; the rest is
sea.

Terrain container headers, hexdump: `Caucasus.surface5` version 2,
header size 0x30, payload field 43 964 216 (file 6 293 651 208), class
`landscape5::Surface5File`; `Caucasus.rn4` payload 203 468 160 (equal to
the file size), `landscape4::lRoadNetwork`; `Caucasus.scn5` payload
11 888 248 (file 1 140 069 192), `landscape5::Scene5File`;
`Caucasus.routes` payload 978 211 232 (file 980 525 840),
`landscape4::lRoutesFile`; `AirfieldsTaxiways/Kutaisi.rn4` payload
183 288 equal to the file size. The payload field is the container's
own and is not the file size for `.surface5`, `.scn5` and `.routes`.

Terrain directories on this install: `Afghanistan`, `Caucasus`,
`GermanyColdWar`, `Kola`, `MarianaIslands`, `MarianasWWII`, `Nevada`,
`PersianGulf`, `Sinai`, `Syria`. Data files are named by theatre id
(`Sinai/Surface/SinaiMap.surface5`, `GermanyColdWar/roads/
GermanyColdWar.rn4`, `MarianasWWII/Scenes/MarianasWWII.scn5`). Every
installed theatre has `Map/towns.lua` and `MissionGenerator/nodes.lua`;
only Caucasus has `nodesMapBorders` in `entry.lua`. `towns.lua` begins
`local gettext = require("i_18n")` / `local _ = gettext.translate` and
sets the global `towns`; `nodes.lua` sets the global `missionNodes`
with `redPos`, `bluePos`, `name`, `id`, template lists.

`onSimulationFrame` fires at the main menu and in the Mission Editor,
and no callback fires during a mission load: the bridge's own hook
(`DcsApiEval.lua`, "The watch loop", D36) records that measurement and
ticks from every callback for that reason.

## Plan measurements X7a and C8a, 2026-09-02

Same build and machine, mission running, hook state.

**X7a, short-path cost.** 441 lattice seeds at 1 km in a 20 × 20 km box
around Kutaisi airfield, snapped with `getClosestPointOnRoads("roads",
...)`: 72 ms, 0.16 ms per call; 309 seeds snapped more than 500 m; 126
seeds have another seed's snap within 100 m. 200 `findPathOnRoads`
calls between each seed's snap and its nearest other snap (mean pair
distance 231 m, mean path length 1 367 m, mean 18.3 points, no nil):
122 ms total, 0.61 ms mean, 166 calls under 1 ms, 24 at 1 ms, 2 at 2 ms,
8 between 6 and 18 ms (`os.clock` resolution 1 ms). The 16 ms figure in
"Roads, hook" is the 23 km path; a 1 km path is 25× cheaper.

**C8a, scenery catalogue.** `world.searchObjects` SCENERY in a 600 km
sphere at (−110 000, 285 000): 888 859 objects, 263 distinct type
names, 2.8 s in the mission state; one example position per model, then
`terrain.getObjectsAtMapPoint` at that position in the hook gives a
footprint for 237 models and nil for 26 (parked vehicles and radars:
`GAZ-66`, `BTR-80`, `KAMAZ-*`, `URAL_*`, `RSP*`, `RSBN*`, `PRW-11`,
`RLS-37`, `ATZ-60`, plus `NASOS`, `UKRYTIE`, `MOST(ROAD)BIG_END`).
`BLK_LIGHT_POLE` is 295 779 objects (33 %). The full table is
`caucasus-scenery-catalogue.tsv` (model, count, life, `getDesc().category`,
`displayName`, OBB w, d, radius, `type`). `displayName` is empty for
every model except `GAZ-66` ("Truck GAZ-66", category 2) and `BTR-80`
("APC BTR-80", category 2); every other model has `category` 4. The
core spec's rules alone class 34.8 % of objects `misc`, 57.8 %
`building`, 7.4 % `industrial`, and no model `wall` (`concrete_wall_01`
from the 5 m lattice probe does not appear among the 263 names; see
"Objects the mission state cannot see"). `type` values seen: 65536,
65537 and 196608 (`IN_PAVEMENT_BI_DERECTIONAL_WHITE_WHITE`).

## Objects the mission state cannot see, 2026-09-02

Mission running on Caucasus. A 5 m lattice over 9 km² of Kutaisi city
through `terrain.getObjectsAtMapPoint` gave 78 distinct (`type`,
`model`) pairs; for each, a `world.searchObjects` SCENERY sphere of
15 m radius at the object's centre was asked for an object whose
`tostring(getName())` equals the hook `id`. Every `type` 65536 object
(28 models) and every `type` 65537 object (46 models) was found; none
of the four `type` 131072 objects was (`wire`, `powertranspole_rail_01`,
`kran-stroi`, `kran_bash`), nor in the STATIC, BASE or UNIT
categories within 25 m. `concrete_wall_01` (sizeOBB 0.08 × 4) was not
found on the 9 km² lattice this time; its `type` is not recorded, and
its absence from the 263-model catalogue matches the 131072 pattern.
The hook's `id` is a Lua string on every object (78 of 78); the mission
state's `getName()` is a Lua number.

Scenery sphere cost, one chunk per sphere returning one line per
object (`getName`, `getTypeName`, `getPoint`):

| Centre | Radius | Objects | Bytes | Server ms | Round trip ms |
|---|---|---|---|---|---|
| Kutaisi airfield | 20 km | 64 992 | 3 175 900 | 479 | 479 |
| Kutaisi airfield | 15 km | 38 408 | 1 870 667 | 225 | 226 |
| Tbilisi | 15 km | 25 118 | 1 256 553 | 353 | 355 |
| Sochi-Adler | 15 km | 7 749 | 379 509 | 297 | 297 |
| Batumi | 15 km | 12 167 | 597 506 | 279 | 282 |
| mountains (−200000, 780000) | 15 km | 297 | 14 931 | 4 | 4 |
| Kutaisi airfield, repeat | 20 km | 64 992 | 3 175 900 | 263 | 263 |

About 7 µs per object serialised; the bridge adds under 3 ms.

## Plan measurement V0: fill and hull on the land-fill theatres, 2026-09-02

Mission Editor open on each map, no mission, hook state. Per map: the
fill triple at 500 km outside the bounds rectangle; a lattice over the
whole bounds rectangle testing the exact triple per point (height
first, the other two calls on a match); and at named sites the
breakpoint count along a 2 km line sampled at 10 m (samples whose
second difference of `GetHeight` exceeds 1e-4) and the distance to the
nearest road snap. A lattice chunk holds the editor's frame for its
whole run (the bridge heartbeat stops), and the editor streams terrain
on first touch, so a 1 km lattice costs about 45 µs per point here
against 1.8 µs on a loaded mission.

| Map | Bounds km SW / NE | Fill triple | Lattice | Exact fill inside | Height 0 | Near-fill, not fill |
|---|---|---|---|---|---|---|
| `Afghanistan` | (−1180, −534) / (532, 757) | 5.000005 / `land` / 0 | 1 km, 2 210 192 pts, 99 s | 0 | 161 134 | 4 920 |
| `PersianGulf` | (−460, −900) / (800, 800) | 10.00001 / `land` / 0 | 2 km, 535 500 pts, 17 s | 0 | 120 838 | 1 196 |
| `GermanyCW` | (−600, −1100) / (200, −300) | 5.000005 / `land` / 0 | 1 km, 640 000 pts, 83 s | 3 at (−79500, −441500), (−8500, −478500), (−7500, −478500) | 142 568 | 3 258 |

Fill lies outside the bounds rectangle on all three; the rectangle is
the raster and the raster is real terrain, coarse beyond the hull.
`nodesMapBorders` exists on every theatre: `entry.lua` on Caucasus,
`MissionGenerator/nodesMap.lua` on the other seven, where Afghanistan's
is (−1180128, −534000, 532000, 756240), its own bounds rectangle, and
`GermanyColdWar`'s is (−696284, −1525514, 190500, 119030), larger than
its bounds; it is the node-map image extent, not the hull.

Breakpoints per 2 km (x-line; Afghanistan also z-line) and road
distance:

| Map | Site | DCS (x, z) | h (m) | Breakpoints | Road (m) |
|---|---|---|---|---|---|
| Afghanistan | Kabul | (78600, 266467) | 1803 | 169 / 165 | 25 |
| Afghanistan | Kandahar | (−258151, −43082) | 1025 | 159 / 146 | 35 |
| Afghanistan | Herat | (41597, −373725) | 932 | 164 / 137 | 40 |
| Afghanistan | Bagram | (125783, 271826) | 1492 | 102 / 96 | 18 |
| Afghanistan | Jalalabad | (75450, 385054) | 573 | 147 / 129 | 8 |
| Afghanistan | Mazar-i-Sharif | (309815, 67064) | 367 | 35 / 33 | 51 961 |
| Afghanistan | Islamabad | (15388, 637697) | 538 | 34 / 21 | 124 090 |
| Afghanistan | Zahedan | (−494436, −503721) | 1376 | 123 / 117 | 37 144 |
| Afghanistan | Quetta | (−411940, 85020) | 1666 | 157 / 131 | 79 584 |
| Afghanistan | Arabian Sea coast | (−961115, −400817) | 44 | 8 / 8 | 438 564 |
| Afghanistan | Turkmen desert | (561402, −559950) | 89 | 0 / 0 | 313 988 |
| Afghanistan | Tajik NE | (544653, 529081) | 90 | 0 / 0 | 283 079 |
| Afghanistan | 35.7 N 51.4 E (outside bounds) | (253645, −1351857) | 5.0 | 0 / 0 | 827 358 |
| PersianGulf | Kerman | (453772, 70947) | 1751 | 130 | 29 |
| PersianGulf | Shiraz | (381004, −351745) | 1487 | 72 | 213 |
| PersianGulf | Bandar Abbas | (114919, 13366) | 5.5 | 58 | 328 |
| PersianGulf | Dubai | (−101294, −89414) | 5.00 | 0 | 403 |
| PersianGulf | Muscat | (−285514, 206363) | 12 | 2 | 95 994 |
| PersianGulf | Doha | (−90269, −467510) | 2.2 | 3 | 184 640 |
| PersianGulf | Sur | (−397915, 335927) | 2.7 | 7 | 261 889 |
| PersianGulf | Riyadh (outside bounds) | (−94553, −967588) | 622 | 26 | 587 907 |
| GermanyCW | Berlin | (−215567, −479854) | 38 | 126 | 33 |
| GermanyCW | Hamburg | (−71606, −692692) | 3.0 | 114 | 8 |
| GermanyCW | Frankfurt | (−437691, −844247) | 104 | 124 | 54 |
| GermanyCW | Fulda | (−400389, −765519) | 267 | 102 | 63 |
| GermanyCW | Copenhagen | (141093, −493968) | 3.0 | 90 | 87 |
| GermanyCW | Bremen | (−110597, −780048) | 12 | 149 | 146 |
| GermanyCW | Leipzig | (−338557, −565195) | 115 | 159 | 86 |
| GermanyCW | Amsterdam | (−136484, −1056873) | 5.3 | 4 | 159 499 |
| GermanyCW | Prague | (−493467, −433746) | 219 | 6 | 122 944 |
| GermanyCW | Munich (outside bounds) | (−686704, −665004) | 524 | 22 | 190 871 |

Sea-fill theatres, same method, 2 km lattices: `Syria` bounds
(−460, −400) / (380, 500), fill 0 / `sea` / 100.000, 189 000 points in
41.5 s, exact fill 0, real sea 48 439 (depth off Cyprus 2 570.8 m);
`SinaiMap` bounds (−500, −280) / (500, 560), fill 0 / `sea` / 100.000,
210 000 points in 28.6 s, exact fill 0, real sea 76 253 (max depth
3 236 m); `MarianaIslands` bounds (−300, −800) / (1000, 800), fill
0 / `sea` / 100.000, 520 000 points in 0.9 s, exact fill 0, real sea
519 743 (max depth 8 944 m), not sea 257. In-bounds sea depth is not
bounded by 100 m; the fill test is exact equality of the triple.

| Map | Site | DCS (x, z) | h (m) | Breakpoints | Road (m) |
|---|---|---|---|---|---|
| Syria | Damascus | (−168750, 31074) | 701 | 113 | 28 |
| Syria | Aleppo | (127835, 117377) | 394 | 82 | 5 |
| Syria | Beirut | (−124389, −40909) | 56 | 73 | 14 |
| Syria | Haifa | (−244734, −92809) | 273 | 95 | 7 |
| Syria | Incirlik | (220967, −34901) | 58 | 60 | 226 |
| Syria | Nicosia | (26626, −231073) | 151 | 105 | 23 |
| Syria | Amman | (−340856, −7396) | 818 | 157 | 41 |
| Syria | Diyarbakir | (315533, 388303) | 674 | 79 | 385 |
| Syria | Palmyra | (−55380, 216749) | 412 | 101 | 71 |
| Syria | Deir ez-Zor | (30380, 387320) | 215 | 127 | 16 |
| Syria | Antalya (outside bounds) | (236233, −457390) | 59 | 2 | 78 740 |
| Syria | Mosul (outside bounds) | (149720, 653520) | 231 | 2 | 206 803 |
| SinaiMap | Cairo | (−790, −471) | 20 | 117 | 54 |
| SinaiMap | Tel Aviv | (225376, 337218) | 10 | 100 | 10 |
| SinaiMap | Sharm el-Sheikh | (−229548, 305923) | 31 | 50 | 267 |
| SinaiMap | Suez | (−9767, 125807) | 3 | 76 | 23 |
| SinaiMap | Alexandria | (130543, −124264) | 3 | 80 | 81 |
| SinaiMap | Aqaba | (−56921, 364010) | 46 | 105 | 68 |
| SinaiMap | Beersheba | (133377, 339672) | 263 | 108 | 29 |
| SinaiMap | Amman | (213330, 446180) | 796 | 143 | 90 |
| SinaiMap | Tabuk | (−180800, 520100) | 770 | 107 | 16 |
| SinaiMap | Asyut | (−317632, −11079) | 49 | 80 | 37 |
| SinaiMap | Luxor | (−483910, 133100) | 88 | 33 | 103 174 |
| MarianaIslands | Guam, Andersen | (10388, 14433) | 171 | 99 | 537 |
| MarianaIslands | Guam, Hagatna | (−1613, −5164) | 51 | 88 | 42 |
| MarianaIslands | Saipan | (187885, 104144) | 238 | 157 | 122 |
| MarianaIslands | Tinian | (166942, 91121) | 87 | 152 | 190 |
| MarianaIslands | Rota | (73222, 44133) | 403 | 172 | 550 |
| MarianaIslands | Pagan | (512056, 108287) | 30 | 190 | nil |
| MarianaIslands | open sea, two points | — | 0 | 0 | nil |

`getClosestPointOnRoads` returns nil on Marianas where no road is
reachable (Pagan, open sea), while on Afghanistan it returned snaps up
to 827 km away. Sharm el-Sheikh (50 breakpoints, road 267 m) and
Pagan (190 breakpoints, no road) are each caught by one of the two
rules and not the other.

Authored sites read 58 to 169 breakpoints with a road within 403 m;
unauthored sites read 0 to 35 with the nearest road 52 to 827 km away;
Dubai (0 breakpoints, flat) is caught by its road. Zahedan and Quetta
are detailed terrain without roads.

Post spacing on Cold War Germany, 4 km lines sampled at 5 m, distance
between consecutive breakpoints: Berlin n=262, min 5, median 5, max
325 m; Fulda 283 / 5 / 5 / 345; Harz hills 281 / 5 / 5 / 245; Leipzig
410 / 5 / 5 / 105; Amsterdam 5 breakpoints, max gap 800 m; Prague 5,
max gap 1 085 m; northern Jutland and eastern Poland no breakpoints.
The interior height function has curvature at nearly every 5 m step.

`GetSurfaceType` strings over a 4 km lattice of the Cold War Germany
bounds rectangle (40 000 points, 4.0 s): `land` 30 815, `sea` 8 914,
`lake` 252, `river` 19; no other string.

## Not measured

`Controller.isTargetDetected` through woods (moot: no vegetation objects
exist). Kola and Nevada are not installed. The `type` of
`concrete_wall_01` (not re-found on the 9 km² lattice). Marianas WWII
was not lattice-tested (it shares Marianas' projection and bounds
class).

## Crash record

The first DCS process of the session closed with "login expired" while a
`server`-state request (`world.searchObjects` + `land.isVisible`) was in
flight. DCS relaunched itself with `--restarted`; at 16:49:11, 0.1 s after
DcsApiEval registered its callbacks and while still at the main menu, that
process crashed: `ACCESS_VIOLATION` in `Terrain.dll createGlobalLand` under
`Scripting::regLuaGroup` under `lua_pcall`. The stale request was executed by
the new process with no terrain loaded. Rule: `server`-state `land`/`world`
calls only in phase `sim`, and clear the transport directory before a restart.
