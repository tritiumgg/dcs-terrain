# DCS terrain dataset: project plan

Build tools that turn a DCS World theatre into one portable SQLite file
and answer terrain siting and routing questions from it, with DCS doing
no terrain work at campaign runtime. The repository ships tools only;
users extract their own theatres.

## Documents

| File | What it is |
|---|---|
| `plan.md` | This file: components, tasks with dependencies, order, milestones |
| `design-and-facts.md` | What DCS exposes, what was measured, design decisions, worked campaign tasks, size budget |
| `probe-log-2.9.29.27278.md` | Raw measurements on DCS 2.9.29.27278: Caucasus in depth, projection and fill for all eight theatres |
| `using-the-data.md` | What every returned metric means and how a campaign turns them into easy, medium, hard and unfair missions |
| `tools/probe-theatre/fit.py` | Fits a theatre projection from 20 measured lat/lon samples |
| `extract-format.md` | The directory the extractor writes and `pack` reads; the Lua/Rust contract |
| `extractor-hook.md` | The Lua GameGUI hook that sweeps a theatre |
| `core.md` | The Rust workspace, packed file schema, derived layers, `pack`, `check` |
| `query-operations.md` | The operations, criteria, and their tests |
| `mcp-server.md` | `dcsterrain serve` |
| `validation.md` | The validation sortie and how to measure a new theatre |

Read `design-and-facts.md` first; every spec assumes it. Before any
DCS call, read its "Sources you have" section: two of the rules there
exist because a call crashed DCS during the measurements.

## Components

| Id | Component | Language | Platform | Spec |
|---|---|---|---|---|
| X | Extractor hook | Lua 5.1 | Windows with DCS | `extractor-hook.md` |
| F | Extract format | contract | both | `extract-format.md` |
| C | `dcsterrain-core` and `pack`/`check` | Rust | any | `core.md` |
| Q | Query operations | Rust | any | `query-operations.md` |
| M | MCP server | Rust | any | `mcp-server.md` |
| V | Validation | Lua via bridge | Windows with DCS | `validation.md` |
| D | Documentation | — | any | this file |
| P | Project infrastructure | — | any | this file |

The work splits across machines at F: X runs where DCS is, everything
after it runs anywhere, and only V comes back to DCS. X and C can be
built in parallel once F is fixed, because both are tested against the
same synthetic theatre and neither needs the other to exist.

## Tasks

Each task has an id, a deliverable, a done test and the tasks it
depends on. A task whose done test has more than three clauses, or that
spans two spec sections, is split into lettered subtasks that keep the
parent id; finer breakdown is left to implementation. Tasks inside a
component are in build order.

### P: project infrastructure

| Id | Deliverable | Done | Depends on |
|---|---|---|---|
| P1 | Repository layout: `dcsterrain/` Cargo workspace, `extractor/` for the Lua hook and its offline Lua tests (ADR 0005: no stub harness is vendored), `tools/validate/`, `docs/` holding these documents; `.gitignore` excludes `*.sqlite`, `extracts/`, `*.bin` | Tree exists; README states "no terrain data is committed" | — |
| P2 | CI: Rust build and test on Windows, macOS, Linux; Lua 5.1 offline tests. One workflow per language, each scoped to the paths it tests, so a documentation commit starts no runner | Green on an empty synthetic run | P1, C1, X1 |
| P3 | Release build: static `dcsterrain` binaries for the three platforms, each built on its own CI runner with `cargo build --release`, attached to tags | The tagged Windows binary packs a synthetic extract on the Windows machine and the tagged macOS binary does the same on macOS, without a toolchain on either | P2, C13 |
| P4 | Licensing note in README: extracts and packed files are derived from ED terrain data and stay on the user's machine; publishing them is the user's call against the ED EULA | Text reviewed | P1 |

### F: extract format

| Id | Deliverable | Done | Depends on |
|---|---|---|---|
| F1 | Freeze the extract format v1: confirm every field against the probe log, decide `water` codes and `nodata` values, write the manifest example by hand | Done; ADR 0007 records the field sources measured on 2.9.29.27468, the `water` codes and the manifest example, and both authors' walk of every field C5c, C7, C8, C9 and C10 read back. Any later change bumps `format_version` | — |
| F2 | Reference constants for the synthetic theatre, one file per language (`synth_constants.lua`, `synth.rs`), same numbers | Both files exist and a comment in each names the other | F1 |

