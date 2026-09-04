# ADR 0007: Extract format v1 is frozen, with measured field sources and a separate code for an unrecognised surface string

## Status

Accepted

## Context

**Affects:** `extract-format.md` "Tile binary layout", "Tables" and
"manifest.json"; `design-and-facts.md`, the `Airdromes` paragraph;
`probe-log-2.9.29.27278.md`, the Caucasus fill row; `plan.md` tasks F1, X1,
X5 and C5c.

Task F1 freezes the extract format at `format_version` 1. The freeze needs
every field the format emits traced to the DCS call that produces it, and
every field `pack` reads traced back to a field the format emits.

`extract-format.md` names the tables and their keys but not, in several
places, where a key's value comes from. `design-and-facts.md` lists the
`Airdromes` entry keys, and `probe-log-2.9.29.27278.md` records the beacon,
radio, runway and stand tables. Neither records the shape of an airdrome's
sub-tables, the town and node table entries, or the Caucasus fill triple,
whose probe log row reads "not re-measured".

The measurements below are from the hook state with the Mission Editor open,
no mission running, on **DCS 2.9.29.27468** (`timestamp` 20260902-093323),
first on Caucasus and then on Syria. The probe log is 2.9.29.27278.

**Airdrome entries.** All 21 Caucasus entries carry `abandoned`, `beacons`,
`civilian`, `class`, `code`, `default_camera_position`,
`default_camera_position_geo`, `display_name`, `fueldepots`, `id`, `names`,
`projectors`, `radio`, `reference_point`, `reference_point_geo`, `roadnet`,
`roadnet5`, `runwayName`, `runways`, `towers` and `warehouses`. `shelters` is
present on 10 of the 21. Kutaisi (key 25) reads `id = "KUTAISI"`,
`code = "UGKO"`, `display_name = "Kutaisi"`, `names = {en = "Kutaisi"}`,
`class = "1"` (a string), `civilian = true`, `abandoned = false`,
`reference_point = {x = -284887.375, y = 683858.71875}` and
`reference_point_geo = {lat = 42.177616, lon = 42.481292}`.

**Sub-table bases differ within one entry.** Kutaisi's `runways`,
`runwayName`, `beacons` and `projectors` are keyed from 0; its `towers` and
`radio` are keyed from 1. The `json()` encoder of `extractor-hook.md` emits
"arrays for tables with consecutive integer keys from 1, objects otherwise",
so a 0-based table encodes as the object `{"0": …, "1": …}`.

**Syria, 225 airdromes, same build, Mission Editor open on the map.** The key
set is the same, and the optional keys are absent in bulk rather than
occasionally: `towers` and `radio` are present on 81 entries, `warehouses` on
77, `fueldepots` on 74, `shelters` on 48. Empty tables are the norm too —
`runways` and `runwayName` are empty on 145 of the 225, `beacons` on 190, and
`projectors` on all 225. The bases match Caucasus: `runways`, `runwayName`
and `beacons` are keyed from 0 where they are not empty, and `radio`,
`towers`, `warehouses`, `fueldepots` and `shelters` from 1.

**Cold War Germany, 227 airdromes, same build.** The bases are the same
again, and the optional keys are absent in bulk again: `towers` and `radio`
on 119 entries, `shelters` on 98, `fueldepots` on 90, `warehouses` on 88,
`runways` and `runwayName` empty on 108, `beacons` empty on 150,
`projectors` empty on all 227. The key set is not fixed across theatres:
Hamburg, alone among the 227 and absent from the other two theatres' 246
airdromes, carries a `zone` of `"GERMANYCW_terrain_32"`.

**The runway name is the edge names joined by a hyphen.** `getRunwayList` is
a 1-based list whose entries hold only `course`, `edge1name`, `edge1x`,
`edge1y`, `edge2name`, `edge2x` and `edge2y`, and an entry's `runways` holds
`{id, name, start, end}`. `runways[k]` corresponds positionally to
`getRunwayList[k + 1]`, and the `name` equals `edge1name`, a hyphen and
`edge2name` — including where the pair reads backwards, as Adana's
`"23-05"` does. Cold War Germany compares 131 runways over its 119 airfields
that have one with no mismatch; Syria's 21 airfields, 19 of them with more
than one runway, and Caucasus's Kutaisi agree by inspection.
`getRunwayList` parses the `.rn4` by path and answers for a theatre that is
not loaded, which is how the Syria list was read while Caucasus was open.

