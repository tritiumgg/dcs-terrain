# dcs-terrain

Tools that turn a DCS World theatre into one portable SQLite file, and answer
terrain siting and routing questions from it, with DCS doing no terrain work at
campaign runtime. The consumers are an MCP server and a command line, both
going through one query engine.

The repository ships **tools only**. Users extract their own theatres.

## Layout

```
docs/STATE.md          where the work is: done, next, then, and the carries
docs/plan.md           the task graph. Tasks have ids. Changes. Not frozen
docs/spec/             frozen: nine documents, with ledger/ beside them
docs/decisions/        ADRs, and the only place divergence is recorded
docs/data/             measured reference data and a model census
dcsterrain/            Cargo workspace: core, cli, mcp                (partial)
extractor/             the Lua GameGUI hook and its offline Lua tests
tools/                 ledger.sh, statecheck.sh, nospecrefs.sh,
                       readmeopen.sh, hooktest.sh, luatest.sh
tools/lua51/           builds Lua 5.1.5 for Windows with MSVC
tools/probe-theatre/   the per-map measurement, packaged             (partial)
tools/validate/        the validation sortie                         (empty)
.claude/hooks/         the frozen-write guard, the shell guard, the commit
                       checks, the session start, the stop check
.github/workflows/     rust, lua, docs
```

Marked entries do not exist yet, or exist only in part. `docs/README.md` is the
index to `docs/`.

## Platforms

Developed on Windows, macOS and Linux from one checkout. Windows is
distinguished by one thing only: **DCS runs there**.

- **Windows, with DCS installed.** Component X (the Lua extractor hook), the V
  validation sortie through the bridge, and the live acceptance steps X10,
  X11, Q9 and V1. Anything that talks to a running DCS happens only here.
- **macOS and Linux.** Everything else — the Rust workspace, the query
  operations, the MCP server, and every document tool. No DCS, no GPU.

The longest dependency chain needs no DCS at all. So do not assume a shell, a
path separator or a tool is present because another platform has it: check
before relying on one, and keep scripts portable where a task does not pin
them.

## `docs/STATE.md` is the handoff between sessions

Read it before anything else. It names what was just done, what to pick up,
what follows, and what carries forward. A `SessionStart` hook prints it into
context, so it arrives without being asked for.

**Update it at the end of every working session, before handing back.** A
session that changed something and left it alone has lost that work. A `Stop`
hook refuses once when the tree is dirty and the stamp is not today's.

- Add what you did to **Done**, newest first, and drop the oldest to keep 5.
- Move the task you finished out of **Next**, promote from **Then**, and refill
  **Then** from `plan.md` in dependency order. Keep 1 and 5.
- **Delete** every carry the session discharged. Do not mark it done, do not
  keep it for the record — git has the record.
- Stamp **Last updated** with a bare UTC datetime, `YYYY-MM-DDTHH:MMZ`. It
  carries a datetime and nothing else: a status clause there repeats what
  **Done** says three lines below and goes stale on the next change, so
  `tools/statecheck.sh` refuses one.

Add a carry only for something **learned while working** that no document
already records, and only with the task id that will discharge it. If
`plan.md`, a spec or an ADR already says it, it is not a carry — nor is a
task's own done test, a rule from this file, or anything in `git log`. Most
sessions add none. A carry with no discharging task, or one that has survived
three turns of the Next slot, means promote it to an ADR and delete the row.

**Keep it small.** It is loaded cold every session, so its size is a tax on
every session. Each section has a line budget, `tools/statecheck.sh` enforces
them, and CI fails when the file is over. Over budget, nothing is deleted — it
moves. A completion older than the last five goes to `git log`. A choice with
reasoning behind it becomes an ADR. A durable fact about the project belongs in
this file, not that one. A discharged carry is deleted.

Never write a paragraph where a line will do, and never copy a fact into
`docs/STATE.md` that already lives here. Do not restate the task graph, a spec
or an ADR — point at them.

## `README.md` is for users, and the change that moves it updates it

The root README is for a human who has never seen the project. It answers what
this is, how to get it, how to build it, how to use it, and the license —
nothing else. Keep it under about 80 lines; detail belongs in `docs/` behind a
link.

Update it in the same session that changes **the surface a user touches**: a
subcommand or flag they type, install or build steps, requirements, the
extract → pack → query pipeline, or the license. If you change that surface and
cannot update the README in the same session, add a carry naming the task that
will.

