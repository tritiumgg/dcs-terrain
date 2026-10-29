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
> and `query ... describe` work today; every other operation is planned, kept
> here so the shape is visible. Progress is in [docs/STATE.md](docs/STATE.md).

## Requirements

A DCS World install on the same machine, which does not have to be running.
On Windows the tool finds it from the registry; `--install <dir>` names
another. On macOS or Linux it needs a copy of the terrain's files and
`--install <dir>`. No GPU.

Building needs [mise](https://mise.jdx.dev), which installs the Rust pinned in
`rust-toolchain.toml`. The optional hook's tests also need Lua 5.1; on Windows,
`mise run lua51` builds it and needs Visual Studio Build Tools.

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

Check what the tool reads of a terrain, file by file, with its cache, tables
and scenery classes; it fails naming each file it cannot read:

```bash
dcsterrain check --terrain Caucasus
```

`dcsterrain cache list` shows the cache (`--cache <dir>` names another) and
the DCS build each terrain's entries were made on; after a DCS update, an
entry whose terrain files did not change is kept. `cache clear` deletes all
of it, asking before the live validation figures.
`dcsterrain index --terrain Caucasus` builds a cache ahead of time. `query`,
`index` and `list-terrains` take `--cache <dir>` too. `index --hook <dir>`
merges the airdrome fields only a running DCS knows, such as the ICAO code,
from the optional hook in `extractor/`; `--allow-crs-residual` accepts a
terrain whose projection fits its beacons more than 2 m off.

Ask a siting question the same way, `find_sites` with its JSON (planned, not
usable yet). The first question on a terrain will build its cache in seconds,
later ones start warm, and many can go in one call as JSON lines on stdin
(planned).
`dcsterrain schema` prints every operation's request and response as JSON
Schema.

## Documentation

[docs/README.md](docs/README.md) indexes the design, the query operations and
the task graph.

## License

[MIT](LICENSE).
