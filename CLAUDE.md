# dcs-terrain

Tools that turn a DCS World theatre into one portable SQLite file, and answer
terrain siting and routing questions from it, with DCS doing no terrain work at
campaign runtime. The consumers are a Kotlin campaign process and an MCP
server, both going through one query engine.

The repository ships **tools only**. Users extract their own theatres.

## Start here

**Read [docs/STATE.md](docs/STATE.md) first.** It holds what was just done,
what is next, what follows, and the carries — the notes a later task must not
lose. It is the only record of where the work is.

Then check [docs/decisions/](docs/decisions/) for an ADR covering whatever you
are about to touch. The specs are frozen; the ADRs override them.

## Documents

[docs/README.md](docs/README.md) is the index. The short version:

- [docs/STATE.md](docs/STATE.md) — where the work is. You maintain it.
- [docs/plan.md](docs/plan.md) — the task graph. Tasks have ids (`C5b`, `X8a`),
  a deliverable, a done test, and dependencies. Work by task id, and edit it as
  the work changes shape.
- **[docs/spec/](docs/spec/) — the specification. Read
  [design-and-facts.md](docs/spec/design-and-facts.md) first**; every other
  document there cites it instead of restating it.
- **[docs/decisions/](docs/decisions/) — the ADRs. These override the specs.**

### Never touch `docs/spec/`

**Everything under `docs/spec/` is frozen: do not edit, rename or move any of
it.** That is the whole rule. Anything you would have changed there goes in
`docs/decisions/` as an ADR instead, and the ADR wins over the spec text.

What lives there: the seven component specs, `design-and-facts.md` (the shared
background every spec cites), the probe log (the measurements the
design rests on), `using-the-data.md` (metric definitions, and the source task
M1 generates `terrain_metrics_help` from), and `ledger/` — the claims ledgers
and glossaries that index them.

The reason it is a hard rule rather than a judgment call: those documents were
reviewed as one set, they cite each other by bare file name, and every
`ledger/` row quotes a verbatim line from them. One edit silently breaks joins
that nothing in this repository can rebuild. See
[ADR-0001](docs/decisions/0001-what-is-frozen.md).

Everything else — `docs/plan.md`, `docs/STATE.md`, both READMEs,
`docs/decisions/`, `docs/data/` — is yours to edit directly.

### Never cite `docs/` from code

Read the specs freely — that is what they are for. But **code never points back
at them**, with ADRs the single exception.

No file path, section title or document name from `docs/` appears in source
code, comments, docstrings, test names, error messages or commit messages. Not
`// per extract-format.md "Tile binary layout"`, not `# see design-and-facts.md`,
not a doc comment quoting a spec paragraph.

The reason is that the specs are frozen, so they will drift from the code as
ADRs accumulate. A citation is a claim that the named section still describes
what the code does, and nothing checks that claim — it rots silently and points
the next reader at text that has been superseded.

Write the reason instead of the reference. `// fill cells are nodata, so an
absent tile reads as sea rather than a hole` survives; a pointer to the
paragraph that says so does not. If the reason is too long for a comment, it is
a decision, and it belongs in an ADR.

**ADRs may be cited**, by id: `// ADR-0007: 100 m cells above 500 000 km²`. An
ADR is a stable, numbered record of one decision, it is superseded rather than
edited, and it is the thing that stays true as the specs age.

Prose documentation is different: `docs/README.md` and the root `README.md` are
navigation and may link into `docs/`. Prefer the index over a deep link to a
spec section.

### Maintaining `docs/STATE.md`

**Update it at the end of every working session, before handing back.** A
session that changed something and left STATE.md alone has lost that work.

- Add what you did to **Done**, newest first, and drop the oldest to keep 5.
- Move the task you finished out of **Next**, promote from **Then**, and refill
  **Then** from `plan.md` in dependency order. Keep 1 and 5.
- **Delete** every carry the session discharged. Do not mark it done, do not
  keep it for the record — git has the record.
- Add a carry only for something **learned while working** that no document
  already records, and only with the task id that will discharge it. If
  `plan.md`, a spec or an ADR already says it, it is not a carry — nor is a
  task's own done test, a rule from this file, or anything in `git log`. Most
  sessions add none. Hard cap 10; a full table, a carry with no discharging
  task, or one that has survived three turns of the Next slot means promote it
  to an ADR and delete the row.
- Update the **Last updated** date.

Do not restate the task graph, a spec, or an ADR in STATE.md — point at them.
The file stays under about 120 lines because it is read every session.

### Maintaining `README.md`

The root README is for a human who has never seen the project. It answers what
this is, how to get it, how to build it, how to use it, and the license —
nothing else. Keep it under about 80 lines; detail belongs in `docs/` behind a
link.

Update it in the same session that changes **the surface a user touches**: a
subcommand or flag they type, install or build steps, requirements, the
extract → pack → query pipeline, or the license. If you change that surface and
cannot update the README in the same session, add a STATE.md carry naming the
task that will.

- **Never document something that does not work yet without saying so.** The
  README currently carries a "not usable yet" banner because no code exists.
  Remove it when the binary can actually do what the page claims, not before.
- Do not put progress, task ids, milestones or a changelog in the README —
  STATE.md holds progress and git holds history. The one exception is the
  status banner, which is for the reader's benefit, not for tracking.
- Milestones MS2, MS4 and MS5 change what a user can do, so each gets a
  deliberate README pass.
- The usage examples must be real commands with real flag names from
  the core spec. Check them before writing them.

### Working with decision records

**Before acting on any frozen text, check `docs/decisions/README.md` for an ADR
covering it.** Where an ADR and a document disagree, the ADR wins; where two
ADRs disagree, the later accepted one wins. The index table is how ADRs are
found, so it is not optional.