### X: extractor hook

| Id | Deliverable | Done | Depends on |
|---|---|---|---|
| X1 | Encoders `i16le`, `u8`, `json`, and the list normalisation of ADR 0007; offline tests on boundary values, on nested tables, strings with quotes, unicode, empty arrays and objects, and on a DCS table keyed from 0 | Offline Lua tests pass | F1, P1 |
| X2a | Config load and validation, progress log, `dcs.log` lines | Every bad config field produces one log line and a disabled run | X1 |
| X2b | `dcs_build` from `autoupdate.cfg`; `terrain_fingerprint` with a pure Lua SHA-256 as a sliced step, tested against a known vector | The fingerprint of a known file matches `sha256sum` of its first 1 MiB | X2a |
| X3 | Grid computation from crop, authored bounds, or the pre-sweep, including the pre-sweep lattice, its authored-cell bounds derivation and its base64 bitmask record; tile addressing; `tiles.jsonl` journal, manifest write and resume | Offline tests for each bounds source, for the bitmask's row padding and its base64 against the RFC 4648 vectors, and for resume with half the tiles journalled | F1, X1 |
| X4 | State machine and frame budget: idle → prepare → hook pass → mission pass → done; `onSimulationFrame` slicing with `os.clock()` | An offline fake driver runs 10 000 frames and the manifest advances monotonically | X3 |
| X5 | Config and tables sweep: `config.json` with the fill triple, `airdromes.json`, `runways.json`, `stands.json`, `beacons.json`, `radio.json`, `towns.json`, `nodes.json`, including the sandboxed table reader | Every swept table matches the same table read back through the bridge | X2a, X4 |
| X6 | Pre-sweep sampling (post density or road proximity) over X3's lattice, `water` and `height` sweeps with the fill and sea skip set, the fill test and per-tile min/max | Tile cells match heights and water re-read through the bridge on a small crop; the fill margin is omitted | X4, F2 |
| X7 | Roads and railroads sweep: seed lattice, snaps (nil accepted), seed merging at 100 m, neighbour pairing, path lines, path calls under the frame budget | Every pair appears once, and a sampled path matches the same path re-read through the bridge | X6, X7a |
| X7a | Short-path cost | Done; measured, probe log "Plan measurements X7a and C8a": 0.61 ms per 1 km path, `road_seed_spacing` 1000, merge radius 100 m | — |
| X8a | Mission-pass transport: `net.dostring_in("server", ...)` with `%q` escaping, length verification, `(string, boolean)` return handling; `surface` quarter-tile chunks | A tile round-trips through the live bridge; a truncated payload is rejected | X4 |
| X8b | Scenery sweep: 15 km spheres on a 20 km lattice, de-dup by id (`tostring` join), `getObjectsAtMapPoint` footprint attach, helipad check | Scenery lines match the same sphere re-read through the bridge, with footprints attached | X8a |
| X8c | `scenery_models.json` catalogue: counts, `getDesc` fields, `type_bits`, OBB and radius medians | Catalogue counts and medians match the swept scenery lines | X8b |
| X9 | Failure handling: `pcall` everywhere, `notes` entries, terrain change mid-run | Offline tests inject failures and the run completes with `nodata` cells and notes | X4 |
| X10 | Live run, cropped: 10 × 10 km around Kutaisi, both passes, ten cells spot-checked through the bridge | Acceptance 2 and 3 in the spec; `check-extract` passes | X5, X6, X7, X8c, X9, C3 |
| X11 | Live run, full Caucasus, both passes; timings recorded in the probe log | Acceptance 4 | X10 |
| X12 | Progress reporting: `start` returns `step, progress` and `progress()` answers `(done, total)` or nil; a wall-clock heartbeat naming phase, sweep, elapsed and fraction; an overall fraction weighted by the measured per-sweep costs, because roads is over half the run and an unweighted bar sits still through it; totals at done. The sweeps from X5 on implement `progress()`. Needs an ADR: the spec's progress log is "one line per tile and per phase change" and this adds a third kind | An offline driver over 10 000 frames emits one heartbeat per interval, never reports a fraction above 1, and names the phase it is actually in; a sweep that cannot count its work still gets a heartbeat | X2a, X4 |
| X13 | Progress window: `Window`, `Panel`, `Static` and `HorzProgressBar` required from the hook state, holding a bar, a line naming the current sweep and a line naming the progress log path; opened at prepare, closed at done. Every widget call `pcall`ed, one failure disabling the window and logging once, because a run must never depend on its own progress display | The window appears with the Mission Editor open during a live cropped run, the bar advances and the text tracks the log; forcing every widget call to fail leaves the sweep running and the extract identical | X12 |

