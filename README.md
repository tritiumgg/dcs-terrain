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

> **Not usable yet.** The specification is complete and the extractor hook is
> part built, so nothing below runs today. The commands are the specified
> interface, kept here so the shape is visible. Progress is in
> [docs/STATE.md](docs/STATE.md).

## Requirements

Extracting needs Windows with DCS World installed. Everything after that —
packing, querying, serving — runs on Windows, macOS or Linux with no DCS and no
GPU.

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
mise install
cargo build --release --manifest-path dcsterrain/Cargo.toml
```

The binary lands at `dcsterrain/target/release/dcsterrain`.

## Use it

Three steps: extract a theatre from DCS, pack it, then query it. Only the first
needs DCS.

**Extract.** Copy the hook to `Scripts/Hooks/DcsTerrainExtract.lua` in your DCS
Saved Games folder and put `enabled = true` in a config at
`Config/DcsTerrainExtract.lua`; without that it does nothing. Start DCS and open
the theatre in the Mission Editor — a mission is not needed, and one with a FARP
or heliport in it would leave holes in the scenery. A window then asks where to
write the extract and starts the run: it sweeps across frames, logs to
`Logs/DcsTerrainExtract.log`, resumes where it left off, and never writes into
your DCS install. *The window is not built yet; nothing runs today.*

**Pack** the extract into a single file, then verify it:

```bash
dcsterrain pack ./extract-caucasus caucasus.sqlite
dcsterrain check caucasus.sqlite
```

**Query** it from the shell, or serve it to an assistant over MCP:

```bash
dcsterrain query caucasus.sqlite find_sites '{"...": "..."}'
dcsterrain serve caucasus.sqlite
```

The file is stock SQLite, so any client can read it. `dcsterrain synth` writes a
synthetic theatre if you want to try the tools without DCS.

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
