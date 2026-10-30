# Plan

The task graph for [`design.md`](design.md) and [`queries.md`](queries.md).
Tasks have ids; a branch is `task/<id>-<summary>`. Edit this file when the
order or a task changes, in the pull request that changes it.

"Verified by" says who can see a task's result: **agent** (tests an agent
runs), **CI**, or **live** (somebody at a Windows machine with DCS on the terrain).

## Already built

Kept from before the reset, tested, and not tasks here:

- File decoders for `.surface5`, `.rn4`, `.scn5`, the airfield files
  (`.rn5`, `assets.assets`) and the four Lua files, with the container
  header, install discovery and the Lua 5.1 sandbox
  (`dcsterrain/crates/dcsterrain-core/src/decode/`).
- The `grid` module: cell and tile addressing, `Window`, `sample`.
- The projection table and fit (`crs.rs`), the request and response types
  (`types/`), the synthetic terrain's closed form (`synth/`).
- The extract reader, kept as the opt-in test oracle.

## Components

| Id | Component | Platform |
|---|---|---|
| A | The reset: removing what the old pipeline left | any |
| N | The install as the source, and the cache | any; N0 and opt-in checks need an install |
| Q | Query operations and the command line | any |
| S | The skill | any |
| V | Live validation | Windows with DCS |
| X | Removing the optional hook | any |
| R | Release and the user guide | any |
| C | Scenery catalog review, a terrain at a time | an install, or its files copied |

## Tasks

### A: the reset

| Id | Deliverable | Done | Depends on | Verified by |
|---|---|---|---|---|
| A1 | Remove `pack/`, the `extract`, `check-extract`, `pack`, `check <file>`, `stamp`, `query`, `serve` and `synth` subcommands, `rusqlite`, the empty MCP crate, `tools/probe-theatre/` and Python from `mise.toml`; replace every citation of a deleted ADR in code, Lua and the READMEs under `extractor/` and `tools/` with the reason it stood for, and every link to a deleted document; reword `.gitattributes`'s comment; update the README to the surface that remains | `mise run check` and `mise run lua-test` pass; no kept file cites a pre-reset ADR or links to a deleted document; the README describes only commands that exist, or says "planned" | — | agent |

### N: the install as the source

