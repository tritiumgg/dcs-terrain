# Documents

Every document that defines this project. No code exists yet; these are the
input to it.

[STATE.md](STATE.md) says where the work is — what was just done, what is next,
and the carries. It is the only record of project state, and it is maintained
every session. Read it first.

For the project itself, read [spec/design-and-facts.md](spec/design-and-facts.md)
first — every other spec cites it rather than restating it. Then the plan, then
the spec you are implementing. Then [decisions/](decisions/), which overrides
all of them.

## `spec/` — the specification, frozen

**Nothing in `spec/` is ever edited, renamed or moved**
([ADR-0001](decisions/0001-what-is-frozen.md)). Anything that diverges from it
is recorded in [decisions/](decisions/), and where an ADR and a spec disagree,
the ADR wins. Everything outside `spec/` is living and edited directly.

| File | What it is |
|---|---|
| [spec/design-and-facts.md](spec/design-and-facts.md) | What DCS exposes, what was measured, design decisions, worked campaign tasks, size budget. **Read first** |
| [spec/probe-log-2.9.29.27278.md](spec/probe-log-2.9.29.27278.md) | Raw measurements on DCS 2.9.29.27278 |
| [spec/using-the-data.md](spec/using-the-data.md) | What every returned metric means, and how a campaign turns metrics into difficulty. Task M1 generates `terrain_metrics_help` from its "The metrics" section |
| [spec/extract-format.md](spec/extract-format.md) | The directory the extractor writes and `pack` reads; the Lua/Rust contract |
| [spec/extractor-hook.md](spec/extractor-hook.md) | The Lua GameGUI hook that sweeps a theatre |
| [spec/core.md](spec/core.md) | The Rust workspace, packed file schema, derived layers, `pack`, `check` |
| [spec/query-operations.md](spec/query-operations.md) | The operations, criteria, and their tests |
| [spec/mcp-server.md](spec/mcp-server.md) | `dcsterrain serve` |
| [spec/kotlin-consumer.md](spec/kotlin-consumer.md) | How the campaign consumes the binary |
| [spec/validation.md](spec/validation.md) | The validation sortie and how to measure a new theatre |

Specs refer to each other by bare file name (`extract-format.md`). Those names
are load-bearing — `ledger/` rows use them as join keys — so a rename is the
procedure in [ADR-0002](decisions/0002-document-names-and-layout.md), never a
casual edit.

## Living documents

Edit these directly; no ADR needed.

| File | What it is |
|---|---|
| [plan.md](plan.md) | Components, tasks with dependencies, build order, milestones, working rules |
| [STATE.md](STATE.md) | Where the work is now |
| [decisions/](decisions/) | The ADRs |
| [data/](data/) | Measured reference data |

## `decisions/` — architecture decision records

Every divergence from the frozen documents, newest wins.
[decisions/README.md](decisions/README.md) has the index, the rules, and when
an ADR is required; `decisions/0000-template.md` is the template.

Check this directory before acting on any frozen text.

## `spec/ledger/` — claims ledgers and glossaries

One `-ledger.tsv` and one `-glossary.tsv` per document, from the 2026-09-02
review that produced them. The ledger is every discrete claim as a row
(`subject`, `kind`, `status`, `claim`, `section`, `anchor`); the glossary maps
every name a subject goes by onto one subject, which is how synonyms join.

A later review of a proposed change loads these **instead of re-reading the
whole document**, so they are working artifacts, not review leftovers. The
glossary in particular cannot be rebuilt reliably later — synonym mappings are
only discovered during a full read.

Each file's stamp lines name the document and its SHA-256. All twenty match
their document and stay matching, because `spec/` is frozen — that is most of
the reason for the freeze. The plan has no ledger: it is living, so its anchors
would break as it is edited, and a ledger nobody maintains is worse than none.

An anchor is a line that must occur exactly once in its document. That makes a
rename checkable without any tooling: substitute across the documents and their
ledgers together, re-stamp, then confirm every anchor still resolves.

Rows with status `UNVERIFIED` are claims that evidence could not settle. Six
remain, in the probe log, `using-the-data.md` and the core spec:

```bash
awk -F'\t' '$3=="UNVERIFIED" {print FILENAME": "$4}' docs/spec/ledger/*-ledger.tsv
```

`query-operations-renames.tsv` records the one cross-cutting rename the
review applied (`max_elev_deg` → `min_elev_deg`); the old term should not
reappear.

The documents came out of a full review on 2026-09-02, which is where these
ledgers and the rename came from. The review's own findings documents and
diffs are deliberately not in this repository — the documents are the result,
and the ledgers are the part that stays useful.

## `data/` — measured reference data

| Path | What it is |
|---|---|
| `caucasus-scenery-catalogue.tsv` | The whole-theatre scenery census: 888 859 objects over 263 models |
| `classes-Caucasus.toml` | Drafted scenery class overrides for Caucasus |

These are measurements and a model census, not extracted terrain data, so they
are committed. Extracts and packed files never are.

`classes-Caucasus.toml` moves to `dcsterrain/crates/dcsterrain-core/classes/Caucasus.toml`
when task C8a runs. Until then it lives here.

The measured projection samples that once sat here are not kept, and `fit.py`
now lives at `tools/probe-theatre/fit.py` —
[ADR-0004](decisions/0004-projection-samples-not-kept.md).
