# Spec: extract format

The directory the extractor writes and `dcsterrain pack` reads. It is the
only contract between the Lua side and the Rust side, and the hand-off
between the Windows machine that runs DCS and any machine that runs
`dcsterrain`. Both sides implement this file exactly; neither reads the
other's code. Background: `design-and-facts.md`.

## Directory layout

```
<extract>/
  manifest.json
  tiles.jsonl              (append-only tile journal)
  config.json
  airdromes.json
  runways.json
  stands.json
  beacons.json
  radio.json
  towns.json
  nodes.json
  roads.jsonl
  railroads.jsonl
  scenery.jsonl            (mission pass)
  scenery_models.json      (mission pass)
  tiles/
    height/<tx>_<tz>.bin
    water/<tx>_<tz>.bin
    surface/<tx>_<tz>.bin  (mission pass)
```

Every path inside the directory is relative, forward-slash, ASCII. Files
are written as `<name>.tmp` and renamed into place; a `.tmp` left behind
means an interrupted write and `pack` ignores it.

## Coordinate conventions

DCS metres. `x` is north, `z` is east. Every JSON object in this format
uses the keys `x` and `z`; the DCS Vec2 convention where `y` means z is
translated at the extractor boundary and never appears here. Heights and
altitudes are metres above the DCS zero plane. Angles are radians unless a
key ends in `_deg`.

## Grid geometry

The grid covers the rectangle `[origin_x, origin_x + height × cell_size)`
by `[origin_z, origin_z + width × cell_size)`. Cell `(row, col)` is
centred at `(origin_x + (row + 0.5) × cell_size, origin_z + (col + 0.5) ×
cell_size)`: rows run north, columns run east. Sampling is at cell
centres.

Tiles are `tile_size × tile_size` cells. Tile `(tx, tz)` holds rows
`tx × tile_size .. (tx + 1) × tile_size − 1` and columns likewise. Within
a tile, sample index is `local_row × tile_size + local_col` (row-major,
columns fastest). The last tile in each direction is a full tile; cells
beyond the grid are written as the layer's `nodata` value.

