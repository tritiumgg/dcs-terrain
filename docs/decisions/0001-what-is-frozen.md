# ADR-0001: `docs/spec/` is frozen; everything else is living

- **Status:** Accepted
- **Date:** 2026-09-03
- **Affects:** all documents in `docs/`; `plan.md` "Working rules", whose
  fourth bullet is edited to match.

## Context

Ten of these documents came out of a full review on 2026-09-02: 0 Blocker, 15
Major, 62 Minor, 67 findings applied, 10 deferred and then settled. They are
internally consistent, they cite each other by bare file name, and each carries
a claims ledger and glossary stamped with the SHA-256 of the document its rows
were read from. An anchor is a verbatim single line that must occur exactly
once in its document.

Editing one of them breaks that. The stamp goes stale and any anchor quoting a
rewritten line stops resolving. Re-verifying every row needs a linter this
repository does not have, so in practice an edited document means a ledger
nobody can trust and nobody will regenerate. They were also audited as a set, so a change to one can falsify
text in another that no one thinks to look at.

The task list is different in kind. It changes as the work reveals what the
work actually is. Freezing it would mean an ADR to add a task — a practice
abandoned within a month, after which nothing is recorded anywhere.

`design-and-facts.md` decides where the boundary sits. It reads like
background, but every spec cites it instead of restating it, so editing it
changes what seven specs mean without touching any of them. That is the
property the freeze protects. The probe log is raw measurement keyed to a
build, and `using-the-data.md` is an input to task M1, which generates
`terrain_metrics_help` from its "The metrics" section. All three are checked
against, not worked on, so all three sit with the specs.

## Decision

**Everything under `docs/spec/` is frozen: never edited, never renamed.** That
is the seven component specs, `design-and-facts.md`,
`probe-log-2.9.29.27278.md`, `using-the-data.md`, and `ledger/`.

**Everything outside `docs/spec/` is living** and is edited directly: `plan.md`,
`STATE.md`, both READMEs, `decisions/`, `data/`.

Anything that diverges from a frozen document is recorded as an ADR in
`docs/decisions/`, numbered from `0001`, using `0000-template.md`. Where an ADR
and a frozen document disagree, the ADR wins, and the most recent accepted ADR
wins over an earlier one. A change to the plan is an edit, not an ADR; write an
ADR alongside it only when the reasoning binds later work.

Measurements on a new DCS build or theatre are not edits: they go into a new
`docs/spec/probe-log-<build>.md`, announced by an ADR saying which probe log now
applies. A measurement contradicting the current probe log on the same build is
a divergence and gets its own ADR.

## Consequences

The rule is a directory, so it needs no judgment at the point of use: if the
path starts `docs/spec/`, do not write to it.

The ledger artifacts stay valid for the life of the project. A future change
review loads them directly instead of re-reading ten documents, and the
glossary — the part that cannot be rebuilt reliably later — keeps working.

Reading cost goes up on the frozen side: the text alone is no longer the
answer, and a reader must also check `docs/decisions/` for anything covering
the section they are in. `docs/decisions/README.md` is the index that keeps
that to one file.

The risk is a spec drifting far enough that the ADR set becomes the real
specification and the frozen document becomes misleading. The remedy is a
deliberate revision cycle, agreed with the user: fold the accumulated ADRs into
a new revised document, regenerate its ledger and glossary, and freeze that.
This ADR does not schedule one; it records that it is the only sanctioned way
to change frozen text.

## Alternatives considered

**Freeze everything, including the plan.** Puts the task list under the same
rule as the specification, so adding a task needs an ADR. The failure mode is
silent: people stop writing them and the plan drifts with no record at all.

**Freeze the seven specs only.** They delegate to `design-and-facts.md`, so it
is normative in everything but name, and an edit there is an untracked change
to seven specs.

**Edit the documents and regenerate the ledgers each time.** The correct
workflow when the tooling exists. It needs a linter and a stamper this
repository does not have, plus the discipline to regenerate every time. An
unregenerated ledger is worse than no ledger, because it looks authoritative.

**Edit the documents and abandon the ledgers.** Cheapest, and it throws away
the glossary, which the review says cannot be reconstructed later.
