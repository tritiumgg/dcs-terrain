# ADR 0003: The user guide is a separate document, written after the tools work

## Status

Accepted

## Context

**Affects:** `using-the-data.md`, which stays as it is; recorded in
`plan.md` as component D and task D1 at MS5.

The project needs a guide that teaches someone how to use the tools. The
obvious candidate to become that guide is `using-the-data.md`, which already
explains what every metric means and how a campaign turns metrics into easy,
medium, hard and unfair missions.

It cannot become that guide. `mcp-server.md` specifies that
`terrain_metrics_help` "returns the metric definitions of `using-the-data.md`
('The metrics') as structured data, one entry per field with its source
operation and its unit", so task M1 generates a tool response from that
section. `kotlin-consumer.md` cites the same document as the reason the
Kotlin module ships no difficulty tables of its own. The document is an input
to two implementations, not only prose, and rewriting it for readability would
change what the MCP server is specified to emit.

Writing the guide now would also mean writing it from the specs rather than
from the software. There is no binary, no real output, no error message and no
first-run experience to describe. A guide written from a specification
documents what was intended, and the gap between that and what shipped is
exactly what a user guide exists to close.

## Decision

The user guide is a new document, `docs/guide.md`, written after the tools
work. `using-the-data.md` stays frozen and stays where it is; the guide
links to it for metric definitions rather than restating them.

The work is task **D1** in `plan.md`, under a new component
**D: documentation**, in milestone MS5 alongside the release binaries and the
licensing note. It is written against real output from a real extract, never
against the specs. Its done test is that a reader who has not seen the project
can follow it to extract a theatre and answer one siting question.

Until D1 runs, the root `README.md` carries the whole user-facing story, and
its "not usable yet" banner is the honest statement of where things are.

## Consequences

The guide gets written once, from working software, instead of being drafted
now and rewritten when reality differs.

`using-the-data.md` keeps its contract with M1 and the Kotlin client intact,
and its ledger stamp stays valid.

The cost is that between now and MS5 the root `README.md` is the only
user-facing document, and it will be carrying more than it comfortably should
by the time D1 runs.

The task and its milestone live in `plan.md`; this record holds only
the reasoning, which is what a reader picking up D1 at MS5 will otherwise not
have.

### Alternatives considered

**Rewrite `using-the-data.md` as the guide.** Breaks the M1 contract, and
its ledger and stamp with it.

**Write the guide now from the specs.** Produces a document describing intended
behaviour, which must then be rewritten against the software. The first version
would also have no way to be checked, since its done test is that a reader can
follow it.

**Leave it to the README.** The README is bounded at about 80 lines on purpose.
A guide that teaches the extract-and-pack workflow, its failure modes and how
to read a score does not fit, and forcing it in costs the README the quality
that makes it useful — being short.
