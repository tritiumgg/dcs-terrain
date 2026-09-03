# ADR-0000: Short present-tense title naming the decision

- **Status:** Proposed | Accepted | Rejected | Deprecated | Superseded by [ADR-NNNN](NNNN-title.md)
- **Date:** YYYY-MM-DD
- **Affects:** the frozen documents and sections this decision touches, e.g.
  `extract-format.md` "Tile binary layout", `plan.md` task C5b.
  Write `none` for a decision that adds to the project without diverging.

## Context

The forces at work. What was believed when the documents were written, what is
true now, and what changed: a measurement, a platform fact, a dependency, a
constraint discovered in implementation. Quote the frozen text this decision
answers, so a reader can see the divergence without opening the spec.

State facts, not conclusions. If a measurement drives this, give the numbers,
the DCS build string, and where the measurement is recorded.

## Decision

One paragraph in the active voice, present tense: "The extractor writes X."
Not "we should" or "it was decided". This paragraph is what an implementer
follows in place of the spec text, so it must be complete enough to act on.

## Consequences

What becomes true, easier, and harder. Include the ones that hurt — an ADR
that lists only benefits is not recording a decision, it is advertising one.

Name the tasks in `plan.md` whose done test changes, and any other
frozen document whose text now reads false.

## Alternatives considered

Each rejected option and the reason it lost. Omit this section only when there
was genuinely one option.