The pair does not identify a runway. Hatzor carries both `"11-29"` and
`"29-11"`, and Tel Nof carries `"33-15"` twice; `runways[k].id` does not
disambiguate them either, since Ben Gurion's ids run 2, 3, 1 and Hatzerim
gives two runways the same id 1.

**Every installed theatre's runway lists, read without loading one.** The
605 `.rn4` files of the eight installed theatres return 449 runways in
0.40 s. The list is keyed from 1 in each of the 350 files that return one,
and returns empty for the rest: 145 of Syria's 225 and 108 of Cold War
Germany's 227, one on Marianas, one on Sinai, none elsewhere. Syria's 145
are exactly the airdromes whose `runways` table is empty, so the two sources
agree on which airfields have no runway. Every runway name is one or two
digits with an optional `L`, `R` or `C`, and the numbers are not zero-padded:
Chaghcharan has `7`, Kandahar Heli `5L`, Al Ain International `1`. Sinai has
the widest field, with four runways.

**An airdrome's `beacons` holds tables, not ids.** Kutaisi's entries are
`{beaconId = "airfield25_0", runwayId = 1, runwayName = "07-25",
runwaySide = "07"}`; its `radio` is `{[1] = "airfield25_0"}`, plain strings.
`design-and-facts.md` calls both "beaconId lists".

**Towns and nodes.** `Map/towns.lua` sets `towns`, a table keyed by town name
with entries `{display_name, latitude, longitude}`; Caucasus has 1691, every
one carrying all three keys, and every `display_name` equal to its key under
an identity `translate`. `MissionGenerator/nodes.lua` sets `missionNodes`, a
contiguous array of 99 entries `{id, name, redPos, bluePos, redTemplates,
blueTemplates}`; the ids are unique and are not the array index, and `redPos`
and `bluePos` are positional arrays `{[1] = x, [2] = z}` in DCS metres, not
`{x, y}` tables. Node 1 reads `redPos = (-235357.1, 638342.9)`, which
`GetHeight` puts at 227.17 m and `convertMetersToLatLon` at 42.66229 N
42.00046 E. Both shapes hold on all eight installed theatres, read without
loading them: every `towns` table is keyed by name with all three keys
present on every entry, from 22 towns on Marianas WWII to 1691 on Caucasus,
and every `missionNodes` is a contiguous array with positional positions and
no duplicate id, from 3 nodes on Marianas to 99 on Caucasus. The directory
holding `towns.lua` is `Map` on four theatres and `map` on four, which
matters to nothing because the extractor takes the path from its config.

**Beacons and radio.** `getBeacons()` returns 164 entries whose union of keys
is `beaconId`, `callsign`, `channel`, `chartOffsetX`, `direction`,
`display_name`, `frequency`, `position`, `positionGeo`, `sceneObjects` and
`type`; `chartOffsetX` is not in the probe log. `getRadio()` returns 21
entries keyed `callsign`, `frequency`, `radioId`, `role` and `sceneObjects`.

**The Caucasus fill triple.** `GetHeight` 5.000005, `GetSurfaceType` `land`,
`GetSurfaceHeightWithSeabed` (5.000005, 0) at each of three points 500 km
outside the bounds rectangle, all three agreeing. Because the seabed return
is 0 for fill and 0 for real land, the seabed channel separates nothing on a
land-fill theatre; it discriminates only where the fill is sea and returns
100 against a real depth.

**The surface strings.** A 100 × 100 lattice over `nodesMapBorders` returns
`land` 5713, `sea` 4222, `lake` 51, `river` 14 and no other string, which is
the set the probe log recorded on the earlier build.

