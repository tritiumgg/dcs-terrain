# dcs-terrain

Turn a DCS World theatre into one portable SQLite file, and answer terrain
siting and routing questions from it — where to put a FARP, what a SAM site can
see, which road a convoy should take — without DCS doing any terrain work while
a campaign runs.

Answers come back as ranked candidates with the reasoning kept as separate
fields, through a command line, an MCP server for assistants, or a Kotlin
client.

**Tools only. No terrain data is shipped.** You extract your own theatres. An
extract is derived from Eagle Dynamics terrain data and stays on your machine;
publishing one is your own call against the ED EULA.

> **Not usable yet.** The specification is complete and implementation has not
> started, so nothing below runs today. The commands are the specified
> interface, kept here so the shape is visible. Progress is in
> [docs/STATE.md](docs/STATE.md).

## Requirements

Extracting needs Windows with DCS World installed. Everything after that —
packing, querying, serving — runs on Windows, macOS or Linux with no DCS and no
GPU. Building needs stable Rust; the Kotlin client additionally needs a JDK.

## Get it

Released builds will be static `dcsterrain` binaries for the three platforms,
attached to tags. To build instead:

```bash
git clone https://github.com/tritiumgg/dcs-terrain
cd dcs-terrain
cargo build --release
```

The binary lands at `target/release/dcsterrain`.

## Use it

Three steps: extract a theatre from DCS, pack it, then query it. Only the first
needs DCS.

**Extract.** Copy the hook to `Scripts/Hooks/DcsTerrainExtract.lua` in your DCS
Saved Games folder and a config to `Config/DcsTerrainExtract.lua`. It does
nothing until the config enables it. Start DCS, load a mission on the theatre,
and it sweeps across frames, logging to `Logs/DcsTerrainExtract.log`. It never
writes into your DCS install, and it resumes where it left off.

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

[docs/spec/using-the-data.md](docs/spec/using-the-data.md) defines every metric an
operation returns — what it measures, its units, and which operation produces
it — and shows how a campaign turns those metrics into easy, medium, hard and
unfair missions. Read it before acting on a score.

## Documentation

[docs/README.md](docs/README.md) indexes everything: the design and the
measurements behind it, the seven component specs, and the task graph.
[docs/decisions/](docs/decisions/) records anything that has since diverged
from them.

## License

[MIT](LICENSE).
