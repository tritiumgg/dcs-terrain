# dcs-terrain

Turn a DCS World theatre into one portable SQLite file, and answer terrain
siting and routing questions from it — where to put a FARP, what a SAM site can
see, which road a convoy should take — without DCS doing any terrain work while
a campaign runs.

Answers come back as ranked candidates with the reasoning kept as separate
fields, through a command line or an MCP server for assistants.

**Tools only. No terrain data is shipped.** You extract your own theatres. An
extract is derived from Eagle Dynamics terrain data and stays on your machine;
publishing one is your own call against the ED EULA.

> **Not usable yet.** The specification is complete and the extractor is being
> rebuilt to read the terrain files directly, so of the commands below only
> `dcsterrain synth` and `dcsterrain check-extract` run today, and `pack` and
> `check` in part. The rest are the planned interface, kept here so the shape
> is visible. Progress is in [docs/STATE.md](docs/STATE.md).

## Requirements

Extracting needs a DCS World install, which does not have to be running: the
extractor reads the terrain files from it. It runs on Windows, macOS or Linux,
wherever it can read those files. Packing, querying and serving need no DCS and
no GPU.

Building needs [mise](https://mise.jdx.dev), which installs the Python pinned
in `mise.toml` and the Rust pinned in `rust-toolchain.toml`. The extractor's
tests also need Lua 5.1; on Windows, `mise run lua51` builds it and needs
Visual Studio Build Tools.

## Get it

Released builds will be static `dcsterrain` binaries for the three platforms,
attached to tags. To build instead:

```bash
git clone https://github.com/tritiumgg/dcs-terrain
cd dcs-terrain
mise trust
mise install
cargo build --release --manifest-path dcsterrain/Cargo.toml
```

The binary lands at `dcsterrain/target/release/dcsterrain`.

## Use it

Three steps: extract a theatre from your DCS install, pack it, then query it.
Only the first needs DCS, and only its files.

**Extract** a theatre from the install into a directory (planned, not usable
yet):

```bash
dcsterrain extract --install "/path/to/DCS World" --map Caucasus --out ./extract-caucasus
```

It reads the map's files and never writes into your DCS install. Four airdrome
fields only a running DCS knows — the ICAO code, and whether a field is civilian,
abandoned, and its class — come from an optional Lua hook run once at the
Mission Editor and passed to `extract` as `--hook <dir>`; without it they are
left empty (the hook's reduced form is planned, not usable yet).

Before moving an extract to another machine, check it; this lists every
problem with its file and exits 1 when there is one:

```bash
dcsterrain check-extract ./extract-caucasus
```

**Pack** the extract into a single file, then verify it. Today `pack` writes
the metadata, the height, water and surface grids, and the airfield, beacon,
radio, town and node tables, with no derived grids, scenery or roads yet, and
`check` reports the metadata and checks nothing else
(not usable yet):

```bash
dcsterrain pack ./extract-caucasus caucasus.sqlite
dcsterrain check caucasus.sqlite
```

**Query** it from the shell, or serve it to an assistant over MCP:

```bash
dcsterrain query caucasus.sqlite find_sites '{"...": "..."}'
dcsterrain serve caucasus.sqlite
```

The file is stock SQLite, so any client can read it. To try the tools without
DCS, `dcsterrain synth ./extract-synth` writes a synthetic theatre in place of
an extract.

## Understanding the results

[docs/spec/using-the-data.md](docs/spec/using-the-data.md) defines every metric
an operation returns and shows how a campaign turns them into easy, medium,
hard and unfair missions. Read it before acting on a score.

## Documentation

[docs/README.md](docs/README.md) indexes everything: the design and the
measurements behind it, the seven component specs, and the task graph.
[docs/decisions/](docs/decisions/) records anything that has since diverged
from them.

## License

[MIT](LICENSE).