**The terrain files did not change between the two builds.**
`Caucasus.surface5` is 6 293 651 208 bytes with payload field 43 964 216,
`Caucasus.rn4` 203 468 160 with payload 203 468 160, `Caucasus.scn5`
1 140 069 192 with payload 11 888 248 — each equal to the probe log's figure
on 2.9.29.27278.

**One code carries two meanings.** The layer table gives `water` "255 unknown
string" and `nodata` 255, and the same section says "Unknown `water` strings
are logged once per distinct string and encoded 255; `pack` reports their
count". A fill cell and a cell whose surface string is unrecognised are then
the same byte, and `pack` can separate them only by reading the `height`
layer at the same cell.

## Decision

`format_version` 1 is `extract-format.md` as written, with the following.

**`water` codes are 0 land, 1 lake, 2 sea, 3 river, 254 unrecognised string,
255 nodata.** The extractor maps the `GetSurfaceType` string through an exact
table, and encodes 254 for any string not in it, logging each distinct
unrecognised string once. `nodata` stays 255 and means fill or a cell outside
the grid. `pack` counts 254 cells to report unrecognised strings, and treats
254 as water for `dist_water` as it treats every non-zero code.

**The extractor normalises DCS lists before encoding, and `json()` is
unchanged.** A DCS table whose keys are consecutive integers from 0 or from 1
is copied to a Lua array indexed from 1, in ascending key order, before it
reaches the encoder. This applies to `runways`, `runwayName`, `beacons`,
`radio`, `towers`, `warehouses`, `fueldepots`, `shelters`, the
`getRunwayList` and `getStandList` results, `sceneObjects`, and
`missionNodes`. `json()` keeps its rule of arrays from 1 and objects
otherwise, so its tests are unaffected. A normalised list that DCS returns
empty is written as `[]`, not `{}`; on Syria 145 of 225 airdromes have an
empty `runways`.

**A key DCS omits is written as JSON `null`, and a key it adds is ignored.**
The extractor records the keys the format names and invents nothing. Syria
and Cold War Germany measure how common the omissions are: `shelters` is
present on 48 of Syria's 225 airdromes and 98 of Germany's 227, `towers` and
`radio` on 81 and 119. The addition is measured too — one Cold War Germany
airdrome carries a `zone` that no other airdrome on any theatre has, and it
does not reach the extract.

**Field sources.** Each extract field comes from the call and key below.

| File and field | Source |
|---|---|
| `airdromes.json` `id` | the numeric key in `GetTerrainConfig("Airdromes")` |
| `name_id`, `code`, `display_name`, `names`, `civilian`, `abandoned`, `class`, `roadnet`, `roadnet5` | the entry's `id`, `code`, `display_name`, `names`, `civilian`, `abandoned`, `class`, `roadnet`, `roadnet5`; `class` is a string |
| `x`, `z` | `reference_point.x`, `reference_point.y` |
| `lat`, `lon` | `reference_point_geo.lat`, `.lon` |
| `runway_names` | `runwayName`, normalised |
| `beacon_ids` | the `beaconId` of each entry of `beacons`, in key order |
| `radio_ids` | `radio`, normalised |
| `towers`, `warehouses`, `fueldepots`, `shelters` | the same keys, normalised |
| `runways.json` one row per `getRunwayList(roadnet)` entry | `edge1_name`, `edge1_x`, `edge1_z`, `edge2_name`, `edge2_x`, `edge2_z`, `course` from `edge1name`, `edge1x`, `edge1y`, `edge2name`, `edge2x`, `edge2y`, `course` |
| `runways.json` `name` | `edge1name .. "-" .. edge2name`, which equals the `name` DCS gives the same runway in the airdrome's `runways` |
| `towns.json` `name` | the table key in `towns` |
| `display_name`, `lat`, `lon` | `display_name`, `latitude`, `longitude` |
| `towns.json` `x`, `z` | `convertLatLonToMeters(lat, lon)` |
| `nodes.json` `id`, `name` | the entry's `id` and `name`; the id is unique and is not the array index |
| `nodes.json` `red`, `blue` | `{x = redPos[1], z = redPos[2]}` and the same for `bluePos` |
| `config.json` `fill` | `GetHeight`, the encoded `GetSurfaceType` class and the second return of `GetSurfaceHeightWithSeabed`, at three points 500 km outside the bounds rectangle |

