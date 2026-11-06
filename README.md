# dcs-terrain

`dcsterrain` answers questions about a DCS World terrain.
Where can a FARP go? What can a SAM site see? Which road should a convoy take?

## How it works

`dcsterrain` reads a terrain's files in your DCS install: the ground, the roads, the objects and the airfields.
From them, it works out heights, slopes, lines of sight and routes.
It never changes the files, and DCS does not need to be running.
It keeps its results in a cache folder, and this project ships no terrain files.

There are three ways to ask, and all three give the same answers:

- **Commands, for people.** One command per kind of question, with the answer in tables.
- **`query`, for software.** The question and the answer in JSON, a plain-text format for data.
- **`mcp`, for AI assistants.** The same questions, as tools an assistant calls through MCP.

## Get it

Download release 0.3.0.
On Windows, run these in PowerShell:

```powershell
Invoke-WebRequest https://github.com/tritiumgg/dcs-terrain/releases/download/v0.3.0/dcsterrain-x86_64-pc-windows-msvc.zip -OutFile dcsterrain.zip
```

```powershell
Expand-Archive -Force dcsterrain.zip -DestinationPath .
```

On macOS or Linux, copy part of a Windows install first, as the [guide](docs/guide.md#on-macos-or-linux) says.
You can also [build it yourself](#build-it-yourself).

## Use it

Run these in PowerShell, in the folder that holds `dcsterrain.exe`.
The tool finds DCS by itself; if it cannot, add `--install` and the DCS folder.
`list-terrains` shows your DCS version and the id, the short name, of each terrain:

```powershell
.\dcsterrain.exe list-terrains
```

Then ask a question, such as the airfield nearest to the town of Zugdidi:

```powershell
.\dcsterrain.exe nearest --terrain Caucasus --kind airdrome --at town:Zugdidi
```

```text
#  Distance (m)  Name           Class  Id        x       z  MGRS
1         33834  Senaki-Kolkhi  —      23  -281782  647279  38T KM 56437 80737
```

A place can be a DCS position such as `-284887,683859`, a grid reference such as `"38T KM 91980 72567"`, a latitude and longitude, `airport:Kutaisi` or `town:Zugdidi`; `.\dcsterrain.exe geo --terrain Caucasus --to-xz "38T KM 91980 72567"` turns a grid reference into DCS x and z.
`--help` after a command lists what it takes.
The [guide](docs/guide.md) walks through a whole siting question, step by step, and shows how software uses `query`.
[docs/queries.md](docs/queries.md) describes all 22 kinds of question.

## Use it from an AI assistant

`dcsterrain mcp` gives an assistant one tool for each kind of question, about every terrain you have.
The [guide](docs/guide.md#ask-through-an-ai-assistant) sets up Claude Code and Claude Desktop, with the [skill](skill/SKILL.md) that teaches the assistant the tools.

## Build it yourself

You need [git](https://git-scm.com) and [mise](https://mise.jdx.dev), which installs the Rust that `rust-toolchain.toml` names.

```bash
git clone https://github.com/tritiumgg/dcs-terrain
cd dcs-terrain
mise trust
mise install
mise exec -- cargo build --release --manifest-path dcsterrain/Cargo.toml
```

The program is `dcsterrain/target/release/dcsterrain`, or `dcsterrain.exe` on Windows.
To run the tests, use `mise run check` from Git Bash.

## License

[MIT](LICENSE).
