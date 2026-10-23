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
| `extract-format.md` | The directory the extractor writes and `pack` reads; the contract between them |
| `extractor-hook.md` | The Lua GameGUI hook. Since ADR 0034 and ADR 0037 only its tables pass is live, and the extract is decoded from the terrain files |
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
| E | Extractor: file decoders and `dcsterrain extract` | Rust | any, with an install's terrain files | ADRs 0034, 0035 |
| X | Extractor hook, tables pass only, optional | Lua 5.1 | Windows with DCS | `extractor-hook.md`, ADR 0037 |
| F | Extract format | contract | any | `extract-format.md` |
| C | `dcsterrain-core` and `pack`/`check` | Rust | any | `core.md` |
| Q | Query operations | Rust | any | `query-operations.md` |
| M | MCP server | Rust | any | `mcp-server.md` |
| V | Validation | Lua via bridge | Windows with DCS | `validation.md` |
| D | Documentation | — | any | this file |
| P | Project infrastructure | — | any | this file |

F is the contract between E and C. E needs a DCS install's terrain files
but never a running sim; X and V are the only components that do. E and C
can be built in parallel once F3 is fixed, because C is tested against the
synthetic theatre and E against the install's files, and neither needs the
other to exist.

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
| F3 | Extract format v2, recorded as an ADR: the layers the files add (road class and kind per edge, the road network's own nodes, forest and built-up ground from the mesh's materials, the tree and line pools, per-model boxes), the class and version of every file read (ADR 0035), the authored rectangle read from the files in place of ADR 0026's measurement, which `config.json` fields survive without the sim and which come only from the hook's tables pass (ADR 0037), the beacon-pair `latlon_samples` (ADR 0038), and the `pass` field's new meaning | Done; ADR 0039 walks every field to its decoder and to the C task that reads it, `format_version` 2; ADR 0040 moves the projection fit into the extractor | F1 |

### E: extractor, the file decoders

Each decoder reads one file class at the versions it declares (ADR 0035).
Their offline tests run on byte buffers built in the test. The real-file
checks are an opt-in group gated on `DCSTERRAIN_INSTALL`, skipped in CI, and
compare against the live Caucasus extract (heights, water, roads and
tables), gated on `DCSTERRAIN_EXTRACT`.

| Id | Deliverable | Done | Depends on |
|---|---|---|---|
| E1 | Container reader: header, class name, format version, and dispatch to a decoder that declares the pair; an unknown pair stops the run, naming the file, class and version; the terrain fingerprint and the file list with versions for the manifest | Offline tests on built headers, including a refused version; opt-in: every map file in an install is classified, and Nevada's scene file reads as version 8 | C1 |
| E2 | `surface5`: blocks, field descriptors, the finest mesh level, a point sampler, and rasterization of `height` (water surface, road strips win), `water` and `surface` to 50 m tiles | Offline tests on a built two-triangle block; opt-in: Caucasus heights 99.95 % exact and water all but 67 cells against the live extract | E1, F3 |
| E3 | `rn4` road network: edges with class, kind and node ids, polylines; roads and railroads written per ADR 0039 | Offline tests on a built edge; opt-in: Caucasus router points within 2 m of a decoded road edge and 3 m of a rail edge at p90 (ADR 0043), every rail-kind edge carries a rail class, and on every installed map one catch-all node id is written null and the rest span at most a few km (ADR 0044) | E1, F3 |
| E4 | `scn5` versions 8 and 9: model table with boxes, placement blocks, the object, line and tree pools, instance ids; writes `scenery.jsonl`, `scenery_models.json` and the `trees` and `lines` count layers (ADR 0039) | Offline tests on a built block for each version; opt-in: Caucasus objects equal the reference decode row for row, and Nevada's runway lights align with their runways | E1, E2, F3 |
| E5 | Airfields: airfield `rn4` and `rn5`, runway ends and their pairing, stands with class, stand parameters from `misc/assets.assets` | Opt-in: Caucasus 900 of 900 stands on all parameters and 21 of 21 runways against the live extract | E1, F3 |
| E6 | Lua tables: beacons, radio with airdrome ids, towns, mission nodes, read without executing the files; `latlon_samples` from the beacons, the fitted `crs`, and town `x, z` through it (ADR 0040) | Opt-in: Caucasus counts and values equal the live extract; the beacon fit reproduces the design table on every installed map | E1, F3 |
| E7 | Ground materials: forest and built-up polygons from the mesh's material subsets, written to `ground.jsonl` (ADR 0039) | Opt-in: on Caucasus, built-up ground holds over 90 % of scenery objects on under 10 % of land | E2, E4 |
| E8 | `dcsterrain extract --install <dir> --map <Map> --out <dir>`: runs the decoders, the sanity checks of ADR 0035, merges a hook directory from the same build given as `--hook <dir>` (ADRs 0037, 0039), refuses a non-empty output directory, writes the manifest last | `check-extract` accepts the output; a hook directory from another build is refused; a decoded road lifted 10 m off the ground fails the run | E2–E7, C3 |
| E9 | Live comparison: a full Caucasus decode against the live extract, Nevada and Syria cross-checks, and a live crop for the `surface` layer and Syria's road and rail heights; figures recorded in `docs/data/` | Every layer within the E2 to E6 figures, and each Syria and `surface` question answered with an ADR where the decode differs from the game | E8, C4 |