`config.json` also carries `presweep` when the pre-sweep ran, as
`extract-format.md` describes under `authored_bounds_m`. The extractor does
not record `default_camera_position`, `default_camera_position_geo`,
`projectors` or a beacon's `chartOffsetX`; nothing reads them.

**`terrain_fingerprint.digest` is the first 8 hex characters of the SHA-256
over the three `head_sha256` values, as lowercase hex strings, concatenated in
the order `surface5`, `rn4`, `scn5`.** On Caucasus at the build above the
three are
`bc2a8ca3cb0d6627451291f3208a4e8e036ead04bc6fa4f6e7f4c95bc4c749aa`,
`535573addea0602a99fec639464b9dd5b92220cc55b16f8c7647a66b479a461d` and
`f3f456954a181faa13afd7fa3f080d5fc2d82b78eee101eab15c78aebf3490b7`, and the
digest is `cc3c003b`. Those four values are X2b's test vector.

**The mission pass applies no fill test.** The fill triple is a hook-state
measurement, so `surface` may hold a real value in a cell whose `height` and
`water` are `nodata`. Readers use `valid`, and the extractor masks nothing.

**The manifest example.** The example in `extract-format.md` is replaced by
the one below, which is Caucasus at the build above with the measured
`nodesMapBorders` and the grid computed from it. `authored_bounds_m` is
`{-418619.1875, 113728.15625, 26382.5, 943187.0625}` read from `entry.lua`;
snapping outward to a multiple of 50 m gives origin (−418650, 113700), and
extents 8901 × 16590, which is 35 × 65 = 2275 tiles of 256 cells.

```json
{
  "format_version": 1,
  "extractor_version": "0.1.0",
  "theatre": "Caucasus",
  "dcs_build": "2.9.29.27468",
  "dcs_build_timestamp": "20260902-093323",
  "terrain_fingerprint": {
    "surface5": { "path": "Mods/terrains/Caucasus/Surface/Caucasus.surface5", "size": 6293651208, "payload_size": 43964216, "head_sha256": "bc2a8ca3cb0d6627451291f3208a4e8e036ead04bc6fa4f6e7f4c95bc4c749aa" },
    "rn4":      { "path": "Mods/terrains/Caucasus/roads/Caucasus.rn4",       "size": 203468160,  "payload_size": 203468160, "head_sha256": "535573addea0602a99fec639464b9dd5b92220cc55b16f8c7647a66b479a461d" },
    "scn5":     { "path": "Mods/terrains/Caucasus/Scenes/Caucasus.scn5",     "size": 1140069192, "payload_size": 11888248, "head_sha256": "f3f456954a181faa13afd7fa3f080d5fc2d82b78eee101eab15c78aebf3490b7" },
    "digest": "cc3c003b"
  },
  "extracted_at": "2026-09-04T09:12:44Z",
  "bounds_km": { "sw": [-600, -560], "ne": [380, 1130] },
  "authored_bounds_m": { "min_x": -418619.1875, "min_z": 113728.15625, "max_x": 26382.5, "max_z": 943187.0625 },
  "authored_bounds_source": "config",
  "crop_m": null,
  "grid": {
    "cell_size": 50,
    "origin_x": -418650,
    "origin_z": 113700,
    "height": 8901,
    "width": 16590,
    "tile_size": 256
  },
  "omit_sea_tiles": true,
  "layers": {
    "height":  { "dtype": "i16", "nodata": -32768, "unit": "m",     "pass": "hook" },
    "water":   { "dtype": "u8",  "nodata": 255,    "unit": "class", "pass": "hook" },
    "surface": { "dtype": "u8",  "nodata": 0,      "unit": "enum",  "pass": "mission" }
  },
  "passes": {
    "hook":    { "complete": true,  "started_at": "2026-09-04T09:12:44Z", "finished_at": "2026-09-04T09:41:02Z", "frames": 101884 },
    "mission": { "complete": false, "started_at": null, "finished_at": null, "frames": 0 }
  },
  "tiles": [
    { "layer": "water",  "tx": 0, "tz": 0, "path": "tiles/water/0_0.bin",  "min": 2, "max": 2 },
    { "layer": "water",  "tx": 4, "tz": 9, "path": "tiles/water/4_9.bin",  "min": 0, "max": 3 },
    { "layer": "height", "tx": 4, "tz": 9, "path": "tiles/height/4_9.bin", "min": -3, "max": 412 }
  ],
  "tables": {
    "config": "config.json", "airdromes": "airdromes.json", "runways": "runways.json",
    "stands": "stands.json", "beacons": "beacons.json", "radio": "radio.json",
    "towns": "towns.json", "nodes": "nodes.json",
    "roads": "roads.jsonl", "railroads": "railroads.jsonl", "scenery": "scenery.jsonl",
    "scenery_models": "scenery_models.json"
  },
  "timing_ms": { "presweep": 0, "water": 98400, "height": 71020, "roads": 612000 },
  "notes": []
}
```