**Never document something that does not work yet without saying so.** Where
the build has not reached a thing the README describes, the sentence says so,
on one line, with "not usable yet", "not final" or "planned".
`tools/readmeopen.sh` lists those lines, and the task that settles one takes
the note out. The README currently carries a "not usable yet" banner; remove it
when the binary can actually do what the page claims, not before.

Do not put progress, task ids, milestones or a changelog in the README —
`docs/STATE.md` holds progress and git holds history. The one exception is the
status banner, which is for the reader's benefit, not for tracking.

Milestones MS2, MS4 and MS5 change what a user can do, so each gets a
deliberate README pass. The usage examples must be real commands with real flag
names from the core spec. Check them before writing them. Every pull request
body answers the `README` heading of `.github/PULL_REQUEST_TEMPLATE.md`.

## `docs/spec/` is frozen. Nothing else is.

**Everything under `docs/spec/` is frozen: do not edit, rename or move any of
it.** That is the whole rule. Anything you would have changed there goes in
`docs/decisions/` as an ADR instead, and the ADR wins over the spec text.

What lives there: the nine documents, `design-and-facts.md` (the shared
background every spec cites), the probe log, `using-the-data.md` (the source
task M1 generates `terrain_metrics_help` from), and `ledger/` — the claims
ledgers and glossaries that index them.

The reason it is a hard rule rather than a judgment call: those documents were
reviewed as one set, they cite each other by bare file name, and every
`ledger/` row quotes a verbatim line from them. One edit silently breaks joins
that nothing in this repository can rebuild. A `PreToolUse` hook refuses the
edit, and `tools/ledger.sh lint` in CI catches one made another way.

Everything else — `docs/plan.md`, `docs/STATE.md`, both READMEs,
`docs/decisions/`, `docs/data/` — is yours to edit directly. The plan states
build order; edit it when the order changes.

**Check `docs/decisions/README.md` before acting on any frozen text.** Where an
ADR and a document disagree, the ADR wins; where two ADRs disagree, the later
accepted one wins. The index table is how an ADR is found, so it is not
optional.

A record has Michael Nygard's four headings and no others, is numbered
`NNNN-title-in-kebab-case.md`, and is cited as `ADR NNNN`. Copy `TEMPLATE.md`
and take the next free number. One decision per record; consequences must
include the ones that hurt. `docs/decisions/README.md` holds the rest.

Write an ADR when a measurement contradicts a document, when a specified
approach does not work or a done test cannot be met as written, when a
dependency or platform fact named in a spec turns out to be wrong, when a
field, name, default or threshold ends up different from the spec's, when
something the specs leave open gets settled, or when a new DCS build or theatre
is measured. No ADR is needed for work that does what the specs already say, or
for a plain task change — that is an edit to the plan.

A committed ADR is not rewritten. A decision that changes gets a new record,
and the old one's Status becomes `Superseded by ADR NNNN`. Before that first
commit it may still be rewritten or dropped, so a decision reversed the same
day is consolidated rather than superseded.

**Propose an ADR before writing it**, unless the user has already agreed to the
decision. An ADR records a choice that binds later work; it is not a
note-to-self. Do not put its content in a code comment or a commit message
instead — `docs/decisions/` is the only place divergence is recorded.

Reworking a frozen document is possible but is a deliberate project, agreed
with the user first: fold the accumulated ADRs into a revised document,
regenerate its ledger and glossary, re-stamp, and freeze that. Never a side
effect of another change.

## The ledger locates a claim

Read the specs freely — that is what they are for. But start from the ledger
beside each one, which holds a row per claim with an anchor that locates the
prose, so retrieval happens in `grep` and `awk` rather than by reading 680
lines to find a sentence.

```
tools/ledger.sh codes                     document codes and paths
tools/ledger.sh subjects CODE             every subject in a ledger
tools/ledger.sh find CODE <text>          rows matching subject, claim or section
tools/ledger.sh show CODE "<anchor>"      the prose around one anchor
tools/ledger.sh sections CODE             the heading tree with line counts
tools/ledger.sh read CODE "<section>"     one whole section
tools/ledger.sh lint                      every stamp, anchor and glossary join
```

Start from `subjects` or `find`, not from `sections`. The ledger is the index.

**The anchor beats the claim.** The anchor is verbatim specification text. The
claim beside it is a summary and can misread what it summarizes.

For a frozen document, `lint` is tamper detection rather than maintenance: a
`MISMATCH` means one was edited.

## Never cite `docs/` from code

**Code never points back at the documents**, with ADRs the single exception.

No file path, section title or document name from `docs/` appears in source
code, comments, docstrings, test names, error messages or commit messages. Not
`// per extract-format.md "Tile binary layout"`, not `# see design-and-facts.md`,
not a doc comment quoting a spec paragraph. Neither does a task id or the
plan's word "done test": tasks are ephemeral, the plan retires when the tools
ship, and a comment reading "task X3" then points at nothing.