### C: core and pack

| Id | Deliverable | Done | Depends on |
|---|---|---|---|
| C1 | Workspace scaffold, `types`, `clap` CLI with subcommand stubs, `schemars` on all request/response types, stderr logging with `--verbose`; the schema is snapshot-tested, so a change to it is a deliberate diff | `dcsterrain schema` prints valid JSON schema, and the snapshot test fails on any change to it | P1 |
| C2 | `synth`: closed-form theatre and extract-directory writer including the fill margin; `dcsterrain synth` | Writes a directory `check-extract` accepts once C3 exists; constants match F2 | F2, C1 |
| C3 | `extract`: manifest and journal parse, validation, tile reader; `dcsterrain check-extract` | Accepts the synthetic extract; rejects each corrupted variant (wrong size, missing tile, extra file, bad min/max, unknown version, journal ahead of manifest) | F1, C1 |
| C4 | `grid`: tile addressing, cross-tile window reads, bilinear sample, absent-tile rules (fill, sea), nodata handling | Window tests at every edge and corner; absent fill and sea tiles read per the spec | C3 |
| C5a | `pack` skeleton: schema creation, `meta` (ADR 0008 adds the authored-rectangle keys and makes the bounds keys metres), pragmas, one transaction per grid, `VACUUM` | Empty packed file opens and `check` reports every `meta` key | C1 |
| C5b | Base grids: re-tiled to the pack `tile_size` with min/max, fill and sea tile omission, `cell_size` 50 or 100 by the size rule with resampling | Synthetic file opens; `describe` matches the manifest at both cell sizes | C4, C5a |
| C5c | Vector tables from JSON (airdromes, runways, stands, beacons, radio, `poi`) and their R-trees | Row counts and R-tree counts match the extract; `nearest` on `poi` finds towns and nodes | C3, C5a |
| C6 | `derive` stored local layers `valid` and `tpi_2000`, and the query-time window layers `slope`, `aspect`, `roughness`, `tpi_300` with their `grid` rows | Closed-form tolerances in the spec; the synthetic fill margin is `valid = 0` | C5b |
| C7 | `derive` distance transforms: road, rail, water, built-up; road rasterisation from edge geometry | Brute-force comparison on 64 × 64 | C5b, C8b, C9 |
| C8 | Scenery classification: rules over `scenery_models.json` (`nofootprint`, `wall`, `misc`, regex, `building`) with per-theatre overrides; `scenery` table; catalogue report with class and reason | Each rule has a positive and negative catalogue test; the report lists every model with its source | C3, C5a |
| C8a | Caucasus class overrides | Done for the draft; move `classes-Caucasus.toml` to `classes/Caucasus.toml` in the crate and confirm the pack report shows every override applied | C8 |
| C8b | SATs (`sat_building`, `sat_industrial`) and `scenery_idx` | SAT totals match the generator's object list; SAT window sums match brute force | C8, C5b |
| C9 | `graph`: build from path lines (snap, split, contract), `crosses_water`, Brandes betweenness with sampling, coverage report | Hand-drawn polyline cases; bridge has max betweenness | C3, C5c |
| C10 | Projection fit from `latlon_samples`; `crs_proj4` in `meta`; residual check | Fitted Caucasus parameters match the design doc table within 1 m on the synthetic samples generated from those parameters | C3, C5a |
| C11 | `derive` horizon and enclosure with early exit, parallel tiles, `--drop` | Brute-force ray march comparison on a small grid; closed-form hill angle within 0.25°; the horizon pack time on the X11 extract is recorded against acceptance 2 of the core spec, and a miss is a decision, not a failure of C11 | C6 |
| C12a | `file` reader: `Theatre::open`, tile LRU, `grid::Window`, table reads | Regional query touches each tile once; nothing resident after the call | C5b |
| C12b | `dcsterrain check` including `--install <dir>` fingerprint comparison | Passes on the synthetic file, fails on each injected corruption, reports a changed fingerprint | C12a, C5c |
| C12c | `dcsterrain stamp`; default output naming `<theatre>-<dcs_build>-<digest>-<cell>m.sqlite`; overwrite refusal | Stamped keys appear in `describe`; a differing file is not overwritten without `--force` | C12a |
| C13 | Pack report and performance: `rayon` per tile, timings, `--threads`, `--json` | Synthetic 100 × 100 km at 50 m packs in under two minutes on a laptop | C5b, C11 |
| C14 | Opt-in real-extract test group under `DCSTERRAIN_EXTRACT`, skipped when unset | Skipped in CI; passes locally on the X10 crop | C12b, X10 |

