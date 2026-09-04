# ADR 0002: Frozen documents live in `docs/spec/` under plain names

## Status

Accepted

## Context

**Affects:** `plan.md` task P1, which specifies `docs/` holding these
documents without naming a layout.

The documents arrived numbered for reading order — `00-project-plan.md`,
`01-design-and-facts.md`, `02-probe-log-2.9.29.27278.md`,
`03-using-the-data.md` — with the specs prefixed `spec-`. The numbers encode a
reading order that `docs/README.md` states anyway, and the `spec-` prefix
restates the directory. `docs/spec/spec-extract-format.md` stutters.

Renaming is not free. The documents cite each other by bare file name —
`extract-format.md` appears 23 times across the set — so a rename means
rewriting prose in every document that cites the renamed one. That prose is
what `ledger/` anchors quote verbatim, and the ledger rows also use file names
as `subject` join keys.

The cost is bounded, though, because it is checkable. An anchor is a line that
must occur exactly once in its document, so applying one substitution to the
documents and their ledgers together, then verifying every anchor still
resolves, proves the rename landed.

A second problem was the frozen boundary. Splitting the documents across
`docs/`, `docs/spec/` and `docs/reference/` meant a rule with a list of
exceptions, and a reader had to remember which side each file was on.

## Decision

Numbers and the `spec-` prefix are dropped. `00-project-plan.md` becomes
`docs/plan.md`.

**Everything frozen lives in `docs/spec/`**: the seven component specs,
`design-and-facts.md`, `probe-log-2.9.29.27278.md`, `using-the-data.md`, and
`ledger/`. Everything outside `docs/spec/` is living. The rule is the directory,
not a list.

A future rename follows the same procedure: substitute across the documents and
their ledgers in one pass, re-stamp each ledger and glossary with the new
SHA-256, then verify that every anchor still occurs exactly once in its
document. A rename that cannot pass that check is not made.

## Consequences

`docs/spec/` is the frozen set and the reading order is `docs/README.md`'s job.
CLAUDE.md carries one rule instead of a table.

The ten documents are no longer byte-identical to the reviewed originals — the
cross-references changed. All twenty ledgers and glossaries were re-stamped,
and all 2 231 anchors were verified to resolve uniquely afterwards.

`plan-ledger.tsv` and `plan-glossary.tsv` are deleted. The plan is living, so
its anchors break as it is edited — two already had. A ledger nobody maintains
is worse than none, and keeping it would have reintroduced the exception this
ADR removes.

Ledger files are named after their document, so a document renamed
later must have its ledger renamed with it.

### Alternatives considered

**Keep the reviewed names.** Preserves byte-identity with the originals and
needs no verification pass. Rejected: it keeps numbers that duplicate the
README's reading order and a prefix that duplicates the directory, permanently,
to avoid a one-off substitution that can be checked mechanically.

**Rename, but keep `docs/reference/` for the probe log and metric
definitions.** Sorts the documents more finely and costs a rule with
exceptions. The finer sort is worth less than the simpler rule.