The reason is that the specs are frozen, so they will drift from the code as
ADRs accumulate. A citation is a claim that the named section still describes
what the code does, and nothing checks that claim — it rots silently and points
the next reader at text that has been superseded.

Write the reason instead of the reference. `// fill cells are nodata, so an
absent tile reads as sea rather than a hole` survives; a pointer to the
paragraph that says so does not. If the reason is too long for a comment, it is
a decision, and it belongs in an ADR.

**ADRs may be cited**, by id: `// ADR 0007: 100 m cells above 500 000 km²`. An
ADR is a stable, numbered record of one decision, it is superseded rather than
edited, and it is the thing that stays true as the specs age.

`tools/nospecrefs.sh` enforces this, and CI runs it. Everything under `docs/`,
plus this file and any `README.md`, is exempt, because documents cite
documents. Prose documentation may link into `docs/`; prefer the index over a
deep link to a spec section.

## The DCS boundary

These are not optional; the phase-`sim` rule exists because that call crashed
DCS.

- **Never write into the DCS install.** It is read-only. `guard-bash.sh`
  refuses a shell write into it, in the quoted form a real install path takes.
- **Know which build you are on before trusting any measurement.** The core
  build is `version` in `autoupdate.cfg` at the install root, with `timestamp`
  beside it. Never state a build from memory — read the file. The install is
  normally ahead of the build `docs/spec/` names, and that gap is the usual
  state of the project, not a decision and not an ADR. What it means is that
  the probe log's figures are unverified on the build in front of you: carry
  the build string with any measurement you take, and confirm a figure through
  the bridge at the point of use rather than trusting the log for it. Only a
  measurement that actually differs is an ADR.
- **Verify every DCS symbol against ED's own code before calling it**, anchored
  on the full dotted path, and read ED's call site when the spec cites one. Do
  not write a call from memory.
- **Never call** `terrain.Init`, `terrain.Create`, `terrain.Release`,
  `terrain.InitLight` or `terrain.getCrossParam` from a probe.
- **Mission-state (`server`) `land` and `world` calls only while terrain is
  loaded, which is `terrain.GetTerrainConfig("id")` returning non-nil**
  (ADR 0010). A `land.getHeight` reaching the `server` state at the main menu,
  with no terrain, crashes DCS with an access violation. A mission is not
  required: with the Mission Editor open on a map those calls answer
  correctly. Do not use the bridge's `phase` field for this — it reports
  `menu` with the editor open on a map, so it cannot tell loaded terrain from
  none. Hook-side `terrain.*` calls are safe at the menu; they return nil.
- **Never load or use the `dcs-scripting` skill.** Symbol authority is the
  `dcs-api-lookup` skill for signatures and existence checks, the
  `dcs-api-bridge` MCP tools for anything a running DCS can answer, and the
  read-only install for ED's own call sites. Prefer the bridge over the index
  where both can answer, and the index over memory always.
- **`docs/spec/` still names `dcs-scripting`, and cannot be edited.** It
  appears in `design-and-facts.md` "Sources you have" and in
  `extractor-hook.md` at the header, the mission-pass sweeps and the encoders.
  Ignore those four citations. Nothing is lost by ignoring them: the
  cross-state rules and the `json()` output format are both stated inline
  beside their citation, and the stub harness the testing section names is not
  vendored.
- Before writing DCS Lua, read the design and facts document's "Facts that
  shape the design", and everything in "Sources you have" except the
  `dcs-scripting` entry: environment choice, the `net.dostring_in` bridge,
  coordinates and the airdrome table are all settled there against
  measurement.

**Say who verifies.** Most done tests in the plan need a running DCS install or
a person watching. Before starting a task, decide whether an agent can observe
the result itself, whether it needs a maintainer reading a CI result, or
whether only somebody at a live install can see it. Write it in
`docs/STATE.md` under the task. An agent that skips this declares victory on
something it never observed.

## Toolchain

Tool versions come from `mise.toml`. Do not install toolchains globally, and do
not use a language's own version manager directly.

Run every project command through mise, because a non-interactive shell does
not pick up mise's PATH activation:

```sh
mise exec -- cargo test
mise run check      # fmt, clippy, test
mise run lua-test   # the offline extractor tests under Lua 5.1
mise run docs       # the ledger, the state file, the citations, the hooks
```

