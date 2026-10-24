# DCS terrain file formats

This document describes the DCS World terrain files that carry heights, water, roads, airfields and scenery.
It also covers the plain Lua files and the map projection, as far as decoding has reached.
All of it was measured on DCS build 2.9.30.28738. Caucasus was checked against a live extract of the same terrain
files; Nevada and Syria were checked by cross-checks between decoders, since they have no live ground truth.
**Every binary value is little-endian.** None of these files is compressed or encrypted.

Notation: `u8/u16/u32/u64/i32` integers, `f32/f64` IEEE floats, `u48` a 6-byte unsigned integer.
`str` is `u32 len` followed by `len` bytes of ASCII with no terminator. `names` is `u32 count, count × str`.
Coordinates are DCS world meters: x is north, z is east, y is up.

## Versions seen

| file | class name | version field | Caucasus | Nevada | Syria |
|---|---|---|---|---|---|
| `Surface/<map>.surface5` | `landscape5::Surface5File` | u32 after class = 5 | 5 | 5 | 5 |
| `roads/<map>.rn4` | `landscape4::lRoadNetwork` | u32 after class = 5 | 5 | 5 | 5 |
| `AirfieldsTaxiways/<name>.rn4` | `landscape4::lRoadNetwork` | u32 after class = 5 | 5 | 5 | 5 |
| `AirfieldsTaxiways/<name>.rn5` | `t5::rn5file` | u32 after class = 0 | 0 | 0 | 0 |
| `Scenes/<map>.scn5` | `landscape5::Scene5File` | u32 after class | **9** | **8** | **9** |

Folder names vary in case (Syria uses `surface/`, `map/`), so match directory and extension names
case-insensitively. Where a folder holds several files with the extension, take the largest; exclude
`*.onlay.sup4` when looking for other `.sup4` files.

## Common container header

| offset | type | meaning |
|---|---|---|
| 0x00 | u32 | container version, always 2 |
| 0x04 | u32 | header size, always 0x30 |
| 0x08 | u64 | **data base B** in `.surface5` and `.scn5`: every data offset in those files is relative to it, and the table of contents fills `[0, B)`. Not used for the other classes |
| 0x10 | 16 bytes | zero in every file seen. Unknown |
| 0x20 | str | class name, e.g. `landscape5::Surface5File` |
| 0x24+len | … | class-specific body; the first u32 is the class version above |

Confirmed: version, class name, B for surface5 and scn5 (scn5's B equals the base found independently by array
alignment). Guessed: nothing else in 0x08..0x1F carries meaning.

## Shared field descriptor

`.surface5` and `.scn5` describe every data array with the same 22-byte tail after a name:

```
u32 1                      marker
str name                   e.g. TRITYPE, Pbase, P, AXIS
u16 fmt                    element format code
u16 stride                 bytes per element
u32 flag                   1 on TRITYPE/TRI, 0 elsewhere (meaning unknown)
u64 size                   bytes in the array
u48 offset                 relative to B; the array is at file offset B + offset
```

Element count = `size / stride`. The format codes seen are 0 (f32), 1 (f32×2), 2 (f32×3), 4 (u32 or i32) and 6.
Code 6 is u32×3 for `TRI` but u16×6 for `AXIS`, so read each array by its **name**, not by `fmt`.

## `landscape5::Surface5File` (`.surface5`)

### Layout

```
container header (0x00..0x23), class name
u32 5, u32 4, u32 0, u32 0, u32 N, u32 7    N = number of finest-mesh blocks (1808 / 9093 / 9772)
table of contents up to B:  a sequence of blocks
  f32[16]  matrix, row-major: identity 3×3 rotation, row 3 = (Tx, Ty, Tz, 1); Ty = 0 in practice
  f32[6]   bbox min x,y,z, max x,y,z in block-local coordinates
  …        framing words (not decoded), then the block's field descriptors
data arrays from B to end of file
```

A decoder does not need to walk the framing. It finds the parts by pattern over `[0, B)`:

1. **Matrices:** byte pattern of the f32 identity (`1,0,0,0, 0,1,0,0, 0,0,1,0`) followed by `Tx, Ty, Tz, w`
   with `w == 1.0`. The bbox is the 6 f32 right after `w`.
2. **Descriptors:** `u32 1, u32 len (2..64), name of exactly len bytes matching [A-Za-z][A-Za-z0-9_]*`, then the
   shared tail. Discard any with `stride > 64` or `size ≥ 2^40`. One field, `N` (stride 12), has a 1-byte name
   that this pattern skips; it is not needed.
