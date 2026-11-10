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
| [data/radar-floor.md](data/radar-floor.md) | The radar floor on every installed terrain: nesting, agreement with `visible`, the raster against the exact floor, and cost |
| [data/exposure.md](data/exposure.md) | `exposure` over seven sites on every installed terrain: against the sites' own floors and `visible`, and cost |
| [data/query-bench.md](data/query-bench.md) | The query benchmark before and after the performance pass: every case, cold and warm, and the cache sizes |
| [data/targets.md](data/targets.md) | Every target on every installed terrain, in cycles with the wall beside them, how cycles are counted, and why each miss |
| [data/q18-performance.md](data/q18-performance.md) | Each operation against its single-thread baseline, cold and on every core, peak commit on every terrain, and each shortfall with its cause |
| [data/answer-memory.md](data/answer-memory.md) | Peak commit of the largest answers at the row caps, built as a tree of values against encoded as written |
| [data/criteria-prune.md](data/criteria-prune.md) | The siting criteria pruned against exhaustive evaluation on every installed terrain: tiles skipped and times |
| [data/route-search.md](data/route-search.md) | A\* against Dijkstra on every installed terrain's roads: agreement, nodes settled, and what a route costs |
| [data/find-sites.md](data/find-sites.md) | A FARP-like `find_sites` on every installed terrain: each candidate rescored alone, the spacing, a brute-force search, and times |
| [data/replay.md](data/replay.md) | The FARP and SAM examples replayed on Caucasus against the live probe, and the same requests on every other terrain |
| [data/airdrome-ids.md](data/airdrome-ids.md) | Airdrome ids joined by position on every installed terrain, against the shipped missions and held out against the labels |
| [data/validation.md](data/validation.md) | The tool against a running DCS: heights, water, sightlines, routes and MGRS, and why routes miss |
| [data/placements.md](data/placements.md) | `validate_placements` on every installed terrain: a building, water and a runway, a random batch against brute force, and times |
| [data/point-info.md](data/point-info.md) | `point_info` on every installed terrain: the nearest town against a scan of every town, bearings against the geodesic, and cost |
| [data/places.md](data/places.md) | Places on every installed terrain: size classes, forms, why the rules are what they are, the models a screenshot should settle, build and query cost, and Caucasus towns against their census |
| [data/airfield-info.md](data/airfield-info.md) | `airfield_info` on every installed terrain: runways against the tables, true courses against the geodesic, widths and elevations against a scan, and cost |
| [data/scenery-review.md](data/scenery-review.md) | Screenshots of the Caucasus models in DCS: how to take them, the heuristics scored against them, what moved |
| [data/caucasus-scenery-catalogue.tsv](data/caucasus-scenery-catalogue.tsv) | The Caucasus scenery model census |
| [data/forest-under-town.md](data/forest-under-town.md) | How much of Caucasus's forest ground town covers, terrain-wide and round a pad near Bakhmaro |

The project was reset on 2026-10-27. The documents before it — nine frozen
specifications, their ledgers and 49 ADRs — were deleted; they are in git at
commit `4b85bd0`.
