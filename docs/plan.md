# Plan

The task graph for [`design.md`](design.md) and [`queries.md`](queries.md).
Tasks have ids; a branch is `task/<id>-<summary>`. Edit this file when the
order or a task changes, in the pull request that changes it.

"Verified by" says who can see a task's result: **agent** (tests an agent
runs), **CI**, or **live** (somebody at a Windows machine with DCS on the map).

## Already built

Kept from before the reset, tested, and not tasks here:

- File decoders for `.surface5`, `.rn4`, `.scn5`, the airfield files
  (`.rn5`, `assets.assets`) and the four Lua files, with the container
  header, install discovery and the Lua 5.1 sandbox
  (`dcsterrain/crates/dcsterrain-core/src/decode/`).
- The `grid` module: cell and tile addressing, `Window`, `sample`.
- The projection table and fit (`crs.rs`), the request and response types
  (`types/`), the synthetic theatre's closed form (`synth/`).
- The extract reader, kept as the opt-in test oracle.
- The optional GameGUI hook (`extractor/`) with its offline Lua tests.

## Components

| Id | Component | Platform |
|---|---|---|
| A | The reset: removing what the old pipeline left | any |
| N | The install as the source, and the cache | any; N0 and opt-in checks need an install |
| Q | Query operations and the command line | any |
| S | The skill | any |
| V | Live validation | Windows with DCS |
| X | The optional hook | Windows with DCS |
| R | Release and the user guide | any |

## Tasks

### A: the reset

| Id | Deliverable | Done | Depends on | Verified by |
|---|---|---|---|---|
| A1 | Remove `pack/`, the extract and directory writers, the `extract`, `check-extract`, `pack`, `check <file>`, `stamp`, `serve` and `synth` subcommands, `rusqlite`, the empty MCP crate, `tools/probe-theatre/` and Python from `mise.toml`; replace every citation of a deleted ADR in code, Lua and the READMEs under `extractor/` and `tools/` with the reason it stood for, and every link to a deleted document; reword `.gitattributes`'s comment; update the README to the surface that remains | `mise run check` and `mise run lua-test` pass; no kept file cites a pre-reset ADR or links to a deleted document; the README describes only commands that exist, or says "planned" | — | agent |

### N: the install as the source