3. Each descriptor belongs to the nearest matrix before it. A block is a matrix with a `Pbase` field.
4. **Finest mesh** = the blocks whose first descriptor is `TRITYPE`. They are the first N blocks, and the count
   matches the header's N on all three maps.

Fields of a finest block, in file order:

| field | element | count | meaning |
|---|---|---|---|
| `TRITYPE` | u32 | = triangles | type bits per triangle, see below |
| `TRI` (first) | u32×3 | triangles | the block's full triangle list, indices into `Pbase` |
| `baseIndex`, `parametricUVW`, `N`, `materialParamsIndex`, `TRI` … | | | per-material render subsets (below) |
| `Pbase` | f32×3 | vertices | block-local position; `y` is ground or bed height |
| `nextLodIndex` | i32 | vertices | not needed |
| `Nbase` | f32×3 | vertices | normals (guessed), not needed |
| `maxEdge` | f32 | vertices | not needed |
| `depth` | f32 | vertices | water depth above `Pbase.y`; treat as 0 when the field is absent |

Where a name repeats, the first occurrence is the block's own field and later ones belong to subsets.
The other blocks (Caucasus 1808..3615 hold the same geometry without `TRITYPE`; the rest are coarser levels of
detail) are not needed.

### Material subsets

After the full `TRI` come render subsets, each made of a `baseIndex` (u32 per subset vertex, an index into
`Pbase`) and **then** its own `TRI` (u32×3 indices into that `baseIndex` list). Optional `parametricUVW`, `N` and
`materialParamsIndex` arrays can sit between them. To pair them, take the nearest `baseIndex` before the
subset `TRI`, with no other `TRI` in between. On Caucasus, Nevada and Syria it is always within 7 descriptors,
but Afghanistan puts about 22 vertex attribute arrays (`mainTexCoord`, `closeup0Tangent`, …) between the two,
so the distance is no guide. A subset vertex is `Pbase[baseIndex[TRI[i]]]`.

A subset with no `baseIndex` before it is a decal over a vertex list of its own, not part of the ground. On
Afghanistan, `RunwayOnlay5.1/Runway` subsets carry their own `parametricUVW` arrays (local coordinates in
meters) and index those. A decoder skips them.

Every finest-block vertex lies inside its block's stated box, on all eight maps installed on 2.9.30.28738.

The material follows each `TRI` descriptor, within 160 bytes of its name. The observed framing is
`u32 (0 or 3), u32 2, str shader, str material, u32 k, k × {str key, u32 0, u32 1, str value}, i32 -1`, then
words and a bbox. Take the first string ending in `<digit>.<digit>` as the shader and the next string as the
material. Pairs seen include `Surface5.0/Land` (the full mesh), `Road5.1/Beach`, `Sea5.0/Sea`, `Field`,
`Forest`, `Town`, `River5.1`, `Lake5.1` and `Runway`.

### Deriving the extract

- **World vertex:** `x = Tx + P.x`, `z = Tz + P.z`, and `h = Ty + P.y + depth`. The result is the water surface
  over water and the ground elsewhere. Sea vertices sit at `P.y = -38.6` with `depth = 38.6`.
- **Coverage:** a block covers `[Tx + bbox.minx, Tx + bbox.maxx] × [Tz + bbox.minz, Tz + bbox.maxz]`. The map
  extent is the union of the finest blocks' boxes. A point that no triangle covers is outside the mesh.
- **Height at (x, z):** find every finest-mesh triangle containing the point in the x–z plane. Use barycentric
  `u, v` with tolerance `u ≥ -1e-7, v ≥ -1e-7, u + v ≤ 1 + 1e-7`, and skip triangles whose determinant is 0.
  Interpolate `h` linearly.
- **Precedence where triangles overlap:** a triangle with `TRITYPE & 8` (road strip) wins, then a water
  triangle (`TRITYPE & 7`), then the higher block index, then the higher triangle index (ADR 0041). Without the
  road rule the Roki tunnel area is off by 729 m, because a road strip runs under a mountain triangle and the
  live terrain reports the road. Without the water rule, lake triangles laid over a land triangle of the same
  block read as land on 43 Caucasus cells.
- **Height layer:** `floor(h + 0.5)` at the center of each 50 m cell, with `h` rounded to f32 first, which
  matches the game on more cells than rounding the f64.
- **Water class:** `TRITYPE & 4` → river (3); else `& 1` → sea (2); else `& 2` → lake (1); else land (0). A
  river bit wins over a lake or sea bit on the same triangle (types 5, 6, 70): 42 Caucasus cells.