### Q: query operations

| Id | Deliverable | Done | Depends on |
|---|---|---|---|
| Q1 | `describe`, `sample` (region and points), `nearest`, `scenery_in`, `geo` | Synthetic tests; `sample` cap errors rather than truncates | C12a, C5c, C10 |
| Q2 | `los`: `visible` with clearance, `viewshed` with polygons, `coverage` with dead zones and blind-approach flag | Hill tests; viewshed fraction against brute force | C12a, C4 |
| Q3 | Criteria: the closed set with hard test and soft score, `tpi_class_in` thresholds, tile skipping from `height` min/max, LOS criteria last | Each criterion has a positive and negative synthetic test | C6, C7, C8b, C11, Q2 |
| Q4 | `score_site` and `find_sites` with footprint orientation, weights, non-maximum suppression, stats | Plane accepted, lake and hill rejected; suppression spacing respected | Q3 |
| Q5 | `unmask_profile` and `approach_spectrum` | First-seen distance matches hill geometry; the spectrum's blind arc lies behind the big hill with the width the hill's geometry predicts; the sector summary equals the full summary when the sector is 0–360 | Q2 |
| Q6a | `route`: subgraph load, avoid regions, exposure weighting, A*, simplification | Bridge chosen, avoid box respected, A* equals Dijkstra | C9, C12a, Q2 |
| Q6b | `route_alternatives` by the penalty method with stretch and overlap limits | On the synthetic theatre's two roads the alternatives are the two roads; the bridge is in `shared_edges_with_all` when it is the only crossing; no alternative exceeds the limits | Q6a |
| Q6c | `trafficability` and `chokepoints` | Blocking sample reported on the lake edge; bridge ranked first | Q6a, C6 |
| Q7 | `dcsterrain query` CLI over all ops with JSON in and out and `--max-rows` | Every op callable from the shell | Q1, Q2, Q4, Q5, Q6b, Q6c |
| Q8 | Benchmarks on the scaled synthetic theatre with CI upper bounds | Targets in the spec met at 3×; a miss is a decision about the target | Q7 |
| Q9 | Opt-in real-data replay of the FARP and SAM examples from the design doc | Acceptance 2 in the spec | Q4, Q2, X11, C13 |

### M: MCP server

| Id | Deliverable | Done | Depends on |
|---|---|---|---|
| M1 | Load the `mcp-builder` skill; `dcsterrain-mcp` crate with `rmcp`; tool registration generated from the operation list and `schemars` | Tool list equals operation list plus the two conveniences | Q7 |
| M2 | `dcsterrain serve`: multi-file, `theatre` parameter, stdio transport, stderr or `--log` logging, fail-fast startup | Stock MCP client calls every tool on the synthetic file | M1 |
| M3 | Response discipline: `--max-rows` on `sample` and `scenery_in`, polygons-by-default viewshed, error mapping, resource per file | Integration test over a `tokio::io::duplex` pair compares each tool result with the direct operation | M2 |
| M4 | Tool descriptions and evaluation per `mcp-builder`: run the design doc's worked tasks as natural-language prompts against the server and check tool choice | Acceptance 2 in the spec | M3 |

### V: validation

| Id | Deliverable | Done | Depends on |
|---|---|---|---|
| V0 | Fill and hull on the land-fill and sea-fill theatres | Done; measured, probe log "Plan measurement V0": fill lies outside every bounds rectangle; the pre-sweep uses post density or road proximity | — |
| V1 | `tools/validate/`: `validate.lua` chunks and the driver script; height, water, surface, LOS and route comparisons; `stamp` | A Caucasus file stamps within the spec's thresholds | X11, C12c, Q7 |
| V2 | `tools/probe-theatre/`: the per-map hook chunk beside the `fit.py` already there, packaged so a new theatre or build is measured in one step | Re-running it on Caucasus reproduces the projection table | — |