| Id | Deliverable | Done | Depends on | Verified by |
|---|---|---|---|---|
| N0 | Live gate: can the tool read a map's files while DCS has that map loaded, and does DCS read them through the OS cache | With DCS in the Mission Editor on Caucasus, the three files open read-only and a header read answers; DCS's `CreateFile` flags on `.surface5` recorded (Process Monitor), and whether its pages are resident before the tool reads them (RAMMap) | — | live |
| N1 | `Source` trait; `Synth` implements it in memory with a patch of each cover class; `grid::Window` reads through it; `--map synth`. Opening an install map by its `entry.lua` id, with `--install` defaulting to the registry value on Windows | Every `grid` test passes through `Synth`; `query --map synth describe` answers with no install; offline: a built install tree resolves `GermanyCW` from a `GermanyColdWar` folder, and an unknown id is refused with the list of ids | A1 | agent |
| N2 | The cache: location, `--cache`, layout keyed by map id and build, entry header and key, atomic write, stale-entry rebuild; `dcsterrain index`, `dcsterrain cache list|prune|clear` | A changed size, payload size, time, build or entry version rebuilds only that entry; two concurrent builders leave a valid entry; `clear` asks before deleting `validation.json` | N1 | agent |
| N3 | The mesh in place: `mesh.toc`, positional block reads, block LRU, 50 m tiles on demand with height, water and cover, point mode with a triangle index | Offline: a built two-triangle block through the full path. Opt-in: Caucasus tiles match the live extract on 99.9 % of cells; open under 20 ms cold on Caucasus | N2 | agent, opt-in |
| N4 | Scenery in place: the pool-tree reader, `scene.nodes`, the trees and lines pools, the removed-object areas | Offline: built files for versions 8 and 9. Opt-in: 400 Caucasus boxes equal a full walk | N2 | agent, opt-in |
| N5 | `tables.json`: airfields, stands, runways, beacons, radio, towns, nodes, the projection fit and residual check with `--allow-crs-residual`; `index --hook` | Opt-in: field for field equal to the live extract; a hook directory from another build is refused by name | N2 | agent, opt-in |
| N6 | The road graph: junction-id graph, contraction, `crosses_water`, sampled betweenness at low priority, raw entry | Synthetic: the bridge is one edge, betweenness peaks there; opt-in: the Caucasus graph builds under 10 s and Germany's betweenness completes | N2 | agent, opt-in |
| N7 | Scenery classes over the model table: per-map overrides keyed on the map id, the `effect` class, the settlement families; the catalog in `check` | Each rule has a positive and negative catalog test; the Caucasus overrides all apply; no `smoke#` model reaches a height, clearing or place answer | N4 | agent |
| N8 | Per-query layers: `valid`, slope, aspect, roughness, both TPIs, `dist_*` with the cap, counts, tree density, `cover` fractions and distances | Closed-form tolerances on `Synth`; window sums against brute force. Opt-in: on every map at hand, each cover bit matches its material subset on at least 90 % of the subset's triangles | N1, N3, N4, N6, N7 | agent, opt-in |
| N9 | Horizon tiles: lazy per-tile cache, `enclosure` from it | Brute-force ray march on a small grid; closed-form hill angle within 0.25°; first-touch and cached cost recorded on Caucasus | N2, N3 | agent, opt-in |
| N10 | `dcsterrain check --install --map` | Passes on Caucasus; a renamed version field is refused by name | N3–N7 | agent, opt-in |
| N11 | Exact sightline experiment: `visible` walking mesh triangle edges instead of half-cell raster steps | Cost and agreement against the raster recorded on Caucasus and Germany; whether to adopt it is an ADR | N3, Q2 | agent, opt-in |

### Q: query operations

| Id | Deliverable | Done | Depends on | Verified by |
|---|---|---|---|---|
| Q1 | `describe`, `sample`, `geo`, `nearest` (all kinds but `place`), `scenery_in` | Synthetic tests; `sample` over its cap errors rather than truncates | N3, N4, N5 | agent |
| Q2 | `visible`, `viewshed` | Hill tests; viewshed fraction against brute force | N1 | agent |
| Q3 | Criteria, the closed set including `cover_in`, `cover_fraction_min`, `dist_cover_band`; tile skipping; LOS criteria last | Each criterion has a positive and negative synthetic test | N8, N9, Q2 | agent |
| Q4 | `score_site`, `find_sites` | Plane accepted, lake and hill rejected; suppression spacing respected | Q3 | agent |
| Q5 | `radar_floor`; `coverage`, `approach_spectrum`, `unmask_profile` computed from it | The floor behind a hill matches its geometry; coverage's dead zone and the spectrum's blind arc lie behind the big hill with the width its geometry predicts | Q2 | agent |
| Q6 | `route`, `route_alternatives`, `chokepoints`, `trafficability` | Bridge chosen, avoid box respected, A* equals Dijkstra; the two synthetic roads are the alternatives with the bridge in `shared_edges_with_all`; bridge ranked first | N6, Q2 | agent |
| Q7 | `dcsterrain query` over every operation: one request or JSON-lines batches, `schema` | Every operation callable from the shell; a batch of 100 requests opens the map once | Q1, Q2, Q4, Q5, Q6 | agent |
| Q8 | Benchmarks: CI upper bounds on `Synth` at 3× the targets; an opt-in benchmark on the install against the targets, including one call end to end | Targets met or a decision recorded | Q7 | agent, opt-in |
| Q9 | Opt-in replay of the FARP and SAM examples on Caucasus. The eight FARP candidates are in the pre-reset `docs/spec/probe-log-2.9.29.27278.md` "FARP siting example", the SAM setup in `docs/spec/design-and-facts.md` "Worked tasks" (both at commit `4b85bd0`); fix the seven western points on the 10 km arc first | The same eight FARP candidates within one 100 m cell, the oracle's resolution; SAM site 1 masked from all seven western points | Q4, Q5 | agent, opt-in |
| Q10 | `point_info` | Each field tested on `Synth`; nearest town bearing and distance against brute force | Q1, N8 | agent |
| Q11 | `airfield_info` | Opt-in: every Caucasus airdrome answers; runway courses and lengths equal the tables | Q1 | agent, opt-in |
| Q12 | `exposure` | The minimum over sites equals per-site floors; the count of sites seeing a cell against brute force | Q5 | agent |
| Q13 | `validate_placements` | One synthetic case per issue kind; opt-in: units placed in a Caucasus building, lake and runway are each reported with the object | Q1, N7, N8 | agent, opt-in |
| Q14 | `low_level_route` | On `Synth`, a route behind a hill from a site stays masked where the hill allows and within the AGL band; cost recorded on Caucasus | Q12 | agent, opt-in |
| Q15 | `towns` and the `places` entry: 200 m grouping, size class and form, naming, outlines on request; `nearest` kind `place` and `point_info`'s place field | Synthetic places against brute-force linkage; opt-in: each map at hand records its class counts; on Caucasus the 47 towns with census figures rank with correlation at least 0.7 | N7, Q1 | agent, opt-in |

