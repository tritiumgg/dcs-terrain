# Decision records

Everything under `docs/spec/` is frozen
([ADR-0001](0001-what-is-frozen.md)); everything else in `docs/` is
living and edited directly. Everything that diverges from a frozen document is
recorded here.

**Where an ADR and a frozen document disagree, the ADR wins.** Where two ADRs
disagree, the later accepted one wins, and the earlier is marked
`Superseded by ADR-NNNN`.

## Index

| ADR | Status | Title | Affects |
|---|---|---|---|
| [0001](0001-what-is-frozen.md) | Accepted | `docs/spec/` is frozen; everything else is living | all documents; `plan.md` "Working rules" |
| [0002](0002-document-names-and-layout.md) | Accepted | Frozen documents live in `docs/spec/` under plain names | `plan.md` task P1 |
| [0003](0003-user-guide-is-a-separate-document.md) | Accepted | The user guide is a separate document, written after the tools work | `using-the-data.md`; task D1 at MS5 |
| [0004](0004-projection-samples-not-kept.md) | Accepted | The projection samples are not kept; `fit.py` moves to `tools/probe-theatre/` | `validation.md`, probe log — both name `projection-samples/` |
| [0005](0005-extractor-verified-live.md) | Accepted | The extractor is verified live; offline Lua tests are minimal | `extractor-hook.md` "Testing"; `plan.md` P1, P2, X1, X3–X9, MS0 |
| [0006](0006-mise-provisions-the-toolchain.md) | Accepted | `mise` provisions the toolchain, and Windows builds its own Lua 5.1 | `core.md` "Workspace"; `kotlin-consumer.md`; `extractor-hook.md`; `plan.md` P1, P2, P3 |
| [0007](0007-extract-format-v1.md) | Accepted | Extract format v1 is frozen, with measured field sources and a separate code for an unrecognised surface string | `extract-format.md` "Tile binary layout", "Tables", "manifest.json"; `design-and-facts.md` `Airdromes`; `plan.md` F1, X1, X5, C5c |
| [0008](0008-packed-meta-carries-the-authored-rectangle.md) | Accepted | The packed file's `meta` carries the authored rectangle, and its bounds keys are metres | `core.md` "Packed file"; `plan.md` C5a, C6 |
| [0009](0009-a-run-may-have-no-authored-rectangle.md) | Accepted | A run may have no authored rectangle, and both manifest keys are then null | `extract-format.md` "manifest.json"; `core.md` `valid`; ADR-0008's `check` clause; `plan.md` X3, X10, C3, C4, C5a |

Add a row when you add an ADR. This table is the index Claude reads, so an ADR
missing from it will not be found.

## Writing one

Copy `0000-template.md` to `NNNN-title-in-kebab-case.md`, taking the next free
number. Fill in Status, Date, Affects, Context, Decision, Consequences, and
Alternatives considered where there was a real alternative. Then add the row
above.

Keep one decision per record. Two decisions in one ADR cannot be superseded
independently, and one of them always turns out to need it.

An ADR is a point-in-time record. Once accepted **and committed**, its Context,
Decision and Consequences are not rewritten — a decision that changes gets a
new ADR, and the old one's Status becomes `Superseded by ADR-NNNN`. The Status
line is the only part of such an ADR that changes.

Before the first commit that carries it, an ADR may still be rewritten or
dropped. A decision reversed the same afternoon it was written never governed
any work, and a supersession chain recording that is noise. Consolidate into
what would have been written had it been right the first time, then commit.
Once it is in history, supersede instead.

## When an ADR is needed

- A measurement contradicts what a frozen document says.
- Implementation shows a specified approach does not work, or a done test in
  `plan.md` cannot be met as written.
- A dependency, version or platform fact named in a spec turns out to be wrong
  or unavailable.
- A field, name, default or threshold ends up different from the spec's.
- A task change carries reasoning that binds later work. The task itself is an
  edit to `plan.md`, not an ADR.
- Something the specs leave open gets settled — the `UNVERIFIED` ledger rows
  are the known list of these.
- A new DCS build or theatre is measured: the ADR says which probe log now
  applies.

An ADR is **not** needed for work that does what the specs already say, or for
detail the specs explicitly leave to implementation.