| TRITYPE bit | meaning | status |
|---|---|---|
| 1 | sea | confirmed (water layer) |
| 2 | lake | confirmed |
| 4 | river | confirmed |
| 8 | road strip | confirmed: 99 % of road vertices sit on it |
| 16 | rail strip | confirmed by cross-check: 98 % of rail vertices sit on it, on all three maps |
| 32 | rare; unknown | guessed |
| 64, 128, 256, 1024 | probably land materials (field, forest, town) | guessed |

**Confirmed:** block and descriptor layout, the finest mesh, heights, water surface and water class, the
precedence rule, and bits 1/2/4/8/16. The method behaves the same on three maps.
**Guessed:** the `surface` enum (ROAD, RUNWAY, SHALLOW_WATER), which no ground truth could score. It probably
follows from bits 8/16, the `Runway` subset and a depth threshold. Also guessed: the subset framing words, the
meaning of `nextLodIndex`/`maxEdge`/`flag`, and the coarser blocks.

## `landscape4::lRoadNetwork`, map-wide (`roads/<map>.rn4`)

### Layout

```
container header, class name
u32 5                         version
names                         road classes: Caucasus 14 (road rdb tunnel_hose rw tunnel_hose_rail
                              roki_tunnel_road vpp_center vpp_border vpp_border_big taxiway taxiway_border
                              stopline stand_border vpp_border_vbig); Nevada and Syria 23 each
node table                    Caucasus: u32 30548, then records {u32 1, u32 id, u32 ?, u32 k, k × (u32, u32)}.
                              Not decoded (guessed: nodes and their links)
edges and polylines           packed with no alignment
spatial index                 last ~10 MB: dense f32 (x, z) pairs (guessed)
```

Edge record, as seen around each polyline (`q` = offset of the point count):

```
…      not decoded
q-14   u8   kind: 2 road, 1 rail, 0 airfield line
q-13   u8   class: index into the class table
q-12   u32  node at the first vertex, or 0xFFFFFFFF for none
q-8    u32  node at the last vertex, likewise
q-4    u32  0           (this zero is what marks a run as a graph edge)
q      u32  n
       n × f64[3]  points (x, y, z)
       u32 n, n × f64[3]  unit tangents (dx, dy, dz)
       u32 m, m × f64  cumulative length          (seen in airfield files; guessed here)
       …              further per-vertex data, not decoded
```

Runs that are not edges (no zero at `q-4`) are short connectors. In airfield files they read
`u32 3, u32 a, u32 b, u32 n, points`. The map file has them too, after the edges: Caucasus 27 955 of 3 to 5
points, framed `u32 3, u32, u32, u32 n`, whose two u32 range over the edge count. They draw the inside of
junctions (guessed: they join one edge to the next).


### Finding the polylines

The record framing between polylines is not fully decoded, so the decoder scans for runs instead:

1. Build a plausibility box from the surface5 finest mesh: its x–z extent ± 20 000 m, and its y range (min of
   the block bbox `miny`, max of `maxy`) ± 500 m.
2. At every byte offset `i`, a candidate is `u32 n` with `2 ≤ n ≤ 100 000`, followed by `n` consecutive f64
   triples at stride 24 from `i + 4`. Every triple must lie inside the box, with `y` strictly inside and
   `|x| > 1`, and the run must end within the file.
3. Accept candidates greedily from left to right, skipping every candidate that starts inside an accepted run.

This reproduces the earlier fixed-box scan exactly: Caucasus 43 010 runs and 1 184 993 vertices. The
data-derived box finds 43 224 runs; the extra runs are airfield-line edges whose y alternates ±524.825.
Of the 43 224, 15 267 are edges (12 221 roads and rail), 27 955 connectors and 2 neither.

### Deriving the extract

A road or rail polyline is an edge with kind 2 or 1. Its class name is `classes[class]`, and its vertices are
the points. `y` is a height for kinds 1 and 2 only. For kind 0 it is not: the median is -200 m.

The node ids name junctions, and an edge stops short of its junction: the ends naming one id lie p50 27 m,
p99 176 m from their mean on Caucasus roads, and at most 2.2 km on any of the eight installed maps. Each
map also has one catch-all id, named by dozens to hundreds of ends spread across the whole map (Caucasus 1697),
which is no junction; the extract writes it, and 0xFFFFFFFF, as null (ADR 0044).

