# dcs-terrain

Answer terrain siting and routing questions about a DCS World terrain — where to
put a FARP, what a SAM site can see, which road a convoy should take, which
ground no radar covers — straight from the terrain's own files in your DCS
install, without DCS running and without DCS doing any terrain work while a
campaign runs.

Answers come back as JSON, with ranked candidates and the reasoning kept as
separate fields, from one command line. An AI assistant with a shell will
learn to use it from a skill shipped here (planned).

**Tools only. No terrain data is shipped.** The tool reads your install in
place and never writes to it. It keeps a small, rebuildable cache beside it in
your user cache directory.

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

List the terrains your install holds and the id each takes, then ask one
question: the operation and its request as JSON, the answer as JSON on stdout.
The first query on a terrain builds its cache in about a second. Without an
install, `--terrain synth` answers from a synthetic terrain.

```bash
dcsterrain list-terrains
```

```bash
dcsterrain query --terrain Caucasus find_sites '{"region": {"Circle": {"x": -281782, "z": 647279, "radius": 10000}}, "footprint": {"width": 60, "depth": 40}, "criteria": [{"name": "slope_max", "params": {"deg": 5}}, {"name": "surface_in", "params": {"classes": ["land"]}}], "spacing": 1000}'
```

| Questions | Operations |
|---|---|
| The ground at a point or in a region | `sample`, `point_info`, `geo`, `describe` |
| Where to put something | `score_site`, `find_sites`, `validate_placements` |
| What can be seen | `visible`, `viewshed`, `radar_floor`, `coverage`, `exposure`, `approach_spectrum`, `unmask_profile` |
| How to get there | `route`, `route_alternatives`, `chokepoints`, `trafficability` |
| What is there | `nearest`, `scenery_in`, `towns`, `airfield_info` |

Those that return geometry take `"geojson": true`, for any map program.
[docs/queries.md](docs/queries.md) describes each, and `dcsterrain schema`
prints every request, response and issue code as JSON Schema
(`--op <op>` for one).

Ask many questions at once with no operation: one `{"op", "request", "id"}`
per line on stdin, one answer per line on stdout, in order, the terrain opened
once. A bad line gets its own `issues` and the batch goes on:

```bash
printf '%s\n' '{"id": 1, "op": "nearest", "request": {"kind": "airdrome", "x": -281782, "z": 647279}}' '{"id": 2, "op": "visible", "request": {"a": {"x": -281782, "z": 647279, "alt_agl": 10}, "b": {"x": -280000, "z": 650000, "alt_agl": 100}}}' | dcsterrain query --terrain Caucasus
```

A failed request prints `{"issues": [{"code", "message", "path"}]}` and exits
1; a batch exits 0 unless its streams fail.

`dcsterrain index --terrain Caucasus` builds a cache ahead of time, among it
the road betweenness and the places, which take minutes and which a query never
builds: until then `chokepoints` and `towns` say so. `dcsterrain check`
reports what the tool reads of a terrain, and `dcsterrain cache list` and
`cache clear` show and delete the cache. `--threads <n>` caps the cores a
request takes, below normal priority so a DCS server beside it keeps the CPU.
`--help` lists the rest.

## Documentation

[docs/README.md](docs/README.md) indexes the design, the query operations and
the task graph.

## License

[MIT](LICENSE).
