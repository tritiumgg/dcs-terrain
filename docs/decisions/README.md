# Decision records

[`design.md`](../design.md) and [`queries.md`](../queries.md) state the design
and are edited directly. An ADR is written when a decision changes that design
and its reasoning is worth keeping beside it: a measurement that contradicts
the design, an approach that does not work, a platform fact that turns out
wrong, or a new DCS build or terrain measured differently. The design document is
updated in the same pull request to match.

No ADR is needed for work that does what the design already says, or for a
change to the plan.

## Rules

- Copy [`TEMPLATE.md`](TEMPLATE.md) to `NNNN-title-in-kebab-case.md` with the
  next free number, and add a row to the index below.
- Four headings: Status, Context, Decision, Consequences. Alternatives sit
  under Decision. Consequences include the ones that hurt.
- One decision per record. Cite one as `ADR NNNN`. Code may cite an ADR; it
  cites no other document.
- A committed ADR is not rewritten. A changed decision gets a new record, and
  the old one's Status becomes `Superseded by [ADR NNNN](...)`. Before its
  first commit a record may still be rewritten or dropped.
- Propose an ADR to the user before writing it, unless they have already
  agreed to the decision.

Numbering restarted at 0001 with the reset of 2026-10-27; the records before it
are in git at commit `4b85bd0`.

## Index

| ADR | Status | Date | Title |
|---|---|---|---|
| [0001](0001-read-file-headers-per-role-when-first-needed.md) | Accepted | 2026-10-27 | Read a terrain's file headers per role, when the role is first needed |
