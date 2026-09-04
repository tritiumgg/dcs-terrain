# ADR 0013: The manifest records one `complete`, not per-pass records

## Status

Accepted

## Context

**Affects:** `extract-format.md` "manifest.json"; ADR 0007's frozen manifest
example; ADR 0010's clause keeping `passes.mission`; `plan.md` X4, C3, C12b.

The manifest carries a `passes` object with a record for each of the two passes:

```json
"passes": {
  "hook":    { "complete": true, "started_at": "...", "finished_at": "...", "frames": 41203 },
  "mission": { "complete": true, "started_at": "...", "finished_at": "...", "frames": 18844 }
}
```

Its purpose was to say which halves of an extract exist. That mattered while a
pass could be switched off: `passes.mission.complete = false` was how a manifest
recorded a run the user had told to skip the mission pass, and ADR 0010 kept the
key deliberately, writing that "The manifest keeps `passes.mission` as its key and
the code keeps the name" even after the mission/hook distinction had been reduced
to which Lua state the calls run in.

ADR 0011 removed the switch. Both passes now always run, so the two records can
only ever say the same thing as each other, and what they say is "the run got this
far" — which is a property of the run, not of a pass.

The per-pass detail does not pay for itself either. `timing_ms` already records
every job's cost by name, at finer grain than a pass, and it is what the
performance targets are measured against. `frames` was diagnostic and is
subsumed: a pass's duration is the sum of its jobs' timings. `started_at` and
`finished_at` bracket a span `extracted_at` and the timings already describe.

Nothing reads the key yet. C3's `check-extract`, which is the consumer that would
validate it, is not written.

## Decision

The manifest carries a top-level `complete` boolean instead of `passes`. It is
false in a fresh manifest and set true when the run reaches `done`, and the
manifest is saved at that transition like any other phase change.

`timing_ms` is unchanged and remains the record of what each job cost.
`extracted_at` is unchanged. The per-pass `started_at`, `finished_at` and `frames`
are not replaced by anything: an interrupted run is already described by its
journal, which names every tile written, and by `timing_ms`, which names every job
that finished.

`check-extract` reads `complete` to distinguish a finished extract from an
abandoned one.

Neither ADR 0007 nor ADR 0010 takes a `Superseded by` status. Neither decision is
wholly reversed — 0007 is still the extract format record and 0010 is still the
record of what a `server`-state call needs — and only their `passes` rows lose,
by the later-ADR-wins rule `README.md` already states for two records that
disagree. A reader who goes looking for a status change on either will not find
one, which is why this paragraph is here.

### Alternatives considered

**Keep `passes` and always write both records complete.** Rejected: two records
that can no longer differ are a format nobody can misread only because nobody
reads them. It also leaves the next reader wondering what would make them differ,
and the answer would be nothing.

**Replace it with per-sweep records — `sweeps: {water: {complete, ms}, ...}`.**
Rejected as the better idea in the wrong place. It carries real information an
interrupted run would want, but it duplicates `timing_ms` and it freezes a sweep
list that X5 to X8c are still adding to. If a resumed run turns out to need more
than the journal gives it, that is the shape to reach for then, and it can be
added beside `complete` rather than instead of it.

**Record nothing and let `check-extract` infer completeness** from the journal
holding every tile the grid implies. Rejected: the inference is not available.
Tiles are legitimately absent — an all-fill tile is never written, and an all-sea
one is omitted under `omit_sea_tiles` — so a reader would have to reconstruct the
skip set to tell a finished extract from a truncated one, and the skip set is
exactly what the manifest does not carry.

## Consequences

`M.new_manifest` writes `complete = false` and `new_pass` is deleted. `PASS_OF`,
`complete_pass`, the `started_at` stamp in `M.enter` and the per-pass frame
counter in `frame_pass` all go, which takes a little over thirty lines out of the
state machine and removes the last place a phase had to know it was a "pass".

**An interrupted run says less about itself than it did.** A manifest with
`complete = false` no longer distinguishes "stopped during the hook pass" from
"stopped during the mission pass" in one field; a reader has to look at
`timing_ms` for which jobs finished. That is a real loss of legibility for a
resumed or abandoned run, and it is accepted because the journal and the timings
between them already answer the question the field was standing in for.

**Done tests that change.** X4's manifest assertions move from `passes.*.complete`
to `complete`. C3's `check-extract` gains `complete` to the set of manifest fields
it validates, and its rejection list should grow a case for a manifest claiming
`complete` with tiles the grid implies still missing. C12b's `check` inherits it.

**Frozen text that now reads false:** the `passes` object in `extract-format.md`
"manifest.json" and in ADR 0007's example, and ADR 0010's sentence keeping
`passes.mission`.
