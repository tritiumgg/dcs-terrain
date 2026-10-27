# dcs-terrain

Answer terrain siting and routing questions about a DCS World map — where to
put a FARP, what a SAM site can see, which road a convoy should take, how to
fly in low and unseen — straight from the map's own files in your DCS install,
without DCS running and without DCS doing any terrain work while a campaign
runs.

Answers come back as JSON, with ranked candidates and the reasoning kept as
separate fields, from one command line. An AI assistant with a shell learns to
use it from a skill shipped here.

**Tools only. No terrain data is shipped.** The tool reads your install in
place and never writes to it. It keeps a small, rebuildable cache beside it in
your user cache directory.

> **Not usable yet.** The project was redesigned to read the install directly,
> and the query engine is being built. The commands below are the planned
> interface, kept here so the shape is visible. Progress is in
> [docs/STATE.md](docs/STATE.md).

## Requirements

A DCS World install on the same machine, which does not have to be running.
On Windows the tool finds it from the registry. On macOS or Linux it needs a
copy of the map's files and `--install <dir>`. No GPU.

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

Ask a question about a map by its id (planned, not usable yet):

```bash
dcsterrain query --map Caucasus find_sites '{"...": "..."}'
```

The first question on a map builds its cache in seconds; later ones start
warm. Many questions can go in one call as JSON lines on stdin. To try it with
no install, use the synthetic map: `--map synth` (planned).

```bash
dcsterrain schema
dcsterrain check --map Caucasus
```

`schema` prints every operation's request and response; `check` reports what
the tool can read from a map and the state of its cache (both planned).

## Documentation

[docs/README.md](docs/README.md) indexes the design, the query operations and
the task graph.

## License

[MIT](LICENSE).