`guard-bash.sh` refuses a bare `cargo`, `lua`, `rustc` or `rustfmt`. Anything
with a loop or a conditional lives in `tools/` and is invoked as `sh
tools/<name>.sh`: mise runs an inline script through `cmd` on Windows, which
does not speak sh.

**Rust is the exception, and `mise use rust@<version>` breaks it.** The
version, profile and components live in `rust-toolchain.toml`, which rustup
reads whether or not mise is installed. mise is told to read the same file, so
Rust has one pin rather than two that can disagree; `mise use` writes a second
version into `[tools]` and mise stops reading it. Edit `channel`, then
`mise install`.

On a fresh checkout, run `mise install` before anything else. On Windows,
`mise run lua51` builds the Lua 5.1.5 interpreter into `.tools/bin`, which
needs Visual Studio Build Tools with the C++ workload.

## Portability

Every script in `tools/` and `.claude/hooks/` is POSIX `sh` and `awk`, running
on all three platforms — on Windows through Git-for-Windows `sh`, which is why
`.claude/settings.json` invokes `sh` with the script as an argument rather than
executing it.

No bash arrays, no `[[`, no `local`. No `sed -i`, no `grep -P`, no
`readlink -f`. Avoid `sed` for tabs, because BSD and GNU disagree on `\t`.
Assume nothing beyond a stock machine: no `jq`, no `python`, no `gawk`. Detect
`sha256sum` versus `shasum`. `tools/lua51/Build-Lua51.ps1` is the one
PowerShell exception, because it drives MSVC.

`guard-bash.sh` refuses the non-portable commands and `precommit.sh` scans
every changed script, so this is checked rather than remembered.

Leave `.gitattributes` alone. It disables line-ending conversion, without which
every ledger stamp breaks on a Windows checkout.

## Data

- **Never commit extracted or packed data.** No `*.sqlite`, no extract
  directories, no tile blobs. `docs/data/` holds measurements and a model
  census only.
- Extracts and packed files are derived from ED terrain data and stay on the
  user's machine; publishing them is the user's call against the ED EULA
  (task P4).

## Testing

- **Test against the synthetic theatre first.** `dcsterrain synth` writes a
  closed-form extract; almost every done test in the plan is stated against it.
- Real data is touched only in the opt-in local test group (gated on
  `DCSTERRAIN_EXTRACT`, skipped in CI) and the live acceptance steps
  (X10, X11, Q9, V1).
- **The extractor is verified live, not against a vendored harness.** X1 to X4
  and X9 keep offline Lua tests, because encoders, grid arithmetic, the
  frame-budget driver and injected failures cannot be exercised against a real
  DCS on demand. The sweeps X5 to X8c are checked through the bridge against a
  running theatre. Nothing named `StubHarness.lua` is vendored into this
  repository.

## Version control

Conventional Commits: `type(scope): summary`, imperative, under 72 characters.
Add a body when the change needs explaining. `postcommit.sh` checks the subject
after the commit; the fix is `git commit --amend`.

**A branch per plan task**, named `task/<id>-<summary>`, such as
`task/X4-frame-budget`. Work that belongs to no task takes the `type` it would
commit under: `fix/`, `docs/`, `build/`, `ci/`.

**Every change is a pull request, and no pull request exceeds 400 lines of
change** — counting tests, and counting the lines it removes. Work larger than
that becomes a stack of branches, each branching off the one before it:
`task/X3-11-resume`, then `task/X3-12-bitmask`, landing in order.

The limit is about review, not tidiness. A 400-line diff gets read; a
2 000-line one gets approved. Splitting also forces the seams to be named
before the code is written, which is where most of the design argument actually
happens. A preparatory refactor always takes its own branch, claiming no change
in behaviour: sharing a diff with the feature hides which lines moved among the
lines that changed.

- **Every pull request in a stack stands on its own.** Its tests pass, so a
  reviewer can stop after any one of them and the repository still works.
- **Order a stack by dependency, not by size.** It is reviewed bottom up, so
  what everything else needs goes first.
- **The last branch in a stack carries the testing steps.** An intermediate
  branch names it instead. One procedure copied onto five branches becomes five
  procedures that drift.
- **History is linear. Rebase, never merge-commit.** Bring a branch up to date
  with `git rebase main`; land it with `git merge --ff-only`. If the
  fast-forward is refused, fix the branch. Git enforces this once
  `merge.ff = only` and `pull.ff = only` are set, which is a machine setting
  rather than a repository one:

  ```sh
  git config --global merge.ff only
  git config --global pull.ff only
  ```