**Confirmed:** the class table, the point and tangent arrays, and that `y` is a height for roads and rail
(Caucasus road vertices: 99.5 % within 0.5 m of the sampled ground; rail 98.8 %). The kind byte is confirmed
because kind 1 equals exactly the rail classes on all three maps (Nevada `rail_two` 214 = kind-1 count 214;
Syria 4 701). On all eight installed maps every kind-1 edge has a rail class; the converse fails once, on
MarianasWWII, whose 1 401 `rail_narrow` edges are kind 2 and are written as roads.
**Confirmed (Caucasus):** `a` is the node at the first vertex: of 35 places where two edge ends coincide,
34 name one id read that way and none read the other way.
**Guessed:** the node table, the connectors, the spatial index, the bytes before `kind`, and the per-vertex data after the
tangents.

## `landscape4::lRoadNetwork`, airfield (`AirfieldsTaxiways/<name>.rn4`)

The file basename is the airdrome's internal name (`name_id`). Same encoding as the map file:

```
container header, class name, u32 5
names                 classes, e.g. vpp, markingforbigair[, marking_type2] (3..6 entries)
edges                 taxi graph, edge record as above (points, tangents, cumulative length, …).
                      Endpoints sit 4-18 m from their node positions, not on them.
nodes                 one per graph crossroad, in index order:
  [connector runs: u32 3, u32 a, u32 b, u32 n, n × f64[3]]   optional
  [str name]                                                  named nodes only (stands, runway ends)
  f64[16] matrix      row-major; row 0 = local X axis, row 3 = (x, y, z, 1)
  u32 count, count × {u32 node, f32 distance}
```

**Finding nodes:** at offset `i`, accept a matrix when all of these hold: `f64 at i+120 == 1.0`;
`m[3] = m[7] = m[11] = 0`; `|m[0..11]| ≤ 1.0001`; `1 < |x| < 1e6`, `|z| < 2e6` and `-500 < y < 6000`. On a
match, skip 128 bytes. Otherwise advance one byte. **The node index is the ordinal of the matrix in the file**,
and it equals the live `crossroad_index`. The name is the `str` that ends exactly at `i`: try `L = 1..39`,
accepting `u32 at i-L-4 == L` with every byte printable. The list `count` is at `i + 128`.

**Stands and runway ends:** named stands list every runway end with a distance, and runway ends list every
stand. Treat the named nodes and their lists as an undirected graph and two-color each connected group of it
on its own:

- In each group, the runway ends are the side that holds a name with `#` (for example `1#27`). If both sides
  do, or neither does, take the smaller side; if the sides are equal, the side of the group's first node.
- A named node that lists nothing and is listed by nothing is a runway end when its name has `#`. An end may
  list nothing: Batumi's `1#31` and Novorossiysk's `1#22` do, while their partners list every stand.
- If no named node has a list, the ends are the `#`-named nodes plus every named node whose designator is
  reciprocal to one of them.
- Every other named node is a stand.

One coloring across the whole airfield, started from the first node, gets these right only by order: an
isolated `#` end lands on the ends' side because it comes before its stands in every Caucasus file.

Strip a `k#` prefix from names. A designator is `0*(\d{1,2})([LRC]?)`. Two ends are reciprocal when
`(a + 17) % 36 + 1 == (b - 1) % 36 + 1`, with L↔R swapped and C or blank kept. Pair ends greedily: repeatedly
take the pair that maximizes (reciprocal, horizontal length). Within a pair, edge1 is the end whose raw name has
`#`. If both or neither have it, edge1 is the lower node index.

| extract value | derivation |
|---|---|
| stand name, x, y, z | node name; matrix row 3 |
| stand `crossroad_index` | node ordinal |
| stand heading | `atan2(m[0][2], m[0][0])`, the local X axis (not checked live) |
| runway edge1/edge2 names, x, z | the two paired ends |
| runway course | `atan2(z2 - z1, x2 - x1)` in radians, edge1 → edge2, true |
| airdrome reference x, z | midpoint of the two ends of a runway |

**Confirmed (Caucasus):** 900/900 stands match the live name and position bit-exactly, and their index equals
`crossroad_index`. Reading the live JSON needs a correctly rounded float parser: serde_json's default one
reads some positions one unit in the last place off. 21/21 runways match names and ends bit-exactly, the course is exact, and the airdrome x, z is
the midpoint. On Nevada and Syria, stand and runway-end y equals the sampled ground to 0.0 m.
**Guessed:** pairing on maps with several runways (Nevada BoulderCity names the partner of runway 9 `1#27L`,
so that pair is geometric); the stand heading; runway width, which is not in the airfield files. A lead for
width: map rn4 class-7 lines run parallel to runways at ±25-29 m. Some Syria airfields have no runway ends
(helipads), and Lakatamia's "ends" are named `Stand01/Stand02`.

