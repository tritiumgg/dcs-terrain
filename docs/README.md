# Documents

Every document here is living: edit it in the pull request whose change moves
it.

| File | What it is |
|---|---|
| [STATE.md](STATE.md) | Where the work is: done, next, then, and the carries. Read it first |
| [design.md](design.md) | What the tool is, how it reads a DCS install, the cache, versions, targets, testing, risks |
| [queries.md](queries.md) | Every operation's request and response, the layers and the criteria |
| [plan.md](plan.md) | The task graph and milestones |
| [decisions/](decisions/README.md) | ADRs: decisions that change the design, with their reasoning |
| [data/file-formats.md](data/file-formats.md) | Byte layouts of the ED files the decoders read, with what is confirmed and what is guessed |
| [data/live-gate.md](data/live-gate.md) | Reading terrain files beside a running DCS: the open test, how DCS opens them, and what it keeps resident |
| [data/sightline-exact.md](data/sightline-exact.md) | The exact sightline over the mesh against the raster walk: agreement and cost on Caucasus and GermanyCW |
| [data/query-bench.md](data/query-bench.md) | The query benchmark before and after the performance pass: every case, cold and warm, and the cache sizes |
| [data/caucasus-scenery-catalogue.tsv](data/caucasus-scenery-catalogue.tsv) | The Caucasus scenery model census |

The project was reset on 2026-10-27. The documents before it — nine frozen
specifications, their ledgers and 49 ADRs — were deleted; they are in git at
commit `4b85bd0`.