A tile absent from `manifest.tiles` was not written. The extractor omits
a tile in two cases. A tile whose every cell is fill (see "Tile binary
layout") is omitted from every layer. A tile whose every cell is sea
(`water` layer entirely `2`) is omitted from `height` and `surface` when
`omit_sea_tiles` is true in the manifest; its `water` tile is written.
Otherwise every tile of every layer in a completed pass is present.

`pack` reads absent tiles of a pass whose `complete` is true by the
`water` tile: no `water` tile means fill (`nodata` in every layer,
`valid` 0); a `water` tile entirely `2` with no `height` tile means sea
(`height` 0, `surface` 3 WATER). It reads an absent
tile of an incomplete pass (`--allow-partial`) as all-`nodata`.

## Tile binary layout

Raw samples, no header, little-endian, fixed dtype per layer, exactly
`tile_size²` samples. File size is therefore `tile_size² × bytes_per_sample`
and `pack` rejects any other size.

| Layer | dtype | Value | nodata | Source | Pass |
|---|---|---|---|---|---|
| `height` | `i16` | metres, `floor(h + 0.5)` | −32768 | `terrain.GetHeight(x, z)` | hook |
| `water` | `u8` | 0 land, 1 lake, 2 sea, 3 river, 255 unknown string | 255 | `terrain.GetSurfaceType(x, z)` string | hook |
| `surface` | `u8` | `land.SurfaceType` value: 1 LAND, 2 SHALLOW_WATER, 3 WATER, 4 ROAD, 5 RUNWAY | 0 | `land.getSurfaceType({x=x, y=z})` | mission |

`i16` values are clamped to `[−32767, 32767]` before encoding so −32768
stays reserved.

A cell is fill when the unrounded returns of all three hook calls equal
the theatre's fill triple (`config.json.fill`): `GetHeight` equal to
`fill.height` exactly, `GetSurfaceType` equal to the fill class, and
the seabed return equal to `fill.seabed`. The extractor tests this
before encoding, because after `floor(h + 0.5)` the Caucasus fill
height 5.000005 is 5 and real 5 m land is 5 too. A fill cell is written
as `nodata` in `height` and `water`. The seabed return is read for the
test only; no layer stores it, since no query reads depth. A tile that
is entirely fill is omitted. Unknown `water` strings are logged once per distinct
string and encoded 255; `pack` reports their count. `land`, `sea`,
`lake` and `river` are the only strings seen (60 000 samples on
Caucasus, 40 000 on Cold War Germany).

Lua 5.1 in DCS has no `string.pack`. The extractor encodes `i16` as two's
complement by hand: `v < 0 and v + 65536 or v`, then low byte, high byte.
`u8` is one `string.char`. The Rust side reads with `i16::from_le_bytes`.

## manifest.json

```json
{
  "format_version": 1,
  "extractor_version": "0.1.0",
  "theatre": "Caucasus",
  "dcs_build": "2.9.29.27278",
  "dcs_build_timestamp": "20260826-084519",
  "terrain_fingerprint": {
    "surface5": { "path": "Mods/terrains/Caucasus/Surface/Caucasus.surface5", "size": 6293651208, "payload_size": 43964216,  "head_sha256": "…" },
    "rn4":      { "path": "Mods/terrains/Caucasus/roads/Caucasus.rn4",       "size": 203468160,  "payload_size": 203468160, "head_sha256": "…" },
    "scn5":     { "path": "Mods/terrains/Caucasus/Scenes/Caucasus.scn5",     "size": 1140069192, "payload_size": 11888248,  "head_sha256": "…" },
    "digest": "3f9a1c2e"
  },
  "extracted_at": "2026-09-02T18:04:11Z",
  "bounds_km": { "sw": [-600, -560], "ne": [380, 1130] },
  "authored_bounds_m": { "min_x": -418619.19, "min_z": 113728.16, "max_x": 26382.5, "max_z": 943187.06 },
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
    "hook":    { "complete": true,  "started_at": "...", "finished_at": "...", "frames": 18211 },
    "mission": { "complete": false, "started_at": null,  "finished_at": null,  "frames": 0 }
  },
  "tiles": [
    { "layer": "height", "tx": 0, "tz": 0, "path": "tiles/height/0_0.bin", "min": -3, "max": 412 }
  ],
  "tables": {
    "config": "config.json", "airdromes": "airdromes.json", "runways": "runways.json",
    "stands": "stands.json", "beacons": "beacons.json", "radio": "radio.json",
    "towns": "towns.json", "nodes": "nodes.json",
    "roads": "roads.jsonl", "railroads": "railroads.jsonl", "scenery": "scenery.jsonl",
    "scenery_models": "scenery_models.json"
  },
  "timing_ms": { "presweep": 58000, "height": 71020, "water": 98400, "roads": 612000 },
  "notes": []
}
```

Rules:

- `format_version` is bumped on any incompatible change; `pack` refuses a
  version it does not know.
- `dcs_build` and `dcs_build_timestamp` are `version` and `timestamp`
  from `autoupdate.cfg` at the install root, which the hook reads
  through `lfs.currentdir()`. They identify the DCS core build.
- `terrain_fingerprint` identifies the terrain module's data, which
  updates on its own schedule and can change heights, roads and scenery
  under an unchanged `dcs_build`. The three files are
  `Mods/terrains/<terrain_dir>/Surface/<id>.surface5`,
  `Mods/terrains/<terrain_dir>/roads/<id>.rn4` and
  `Mods/terrains/<terrain_dir>/Scenes/<id>.scn5`, where `<id>` is the
  theatre id and `<terrain_dir>` is the directory name from the
  extractor config (`Sinai` for `SinaiMap`, `GermanyColdWar` for
  `GermanyCW`, `MarianasWWII` for `MarianaIslandsWWII`; the file base
  names use the id). Directory-name case does not matter on Windows.
  For each file the hook records the file size from
  `lfs.attributes(path, "size")` (hook-state file handles have no
  `seek`), the little-endian `u64` at bytes 8–15 of the container header
  (the container's own payload-size field, which is smaller than the
  file for `.surface5` and `.scn5`), and the SHA-256 of the first 1 MiB,
  all taken from one `read` of the file head. `digest` is
  the first 8 hex characters of the SHA-256 over the three `head_sha256`
  values concatenated, and is the short id used in file names. Hashing
  1 MiB is instant; hashing the 6 GB surface file is not, and the header
  and head change whenever the terrain is rebuilt. The hook needs a
  SHA-256 in pure Lua 5.1: the hook state has no `bit` library and no
  `string.pack`; use the arithmetic implementation and test it against a
  known vector.
- `bounds_km` is `GetTerrainConfig("SW_bound")[1], [3]` and `NE_bound`
  likewise, in kilometres as DCS gives them.
- `authored_bounds_m` is `nodesMapBorders` from the theatre's `entry.lua`
  when the extractor is given it in config
  (`authored_bounds_source: "config"`), else the bounding rectangle,
  expanded by 10 km, of the authored cells of a 5 km pre-sweep over the
  bounds rectangle (`authored_bounds_source: "presweep"`). A cell is
  authored when a 2 km line from its centre sampled at 10 m has at
  least 60 samples whose second difference of `GetHeight` is non-zero,
  or `getClosestPointOnRoads("roads", ...)` snaps within 5 km
  (`design-and-facts.md`, "Geometry facts"). `config.json.presweep`
  records `cell_km`, `breakpoint_min`, `road_max_m`, the counts of
  authored and total cells, and the authored cells as a bitmask row by
  row (base64). Either
  rectangle can include a fill margin (on Caucasus 3.2 % of a 1 km
  lattice inside `nodesMapBorders` reads the fill value), so it bounds
  the sweep and does not by itself mark valid terrain; the per-cell
  fill test and fill-tile omission do. `crop_m` is the
  user's bounding box when one was given, else null.
- `grid.origin_*` and extents are computed by the extractor from
  `crop_m`, else `authored_bounds_m`, snapped
  outward to a multiple of `cell_size`. `cell_size` is 50 in every
  extract; a coarser packed base is `pack`'s choice. The manifest records the result;
  `pack` never recomputes it.
- `tiles` lists every tile file present, with per-tile `min` and `max`
  over non-nodata samples (null when every sample is nodata). The
  extractor appends one line per written tile to `tiles.jsonl`, the
  same object as a `tiles` entry, and writes `tiles` into the manifest
  at the end of each pass and at every phase change. When the journal
  holds lines the manifest lacks, `pack` and `check-extract` take the
  union and report the count; a line for a file that does not exist,
  or a file with no line, is an error. Resume reads the journal, so an
  interrupted run loses at most the tile being written.
- `passes.<name>.complete` is true only when every tile and table of that
  pass has been written. `pack` packs an incomplete hook pass only with
  `--allow-partial`; an incomplete mission pass is normal and simply
  leaves `surface` and `scenery` absent from the packed file.

## Tables

All JSON files are UTF-8, one document per file, keys as below. Numbers
are written with enough precision to round-trip a double (`%.17g`).

**config.json**: `id`, `bounds_km`, `default_bullseye` (`{blue: {x, z},
red: {x, z}}`), `sea_enabled`, `default_camera_km` (`[x, alt, z]`),
`summer_time_delta`, `shape` (the `getTerrainShpare()` string), `crs`
(object with `proj4` string and the fitted parameters when the extractor
was given them, else null), `latlon_samples`: 20 entries `{x, z, lat,
lon}` from `terrain.convertMetersToLatLon` on a regular lattice inside the
grid, for `pack` to verify or fit the projection; `fill`: `{height,
water, seabed, samples: [{x, z, height, water, seabed}]}`, the fill
triple the theatre returns outside its terrain, measured at three
points 500 km outside the bounds rectangle (`height` as returned,
`water` as the encoded class, `seabed` as returned). The extractor uses
it for the per-cell fill test ("Tile binary layout") and `pack` copies
it into `meta`. The three samples must agree; the extractor
logs, writes `fill: null` and skips the fill test when they do not.

**airdromes.json**: array of `{id, name_id, code, display_name, names, x,
z, lat, lon, civilian, abandoned, class, runway_names, beacon_ids,
radio_ids, roadnet, roadnet5, towers, warehouses, fueldepots,
shelters}`. `id` is the numeric key of the entry in the `Airdromes`
table (25 for Kutaisi; it equals `Airbase:getID()` in the mission
state); `name_id` is the entry's own `id` field, a string (`"KUTAISI"`).
The last four are arrays of `"externalId:NNN"` strings as DCS gives
them. `x, z` from `reference_point` (`{x, y}`, `y` is z). Entries with
`abandoned == true` are included and flagged.

**runways.json**: array of `{airdrome_id, name, edge1_name, edge1_x,
edge1_z, edge2_name, edge2_x, edge2_z, course}`, from
`terrain.getRunwayList(roadnet)` per airdrome, `course` in radians as
returned.

**stands.json**: array of `{airdrome_id, crossroad_index, name, flag, x,
z, params: {SHELTER, FOR_HELICOPTERS, FOR_AIRPLANES, WIDTH, LENGTH,
HEIGHT}}`, from `terrain.getStandList(roadnet, [...])`.

**beacons.json**: array of `{beacon_id, callsign, display_name, type,
frequency_hz, channel, direction, x, alt, z, lat, lon, scene_objects}`,
from `terrain.getBeacons()`; `x, alt, z` are `position[1..3]` (a
positional array), `lat, lon` are `positionGeo.latitude/longitude`;
`channel` null when absent (27 of 164 Caucasus beacons carry one).

**radio.json**: array of `{radio_id, callsigns, roles, frequencies_hz:
{hf, fm, vhf, uhf}, scene_objects}`, from `terrain.getRadio()`.
`frequency` is keyed `0..3`, each value a pair whose second element is
the frequency in Hz (Anapa: 3.75, 38.4, 121, 250 MHz), mapped to `hf`,
`fm`, `vhf`, `uhf` in that order. `callsign` is an array of
`{<lang> = {name, name}}` tables; `callsigns` records it as
`{<lang>: name}` using the first name of each entry.

**towns.json**: array of `{name, display_name, lat, lon, x, z}` from the
theatre's `Map/towns.lua` (path given in config; the extractor reads it
with `dofile` in a sandboxed environment), `x, z` from
`terrain.convertLatLonToMeters`.

