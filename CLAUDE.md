# dcs-terrain

A command-line tool that answers terrain siting and routing questions about a
DCS World map by reading the map's own files from the DCS install, read-only,
in place, with a small rebuildable cache beside it. DCS does no terrain work at
campaign runtime and need not be running. There is one interface,
`dcsterrain query`, JSON in and out; an assistant learns it from a skill.

The repository ships **tools only**. Users run them against their own install.

## Layout

```
docs/STATE.md          where the work is: done, next, then, and the carries
docs/design.md         the design: sources, decoders, cache, engine, CLI, targets
docs/queries.md        every operation, layer and criterion
docs/plan.md           the task graph. Tasks have ids
docs/decisions/        ADRs: decisions that change the design
docs/data/             measured reference data and a model census
dcsterrain/            Cargo workspace: core, cli                    (partial)
extractor/             the optional Lua GameGUI hook (tables pass) and its tests
skill/                 the skill an assistant loads                  (planned)
tools/                 statecheck.sh, nospecrefs.sh, readmeopen.sh,
                       hooktest.sh, luatest.sh
tools/lua51/           builds Lua 5.1.5 for Windows with MSVC
tools/validate/        the live validation sortie                    (empty)
.claude/hooks/         the shell guard, the commit checks, the session start,
                       the stop check
.github/workflows/     rust, lua, docs
```

`docs/README.md` is the index to `docs/`. The project was reset on 2026-10-27;
the documents before it are in git at commit `4b85bd0`.

## Platforms

Developed on Windows, macOS and Linux from one checkout. Windows is
distinguished by one thing only: **DCS runs there**.

- **Windows, with DCS installed.** The optional hook, the live validation
  sortie through the bridge, and the live steps of tasks marked *live* in
  `docs/plan.md`. The install's terrain files are here too, so the opt-in
  checks against a real install run here unless the files are copied.
- **macOS and Linux.** Everything else — the Rust workspace, the decoders, the
  query engine on the synthetic map, and every document tool. No DCS, no GPU.

Do not assume a shell, a path separator or a tool is present because another
platform has it: check before relying on one, and keep scripts portable.

## `docs/STATE.md` is the handoff between sessions

Read it before anything else. A `SessionStart` hook prints it into context.

**Update it at the end of every working session, before handing back.** A
`Stop` hook refuses once when the tree is dirty and the stamp is not today's.

- Add what you did to **Done**, newest first, and drop the oldest to keep 5.
- Move the task you finished out of **Next**, promote from **Then**, and refill
  **Then** from `plan.md` in dependency order. Keep 1 and 5.
- **Delete** every carry the session discharged. Git has the record.
- Stamp **Last updated** with a bare UTC datetime, `YYYY-MM-DDTHH:MMZ`, and
  nothing else; `tools/statecheck.sh` refuses a status clause.

Add a carry only for something **learned while working** that no document
already records, with the task id that will discharge it. Most sessions add
none. A carry that has survived three turns of the Next slot goes into
`design.md`, or an ADR, and the row is deleted.

**Keep it small.** Each section has a line budget, `tools/statecheck.sh`
enforces it, and CI fails when the file is over. Never copy a fact into
`STATE.md` that lives in this file or in `docs/`.

## The design documents are living

`docs/design.md`, `docs/queries.md` and `docs/plan.md` are edited directly, in
the pull request whose change moves them: a `plan.md` row and the code that
reshapes it are one reviewable thought. Nothing in `docs/` is frozen.

**An ADR records a decision that changes the design**, when its reasoning is
worth keeping: a measurement that contradicts the design, an approach that does
not work, a platform fact that turns out wrong, a new DCS build or map measured
differently. Update `design.md` or `queries.md` to match in the same pull
request. `docs/decisions/README.md` has the rules and the index. **Propose an
ADR to the user before writing it**, unless they have already agreed to the
decision. A committed ADR is not rewritten; a changed decision gets a new
record and supersedes the old.

## `README.md` is for users, and the change that moves it updates it

The root README answers what this is, how to get it, how to build it, how to
use it, and the license — under about 80 lines, detail behind a link into
`docs/`.

Update it in the same pull request that changes **the surface a user
touches**: a subcommand or flag, install or build steps, requirements, or the
license. **Never document something that does not work yet without saying
so**, on its line, with "not usable yet", "not final" or "planned";
`tools/readmeopen.sh` lists those lines. Remove the "not usable yet" banner
when the binary does what the page claims, not before. No progress, task ids or
changelog in the README. Each milestone gets a deliberate README pass, with
real commands and real flag names.

## Never cite `docs/` from code