Write an ADR when a measurement contradicts a document, when a specified
approach does not work or a done test cannot be met as written, when a
dependency or platform fact named in a spec turns out to be wrong, when a
field, name, default or threshold ends up different from the spec's, when
something the specs leave open gets settled, or when a new DCS build or theatre
is measured. No ADR is needed for work that does what the specs already say, or
for a plain task change — that is an edit to the plan.

To write one: copy `docs/decisions/0000-template.md` to
`NNNN-title-in-kebab-case.md` with the next free number, fill it in, and add
the row to the index. One decision per record. Consequences must include the
ones that hurt.

An ADR that is already committed is not rewritten — a decision that changes
gets a new ADR, and the old one's Status becomes `Superseded by ADR-NNNN`.
Before that first commit it may still be rewritten or dropped, so a decision
reversed the same day is consolidated rather than superseded.

**Propose an ADR to the user before writing it**, unless they have already
agreed to the decision. An ADR records a choice that binds later work; it is
not a note-to-self.

Do not put an ADR's content in a code comment or a commit message instead.
`docs/decisions/` is the only place divergence is recorded.


## Working rules

These are the plan's rules plus what the review established. They are not
optional; the phase-`sim` rule exists because that call crashed DCS.

### The DCS boundary

- **Never write into the DCS install.** It is read-only.
- **Know which build you are on before trusting any measurement.** The core
  build is `version` in `autoupdate.cfg` at the install root, with `timestamp`
  beside it. Never state a build from memory — read the file. The specs are
  written against one build, and `docs/spec/` names it; if the install reports
  a different one, the measurements are unverified on it and that is an ADR.
- **Verify every DCS symbol against ED's own code before calling it**, anchored
  on the full dotted path, and read ED's call site when the spec cites one. Do
  not write a call from memory.
- **Never call** `terrain.Init`, `terrain.Create`, `terrain.Release`,
  `terrain.InitLight` or `terrain.getCrossParam` from a probe.
- **Mission-state (`server`) `land` and `world` calls only while
  `dcs_bridge_status` reports phase `sim`.** A `land.getHeight` reaching the
  `server` state at the main menu crashes DCS with an access violation. Hook-side
  `terrain.*` calls are safe at the menu; they return nil.
- Before writing DCS Lua, read the design and facts document's "Sources you
  have" and "Facts that shape the design": environment choice, the
  `net.dostring_in` bridge, coordinates and the airdrome table are all settled
  there against measurement.

### Data

- **Never commit extracted or packed data.** No `*.sqlite`, no extract
  directories, no tile blobs. `docs/data/` holds measurements and a model
  census only.
- Extracts and packed files are derived from ED terrain data and stay on the
  user's machine; publishing them is the user's call against the ED EULA
  (task P4).

### Testing

- **Test against the synthetic theatre first.** `dcsterrain synth` writes a
  closed-form extract; almost every done test in the plan is stated against it.
- Real data is touched only in the opt-in local test group (gated on
  `DCSTERRAIN_EXTRACT`, skipped in CI) and the live acceptance steps
  (X10, X11, Q9, V1).

### Keeping the record true

- **A measurement that contradicts a frozen document becomes an ADR**, with the
  numbers and the DCS build string in it. A measurement that only lives in a
  conversation is lost.
- A change to the plan is an edit, not an ADR. Write an ADR alongside it only
  when the reasoning binds later work — as ADR-0003 does for task D1.
- A new DCS build or theatre gets a new `docs/spec/probe-log-<build>.md` and an
  ADR saying which probe log now applies. The existing one is not appended to.
- Design decisions in `design-and-facts.md` are decided unless a probe
  contradicts them. Do not relitigate them from first principles — and if a
  probe does contradict one, that is an ADR, not an edit.
- ADR prose matches the specs' style: plain words, one idea per sentence,
  active voice with a named actor, present tense, no narrative or changelog
  prose.
- Reworking a frozen document is possible but is a deliberate project, agreed
  with the user first: fold the accumulated ADRs into a revised document,
  regenerate its ledger and glossary, re-stamp, and freeze that. Never a side
  effect of another change.

## Facts worth having in hand

Cited so they can be checked, not so they replace reading the spec.

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

## Target layout

From task P1. None of it exists yet.

```
dcsterrain/          Cargo workspace
  crates/dcsterrain-core/    extract reader, packer, file reader, operations
  crates/dcsterrain-cli/     binary `dcsterrain`
  crates/dcsterrain-mcp/     MCP tool definitions over core
  tests/                     workspace integration tests (synthetic only)
extractor/           the Lua hook and its stub-harness tests
kotlin/              the campaign client module
tools/validate/      the validation sortie
tools/probe-theatre/ the per-map measurement, packaged
docs/                the frozen documents
docs/decisions/      ADRs — the only place divergence is recorded
```

X (extractor, Windows with DCS) and everything after F (the extract format
contract) can be built in parallel: both are tested against the same synthetic
theatre and neither needs the other to exist. The longest dependency chain
needs no DCS at all.

## Two machines

The work is split across platforms, and which one you are on decides what you
can do:

- **Windows, with DCS installed.** Component X (the Lua extractor hook), the
  V validation sortie through the bridge, and the live acceptance steps X10,
  X11, Q9 and V1. Anything that talks to a running DCS happens only here.
- **macOS.** Everything else — the Rust workspace, the query operations, the
  MCP server, the Kotlin client. No DCS, no GPU.

So do not assume a shell, a path separator or a tool is present because the
other machine has it. Check before relying on one, and keep scripts portable
where a task does not pin them to a platform.

