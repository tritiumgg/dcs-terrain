# dcs-terrain

`dcsterrain` answers questions about a DCS World terrain.
Where can a FARP go? What can a SAM site see? Which road should a convoy take?

## How it works

A DCS terrain is a set of files: the ground, the roads, the objects and the airfields.
`dcsterrain` reads those files in your DCS install.
From them, it works out heights, slopes, lines of sight and routes.
It never changes the files.
DCS does not need to be running.
It keeps its results in a cache folder, so later questions are fast.
This project ships no terrain files.

You send a question as one line of JSON, a plain-text format for data.
The answer comes back as JSON too.
The [skill](skill/SKILL.md) teaches an AI assistant to ask the questions for you.

## Quick start

Download the latest release.
You can also [build it yourself](#build-it-yourself).
On Windows, run this in PowerShell:

```powershell
Invoke-WebRequest https://github.com/tritiumgg/dcs-terrain/releases/latest/download/dcsterrain-x86_64-pc-windows-msvc.zip -OutFile dcsterrain.zip
```

```powershell
Expand-Archive dcsterrain.zip -DestinationPath .
```

Run the commands below in the folder that holds `dcsterrain.exe`.
The tool finds DCS by itself.
If it cannot, add `--install <folder>` to name the DCS folder.
On macOS or Linux, copy part of a Windows install first.
The [guide](docs/guide.md#on-macos-or-linux) says which part.

`list-terrains` shows your DCS version and the id of each terrain.
You give that id to `--terrain`:

```powershell
.\dcsterrain.exe list-terrains
```

```text
DCS 2.9.31.29434 at C:\Program Files\Eagle Dynamics\DCS World
…
id                  folder          cache                 files
Caucasus            Caucasus        8 entries, 13.5 MiB   50 of 50 readable
GermanyCW           GermanyColdWar  6 entries, 265.1 MiB  462 of 462 readable
…
```

`index` builds a terrain's cache before your first question:

```powershell
.\dcsterrain.exe index --terrain Caucasus
```

```text
Caucasus 2.9.31.29434 at C:\Users\tritiumgg\AppData\Local\dcs-terrain\cache\Caucasus
mesh.toc: built
…
places: built
```

`query` answers a question.
`op` names the kind of question, and `request` holds its details.
The `|` passes the line to the tool.
This one asks for the airfield nearest to a point.
`x` points north and `z` points east, in meters:

```powershell
'{"op": "nearest", "request": {"kind": "airdrome", "x": -284900, "z": 683800}}' | .\dcsterrain.exe query --terrain Caucasus
```

```text
{"line":1,"op":"nearest","result":{…,"results":[{…,"distance":60.0606545632205,"id":"25","kind":"airdrome","name":"Kutaisi","x":-284887.375,"z":683858.71875}],"terrain":"Caucasus",…}}
```

Kutaisi is 60 m away.
The [guide](docs/guide.md) walks through a whole siting question.
[docs/queries.md](docs/queries.md) describes all 22 kinds of question.

Other commands:

- `check` reports which terrain files the tool can read.
- `cache list` shows the cache.
- `cache clear` deletes it. The tool builds it again when needed.
- `schema` prints the fields of every question and answer.
- `mcp --terrain <id>` answers the same questions as tools for an AI assistant.
- `--help` lists every command and option.

## Use it from an AI assistant

`dcsterrain mcp` gives an assistant one tool per kind of question through MCP, with no shell.
Release 0.1.0 has no `mcp`; until the next release, build the tool yourself.
The [guide](docs/guide.md#ask-through-an-ai-assistant) sets up Claude Code and
Claude Desktop, with the [skill](skill/SKILL.md) that teaches the assistant the tools.

## Build it yourself

You need [git](https://git-scm.com) and [mise](https://mise.jdx.dev).
mise installs the version of Rust that `rust-toolchain.toml` names.

```bash
git clone https://github.com/tritiumgg/dcs-terrain
cd dcs-terrain
mise trust
mise install
mise exec -- cargo build --release --manifest-path dcsterrain/Cargo.toml
```

The program is `dcsterrain/target/release/dcsterrain`.
On Windows, it is `dcsterrain.exe`.
To run the tests, use `mise run check` from Git Bash.

## License

[MIT](LICENSE).
