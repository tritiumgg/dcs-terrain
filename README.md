# dcs-terrain

Answer terrain siting and routing questions about a DCS World terrain — where to
put a FARP, what a SAM site can see, which road a convoy should take, how to
fly in low and unseen — straight from the terrain's own files in your DCS install,
without DCS running and without DCS doing any terrain work while a campaign
runs.

Answers come back as JSON, with ranked candidates and the reasoning kept as
separate fields, from one command line. An AI assistant with a shell will
learn to use it from a skill shipped here (planned).

**Tools only. No terrain data is shipped.** The tool reads your install in
place and never writes to it. It keeps a small, rebuildable cache beside it in
your user cache directory.

> **Not usable yet.** The project was redesigned to read the install directly,
> and the query engine is being built. `list-terrains`, `check`, `cache`, `schema`
> and the `query` operations `describe`, `sample`, `geo`, `nearest`,
> `scenery_in`, `visible`, `viewshed`, `radar_floor`, `coverage`,
> `approach_spectrum`, `unmask_profile`, `route`, `route_alternatives`,
> `chokepoints`, `trafficability`, `score_site`, `find_sites`, `point_info`,
> `airfield_info`, `validate_placements` and `towns` work today; every other
> operation is planned, kept here so the shape is visible. Progress is in
> [docs/STATE.md](docs/STATE.md).

## Requirements

A DCS World install on the same machine, which does not have to be running.
On Windows the tool finds it from the registry; `--install <dir>` names
another. On macOS or Linux it needs a copy of the terrain's files and
`--install <dir>`. No GPU.

Building needs [mise](https://mise.jdx.dev), which installs the Rust pinned in
`rust-toolchain.toml`.

## Get it

Released builds will be static `dcsterrain` binaries for the three platforms,
attached to tags (planned). To build instead:

```bash
git clone https://github.com/tritiumgg/dcs-terrain
cd dcs-terrain
mise trust
mise install
mise exec -- cargo build --release --manifest-path dcsterrain/Cargo.toml
```

The binary lands at `dcsterrain/target/release/dcsterrain`.

## Use it

List the terrains your install holds, with the id each one takes and whether
the tool reads each of its files (`--json` for a document):

```bash
dcsterrain list-terrains
```

Describe a terrain by its id: its build, its extent, its files and their
versions (`--terrain synth` needs no install). The first query on a terrain
builds its cache, in about a second; later ones read it:

```bash
dcsterrain query --terrain Caucasus describe
```

Ask for layer values at points or on a lattice in a region, convert positions
to latitude, longitude and MGRS, find the nearest road, airdrome, town, place, node,
beacon or scenery object, list the scenery in a region, list the places
its buildings form, from hamlet to city, or describe a point: its ground,
the place it stands in, and how far and which way it lies from the nearest
town:

```bash
dcsterrain query --terrain Caucasus sample '{"points": [{"x": -281782, "z": 647279}], "layers": ["height", "slope", "dist_road"]}'
```

Ask whether the terrain hides one point from another, or which ground around
an observer sees a target; `"geojson": true` adds the answer as GeoJSON for any
map program:

```bash
dcsterrain query --terrain Caucasus viewshed '{"observer": {"x": -260000, "z": 690000, "alt_agl": 10}, "radius": 20000, "target_alt_agl": 2, "geojson": true}'
```

`dcsterrain schema` names every field, and
[docs/queries.md](docs/queries.md) every layer. A query refuses a terrain
whose projection fits its beacons more than 2 m off unless given
`--allow-crs-residual`. A large request spreads its work over every core,
below normal priority so a DCS server beside it keeps the CPU;
`--threads <n>` caps how many it takes.

Check what the tool reads of a terrain, file by file, with its cache, tables
and scenery classes; it fails naming each file it cannot read:

```bash
dcsterrain check --terrain Caucasus
```

`dcsterrain cache list` shows the cache (`--cache <dir>` names another) and
the DCS build each terrain's entries were made on; after a DCS update, an
entry whose terrain files did not change is kept. `cache clear` deletes all
of it, asking before the live validation figures.
`dcsterrain index --terrain Caucasus` builds a cache ahead of time, among it
the road betweenness and the places, which a query never waits minutes for:
until they are built, `chokepoints` and `towns` say so. `query`,
`index` and `list-terrains` take `--cache <dir>` too, and `index` takes
`--allow-crs-residual` as `query` does.

Ask a siting question the same way: `score_site` judges one position against
a list of criteria, and `find_sites` returns the best positions in a region,
ranked and kept a spacing apart:

```bash
dcsterrain query --terrain Caucasus find_sites '{"region": {"Circle": {"x": -281782, "z": 647279, "radius": 10000}}, "footprint": {"width": 60, "depth": 40}, "criteria": [{"name": "slope_max", "params": {"deg": 5}}, {"name": "surface_in", "params": {"classes": ["land"]}}], "spacing": 1000}'
```

Many questions can go in one call as JSON lines on stdin (planned).
`dcsterrain schema` prints every operation's request and response as JSON
Schema.

## Documentation

[docs/README.md](docs/README.md) indexes the design, the query operations and
the task graph.

## License

[MIT](LICENSE).