**Code never points at a document**, with ADRs the single exception. No path,
section title or document name from `docs/` in source, comments, test names,
error messages or commit messages, and no task id or the phrase "done test":
documents move and tasks retire, and a citation then points at nothing. Write
the reason instead: `// a road strip wins, because the game reports the road
where one runs under a mountain` survives; a pointer to the paragraph does not.
Cite an ADR by id where the reason is too long for a comment.

`tools/nospecrefs.sh` enforces this, and CI runs it. Everything under `docs/`,
this file and any `README.md` are exempt.

## The DCS boundary

These are not optional; the server-state rule exists because a call crashed
DCS.

- **Never write into the DCS install.** It is read-only. `guard-bash.sh`
  refuses a shell write into it.
- **Never open a `.crypt` file.**
- **Know which build you are on before trusting a measurement.** The core build
  is `version` in `autoupdate.cfg` at the install root, with `timestamp`
  beside it. Never state a build from memory — read the file, and carry the
  build string with any measurement you take.
- **Verify every DCS symbol against ED's own code before calling it**, anchored
  on the full dotted path, and read ED's call site. Do not write a call from
  memory.
- **Never call** `terrain.Init`, `terrain.Create`, `terrain.Release`,
  `terrain.InitLight` or `terrain.getCrossParam` from a probe.
- **Mission-state (`server`) `land` and `world` calls only while terrain is
  loaded**, which is `terrain.GetTerrainConfig("id")` returning non-nil. A
  `land.getHeight` reaching the `server` state at the main menu crashes DCS
  with an access violation. A mission is not required: with the Mission Editor
  open on a map those calls answer correctly. The bridge's `phase` field
  reports `menu` with the editor open on a map, so it cannot tell. Hook-side
  `terrain.*` calls are safe at the menu; they return nil.
- **Never load or use the `dcs-scripting` skill.** Symbol authority is the
  `dcs-api-lookup` skill for signatures and existence checks, the bridge for
  anything a running DCS can answer, and the read-only install for ED's own
  call sites. Prefer the bridge over the index, and the index over memory.

**Say who verifies.** Before starting a task, decide whether an agent can
observe the result itself, whether it needs CI, or whether only somebody at a
live install can see it; `plan.md` marks each task, and `STATE.md` repeats it
under Next. An agent that skips this declares victory on something it never
observed.

## Toolchain

Tool versions come from `mise.toml`. Do not install toolchains globally, and do
not use a language's own version manager directly. Run every project command
through mise, because a non-interactive shell does not pick up its PATH:

```sh
mise exec -- cargo test
mise run check      # fmt, clippy, test
mise run lua-test   # the hook's offline tests under Lua 5.1
mise run docs       # the state file, the citations, the hooks
```

`guard-bash.sh` refuses a bare `cargo`, `lua`, `rustc` or `rustfmt`. Anything
with a loop or a conditional lives in `tools/` and is invoked as
`sh tools/<name>.sh`: mise runs an inline script through `cmd` on Windows.

**Rust's pin is `rust-toolchain.toml`, and `mise use rust@<version>` breaks
it** by writing a second version into `mise.toml`. Edit `channel`, then
`mise install`.

On a fresh checkout, run `mise trust` and then `mise install` first.

**On Windows, run mise tasks from Git Bash**: a task body is `sh tools/...`,
and `cmd` finds `sh` only on a PATH Git Bash has set. `mise run lua51` builds
the Lua 5.1.5 interpreter into `.tools/bin` and needs Visual Studio Build
Tools with the C++ workload.

## Portability

Every script in `tools/` and `.claude/hooks/` is POSIX `sh` and `awk`, running
on all three platforms — on Windows through Git-for-Windows `sh`, which is why
`.claude/settings.json` invokes `sh` with the script as an argument.

No bash arrays, no `[[`, no `local`. No `sed -i`, no `grep -P`, no
`readlink -f`. Avoid `sed` for tabs. Assume nothing beyond a stock machine: no
`jq`, no `python`, no `gawk`. Detect `sha256sum` versus `shasum`.
`tools/lua51/Build-Lua51.ps1` is the one PowerShell exception, because it
drives MSVC. `guard-bash.sh` and `precommit.sh` check this.

Leave `.gitattributes` alone: it disables line-ending conversion, without
which a script checked out on Windows gains carriage returns and `sh` fails on
it.

## Data

- **Never commit terrain data.** No cache entries, no extract directories, no
  tile blobs, no fixture cut from an ED file. `docs/data/` holds measurements
  and a model census only.
- The cache lives in the user's cache directory, never in the repository.

## Testing

- **Test against the synthetic map first.** `--map synth` answers from a
  closed-form theatre in memory; almost every done test is stated against it.