| Id | Deliverable | Done | Depends on | Verified by |
|---|---|---|---|---|
| N0 | Live gate: can the tool read a terrain's files while DCS has that terrain loaded, and does DCS read them through the OS cache | With DCS in the Mission Editor on Caucasus, the three files open read-only and a header read answers; DCS's `CreateFile` flags on `.surface5` recorded (Process Monitor), and whether its pages are resident before the tool reads them (RAMMap) | — | live |
| N1 | `Source` trait; `Synth` implements it in memory with a patch of each cover class; `grid::Window` reads through it, and the `grid` and extract-reader tests stop building fixtures with the synthetic extract writer, which is then removed; `--terrain synth`. Opening an install terrain by its `entry.lua` id, with `--install` defaulting to the registry value on Windows; `dcsterrain list-terrains` with ids, folders and decoder support | Every `grid` test passes through `Synth`; `query --terrain synth describe` answers with no install; offline: a built install tree resolves `GermanyCW` from a `GermanyColdWar` folder, and an unknown id is refused with the list of ids; `list-terrains` and `list-terrains --json` on that tree show both terrains with their folders, and a file with an unknown version marked unreadable | A1 | agent |
| N2 | The cache: location, `--cache`, layout keyed by terrain id and build, entry header and key, atomic write, stale-entry rebuild; `dcsterrain index`, `dcsterrain cache list|clear`; the cache column of `list-terrains` | A changed size, payload size, time, build or entry version rebuilds only that entry; two concurrent builders leave a valid entry; `clear` asks before deleting `validation.json`; `list-terrains` shows a terrain's cache after `index` and none after `clear` | N1 | agent |
| N2b | Per-role classification and metadata keys (ADR 0001) | GermanyCW opens in under 20 ms from the cache; a cache check on GermanyCW reads no airfield header; an unknown taxiway pair refuses `tables.json` but not `sample`; `list-terrains` reads each header once | N2 | agent, opt-in |
| N3 | The mesh in place: `mesh.toc`, positional block reads, block LRU, 50 m tiles on demand with height, water and cover, point mode with a triangle index; the opt-in tests compare tiles in memory and the extract tile writer is removed | Offline: a built two-triangle block through the full path. Opt-in: Caucasus tiles match the live extract on 99.9 % of cells; open under 20 ms cold on Caucasus | N2 | agent, opt-in |
| N3b | Runway bins and a runway-only `mesh.toc`: bin each prepared block's runway triangles like its full triangle list, so `on_runway` stops scanning every runway triangle; store only the Runway material subsets in `mesh.toc` and bump its entry version | 10k points over an airfield block on GermanyCW answer within 2× of points elsewhere (now 279–978 ms against 1.4–6.6 ms); GermanyCW `mesh.toc` about 11× smaller (8.25 MB → ~0.75 MB); the cross-block runway case still answers correctly | N2b, N3 | agent, opt-in |
| N4 | Scenery in place: the pool-tree reader, `scene.nodes`, the trees and lines pools; the opt-in tests compare in memory and the scenery JSONL writer is removed | Offline: built files for versions 8 and 9. Opt-in: 400 Caucasus boxes equal a full walk | N2 | agent, opt-in |
| N5 | `tables.json`: airfields, stands, runways, beacons, radio, towns, nodes, the projection fit and residual check with `--allow-crs-residual`; `index --hook` | Opt-in: field for field equal to the live extract; a hook directory from another build is refused by name | N2b | agent, opt-in |
| N6 | The road graph: junction-id graph, contraction, `crosses_water`, sampled betweenness at low priority, raw entries; one cache directory per terrain, an entry whose files survive an update re-stamped, `cache prune` removed (ADR 0002) | Synthetic: the bridge is one edge, betweenness peaks there; opt-in: the Caucasus graph builds under 10 s and Germany's betweenness completes; a build change with unchanged road files re-stamps `roads.graph` without a rebuild | N2 | agent, opt-in |
| N7 | Scenery classes over the model table: per-terrain overrides keyed on the terrain id, the `effect` class, the settlement families; the catalog in `check` | Each rule has a positive and negative catalog test; the Caucasus overrides all apply; no `smoke#` model reaches a height, clearing or place answer | N4 | agent |
| N8 | Per-query layers: `valid`, slope, aspect, roughness, both TPIs, `dist_*` with the cap, counts, tree density, `cover` fractions and distances | Closed-form tolerances on `Synth`; window sums against brute force. Opt-in: on every terrain at hand, each cover bit matches its material subset on at least 90 % of the subset's triangles | N1, N3, N4, N6, N7 | agent, opt-in |
| N9 | Horizon tiles: lazy per-tile cache, `enclosure` from it | Brute-force ray march on a small grid; closed-form hill angle within 0.25°; first-touch and cached cost recorded on Caucasus | N2, N3 | agent, opt-in |
| N10 | `dcsterrain check --install --terrain` | Passes on Caucasus; a renamed version field is refused by name | N3–N7 | agent, opt-in |
| N11 | Exact sightline experiment: `visible` walking mesh triangle edges instead of half-cell raster steps | Cost and agreement against the raster recorded on Caucasus and Germany; whether to adopt it is an ADR | N3, Q2 | agent, opt-in |
| N12 | Airdrome ids by position (ADR 0005): where `Radio.lua`'s label gives none, the `N` of the `airfieldN_k` beacons nearest the airdrome within 15 km, when all of them agree, then a `Radio.lua` entry's scene object within its extent. `AirdromeBand`'s and `dist_airdrome_band`'s `airdrome_id` becomes `airdrome`, and they and `airfield_info` take a number as an id and a string as a `name_id` or, failing any, an id in digits; the schema and its examples to match | Opt-in on 2.9.30.28738: Nevada at least 6 of 17 ids and PersianGulf 27 of 30, each equal to the `airdromeId` the install's shipped missions give that airdrome, and no id on another terrain changes; a tower join that disagrees with the missions anywhere is dropped; on `Synth`, an id named in digits and a `name_id` resolve to the same airdrome | N5 | agent, opt-in |

### Q: query operations

**Part of every Q task's done test,** without repeating it in each row: each
operation the task delivers that returns geometry gives, with `geojson: true`,
a valid RFC 7946 `FeatureCollection` whose features match its JSON response.