**nodes.json**: array of `{id, name, red: {x, z}, blue: {x, z}}` from
`MissionGenerator/nodes.lua`, same reading rule, positions as given.

**roads.jsonl** and **railroads.jsonl**: one JSON object per line.
`{"kind": "seed", "id": n, "x": ..., "z": ..., "snap_x": ..., "snap_z": ...,
"snap_dist": ...}` for each lattice seed and its
`getClosestPointOnRoads` snap (`snap_x`, `snap_z`, `snap_dist` null
when the call returns nil, which it does where no road is reachable,
as on open sea on Marianas; such a seed gets no paths), then `{"kind": "path", "from": seed_id,
"to": seed_id, "points": [[x, z], ...]}` for each `findPathOnRoads` result,
and `{"kind": "nopath", "from": ..., "to": ...}` where it returned nil.
Seeds are a lattice at `road_seed_spacing` metres (default 1000) over the
grid plus every airdrome reference point and every town. Paths are
requested from each seed to its `road_seed_neighbours` (default 4) nearest
seeds by snap point, each unordered pair once. Graph building is `pack`'s
job; the extractor records what the engine said.

**scenery.jsonl** (mission pass): one object per line, `{"id": n,
"model": "SKLAD_NEW", "x": ..., "alt": ..., "z": ..., "obb": {"w": ...,
"d": ..., "rotation": ..., "cx": ..., "cz": ...} | null, "radius": ... |
null}`. `id`, `model`, position from `world.searchObjects` over
`Object.Category.SCENERY`: `id` is `obj:getName()`, a Lua number
(scenery objects have no `getID`; 70741507), `model` is `obj:getTypeName()`,
position is `obj:getPoint()` with `alt` its `y`. `obb` and `radius` from
`terrain.getObjectsAtMapPoint(x, z)` at the object's own position when
that returns an entry whose `id` (a string of digits in the hook)
equals `tostring` of the same number, else null; in
that entry `sizeOBB`, `center`, `boxMin` and `boxMax` are positional
arrays, so `w, d` are `sizeOBB[1], [2]` and `cx, cz` are
`center[1], [2]`. Objects are unique
by `id`; the extractor de-duplicates across overlapping search spheres.
Objects whose hook `type` is 131072 (wires, power-line poles, cranes,
walls) are not returned by `world.searchObjects` and are absent from
the extract.