- **The decoders** have offline tests on byte buffers the test builds, and an
  opt-in group gated on `DCSTERRAIN_INSTALL` that reads a real install,
  comparing against the live Caucasus extract when `DCSTERRAIN_EXTRACT` names
  it. Both are skipped in CI.
- **The hook is verified live**, with offline Lua tests for its encoders and
  driver. Nothing named `StubHarness.lua` is vendored.

## Version control

Conventional Commits: `type(scope): summary`, imperative, under 72 characters,
with a body when the change needs explaining. `postcommit.sh` checks the
subject; the fix is `git commit --amend`.

**A branch per plan task**, `task/<id>-<summary>`, such as `task/N3-mesh`. Work
that belongs to no task takes its commit `type`: `fix/`, `docs/`, `build/`,
`ci/`.

**One pull request per task, reviewed commit by commit.** A large task is a
series of small commits on one branch. Commit each slice as soon as its test
passes; each commit does one thing and leaves the tree passing
`mise run check`. About 100 changed lines is easy to review, about 300 is fine
for one logical change, 1 000 is split. Formatting and behavior, a refactor and
a feature, and an unrelated fix each take their own commit.

**Stage paths by name, never `git add -A`, `-u` or `.` and never
`git commit -a`, and read the staged diff before writing the message.**
`guard-bash.sh` refuses the blanket forms.

- **History is linear. Rebase, never merge-commit.** Update with
  `git rebase main`; land with `git merge --ff-only`. Set
  `git config --global merge.ff only` and `pull.ff only`.
- **Deleting a remote branch is a prompt, not a reflex.**

**Review the branch locally, then push.** One adversarial reviewer, on Sonnet,
read-only, told the claim the branch makes and asked to break it; every
finding fixed; then `mise run check`, `mise run lua-test` and `mise run docs`.
Only then push and open the pull request.

`docs/STATE.md` is updated in the task's own pull request, in whichever session
ends, including a session that stops mid-task.

**Every pull request body follows `.github/PULL_REQUEST_TEMPLATE.md`**;
`gh pr create --body` does not read it, and `guard-bash.sh` refuses a body
missing its headings. Summary ends with what the change is reviewed against:
the plan task or the ADR. Testing says how a reader runs the tests, in
numbered imperative steps from a clean checkout, grouped by phase (without
DCS, then with DCS) and platform (PowerShell on Windows, bash on macOS and
Linux), each command in its own code block. A heading with nothing to say
reads `none`.

Do these without asking: branch, commit, rebase onto `main`, push a topic
branch, force-with-lease a topic branch that is yours, delete a merged branch,
and open a pull request with `gh`.

**Ask first before merging a pull request, pushing `main`, rewriting pushed
history, or tagging.** Permission for one pull request does not carry to the
next. `guard-bash.sh` turns each into a prompt.

## Subagents

Worth it for work that fans out: sweeping the code for every place a rule
applies, pressure-testing a plan, or reviewing a change. Work you can finish by
reading three files you can name is faster done directly.

- **A subagent never calls the DCS bridge.** A running sim is the scarce
  resource, and one call crashed it.
- **A subagent never writes `docs/`.** Decisions are agreed with the user
  before they are written down. Reading and reporting is fine.
- **A subagent's report is a claim, not a source.** Check a number, a symbol or
  a quote before acting on it.

## Facts worth having in hand

- **Paths are discovered, never recorded.** On Windows the install root is the
  registry value `HKCU\Software\Eagle Dynamics\DCS World\Path`. Through the
  bridge, `lfs.currentdir()` in the hook state is the install root and
  `lfs.writedir()` the Saved Games directory. The shipped tool takes an
  install as `--install <dir>` where there is no registry.
- **A map is named by its `entry.lua` id**, not its folder: Germany's folder
  is `GermanyColdWar`, its id `GermanyCW`.
- **Coordinates.** DCS x is north, z is east, meters. The terrain module takes
  `(x, z)` scalars; `land.*` takes `{x, y}` where `y` is DCS z, or `{x, y, z}`
  where `y` is altitude. Convert once at the bridge boundary.
- **Region bound.** The canonical form is an axis-aligned box in DCS meters
  `{minX, minZ, maxX, maxZ}`; circles, polygons and airfield bands become a box
  plus a post-filter.
- **Two stamps.** The DCS core build (`autoupdate.cfg`) is the version, since
  ED ships terrain only inside updates; the terrain files' sizes are a
  fingerprint of what answered. Every response carries both.
- **Every map file states its format**: a container header, a class name and a
  format version. A decoder declares the pairs it reads, and an unknown one
  refuses the map.
- **Hook-state Lua is 5.1** with no `string.pack`, no `bit`, no LuaJIT, and file
  handles with no `seek`.
- **DCS's own sightline ignores buildings and trees.** So does `visible`.
