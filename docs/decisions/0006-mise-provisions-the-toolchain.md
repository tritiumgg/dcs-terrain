# ADR 0006: `mise` provisions the toolchain, and Windows builds its own Lua 5.1

## Status

Accepted

## Context

**Affects:** `core.md` "Workspace" (dependencies and the Rust it names);
`extractor-hook.md` (the Lua the offline tests run under);
`plan.md` tasks P1, P2, P3.

The frozen documents name the tools a build needs but not how a machine gets
them. `core.md` says only:

> Builds on Windows, macOS and Linux
> with stable Rust; CI builds all three.

`extractor-hook.md` needs Lua 5.1 for the offline tests, and
`tools/probe-theatre/fit.py` needs Python. Three toolchains, two developer
machines, three CI platforms, and nothing saying which versions.

The work is split across a Windows machine with DCS and a macOS machine
(ADR 0005), so "whatever is installed" means two different sets of versions,
and a version difference that only shows up on one of them is the kind of
failure that costs a session to find.

Lua is the hard case. The hook state runs Lua 5.1, and the offline tests exist
precisely to catch what 5.1 does differently: no `string.pack`, no `bit`, no
LuaJIT. The patch level is 5.1.5, measured on DCS build 2.9.29.27468
(`20260902-093323`): the install carries one Lua virtual machine, a
byte-identical `lua.dll` in `bin` and `bin-mt` that both `DCS.exe` import, and
the `luae.exe` beside it reports `Lua 5.1.5  Copyright (C) 1994-2012 Lua.org,
PUC-Rio`. That machine is stock — its number format is `%.14g` — and it carries
no LuaJIT. `mise`'s only Lua plugin builds 5.1.5 from source with `make`, which
fails on Windows. The obvious substitutes are worse than the problem. LuaJIT is
5.1-compatible but supplies `bit`, so it would hide the exact defects these
tests are for. The prebuilt LuaBinaries distribution is on SourceForge, whose
download hosts do not answer from the Windows machine. The GitHub repositories
carrying prebuilt Lua 5.1 binaries for Windows are third-party, mostly
unmaintained, and pinning one puts an unaudited binary in the toolchain.

## Decision

`mise` provisions the toolchain. `mise.toml` at the repository root pins
`python` `3.12` and Lua `5.1.5` on macOS and Linux. Rust is pinned in
`rust-toolchain.toml` beside it — an exact version, the `minimal` profile,
`clippy` and `rustfmt` — because rustup reads that file whether or not `mise`
is installed. `mise` is told to read it too, through
`idiomatic_version_file_enable_tools`, so Rust has one pin rather than two that
can disagree. On Windows the `lua51` task builds Lua 5.1.5 with MSVC instead,
from the official lua.org release verified against its published
SHA-256, and writes `lua.exe` into `.tools/bin`, which `mise` puts on `PATH`.
That directory is not committed. `tools/lua51/Build-Lua51.ps1` is the build; it
needs Visual Studio Build Tools with the C++ workload.

## Consequences

Both machines and all three CI runners build against the same versions, and a
contributor runs one command rather than installing three toolchains by hand.

The Lua the offline tests run under is the real 5.1.5 interpreter on every
platform, so a test that passes on the Windows machine means the same thing as
one that passes in CI. The Windows build matches the one DCS ships: run
side by side, ED's interpreter and this one agree line for line on number
formatting, `%q` escaping, `%d` overflow and the non-finite spellings `inf`
and `-nan(ind)`, which are the cases a different compiler would change.

Windows needs a C compiler to run the Lua tests at all. That is a real cost:
Visual Studio Build Tools is a large install, the script depends on `vswhere`
and on `vcvars64.bat` continuing to work as they do today, and a machine
without the C++ workload gets an error instead of an interpreter. It is paid
only on Windows, and only by someone running the extractor tests.

The Rust version is exact, so a P3 release binary is reproducible from the tag,
and `clippy` and `rustfmt` are present on every machine without a separate
install step. A contributor who has rustup but not `mise` still compiles
against the pinned toolchain, because `rust-toolchain.toml` is rustup's own
mechanism and needs nothing else to work.

The cost is that a Rust release does not arrive on its own. Someone edits
`rust-toolchain.toml` to take it, and a toolchain that is never bumped goes
stale quietly rather than breaking loudly.

P2 configures CI against these versions rather than choosing its own, and P3's
release builds inherit them. P1 carries `mise.toml`, the `lua51` task and the
build script.

### Alternatives considered

**Document the tools in the README and let each machine install them.** It
loses because it is what produces the version drift described above, and
because the Lua 5.1 problem does not go away — it just becomes every
contributor's problem instead of a task's.

**Pin LuaBinaries through `mise`'s `http` backend.** It is the obvious option:
a prebuilt binary, and no compiler on the Windows machine. It loses on
availability: SourceForge's download hosts time out from the
Windows machine, and its project pages serve an HTML interstitial that a
checksum pin would reject as a corrupt archive.

**Pin Rust in `mise.toml` alongside the other tools.** It is the consistent
shape, and it needs no extra file. It loses because rustup reads
`rust-toolchain.toml` regardless, so a project that pinned Rust only in
`mise.toml` would silently use a different compiler the moment anyone added
that file — and CI runners and IDEs read it before they read `mise.toml`.

**Use LuaJIT.** It installs cleanly on Windows through `mise`. It loses because
it provides `bit` and a different number implementation, so the tests written
to catch 5.1's absences would pass against capabilities the hook state does not
have.

**Vendor a prebuilt `lua.exe` in the repository.** It loses because the binary
would be unauditable in review, it grows the repository, and it has to be
rebuilt by hand for a new Lua patch release.