## `t5::rn5file` (`AirfieldsTaxiways/<name>.rn5`)

```
container header, class name ("t5::rn5file"), u32 0          version
names     road classes                                       (starts at 0x33)
names     stand and marking classes: stand_end_24m, stand_52x60m_end, stand_shelter, stand_end_all,
          stand_40x40m_end[_only_airplanes], hellicopter_no_taxiway, marking_* …
nodes     the same nodes as the .rn4, with y = 0, not always in the same order:
  u32 ordinal, u32 standClass, u32 k, k × u32,
  f64[16] matrix, f64[16] its inverse
```

Find the matrices as in `.rn4`, but allow entries of the upper 3 × 3 up to 100: GermanyColdWar scales them
by about 1.09, so the unit test finds only the inverses there. A matched matrix and its inverse are one node;
skip 256 bytes. Either class table may be empty (`u32 0`), as on GermanyColdWar's helipads.

A node's first word is its **ordinal in the `.rn5`**, which is not always the `.rn4` index: GermanyColdWar's
`Buchel.rn5` lists `.rn4` node 0 as its node 3. Count the nodes as they are found. Then, with `o` the matrix
offset and `n` its ordinal, find the `k` in 0..5 for which `u32[o-4-4k] == k` and `u32[o-12-4k] == n` and
`u32[o-8-4k] < len(stand classes)`. The stand class is `classes[u32[o-8-4k]]`, and it belongs to the `.rn4`
node at the same position: `|Δx| < 1e-3`, `|Δz| < 1e-3` and `y == 0`. On Caucasus the two numberings agree.

**Test `k` from 5 down to 0.** Testing upward misreads `idx 0, class 0, k 1, [0]`: the `k = 0` test also passes
and returns class 1. That was the only one of 900 Caucasus stands that disagreed.

**Confirmed:** the stand class (900/900 Caucasus stands give the live parameters through `assets.assets`).
Every stand on all eight installed maps of 2.9.30.28738 gets a class.
**Guessed:** the k-list (probably links), the road-class table's use, and non-stand classes listed among the
nodes: on 2.9.30.28738, 43 Syria nodes of `taxi_point_holding_position` and 4 MarianaIslands nodes of
`rwy_point_takeoff_landing` are named like stands, and `assets.assets` holds no stand record for either class.

## `misc/assets.assets`: stand class parameters

The file is a plain binary of asset records. Stand records look like this:

```
str name                       record name = the stand class, e.g. stand_shelter
str category                   (assetCategory value; the name is the last str before DummySize that is not it)
str "DummySize", str value     first key/value pair
{str key, str value} …         height ("18", "5.5"; may be absent), aircraftTypes ("airplanes" | "helicopters" |
                               absent), assetCategory, assetClass, proxy, sourceFile*, WireColor, …
str "stand", str "true"        last pair
geometry                       f64 triples; the stand outline is the set of (x, 0.0, z) vertices in meters
```

Strings here are `u32 len` (1..300) of printable ASCII. Locate each record by the bytes
`05 00 00 00 "stand" 04 00 00 00 "true"`, then search back up to 3000 bytes for `09 00 00 00 "DummySize"`, and
read pairs forward from there. The name is in the 300 bytes before `DummySize`. Outline vertices are the f64
triples at any byte offset from the `stand=true` mark to the next `DummySize` that satisfy all of these:
`y == 0.0`; x and z finite; `2.5 < max(|x|, |z|) ≤ 1000`; and neither x nor z a nonzero below 1e-6 in
magnitude. Small icon geometry stays within ±2.0 and is excluded by this test.

| live value | derivation |
|---|---|
| WIDTH | max x - min x of the outline |
| LENGTH | max z - min z |
| HEIGHT | `height` string, or "" when absent |
| FOR_AIRPLANES | `aircraftTypes != "helicopters"` |
| FOR_HELICOPTERS | `aircraftTypes != "airplanes"` |
| flag | 64 if FOR_HELICOPTERS else 32 |
| SHELTER | no key found; all Caucasus live values are 0 |

The values differ per map (`stand_40x40m_end` LENGTH is 43.057953 on Caucasus and 39.857483 on Nevada), so read
each map's own file and never parse dimensions from the class name.
**Confirmed:** Caucasus 900/900 on all six values plus flag, WIDTH and LENGTH equal to the live strings at six
decimals (decoded on 2.9.30.28738 against the live extract of 2.9.29.27468, same terrain files).
**Guessed:** the other maps, which have no live dump; SHELTER. Every stand class the other seven maps use has
a record, Nevada's included.