Tile `(0, 0)` is sea, so it appears for `water` and not for `height`, which is
`omit_sea_tiles` at work; tile `(4, 9)` is coastal Rioni and carries both.

## Consequences

X1 gains one encoder case that is not in its done test: the normalisation of
a 0-based DCS list. It belongs with the encoder tests because it is where an
airdrome's `runway_names` stops being an object keyed `"0"`.

X5 writes `beacon_ids` by reaching into each `beacons` entry rather than
copying a list. It names runways from the `getRunwayList` entry alone and
never reads the config table's `runways`. An airfield with no runway gets no
`runways.json` row, which is what both sources say: Syria returns runways for
80 of its 225 `.rn4` files and an empty list for the other 145, exactly the
split of its `runways` tables, and Cold War Germany splits 119 to 108. Those
airfields are heliports and strips.

`pack` counts unrecognised surface strings from the `water` layer alone. The
cost is that the extractor's code table is one value further from the DCS
string set, and any reader that treats every non-zero `water` code as a water
body — `dist_water` does — counts an unrecognised cell as water. No fifth
string has been seen in 110 000 samples across two theatres and two builds.

`design-and-facts.md` now reads false where it calls an airdrome's `beacons`
a beaconId list, and its key list omits `default_camera_position`,
`default_camera_position_geo` and `projectors`. The probe log's Caucasus fill
row still says "not re-measured"; the triple is 5.000005 / `land` / 0 and is
recorded here.

The probe log stands as measured. Nothing in it differs on 2.9.29.27468, so
no new probe log is written; the figures above are new measurements, not
corrections, and the terrain files are identical between the two builds.

F2 takes the fill triple, the water codes and the layer table from this
record. C3 validates against it, and C5c reads the table fields it names.

### Alternatives considered

**Leave `water` 255 with both meanings.** No divergence, and `pack` separates
fill from an unrecognised string by reading `height` at the same cell.
Rejected: the disambiguation is implicit, has to be restated wherever the
water layer is read, and the format asks `pack` for a count it cannot take
from the layer it is counting.

**Take the runway name from the airdrome's `runways`, joining on the edge
name pair.** It uses the name ED wrote rather than one the extractor builds.
Rejected: the pair does not identify a runway — Hatzor has `"11-29"` and
`"29-11"`, Tel Nof has `"33-15"` twice — and 145 of Syria's 225 airdromes
have no `runways` to join to. The positional correspondence would work, but
it buys nothing when the composed name is identical in all 22 airfields
measured.

**Emit 0-based DCS tables as JSON objects.** The encoder is already
consistent about it, and a consumer can read `{"0": …}`. Rejected: the same
field would be an array on one airdrome and an object on another as soon as a
theatre keys a list from 1, and `pack` would carry the ambiguity into every
table read.

**Change `json()` to treat keys from 0 as an array.** Fewer call sites to
touch. Rejected: it makes the encoder's output depend on a property of the
data that no caller states, so a table legitimately keyed `{0 = …}` would
silently lose its keys.