### S, V, X, R

| Id | Deliverable | Done | Depends on | Verified by |
|---|---|---|---|---|
| S1 | A skill in the repository teaching an assistant with a shell which operation answers which question, by example, with batching, the provenance fields, `dcsterrain schema`, and the metric meanings. Source material: the pre-reset `docs/spec/using-the-data.md` and the worked tasks (commit `4b85bd0`), rewritten for the current operations | The worked tasks, asked as plain prompts of an assistant with the skill, reach the right operations; the FARP answer equals the direct `query` result | Q7 | agent |
| V1 | `tools/validate/`: the live sortie through the bridge — point heights, `visible` against `terrain.isVisible`, route lengths against `findPathOnRoads`, MGRS against `coord.LLtoMGRS` — written to `validation.json` | Caucasus within the accuracy targets | Q7, N0 | live |
| X1 | Strip the hook to `identity`, `config` and `tables`: delete the sweeps, the mission-pass transport and their tests; the control window loses the crop, grid and per-sweep controls | Offline Lua tests pass; a live run at the menu writes the tables and nothing else | — | agent, live |
| R1 | Release builds: static `dcsterrain` binaries for the three platforms on their own CI runners, attached to tags | The tagged Windows binary answers `query --map synth describe` on a machine without a toolchain, and so does the macOS one | Q7 | CI, live |
| R2 | `docs/guide.md`: point the tool at an install, optionally import the hook's tables, ask a siting question, read the answer, and the failure modes. Written against real output | A newcomer answers one siting question following only the guide | Q7, S1 | live |

## Milestones

| Milestone | Contains | Proof |
|---|---|---|
| MS1 Engine on the install | A1, N0–N7, N10, X1 | `query --map Caucasus describe` and `sample` answer from the install with no step before; every decoder passes its opt-in checks; `Synth` passes every grid test |
| MS2 Queries | N8, N9, N11, Q1–Q15 | Every operation tested on `Synth`; opt-in benchmarks within the targets on Caucasus |
| MS3 Assistant and release | S1, V1, R1, R2 | An assistant with the skill answers the FARP example; live validation within target; release binaries; a guide a newcomer follows |

N0 goes first among the N tasks because it could stop the design: if DCS holds
its terrain files with a share mode that denies readers, the tool cannot run
beside a live server. It needs DCS; N1 onward do not, and can proceed beside
it. Each milestone gets a README pass.