### X: extractor hook, tables pass only

ADR 0037 keeps the tables pass and nothing else. X1 to X5, X9 and X12 to X14
are done. X6, X7, X7a, X8a, X8b, X8c, X10 and X11 are retired by ADR 0034.

| Id | Deliverable | Done | Depends on |
|---|---|---|---|
| X15 | Strip the hook to `identity`, `config` and `tables`: delete the pre-sweep, the water and height, road and scenery sweeps, the mission-pass transport and their offline tests; a run needs no crop and no grid; the control window and progress reporting (X12 to X14) lose the crop, grid and per-sweep controls | Offline Lua tests pass; a live run at the menu writes the tables and nothing else, and the window says so | X5, F3 |

### C: core and pack

| Id | Deliverable | Done | Depends on |
|---|---|---|---|
| C1 | Workspace scaffold, `types`, `clap` CLI with subcommand stubs, `schemars` on all request/response types, stderr logging with `--verbose`; the schema is snapshot-tested, so a change to it is a deliberate diff | `dcsterrain schema` prints valid JSON schema, and the snapshot test fails on any change to it | P1 |
| C2 | `synth`: closed-form theatre and extract-directory writer including a margin outside the mesh; `dcsterrain synth` | Writes a v2 directory, the F3 layers included, that `check-extract` accepts once C3 exists; constants match F2 | F2, F3, C1 |
| C3 | `extract`: manifest parse, validation, tile reader; `dcsterrain check-extract` | Accepts the synthetic extract; rejects each corrupted variant (wrong size, missing tile, extra file, bad min/max, unknown version, no manifest) | F3, C1 |
| C4 | `grid`: tile addressing, cross-tile window reads, bilinear sample, absent-tile rules (outside the mesh, sea, all-zero counts; ADR 0039), nodata handling | Window tests at every edge and corner; each absent-tile case reads per ADR 0039 | C3 |
| C5a | `pack` skeleton: schema creation, `meta` (ADR 0008 adds the authored-rectangle keys and makes the bounds keys metres), pragmas, one transaction per grid, `VACUUM` | Empty packed file opens and `check` reports every `meta` key | C1 |
| C5b | Base grids: re-tiled to the pack `tile_size` with min/max, fill and sea tile omission, `cell_size` 50 by default on every map and 100 when asked for, with resampling (ADR 0036) | Synthetic file opens; `describe` matches the manifest at the default and at an explicit 100 | C4, C5a |
| C5c | Vector tables from JSON (airdromes, runways, stands, beacons, radio, `poi`) and their R-trees | Row counts and R-tree counts match the extract; `nearest` on `poi` finds towns and nodes | C3, C5a |
| C6 | `derive` stored local layers `valid` and `tpi_2000`, and the query-time window layers `slope`, `aspect`, `roughness`, `tpi_300` with their `grid` rows | Closed-form tolerances in the spec; the synthetic fill margin is `valid = 0` | C5b |
| C7 | `derive` distance transforms: road, rail, water, built-up; road rasterisation from edge geometry | Brute-force comparison on 64 × 64 | C5b, C8b, C9 |
| C8 | Scenery classification: rules over `scenery_models.json`, which E4 writes from the scene file's model table (`nofootprint`, `wall`, `misc`, regex, `building`) with per-theatre overrides; `scenery` table; catalogue report with class and reason | Each rule has a positive and negative catalogue test; the report lists every model with its source | C3, C5a |
| C8a | Caucasus class overrides | Done for the draft; move `classes-Caucasus.toml` to `classes/Caucasus.toml` in the crate and confirm the pack report shows every override applied | C8 |
| C8b | SATs (`sat_building`, `sat_industrial`) and `scenery_idx` | SAT totals match the generator's object list; SAT window sums match brute force | C8, C5b |
| C9 | `graph`: build from the road network's own edges and nodes with their class (E3), each edge end joined to its node and a null end a dead end (ADR 0044), contract, `crosses_water`, Brandes betweenness with sampling, coverage report | Hand-drawn polyline cases; bridge has max betweenness | C3, C5c |
| C10 | Projection check: `latlon_samples` through the extract's `crs` (ADR 0040); `crs_proj4` in `meta`; residual check | The synthetic extract passes; a hand-altered `crs` fails the residual unless `--allow-crs-residual` | C3, C5a |
| C11 | `derive` horizon and enclosure with early exit, parallel tiles, `--drop` | Brute-force ray march comparison on a small grid; closed-form hill angle within 0.25°; the horizon pack time on the E9 Caucasus extract is recorded against acceptance 2 of the core spec, and a miss is a decision, not a failure of C11 | C6 |
| C12a | `file` reader: `Theatre::open`, tile LRU, `grid::Window`, table reads | Regional query touches each tile once; nothing resident after the call | C5b |
| C12b | `dcsterrain check` including `--install <dir>` fingerprint comparison | Passes on the synthetic file, fails on each injected corruption, reports a changed fingerprint | C12a, C5c |
| C12c | `dcsterrain stamp`; default output naming `<theatre>-<dcs_build>-<cell>m.sqlite`, with no terrain digest because the build is the terrain version (ADR 0023); overwrite refusal | Stamped keys appear in `describe`; a differing file is not overwritten without `--force` | C12a |
| C13 | Pack report and performance: `rayon` per tile, timings, `--threads`, `--json` | Synthetic 100 × 100 km at 50 m packs in under two minutes on a laptop | C5b, C11 |
| C14 | Opt-in real-extract test group under `DCSTERRAIN_EXTRACT`, skipped when unset | Skipped in CI; passes locally on the E9 Caucasus extract | C12b, E8 |
| C15 | The layers F3 adds (ADR 0039): road class and kind on `road_edge`, forest and built-up polygons from `ground.jsonl`, tree and line density SATs from the count layers | Synthetic tests for each layer; SAT window sums match brute force | F3, C5b, C8b |

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
| Q9 | Opt-in real-data replay of the FARP and SAM examples from the design doc | Acceptance 2 in the spec | Q4, Q2, E9, C13 |
| Q10 | Criteria over the F3 layers: forest cover, tree density, built-up region, road class on routes and `nearest` | Each criterion has a positive and negative synthetic test | C15, Q3, Q6a |

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
| V1 | `tools/validate/`: `validate.lua` chunks and the driver script; height, water, surface, LOS and route comparisons; `stamp` | A Caucasus file stamps within the spec's thresholds | E9, C12c, Q7 |
| V2 | `tools/probe-theatre/`: the per-map hook chunk beside the `fit.py` already there, packaged so a new theatre or build is checked in one step; a check, not a requirement, since the extractor fits the projection from the beacons (ADR 0040) | Re-running it on Caucasus reproduces the projection table | — |