**scenery_models.json** (mission pass): the per-theatre model
catalogue, an array of `{"model": "SKLAD_NEW", "count": 1418,
"display_name": "...", "category": "...", "life": 400, "type_bits":
[65536, 65537], "obb_w_median": 41.4, "obb_d_median": 43.3, "radius_median":
30.1, "example": {"x": ..., "z": ...}}`. `display_name`, `category` and
`life` come from `getDesc()` and `getLife()` on the first instance seen;
`type_bits` is the sorted array of distinct `type` values seen (65536,
65537 and 131072 occur on Caucasus), the OBB medians and `radius` from
`getObjectsAtMapPoint` over every instance that returned one (null when
none did). The extractor
builds it while writing `scenery.jsonl` and writes it at the end of the
scenery sweep. It is what `pack` classifies from
(`core.md`, "Scenery classification"), and it is small
enough (about 130 rows on Caucasus) to paste into a review.

## Versioning and validation

`pack` validates: `format_version`; every listed tile exists with the
exact byte size; no unlisted tile files; per-tile min/max match the
contents; every JSON file parses and has the keys above; `passes.hook.
complete` or `--allow-partial`. It reports each failure with the path
and stops. A `dcsterrain check-extract <dir>` subcommand runs the same
validation without packing, so a user can confirm an extraction on the
Windows machine before moving it.

## Test contract

The synthetic theatre generator (`core.md`) writes a
directory in this format; the stub-harness run of the extractor
(`extractor-hook.md`) writes one from the same closed-form terrain.
Both are read by the same `pack`, and a test asserts the two directories
are sample-for-sample identical. That is the cross-language contract
test.