## `landscape5::Scene5File` (`.scn5`)

### Layout

```
container header, class name ("landscape5::Scene5File")
u32 version                  9 (Caucasus, Syria) or 8 (Nevada)
u32 M, M × str               model names, lowercase UTF-8 (Syria spells 62 with a Cyrillic letter, and one
                             GermanyColdWar name ends in a space); model index = position
u32 M (again), M × f32[6]    model bbox min x,y,z, max x,y,z (model-local). Width = x extent, depth = z extent,
                             height = y extent; zero for models placed elsewhere (vehicles)
u32 count, material descriptors  u32 2, str "HwInstancer5.2", str "Undef", u32 k,
                             k × {str key, u32 0, u32 1, str value}, i32 -1
instancer nodes              Caucasus 25 283, Nevada 13 434, Syria 261 832
non-instanced group          optional, right after the last node
section table                pool descriptors (below)
i32 × M                      model → destroyed-model map (guessed), then a packed u32 stream (not decoded)
B: data arrays               all attribute and pool offsets are relative to B
```

### Instancer node

Anchor each node on its header at offset `p`: `u32 2, u32 n, u32 nattr (1..8)`, followed by a descriptor whose
name is uppercase (`[A-Z][A-Z_]*`, 1..32 bytes). Accept the node only if all `nattr` descriptors parse (marker 1,
name length ≤ 40) and `n > 0`.

```
p-160  f64[16]   matrix, row-major; rotation identity, row 3 = node translation (x, 0, z, 1)
p-32   f32       unknown (not a radius: 147 829 on Caucasus, 9 046 370 on Nevada)
p-28   f32[6]    node bbox, world min x,y,z, max x,y,z
p-4    u32 0
p      u32 2, u32 n (instances), u32 nattr
       nattr × shared field descriptor (offset relative to B, size = n × stride)
       u32 0, u32 k, k × u32 model indices used in the node, u32 8, i32 -1
```

| attribute | v9 fmt/stride | v8 fmt/stride | content |
|---|---|---|---|
| `P` | 2/12 | 2/12 | f32 x, y, z; world = P + node translation; y is absolute altitude |
| `TYPE_OPTIONS` | 4/4 | — | u32: low 16 bits model index, high 16 options (0x30; 0x31 for wire) |
| `TYPE` | — | 4/4 | u32 model index |
| `OPTIONS` | — | 4/4 | u32 options (0x30; 0x31 wire) |
| `AXIS` | 6/12 | — | 6 × u16 offset-binary, `v = (u - 32768) / 32767`: local X axis (x,y,z), then local Z axis (x,y,z) |
| `AXISX` | — | 1/8 | f32 (x, z) of the local X axis; unit, or the span vector for wires; no Z axis and no tilt |
| `SEED_SCALE` | 4/4 | — | u32, not decoded (seed and scale?) |
| `SEED` | — | 0/4 | f32, not decoded |

Instances are numbered globally in file order: `global = start[node] + index`, where `start` is the running
sum of `n` over the nodes before it.

### Instance id and collision pools

`id = node << S | index`, where **S = 13 when every node holds at most 8 192 instances, else 15** (ADR 0045).
The version does not decide it: Nevada (version 8, largest node 8 808) and PersianGulf (version 9, 14 872) use
15, and bits 13-14 of their ids are never set; the six other installed maps use 13. A decoder checks S by the
pool's own AABBs: an id split at the right S puts its instance inside its box (±0.5 m in x and z).

The section table lies between the end of the last node and B. Its entries are 14 bytes each, `u64 count,
u48 offset`, three to a pool:

| entry | content |
|---|---|
| 1 | `count` ids at B + offset: u64, or u32 for the non-instanced group's pool |
| 2 | same `count`; `count × f32[6]` AABBs (world min x,y,z, max x,y,z) at B + offset |
| 3 | BVH nodes (48 B each, not decoded) |

Find a pool by its structure: equal counts, `0 < count < 2^32`, and `offset2 - offset1` equal to `count × 8`
(u64 ids), `count × 4`, or `count × 4` rounded up to 8 (u32 ids). The AABBs must end within the data region.

| pool, in file order | Caucasus | Nevada | Syria | contents |
|---|---|---|---|---|
| u32 ids (optional) | 18 976 | — | 441 | the non-instanced group |
| objects | 917 847 | 553 400 | 6 328 285 | buildings, light poles, bridges, airfield lights |
| lines | 1 069 315 | 8 551 832 | 28 832 057 | wire, power poles, fences, walls, barriers |
| trees | 12 967 324 | 889 299 | 25 809 719 | trees and shrubs |

