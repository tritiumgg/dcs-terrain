# dcs-terrain

`dcsterrain` answers questions about a DCS World terrain.
Where can a FARP go? What can a SAM site see? Which road should a convoy take?
Which ground does no radar cover?

It finds the answers in the terrain files of your DCS install.
DCS does not need to be running.
The tool only reads your DCS files.
It never changes them.
On macOS or Linux, it reads a copy of part of a Windows install.
The [guide](docs/guide.md#on-macos-or-linux) says which part.
It needs no GPU.

**This project contains no terrain files.** The tool reads the ones already in your DCS install.

## How it works

A DCS terrain is a set of files.
They hold the ground's shape, its roads, its objects and its airfields.
`dcsterrain` reads those files.
From them, it works out heights, slopes, lines of sight and routes.
It saves what it works out in a cache folder, so later questions are fast.

You type a command that carries a question, written in JSON.
JSON is a plain-text format for data.
The tool prints the answer, also in JSON.
Some answers hold shapes.
The tool can also give those as GeoJSON, a file format that map programs open.
The [skill](skill/SKILL.md) teaches an AI assistant to ask the questions for you.

## Quick start

Download, not usable yet: the first release, v0.1.0, is not published.
Until it is, [build it yourself](#build-it-yourself).
When it is, Windows users run this in PowerShell:

```powershell
Invoke-WebRequest https://github.com/tritiumgg/dcs-terrain/releases/latest/download/dcsterrain-x86_64-pc-windows-msvc.zip -OutFile dcsterrain.zip
```

```powershell
Expand-Archive dcsterrain.zip -DestinationPath .
```

In the folder that holds `dcsterrain.exe`, list your terrains.
The tool finds DCS by itself.
`--install <folder>` points it at another DCS folder.

```powershell
.\dcsterrain.exe list-terrains
```

Prepare a terrain before you ask about it:

```powershell
.\dcsterrain.exe index --terrain Caucasus
```

Ask a question, here for the airfield nearest to a point.
`x` and `z` are DCS coordinates in meters.
The question goes in on one line, after `|`:

```powershell
'{"op": "nearest", "request": {"kind": "airdrome", "x": -284900, "z": 683800}}' | .\dcsterrain.exe query --terrain Caucasus
```

The [guide](docs/guide.md) walks through a whole siting question, step by step.

## What the output looks like

`list-terrains` shows where DCS is and the id of each terrain.
These are its first rows:

```text
DCS 2.9.31.29434 at C:\Program Files\Eagle Dynamics\DCS World
cache at C:\Users\tritiumgg\AppData\Local\dcs-terrain\cache

id                  folder          cache                 files
Afghanistan         Afghanistan     6 entries, 110.1 MiB  60 of 60 readable
Caucasus            Caucasus        8 entries, 13.5 MiB   50 of 50 readable
GermanyCW           GermanyColdWar  6 entries, 265.1 MiB  462 of 462 readable
```

`index` builds the cache.
On the first run, each part says `built`.
On later runs, each part says `current`:

```text
Caucasus 2.9.31.29434 at C:\Users\tritiumgg\AppData\Local\dcs-terrain\cache\Caucasus
mesh.toc: built
scene.nodes: built
tables.json: built
roads.graph: built
roads.betweenness: built
places: built
```

`query` with `nearest` finds Kutaisi 60 m from the point.
Every answer also names the terrain and the DCS build.
It ends with a fingerprint of the terrain files that gave the answer:

```text
{"line":1,"op":"nearest","result":{"cell_size":50.0,"dcs_build":"2.9.31.29434","elapsed_ms":1,"results":[{"class":null,"distance":60.0606545632205,"id":"25","kind":"airdrome","name":"Kutaisi","x":-284887.375,"z":683858.71875}],"terrain":"Caucasus","terrain_fingerprint":"6293651208-203468160-1140069192"}}
```

`query` can ask 22 kinds of question.
Each kind is called an operation.
They cover choosing sites, lines of sight, radar cover, routes, towns and airfields.
[docs/queries.md](docs/queries.md) describes each one.
The other commands:

- `schema` prints what every request, answer and error code looks like.
- `check` reports which terrain files the tool can read.
- `cache list` shows the cache. `cache clear` deletes it.
- `--help` lists every command and option.

## Build it yourself

Building needs [git](https://git-scm.com) and [mise](https://mise.jdx.dev).
mise installs the Rust toolchain that `rust-toolchain.toml` pins.

```bash
git clone https://github.com/tritiumgg/dcs-terrain
cd dcs-terrain
mise trust
mise install
mise exec -- cargo build --release --manifest-path dcsterrain/Cargo.toml
```

The program lands at `dcsterrain/target/release/dcsterrain`, with `.exe` on Windows.
To check a change, `mise run check` runs the formatter, the linter and the tests.
On Windows, run `mise run` tasks from Git Bash.
[docs/README.md](docs/README.md) indexes the design documents.

## License

[MIT](LICENSE).