- **A stack lands as one fast-forward, and the pull requests are retargeted
  first.** Merge each branch locally bottom-up, then push `main` once. Retarget
  every stacked pull request to `main` **before** that push: afterwards GitHub
  refuses, because the head is already contained in `main` and there is nothing
  to diff, and those requests can then only be closed rather than merged.
- **Deleting a remote branch is a prompt, not a reflex.** A local branch is
  cheap to restore from the reflog; a remote one is shared, and deleting the
  base of an open pull request closes it.
- Put a document edit in the pull request whose code changes it, not in a
  documentation pull request of its own. A `plan.md` row and the code that
  reshapes it are one reviewable thought.

Splitting a task across pull requests does not split it across sessions.
`docs/STATE.md` is updated in whichever pull request ends the session, whether
or not the task it names is finished.

**Every pull request body follows `.github/PULL_REQUEST_TEMPLATE.md`.** `gh pr
create --body` does not read the template, so write the body to its headings;
`guard-bash.sh` refuses one that is missing them. Summary ends with what the
change is reviewed against: the plan task, the ADR, or for a stacked branch the
claim that branch alone makes. Testing says how a reader runs the tests, not
that they were run. Its steps are numbered, start from a clean checkout, and
are grouped by phase (without DCS, then with DCS) and by platform (PowerShell
on Windows, bash on macOS and Linux). Each step is an imperative sentence: an
action, or a `Verify ...` naming what the tester sees when the steps before it
worked. Each heading reads `none` where nothing applies.

Do these without asking: branch, commit, rebase onto `main`, push a topic
branch, force-with-lease a topic branch that is yours, delete a branch that is
already merged, and open a pull request with `gh`.

**Ask first before merging a pull request, before pushing `main`, before
rewriting history that has been pushed, and before tagging.** A small diff and
a green CI run are not permission, and permission given for one pull request
does not carry to the next. `guard-bash.sh` turns each of those into a prompt
rather than refusing it.

## Subagents

Spawning a subagent is allowed, and worth it for work that fans out: sweeping
the specs for every place a claim appears, pressure-testing a plan before it is
executed, or reviewing a change against the frozen documents. Work you can
finish by reading three files you can already name is faster done directly.

- **A subagent never calls the DCS bridge.** The rules in "The DCS boundary"
  exist because one of those calls crashed DCS, and a running sim is the
  scarce resource on the Windows machine. Bridge calls stay in the session
  that is talking to the user.
- **A subagent never writes `docs/`.** Not an ADR, not `STATE.md`, not the
  plan. Those record decisions, and a decision is agreed with the user before
  it is written down. Reading all of it and reporting back is fine.
- **A subagent's report is a claim, not a source.** Check a number, a symbol
  or a quoted spec line before acting on it, the same as any other second-hand
  figure.

## Facts worth having in hand

Cited so they can be checked, not so they replace reading the spec.

- **Paths are discovered, never recorded.** `lfs.currentdir()` in the hook
  state returns the DCS install root and `lfs.writedir()` returns the Saved
  Games directory, both readable through the bridge at the menu. No install
  path belongs in this repository: the shipped tool takes one from the user
  as `dcsterrain check --install <dir>`, and a session that needs one asks
  the running sim.
- **Coordinates.** DCS x is north, z is east, metres. The terrain module takes
  `(x, z)` scalars; `land.*` takes `{x, y}` where `y` is DCS z, or `{x, y, z}`
  where `y` is altitude. Convert once at the bridge boundary and never pass a
  table to a terrain-module call.
- **Region bound.** The canonical form is an axis-aligned box in DCS metres
  `{minX, minZ, maxX, maxZ}`. Circles, polygons and airfield distance bands are
  convenience inputs converted to a box plus a post-filter.
- **Two version stamps.** The DCS core build (`autoupdate.cfg`) and a
  fingerprint of the terrain data files (`.surface5`, `.rn4`, `.scn5`) are
  versioned separately by ED. Every extract, packed file, query response and
  file name carries both.
- **Cell size.** The extract is always 50 m. The packed base is a pack
  parameter: 50 m under 500 000 km² of authored area, 100 m above.
- **Fill is `nodata`.** The extractor tests each cell against the exact fill
  triple before rounding. An absent tile is sea (`height` 0, `surface` 3), not
  a hole — reading absent tiles as `nodata` made every sightline across a bay
  fail.
- **Hook-state Lua is 5.1** with no `string.pack`, no `bit`, no LuaJIT, and file
  handles with no `seek` (sizes come from `lfs.attributes`).
- **No vegetation.** DCS exposes no tree layer to query, so the schema has no
  tree class. Concealment is terrain masking plus building density, stored
  separately and never fused.
