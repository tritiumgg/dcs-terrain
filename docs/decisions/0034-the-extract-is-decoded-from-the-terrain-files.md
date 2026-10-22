# ADR 0034: The extract is decoded from the terrain files

## Status

Accepted

## Context

**Affects:** `extractor-hook.md` throughout; `design-and-facts.md` "Design
decisions" (the road path oracle and its bounded reverse-engineering session,
"Do not multi-resolution the DCS sweep"), "Sources you have"; `extract-format.md`
"manifest.json" (`pass` per layer); `validation.md` "Measuring a new theatre or
build"; `plan.md` components X and V, tasks X5 to X14, C14, V1, V2.

The extractor spec opens with the premise everything else follows from:

> A GameGUI hook that sweeps a loaded DCS theatre and writes an extract
> directory in the format of `extract-format.md`. Lua 5.1, one file,
> runs on the Windows machine with DCS.

and the design allowed the files one look:

> Spend at most one bounded session on `<Map>.rn4`
> using the known-plaintext oracle; stop if the node table is not located.

That session ran, and went on to every map file. On DCS **2.9.30.28738**
(`timestamp` 20261006-075422), with Caucasus terrain files dated 2025-11-14,
none of the files that matter is encrypted or compressed. Each opens with a
container header and a class name, and the data is plain floats and integers.
Decoded with no DCS running and compared against the live Caucasus extract
taken on 2.9.29.27468 over the same terrain files:

| Layer | File | Agreement with the live extract |
|---|---|---|
| `height` | `Surface/<Map>.surface5`, a triangle mesh | 99.95 % of 141.2 M cells exact, 99.9986 % within 1 m, worst 29 m in a few lake clusters |
| `water` | the same mesh's per-triangle type bits | all but 67 of 141.2 M cells |
| roads, railroads | `roads/<Map>.rn4`, f64 polylines at about 5 m spacing | router points lie 1.4 m (roads) and 2.2 m (rail) from the decoded centerline at p50, 4.3 m and 2.8 m at p99 |
| stands, runways | `AirfieldsTaxiways/<Airfield>.rn4` and `.rn5` | 900 of 900 stands and 21 of 21 runways, positions bit-exact |
| stand sizes and permissions | `misc/assets.assets`, one record per stand class with its outline | 899 of 900 stands on all six parameters; the miss is a decoder bug that reads the wrong class index |
| beacons, radio, mission nodes | `Beacons.lua`, `Radio.lua`, `MissionGenerator/nodes.lua` | counts and values identical |
| towns | `Map/towns.lua` | 1 691 names, lat/lon exact |
| scenery | `Scenes/<Map>.scn5` | 917 847 objects with model, position, heading and footprint; model counts within a few percent of the measured catalogue |

Rasterizing Caucasus's height and water from the mesh takes about 30 s. The
same decoders, run on Nevada and Syria without a dump to compare against, put
road vertices, scenery, stands and runway ends on the decoded ground at the
Caucasus rates, with two exceptions recorded under Consequences.

The `surface` layer is the one not yet scored: the live extract holds no
surface tiles. The files also carry data the live API never exposed: a class
for every road polyline, forest areas, and individual trees.

ADRs 0026 to 0033 decided how the live sweeps find the authored rectangle,
sample water and height, seed and journal the road sweep, and search and
journal the scenery sweep. Each answered a cost of asking a running sim one
point at a time, and that cost is what the files remove.

## Decision

`dcsterrain extract --install <dir> --map <Map> --out <dir>` writes the extract
directory by decoding the map's files in the install. DCS is installed but need
not run, and the command runs on any platform that can read the files. It
decodes `height`, `water` and `surface` from `.surface5`, roads and railroads
from `.rn4`, scenery from `.scn5`, stands and runways from the airfield `.rn4`
and `.rn5`, each stand class's size and permissions from `misc/assets.assets`,
and beacons, radio, mission nodes and towns from the map's Lua files. The live
sweeps are retired: the pre-sweep, the water and height sweep,
the road and railroad sweeps, and the scenery sweep and its catalogue. Data no
readable file carries is outside this decision.

### Alternatives considered

**Keep the live sweeps and use the files only to check them.** The sweeps are
built and verified, and they rely on nothing ED might change without notice.
They lost on what they need and what they return. On Caucasus they took
100 s for water and height and about 10 minutes for the four road sweeps, but
every minute of it holds a running sim on the Windows machine, the
scarce resource. They also return less: the API has no road class, no forest
and no trees, and its router returns lane-offset points rather than the
stored centerlines.

**Decode outside the project and hand it an extract.** This keeps a parser for
ED's formats out of the published repository. It lost because the user wants
the tool shipped: a user extracts their own theatre with the binary they
already download, and no data leaves their machine.

## Consequences

Extraction becomes a command, not a session in a running sim: it runs on
macOS and Linux as well as Windows, given access to an install's files, and
the height and water of a whole map take about 30 s. The extract format stays
the contract between the decoders and `pack`. It moves to a version 2 for the
layers the files add, and `pack` and what follows it change only to read those
layers.

Component X shrinks to the tables pass, and the decoders become component E.
The Lua hook's sweep code and its offline tests for those sweeps retire, and
ADRs 0026 to 0033 are superseded by this one. The authored rectangle, which
ADR 0026 measured, needs a definition read from the files; the extract format
revision that carries the new layers settles it. Tasks X6, X7, X7a, X8a, X8b,
X8c, X10 and X11 retire, and a run of the decoder compared against the live
Caucasus extract takes the place of X10 and X11. The `pass` field of each
layer in `manifest.json` no longer names a hook or mission pass.

The project now publishes a parser for ED's file formats. The extracts and
packed files still stay on the user's machine. Whether shipping the parser sits
within the EULA is the maintainer's call, made when this was decided.

ED can change a format in any build, and a decoder that misreads one produces
wrong terrain rather than an error. ADR 0035 makes an unknown format version an
error. A change ED makes without bumping the version is caught only by the
decoder's own sanity checks.

Two figures are open on Syria: 7.5 % of road vertices and 59 % of rail
vertices sit 0.5 to 3 m off the decoded ground, and which of the two DCS uses
there needs a live height check. The `surface` layer needs a live crop to be
scored. Both reopen this decision only if the files cannot match the game.