### D: documentation

| Id | Deliverable | Done | Depends on |
|---|---|---|---|
| D1 | `docs/guide.md`: run `dcsterrain extract` on an install, optionally the hook's tables pass, pack it, query it, read the results, and the failure modes of each step. Written against real output, never against the specs; links to `using-the-data.md` for metric definitions rather than restating them | A reader who has not seen the project extracts a theatre and answers one siting question from it, following only the guide | Q7, M4, E9 |

## Order and milestones

Build order is P1, F1, F2, then F3, then C1 and C2, then C3 and E1, then
E2 to E8 beside the rest of C, then E9, Q, M and V. F3 comes first because
both E and C are written against it: it settles the layers the files add
before either side encodes or reads them.

The longest dependency chain is P1 → C1 → C3 → C4 → C5b → C6 → C11 →
Q3 → Q4 → Q7 → M1 → M2 (twelve tasks; C5b → C8b → C7 → Q3 is a chain of
the same length), and nothing on it needs DCS or its files. C11 sits on the
chain only because Q3 tests the horizon criteria; building Q3 with
`--drop horizon` first and adding those two criteria last takes it off. The
`Depends on` column is the graph: a task may start when every task it names
is done, and tasks that share no chain run in parallel.

The work is staged by what each task needs. E needs an install's terrain
files and nothing else, so its opt-in checks run on the Windows machine,
or on any machine the files are copied to; its offline tests run anywhere.
Only X15, E9's live crop and V1 need a running DCS, so the Windows machine
is no longer the scarce resource the plan is ordered around. E9 waits for
C4 onward to read the first decoded extract, so a field missing from F3
costs a decoder change rather than a live session.

