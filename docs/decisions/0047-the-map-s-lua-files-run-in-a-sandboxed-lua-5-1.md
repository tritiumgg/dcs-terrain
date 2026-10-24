# ADR 0047: The map's Lua files run in a sandboxed Lua 5.1

## Status

Accepted

## Context

**Affects:** ADR 0039 "beacons.json", "radio.json", "nodes.json" and
"towns.json"; `extract-format.md` "towns.json", "nodes.json";
`extractor-hook.md` "Hook-pass sweeps"; `plan.md` E6.

ADR 0039 has the decoders read the map's Lua files as data:

> **beacons.json**, **radio.json**, **nodes.json**: v1's keys, read from
> `Beacons.lua`, `Radio.lua` and `MissionGenerator/nodes.lua` as data, never
> executed (E6).

It gives no reason and weighs no alternative. The frozen format had the
extractor read them the other way:

> the extractor reads it
> with `dofile` in a sandboxed environment

E6 first did as ADR 0039 says, with a reader written for the purpose: a
lexer and parser for the subset of Lua 5.1 the files use, about 1 000 lines
with its tests. It read every file on all eight maps of DCS **2.9.30.28738**,
and Caucasus's tables equal the live extract. A review of it found Lua
semantics it had got wrong in ways the files do not reach yet: line breaks
inside strings, a positional item against an explicit key, and error lines.
It also read ED's shared scripts (`BeaconTypes.lua`, `FrequencyBands.lua`,
`wsTypes.lua`) for their constants by scanning for `NAME = number` lines,
because they hold code that the reader could not run. Each such gap is a way
for a later ED file to be misread rather than refused.

The files are Lua 5.1 source, and Lua 5.1 is a small C library. The `mlua`
crate builds it from source into the binary (`lua51` and `vendored`) and runs
on all three platforms. Building it needs a C compiler. Its current version,
0.12, needs Rust 1.88, so the workspace's declared minimum rises from 1.85;
the pinned toolchain is 1.98.1.

## Decision

`dcsterrain extract` runs each of `Beacons.lua`, `Radio.lua`, `Map/towns.lua`
and `MissionGenerator/nodes.lua` in Lua 5.1, built into the binary, and reads
back the globals it sets. Each file runs in a fresh environment holding
`assert`, `error`, `ipairs`, `next`, `pairs`, the `raw` functions, `select`,
`setmetatable`, `getmetatable`, `tonumber`, `tostring`, `type`, `unpack`, and
the `string`, `table` and `math` libraries. Nothing in it reaches a file, the
operating system or another environment: there is no `io`, `os`, `debug`,
`load`, `loadfile`, `loadstring`, `getfenv` or `setfenv`. Nothing in it can
outrun the caps below either: there is no `pcall`, which would catch the
instruction cap's error and carry on, and the state's `string` table has no
`find`, `match`, `gmatch` or `gsub`, whose pattern matching runs in C, where
no instruction is counted, and can take exponential time on a short pattern.
It also has no `dump`, and only source is loaded, never bytecode, which Lua
5.1 does not verify. No ED file calls any of them. `dofile(path)` runs a file of the
install, found without regard to case and never outside it, in the same
environment. `require("i_18n")` returns a table whose `translate` returns its
argument; any other module is an empty table and is reported. `package` is a
table with empty `path` and `cpath`. A global that nothing assigned reads as
nil, as in the game, and its name is reported. A run stops with an error
after 200 000 000 instructions or 512 MB, and an error stops the extract with
Lua's message, which names the file and line. Values come back as `nil`,
booleans, numbers, strings and tables, at most 1 000 000 of them counting a
table each time it is reached, and nested at most 64 deep; anything else
reads as absent.
Comments are not values, so `Radio.lua`'s labels are read from the text: the
comment on the line before each `radioId = '…'`.

### Alternatives considered

**Keep the reader written for the purpose (ADR 0039 as written).** It had
been built and checked against the live extract. It lost because it is a
second implementation of Lua to maintain, it is right only for the files it
was tested on, and Lua itself is available and authoritative. The maintainer
chose against owning a Lua parser.

**Ask a running DCS through the hook's tables pass.** That is the game's own
Lua. It lost because it needs DCS running, which ADR 0034 removed from
extraction, and runs only on the Windows machine.

**A Lua implementation written in Rust.** It would avoid the C compiler. It
lost on fidelity: the C library is the reference Lua 5.1, the version the
files are written for, and any reimplementation carries the risk this
change removes, only in someone else's code.

## Consequences

The files are read by the language they are written in, so a construct ED
adds later is read as the game reads it, not refused or misread. ED's shared
scripts run in full rather than being scanned, so a constant defined by code
reaches the files as it does in the game. The reader shrinks from about
1 000 lines to about 540, of which 175 are tests.

Building `dcsterrain` now needs a C compiler on every platform, in CI and for
anyone building from source. Linux and macOS have one by default; Windows
already needs Visual Studio Build Tools for the `lua51` task. The workspace's
declared minimum Rust is 1.88.

The extractor now runs ED's code. It is local, read-only and installed by the
user, and the sandbox bounds it: no file or system access, and a cap on
instructions and memory. The caps count Lua instructions and allocations, so
a library function that spends time in C without allocating is outside them.
The ones that could do so without bound are taken out, and a later ED file
that calls one is refused, not read. The remaining C functions finish in
time about proportional to the memory they touch, which the memory cap bounds.

The stand-ins show in the report. On every installed map the decode reports
`require("utils")` from `BeaconTypes.lua`, and `SA2_SR`, which ED's
`wsTypes.lua` reads without defining; neither reaches a table. GermanyCW
also reports `BEACON_TYPE_AIRPORT_TACAN`, as before.

`Radio.lua`'s labels depend on ED's layout, a comment on the line before the
`radioId`. That holds on every installed map; a label written elsewhere would
leave its airdrome's id null.

E6's done test is unchanged. Its plan row reads "run in a sandboxed Lua 5.1"
in place of "read without executing the files".