| Id | Deliverable | Done | Depends on | Verified by |
|---|---|---|---|---|
| Q1 | `describe`, `sample`, `geo`, `nearest` (all kinds but `place`), `scenery_in`; `sample` over scattered points answers block by block: each block prepared once, results merged by rank; `query --allow-crs-residual`, without which a terrain whose fit misses a beacon by over 2 m is refused, as `index` refuses it | Synthetic tests; `sample` over its cap errors rather than truncates; 10k scattered points at the 32 MiB block budget answer in under 3 s on GermanyCW (measured 25.9 s naive, 13.5 s Z-sorted, 2.9 s block by block on 2.9.30.28738) | N3, N3b, N4, N5 | agent |
| Q2 | `visible`, `viewshed`; the shared GeoJSON writer | Hill tests; viewshed fraction against brute force; the GeoJSON is a valid RFC 7946 `FeatureCollection` whose area, projected back, matches the visible cells within one cell along the edge | N1 | agent |
| Q3 | Criteria, the closed set including `cover_in`, `cover_fraction_min`, `dist_cover_band`; tile skipping; LOS criteria last | Each criterion has a positive and negative synthetic test | N8, N9, Q2 | agent |
| Q4 | `score_site`, `find_sites` | Plane accepted, lake and hill rejected; suppression spacing respected | Q3 | agent |
| Q5 | `radar_floor`; `coverage`, `approach_spectrum`, `unmask_profile` computed from it | The floor behind a hill matches its geometry; coverage's dead zone and the spectrum's blind arc lie behind the big hill with the width its geometry predicts; `radar_floor`'s GeoJSON bands nest, each lower band inside the next | Q2 | agent |
| Q6 | `route`, `route_alternatives`, `chokepoints`, `trafficability` | Bridge chosen, avoid box respected, A* equals Dijkstra; the two synthetic roads are the alternatives with the bridge in `shared_edges_with_all`; bridge ranked first | N6, Q2 | agent |
| Q7 | `dcsterrain query` over every operation: one request or JSON-lines batches, `schema` | Every operation callable from the shell; a batch of 100 requests opens the terrain once | Q1, Q2, Q4, Q5, Q6 | agent |
| Q8 | Benchmarks: CI upper bounds on `Synth` at 3× the targets; an opt-in benchmark on the install against the targets, including one call end to end | Targets met or a decision recorded | Q7 | agent, opt-in |
| Q9 | Opt-in replay of the FARP and SAM examples on Caucasus. The eight FARP candidates are in the pre-reset `docs/spec/probe-log-2.9.29.27278.md` "FARP siting example", the SAM setup in `docs/spec/design-and-facts.md` "Worked tasks" (both at commit `4b85bd0`); fix the seven western points on the 10 km arc first | The same eight FARP candidates within one 100 m cell, the oracle's resolution; SAM site 1 masked from all seven western points | Q4, Q5 | agent, opt-in |
| Q10 | `point_info` | Each field tested on `Synth`; nearest town bearing and distance against brute force | Q1, N8 | agent |
| Q11 | `airfield_info` | Opt-in: every airdrome of every installed terrain answers; runway courses and lengths equal the tables, true courses the geodesic's, widths and elevations a brute-force scan's | Q1 | agent, opt-in |
| Q12 | `exposure` | The minimum over sites equals per-site floors; the count of sites seeing a cell against brute force | Q5 | agent |
| Q13 | `validate_placements` over point, rectangle, circle and polygon shapes, with the land or water, slope and depth each placement asks for | One synthetic case per issue kind and per shape, including a shape that touches an object only at its edge; opt-in: placements in a Caucasus building, lake and runway are each reported with the object | Q1, N7, N8 | agent, opt-in |
| Q15 | `towns` and the `places` entry: 200 m grouping, size class and form, naming, outlines on request; `nearest` kind `place` and `point_info`'s place field | Synthetic places against brute-force linkage; opt-in: each terrain at hand records its class counts; on Caucasus the 72 towns of the 2014 Georgian and 2021 Russian censuses rank by residential floor with Spearman correlation at least 0.6 | N7, Q1 | agent, opt-in |
| Q16 | Performance pass over the operations built so far: the query benchmark (`cargo bench --bench query`) over `Synth` and, opt-in, an install, cold and warm, with allocations and the cache's size per entry; each kept optimization its own commit with its measured delta | About 2× on the geometric mean of the benchmark's cases on Caucasus and GermanyCW, or the shortfall recorded with its reason; answers unchanged; no cache entry larger | Q2, N11 | agent, opt-in |

### S, V, X, R