The milestones below are proof points, not a schedule. MS2 already
depends on C11, which MS3 contains, through C13.

| Milestone | Contains | Proof |
|---|---|---|
| MS0 Contract | P1, P2, F1, F2, F3, C1, C2, C3, E1 | `check-extract` accepts the Rust synthetic v2 extract, and E1 classifies every terrain file of an install |
| MS1 Packed file | C4, C5a–C5c, C6–C10, C8a, C8b, C12a–C12c, C15, E2–E7, X15 | `pack` and `check` pass on the synthetic extract; every decoder passes its opt-in checks on Caucasus |
| MS2 Real extract | E8, E9, C13, C14 | A decoded full Caucasus extract exists, packs, and `check` passes |
| MS3 Queries | Q1–Q5, Q6a–Q6c, Q7, Q8, Q10, C11 | Every operation tested on the synthetic theatre; benchmarks within target |
| MS4 Server | M1–M4, Q9 | The assistant answers the FARP example, and `query` returns the same result for it as the server does |
| MS5 Validated | V1, V2, P3, P4, D1 | A stamped Caucasus file, release binaries, and a guide a newcomer can follow |

## Working rules

- Verify every DCS symbol in the `dcs-api-lookup` index before writing
  a call, and read ED's own call site when one is cited.
- Never write into the DCS install. Never commit extracted or packed
  data. Never run a `server`-state call unless terrain is loaded, which
  is `terrain.GetTerrainConfig("id")` returning non-nil (ADR 0010); the
  bridge's `phase` field cannot answer that question.
- Test against the synthetic theatre first; touch real data only in
  the opt-in local group and the live acceptance steps.
- When a measurement contradicts the design doc, a spec, or the probe
  log, record an ADR in `docs/decisions/` with the build string. Those
  documents are frozen. This plan is not: edit it directly.
- The decoders open the install's files for reading only, and never
  touch a `.crypt` file: what only the encrypted files hold comes from
  the hook's tables pass or is left null (ADR 0037).