### D: documentation

| Id | Deliverable | Done | Depends on |
|---|---|---|---|
| D1 | `docs/guide.md`: install the hook, run an extract, pack it, query it, read the results, and the failure modes of each step. Written against real output, never against the specs; links to `using-the-data.md` for metric definitions rather than restating them | A reader who has not seen the project extracts a theatre and answers one siting question from it, following only the guide | Q7, M4, X11 |

## Order and milestones

Build order is P1, F1, F2, then X1 to X9, then C1 to C3, then X10, then
the rest of C, then Q, M, V. X leads because the Windows machine is the
scarce resource and X is the only component that needs it. Twelve X
tasks, X1 through X9 counting the sub-tasks, depend on nothing outside
the component, and the
only thing X ever needs from C is `check-extract` at X10, so C1 to C3
are the whole Rust interlude that finishing the extractor costs.

The longest dependency chain is P1 → C1 → C3 → C4 → C5b → C6 → C11 →
Q3 → Q4 → Q7 → M1 → M2 (twelve tasks; C5b → C8b → C7 → Q3 is a chain
of the same length), and nothing on it needs DCS. C11 sits on the chain
only because Q3 tests the horizon criteria; building Q3 with
`--drop horizon` first and adding those two criteria last takes it off.
The `Depends on` column is the graph: a task may start when every task
it names is done, and tasks that share no chain run in parallel.

The work is also staged by machine. Component X is developed on the
Windows machine, because ADR 0005 verifies the sweeps against a running
theatre through the `dcs-api-bridge` MCP rather than against a stub of
one; only the offline tests in X1 to X4 and X9 would run anywhere. The
Rust and MCP work is otherwise developed on macOS, but the Windows
machine does carry the pinned Rust toolchain, so C1 to C3 are built
there when they are reached as the X10 interlude and `check-extract`
runs on the crop where it was extracted.

X11, the full Caucasus sweep, waits until C4 onward have read the X10
crop. A field missing from F1 then costs a re-run of a 10 x 10 km crop
rather than of a whole theatre. So the machine is never free for good
before MS5: X11 calls it back once the C tasks have read the crop, Q9
replays the design doc's examples against that full extract, and V1
returns at MS5 because the validation sortie compares the packed file
against live DCS.

The milestones below are proof points, not a schedule. MS2 already
depends on C11, which MS3 contains, so deferring X11 past the C tasks
changes nothing about them.

| Milestone | Contains | Proof |
|---|---|---|
| MS0 Contract | P1, P2, F1, F2, C1, C2, C3, X1, X3 | `check-extract` accepts the Rust synthetic extract, and the hook's encoders and grid computation reproduce the F2 constants byte for byte on the same inputs (ADR 0005: the full-format contract is proven at X10) |
| MS1 Packed file | C4, C5a–C5c, C6–C10, C8a, C8b, C12a–C12c, X2a, X2b, X4–X9, X12 | `pack` and `check` pass on the synthetic extract; the hook completes a live cropped sweep |
| MS2 Real extract | X10, X11, X13, C13, C14 | A full Caucasus extract exists, packs, and `check` passes |
| MS3 Queries | Q1–Q5, Q6a–Q6c, Q7, Q8, C11 | Every operation tested on the synthetic theatre; benchmarks within target |
| MS4 Server | M1–M4, Q9 | The assistant answers the FARP example, and `query` returns the same result for it as the server does |
| MS5 Validated | V1, V2, P3, P4, D1 | A stamped Caucasus file, release binaries, and a guide a newcomer can follow |

## Working rules

- Verify every DCS symbol in the `dcs-api-lookup` index before writing
  a call, and read ED's own call site when one is cited.
- Never write into the DCS install. Never commit extracted or packed
  data. Never run a `server`-state call unless the bridge phase is
  `sim`.
- Test against the synthetic theatre first; touch real data only in
  the opt-in local group and the live acceptance steps.
- When a measurement contradicts the design doc, a spec, or the probe
  log, record an ADR in `docs/decisions/` with the build string. Those
  documents are frozen. This plan is not: edit it directly.