| Id | Deliverable | Done | Depends on | Verified by |
|---|---|---|---|---|
| S1 | A skill in the repository teaching an assistant with a shell which operation answers which question, by example, with batching, the provenance fields, `dcsterrain schema`, and the metric meanings. Source material: the pre-reset `docs/spec/using-the-data.md` and the worked tasks (commit `4b85bd0`), rewritten for the current operations | The worked tasks, asked as plain prompts of an assistant with the skill, reach the right operations; the FARP answer equals the direct `query` result | Q7 | agent |
| V1 | `tools/validate/`: the live sortie through the bridge — point heights, `visible` against `terrain.isVisible`, route lengths against `findPathOnRoads`, MGRS against `coord.LLtoMGRS` — written to `validation.json` | Caucasus within the accuracy targets | Q7, N0 | live |
| X1 | Remove the hook (ADR 0005): `extractor/`, its Lua tests and CI workflow, the Lua 5.1 build and tasks, `index --hook` and the import in `tables.json`, and the `code`, `civilian`, `abandoned` and `class` fields; `tables.json` re-stamped after an update like the other entries; `CLAUDE.md` and the README to match | `mise run check` and `mise run docs` pass; `check` and `describe` answer on Caucasus with no hook field; nothing outside `docs/` names the hook | — | agent |
| R1 | Release builds: static `dcsterrain` binaries for the three platforms on their own CI runners, attached to tags | The tagged Windows binary answers `query --terrain synth describe` on a machine without a toolchain, and so does the macOS one | Q7 | CI, live |
| R2 | `docs/guide.md`: point the tool at an install, ask a siting question, read the answer, and the failure modes. Written against real output | A newcomer answers one siting question following only the guide | Q7, S1 | live |

### C: catalog review

| Id | Deliverable | Done | Depends on | Verified by |
|---|---|---|---|---|
| C1 | Review the scenery catalog of every terrain with no override file — Afghanistan, GermanyCW, MarianaIslands, MarianaIslandsWWII, Nevada, PersianGulf and Syria after N7 — each into its own classes file, each terrain its own pull request, following the review below | Every terrain has an override file whose keys all apply; no top-100 model is classed building unless it is one | N7 | agent for the clear cases; live for the ambiguous models |

How to review one terrain:

1. Print the catalog with `dcsterrain check --terrain <id> --top 100`. It
   lists the 100 models placed most and the 10 tallest structures, each with
   its count, class, family, box and the source that decided it.
2. Read the models placed most first: the top 50 to 100 hold most objects.
   Look for four faults:
   - a large box that is not a building — a bridge, pier, pitch or parked
     vehicle — which should be `misc`;
   - an industrial structure the name rule misses — a tank, cooling tower or
     substation — which should be `industrial`;
   - a wrong family, or `unknown` where it should be house, apartment or
     shop (PersianGulf's top houses are `unknown` after N7);
   - terrain totals that look off: Afghanistan has 67 `misc` and 0 `wall`
     after N7, which suggests oversized boxes, so check a few of its models
     by hand.
3. Write `dcsterrain/crates/dcsterrain-core/classes/<terrain id>.toml`, one
   line per model: `"model" = { class = "...", family = "...", reason = "..." }`.
   Keys match in any case. Never name a `smoke#` model; a test refuses it.
   Register the file in `OVERRIDES` in `classes.rs`.
4. Run the test again. It fails on a key that names no model on the
   terrain. Check that the class counts moved as expected.
5. List the ambiguous models, such as Caucasus's `korpus_*` blocks, for the
   user to judge in the Mission Editor. Do not guess them.

## Milestones

| Milestone | Contains | Proof |
|---|---|---|
| MS1 Engine on the install | A1, N0–N7, N10, N12, X1 | `query --terrain Caucasus describe` and `sample` answer from the install with no step before; every decoder passes its opt-in checks; `Synth` passes every grid test |
| MS2 Queries | N8, N9, N11, Q1–Q13, Q15 | Every operation tested on `Synth`; opt-in benchmarks within the targets on Caucasus |
| MS3 Assistant and release | S1, V1, R1, R2, C1 | An assistant with the skill answers the FARP example; live validation within target; release binaries; a guide a newcomer follows |

N0 goes first among the N tasks because it could stop the design: if DCS holds
its terrain files with a share mode that denies readers, the tool cannot run
beside a live server. It needs DCS; N1 onward do not, and can proceed beside
it. Each milestone gets a README pass.

Q14, `low_level_route`, is deferred until a user asks for routes. `exposure`
already maps where the sites cannot see, and that answers most of the
question. A route would stack approximations: terrain-only clearance, line of
sight with no radar model, and waypoints that DCS's AI flies as straight
lines. Nothing at a live install can check it. As specified it also ignores
speed. Turn radius grows with the square of speed, so a jet and a helicopter
need different routes, and the time a site sees a leg needs a ground speed.
Picked up, it is helicopter first: a `speed` in KCAS and a `max_g`, the time
seen per leg from ground speed, and a refusal when the turn radius is more
than the grid can honor.