The u64 pools are named by order: objects, lines, trees. The contents agree on all three maps. Instances in no
pool (light halos, lamps, small plants) are not scenery objects. No pool lists an id twice on any installed map,
but a few instances are in both the objects and the lines pool: 391 in-pavement lights on Caucasus and 9 439
`leaf_wall` on Nevada, the only two maps measured for it, and none in another pair. Each pool keeps its own, so
such an instance is a scenery object and counts as a line.

### Non-instanced group

Right after the last node's end, scan forward up to 4096 bytes for `u32 h, u32 count` that is followed by the
records below. The test: `h < M`, `0 < count < 1e7`, the next 4 bytes zero, a first matrix with `m[15] = 1`,
`m[3] = m[7] = m[11] = 0` and `|m[0..11]| ≤ 1.0001`, and the last record's `m[15] = 1`. `h` is not the model.

```
count × 140-byte record:  u32 0, u32 model index, f64[16] matrix (row 3 = x, y, z, 1), f32 (unknown)
```

The id of record `i` is `(node_count << S) + i`, a flat range that does not split into node and index past
`2^S - 1` records, and the list in `NotInstances.lua`. The models include
smoke stacks, nasos, ukrytie and kamaz/ural/gaz vehicles.

### Deriving the extract

| value | v9 | v8 |
|---|---|---|
| id | from the pool's u64 id (`node << 13 \| index`) | `node << 15 \| index` |
| model | `names[TYPE_OPTIONS & 0xffff]` | `names[TYPE]` |
| x, y, z | `P + node translation` | same |
| heading of local X | `atan2(AXIS.X.z, AXIS.X.x)` in degrees mod 360 | `atan2(AXISX.z, AXISX.x)` |
| width, depth, height | model bbox x, z, y extents | same |

The long axis of a bridge or fence is local Z, so its heading is the local X heading ± 90°. Non-instanced
records take x, y, z from row 3 and heading `atan2(m[0][2], m[0][0])`.

**Confirmed:** layout, B, both versions, and ids, because 100 % of Caucasus ids land in their own AABB. Runway
light headings match the runway course within 2° (Caucasus 91.4 %, Nevada 87.8 %, Syria 82 %; with the axes
swapped, 1-2 %). Bridge X axes are perpendicular to the road beneath them in 87 % of cases. The 208 Kutaisi
runway lights fit a line at 74.0°, y 45.7 m.
**Guessed:** that the live scenery id equals `node << S | index`. It is structural only and not checked
against a live `getName()`. Also guessed: `SEED_SCALE`/`SEED`, the f32 at p-32, BVH nodes, the packed u32 stream
and the destroyed-model map. Pool t2 holds about 3 % more objects than the live catalogue (917 847 vs 888 854),
which is unexplained.

## Plain Lua files (`Mods/terrains/<map>/`)

All are Lua 5.1 source and parse as is.

| file | shape | gives |
|---|---|---|
| `Beacons.lua` | `beacons = { { display_name = _('…'); beaconId = 'airfield<ID>_<n>'; type; callsign; frequency; position = { x, alt, z }; direction; positionGeo = { latitude = …, longitude = … }; sceneObjects = {'t:<id>'} }; … }` | airdrome display name by id; the projection pairs |
| `Radio.lua` | `{ -- <name_id>` newline `radioId = 'airfield<ID>_0'; role; callsign; frequency }` | airdrome id ↔ `name_id` (from the comment); `sceneObjects` there is the tower |
| `Map/towns.lua` | `towns = { ["NAME"] = { latitude, longitude, display_name } }` | towns, lat/lon only (x, z via the projection) |
| `MissionGenerator/nodes.lua` | `missionNodes = { [k] = { bluePos = {x, z}, redPos = {x, z}, id, name, … } }` | mission nodes |
| `NotInstances.lua` | `notinstances = { "t:<id>", … }` | ids of the scn5 non-instanced group (can be empty) |

The `name_id` is also the basename of the airfield `.rn4/.rn5`. Caucasus only: `Scripts/Database/wsTypes.lua`
maps `Anapa = 12 … Beslan = 32`. ICAO codes, airdrome class, civilian flag and display names for airdromes
without beacons live only in the encrypted `terrain.cfg.lua.pak.crypt`.

### Projection fitted from the beacons

DCS x and z are a WGS84 transverse Mercator with `lat_0 = 0`, `k_0 = 0.9996`, an integer `lon_0`, and a
false-origin offset:

```
(E, N) = tmerc_WGS84(lon, lat; lon_0, k0 = 0.9996, x_0 = 0, y_0 = 0)
z = E - easting        x = N - northing
proj: +proj=tmerc +lat_0=0 +lon_0=<L> +k_0=0.9996 +x_0=-<easting> +y_0=-<northing> +datum=WGS84 +units=m
```

To fit, take the unique `(position, positionGeo)` pairs. For each integer `L` from `floor(min lon) - 12` to
`ceil(max lon) + 12`, project the pairs and form `E - z` and `N - x`. Take their medians, and score `L` by the RMS
of the residuals. Keep the best `L`. Then drop pairs with a residual of 5 m or more and set easting and northing
to the means of the rest. A wrong `L` scores at least about 1.7 km, so the choice is unambiguous.

| map | lon_0 | easting | northing | fit |
|---|---|---|---|---|
| Caucasus | 33 | 99 517 | 4 998 115 | n 164, rms 0.04 m, max 0.07 |
| Syria | 39 | -282 801 | 3 879 866 | n 151, 0.04 / 0.07 |
| Nevada | -117 | 193 996.81 | 4 410 028.06 | n 45, 0.04 / 0.07 |
| PersianGulf | 57 | -75 756 | 2 894 933 | n 101, 0.04 / 0.07 |
| Afghanistan | 63 | 300 150 | 3 759 657 | n 49, 0.04 / 0.07 |
| GermanyColdWar | 21 | -35 427.62 | 6 061 633.13 | n 413, 0.04 / 0.06 |
| MarianaIslands (and MarianasWWII) | 147 | -238 418 | 1 491 840 | n 19, 0.05 / 0.07 |
| Sinai | 33 | -169 222 | 3 325 313 | live table only, not refitted |

**Confirmed:** every fitted map reproduces the live-measured parameters to 0.01 m or better.
**Guessed:** none for the fitted maps. MarianasWWII has no beacon positions and shares MarianaIslands' values
according to the live table.

## Files not needed

| file | class | note |
|---|---|---|
| `roads/*.routes` | `landscape4::lRoutesFile` (u32 3, u32 481 on Caucasus) | rn4 polyline encoding; lane-offset driving paths 1.4 m off the road centerlines; guessed to be AI convoy routes |
| `Surface/*.ng5` | `navGraph5File` (u32 10, u32 count) | ground-unit navigation graph (guessed); adjacency index arrays |
| `Map/*.gn4` | `landscape4::lGeoNamesFile` | F10 place-name labels: f64 positions and a string pool (guessed) |
| `Map/*.sup5` | `landscape5::sup5File` | F10 map vector layers (river, runway lines and meshes) (guessed) |
| `Surface/*.onlay.sup4` | `landscape4::lSuperficialFile` | overlay instancers: radar points, birds, buildings (guessed) |
| `Surface/*.tile` | `landscape5::SurfaceTile` | small; not examined |

## Key validation figures

- Caucasus heights, by the rules above (ADR 0041): 99.9485 % of 141.19 M cells exact, 99.99859 % within 1 m,
  and 1 707 cells off by more than 2 m (lake clusters), worst 29 m. Land only: 99.9195 % exact.
- Caucasus water class: all but 4 of 141 190 035 cells agree. The 4 are land 0.5 m over a lake, where the game
  reports land.
- Caucasus coverage: no cell the live extract wrote lies outside the mesh. The 6.39 M cells it wrote as fill are
  flat mesh land at exactly 5 m, which the sim reports with its off-map value (ADR 0042).
- Caucasus road polylines against the live road router: p50 1.42 m, p99 4.3 m. Rail: p50 2.18 m. Both look like
  a lane or track offset from the stored centerline.
- Road vertices within 0.5 m of the sampled ground: Caucasus 99.5 %, Nevada 99.0 %, Syria 92.5 %. On Syria the
  polyline y differs from the strip surface by 0.5-3 m; which one DCS uses is open.
- Rail vertices within 0.5 m: Caucasus 98.8 %, Nevada 99.4 %, Syria 40.8 %. Syria rail sits a steady 0.5 m below
  the rail strip.
- Scenery objects within 0.5 m of the ground: Caucasus 98.8 %, Nevada 99.8 %, Syria 98.4 %. Trees: 99.9-100 %.
  Wires hang 7-27 m up.
- Runway course against the designator implies a magnetic variation of median 0.0° on Caucasus, 13.7° on Nevada
  and 3.8° on Syria.
