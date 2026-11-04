# Plan

The task graph for [`design.md`](design.md) and [`queries.md`](queries.md).
Tasks have ids; a branch is `task/<id>-<summary>`. Edit this file when the
order or a task changes, in the pull request that changes it.

"Verified by" says who can see a task's result: **agent** (tests an agent
runs), **CI**, **live** (somebody at a Windows machine with DCS on the terrain),
or **user** (the user tries it, with no DCS).

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
| M | The MCP server | any; opt-in checks need an install |
| H | Commands for people | any; opt-in checks need an install |

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
| N13 | Connectors in the road graph: the most efficient route among those the game accepts, and likewise alternatives. Decode the road file's table and connectors, and search over the passages they allow (ADR 0009) | Opt-in, against the replies V1 recorded on Caucasus: every route the game refuses is refused, every route it answers is answered, and none longer than the game's by more than 0.5 % of its length plus the distances the game's path starts and stops off the ends asked; a shorter one takes only passages the road file lists. On `Synth`, a junction with a forbidden turn routes around it | N6 | agent, opt-in |
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
| Q8 | Benchmarks: upper bounds on `Synth` at 3× the targets, a command run by hand on one machine, not a CI check; an opt-in benchmark on the install against the targets, including one call end to end | Targets met or a decision recorded | Q7 | agent, opt-in |
| Q9 | Opt-in replay of the FARP and SAM examples on Caucasus. The eight FARP candidates are in the pre-reset `docs/spec/probe-log-2.9.29.27278.md` "FARP siting example", the SAM setup in `docs/spec/design-and-facts.md` "Worked tasks" (both at commit `4b85bd0`); fix the seven western points on the 10 km arc first | The same eight FARP candidates within one 100 m cell, the oracle's resolution; SAM site 1 masked from all seven western points | Q4, Q5 | agent, opt-in |
| Q10 | `point_info` | Each field tested on `Synth`; nearest town bearing and distance against brute force | Q1, N8 | agent |
| Q11 | `airfield_info` | Opt-in: every airdrome of every installed terrain answers; runway courses and lengths equal the tables, true courses the geodesic's, widths and elevations a brute-force scan's | Q1 | agent, opt-in |
| Q12 | `exposure` | The minimum over sites equals per-site floors; the count of sites seeing a cell against brute force | Q5 | agent |
| Q13 | `validate_placements` over point, rectangle, circle and polygon shapes, with the land or water, slope and depth each placement asks for | One synthetic case per issue kind and per shape, including a shape that touches an object only at its edge; opt-in: placements in a Caucasus building, lake and runway are each reported with the object | Q1, N7, N8 | agent, opt-in |
| Q15 | `towns` and the `places` entry: 200 m grouping, size class and form, naming, outlines on request; `nearest` kind `place` and `point_info`'s place field | Synthetic places against brute-force linkage; opt-in: each terrain at hand records its class counts; on Caucasus the 72 towns of the 2014 Georgian and 2021 Russian censuses rank by residential floor with Spearman correlation at least 0.6 | N7, Q1 | agent, opt-in |
| Q16 | Performance pass over the operations built so far: the query benchmark (`cargo bench --bench query`) over `Synth` and, opt-in, an install, cold and warm, with allocations and the cache's size per entry; each kept optimization its own commit with its measured delta | About 2× on the geometric mean of the benchmark's cases on Caucasus and GermanyCW, or the shortfall recorded with its reason; answers unchanged; no cache entry larger | Q2, N11 | agent, opt-in |
| Q17 | Run `find_sites` sightline criteria in parallel, below normal priority (ADR 0004). They run on one thread today: `find_sites_los_36` takes 27.6 s on Afghanistan and 70 and 82 s on the Marianas against 10 s (`data/targets.md`) | Answers unchanged; the 10 s target met on Afghanistan; the Marianas met, or the shortfall recorded with its reason | Q8 | agent, opt-in |
| Q18 | Performance: rank the 22 operations on a single-thread baseline of cycles, warm, over the 8 terrains; the slowest 11 to at least 3×, the fastest 11 to at least 2×, cold and multi-threaded times no worse, answers byte-identical; a query process at most 64 MiB (ADR 0008). A cache precompute must give 1.5× on an operation, add at most 5 % to the terrain's cache and repay its index time within 1 000 calls | Every operation's figure and every shortfall with its measured cause in `data/q18-performance.md` | Q17 | agent, opt-in |
| Q19 | Encode each answer as it is written, from its typed response, rather than building it whole as a tree of values and copying it before writing (ADR 0012). First, in a commit of its own, `serde_json`'s `preserve_order`, so a tree keeps its keys in the order a response declares them and a response written straight gives the same bytes; every snapshot refreshed, and `schema` and the tool schemas checked for what moved. Then `sample`, `point_info` and `scenery_in` rows and every operation's GeoJSON features written as encoded; the MCP server writes a `geojson` member to its file as it is encoded, and the answer to its own, holding neither whole | After the first commit, every answer equal as JSON to before; after the rest, byte-identical to the first commit's; peak commit at the row caps measured below the figures of ADR 0012 on `Synth` (`sample` of 10 000 rows with 1 and with 57 layers, `point_info` of 10 000 points, each with and without GeoJSON) and on Caucasus (`scenery_in` of 4 987 objects), each figure recorded | Q18, Q21 | agent |
| Q20 | The off-road search (ADR 0013): the least-cost way over the 50 m cells from a point to every cell of its network within `max_distance`, 16 neighbors, at each cell's off-road cost per meter, then straightened wherever a straight cut costs no more, judged over the cells it crosses. `trafficability` answers with it: `reachable` when a way is found, `way` with its path, length, cost, ascent, descent and where and on which edge it meets the road, and the straight line kept as `straight` with its cost and blocking point; `max_distance` a request field, 20 km by default and at most, or the most that holds the engine to 64 MiB, the search ending at the first road cell it reaches; a table page for people; README, guide, skill and tool descriptions | On `Synth`: a point whose straight line to the road climbs the big hill past `max_slope_deg` reaches the road round it; wherever the straight line passes, the way costs no more; the search equals Bellman–Ford over a small window, and the straightened way costs no more than the search's; a point across the river with no bridge within reach is unreachable, its reason saying a river blocks; a point 500 m from a road searches no cell past that road's cost; past `max_distance` is `too_far`; text snapshots within 100 columns. Opt-in: peak commit at the largest `max_distance` on GermanyCW under 64 MiB, recorded | Q6, M4 | agent, opt-in |
| Q21 | `route` off the road (ADR 0013): `offroad`, `max_offroad` (5 km), `max_slope_deg` (30) and `offroad_weight` (2, at least 1), bounded as ADR 0013 says and refused on railroads; each end searched as Q20 searches, at the off-road cost times `offroad_weight` and, with observers, times one plus `exposure_weight` times seen; every road cell reached a start or a goal of one road search, its off-road cost carried; a way off the road the whole way when it costs least. `offroad_from` and `offroad_to`; `length`, `ascent`, `descent` and `max_grade_pct` over the whole way; the off-road turns among the waypoints; `route_alternatives` takes the same fields; without `offroad`, a `no_route` whose end lies on a piece that joins nothing says to add it; people's page names the off-road share and lists its waypoints, and without `--offroad` says when an end is over 250 m, five cells, from the road, a stretch it did not plan; README, guide (its "No route"), skill and tool descriptions | On `Synth`: an off-road end's route is an off-road leg, road legs and an off-road leg, `length` their sum, A* equal to Dijkstra over the joined search; an end whose nearest road joins nothing, in a graph the test builds, routes through another road; no off-road leg crosses water; with observers, a hidden way is chosen over a shorter seen one; requests without `offroad` answer byte-identically (snapshots unchanged). Opt-in on Caucasus: `airport:Kobuleti` to `airport:Batumi` with `offroad` answers; without it, every route V1 compared with the game answers as before; the largest case's peak commit under 64 MiB | Q20 | agent, opt-in |

### S, V, X, R

| Id | Deliverable | Done | Depends on | Verified by |
|---|---|---|---|---|
| S1 | A skill in the repository teaching an assistant with a shell which operation answers which question, by example, with batching, the provenance fields, `dcsterrain schema`, and the metric meanings. Source material: the pre-reset `docs/spec/using-the-data.md` and the worked tasks (commit `4b85bd0`), rewritten for the current operations | The worked tasks, asked as plain prompts of an assistant with the skill, reach the right operations; the FARP answer equals the direct `query` result | Q7 | agent |
| V1 | `tools/validate/`: the live sortie through the bridge — point heights, `visible` against `terrain.isVisible`, route lengths against `findPathOnRoads`, MGRS against `coord.LLtoMGRS` — written to `validation.json` | Caucasus within the accuracy targets. Passed on seed 2, after N13, with the route target changed as `data/validation.md` records | Q7, N0, N13 | live |
| X1 | Remove the hook (ADR 0005): `extractor/`, its Lua tests and CI workflow, the Lua 5.1 build and tasks, `index --hook` and the import in `tables.json`, and the `code`, `civilian`, `abandoned` and `class` fields; `tables.json` re-stamped after an update like the other entries; `CLAUDE.md` and the README to match | `mise run check` and `mise run docs` pass; `check` and `describe` answer on Caucasus with no hook field; nothing outside `docs/` names the hook | — | agent |
| R1 | Release builds: `dcsterrain` archives for Windows, Linux, and macOS on Apple silicon and Intel, each built on its platform's own CI runner, the C runtime linked statically on Windows and Linux, attached with checksums to a release for each `v*` tag | On each platform's runner, a CI job that only downloads the archive answers `query --terrain synth describe` with no checkout, no cargo call and a bare environment: Linux in a container without glibc, Windows with System32 as its whole path and no dynamic C runtime imported | Q7 | CI |
| R2 | `docs/guide.md`: point the tool at an install, ask a siting question, read the answer, combine questions, draw the answer as GeoJSON, and the failure modes. Written against real output. The tool is mainly for assistants and software, with the skill as their entry point; the guide serves a person who drives it by hand | Every command and output in the guide reruns on Caucasus in Windows PowerShell 5.1 and 7; a stand-in newcomer subagent, given only the guide, answers a siting question it was not shown. The live newcomer test was not run: the user reviewed the guide and judged it adequate | Q7, S1 | agent, user review |

### M: the MCP server

Over the same operations as `query` (ADR 0011). M and H can run in parallel,
but they share files: the CLI's subcommand list, the README (each new
subcommand gets its line, marked planned until it works), the skill and
`STATE.md`. Each series rebases on the other's merges. The tool schemas and the
flags are both generated from `schema`, so H4's change reaches them with no
edit. H5 waits for M3, since both rewrite the guide.

| Id | Deliverable | Done | Depends on | Verified by |
|---|---|---|---|---|
| M1 | `dcsterrain mcp` over stdio: one tool per operation, its input schema generated from `schema`: the request schema plus `terrain`, the terrain the call is about, each call running the engine `query` runs; an error is a tool error naming its issues | An MCP client in a test lists one tool per operation, each schema equal to `schema`'s plus `terrain`; on `Synth` every tool's answer is byte-identical to `query`'s for every request the CLI's snapshot tests send | Q7 | agent |
| M2 | The long-lived server: the terrain held open and reopened when its files change, cancellation, progress on long calls (`find_sites`, `exposure`), a GeoJSON over a size limit written to a file whose path is returned | On `Synth`: cancelling `find_sites` stops it within 1 s; `find_sites` and `exposure` send at least one progress notice; a reopened terrain answers after its files change; a GeoJSON over the limit comes back as a path to a valid file. Opt-in, 100 calls in one session stay within 64 MiB of peak commit on Caucasus | M1 | agent, opt-in |
| M3 | Client setup and the skill: configuration for Claude Desktop and Claude Code, the skill told to use the tools when present, a short section in the README and the guide; one server for every terrain, each call naming its own, and a `list_terrains` tool | An assistant connected only through MCP, with no shell, answers the FARP example on Caucasus, equal to the direct `query` result | M2, S1 | user: the user runs the assistant; no DCS |
| M4 | The guidance an MCP client gets with no skill: the server's `instructions` and each tool's description cut from marked sections of `skill/SKILL.md`, which stays the one file edited, the binary holding it; the instructions carry the conventions, the which-operation table, `terrain` on every call, the answer files, the general issue codes, provenance and cost, and each tool its own question, limits, codes and the meaning of its numbers; a `guide` tool returning the worked tasks and the metric meanings by topic; no shell-only section reaches the server | An MCP client test finds the instructions and every tool's description equal to their marked parts of the committed skill; every operation has its section; every issue code the skill names is in `schema`; the instructions and the descriptions stay within the budgets the task measures for what Claude Desktop and Claude Code keep; the user, in Claude Desktop with no skill installed, answers the FARP example, equal to the direct `query` result | M3 | agent; user: no DCS |

### H: commands for people

| Id | Deliverable | Done | Depends on | Verified by |
|---|---|---|---|---|
| H1 | One parser for every place a flag takes: DCS `x,z`, MGRS, latitude and longitude, an airfield name or a town name | On `Synth`, using its airdromes and towns, and opt-in on Caucasus, each form round-trips against `geo`; an ambiguous name is refused with its choices | Q7 | agent, opt-in |
| H2 | One subcommand per operation, its flags generated from the request schema and its `--help` from the schema's descriptions; `--json` prints `query`'s answer, the default a plain list of the answer's fields, and `--geojson-out <file>` writes the GeoJSON of any operation that returns one | For every request the CLI's snapshot tests send, the flag form with `--json` prints JSON byte-identical to `query`'s; the default list names every top-level field of that answer; `--geojson-out` writes the same `FeatureCollection` `query` returns | H1 | agent |
| H3 | Readable output for `find_sites`, `route`, `route_alternatives`, `nearest`, `point_info`, `airfield_info`, `towns` and `visible`: tables with units, and MGRS beside x and z | Text snapshots on `Synth`, each within 100 columns; every MGRS shown equals `geo`'s for that point; no color when the output is not a terminal | H2 | agent |
| H4 | GeoJSON on `sample`, `nearest` and `scenery_in`, which today ignore `geojson: true` | Each returns a valid RFC 7946 `FeatureCollection` on `Synth` | Q2 | agent |
| H5 | The README and the guide: the commands for people as the path for a person, `query` as the path for software, in plain language, one idea per sentence, reviewed by subagents | The user, with only the guide and the commands for people, answers the FARP question, and the answer names a site the enemy post cannot see that equals the `query` result; every point where the user got stuck is fixed | H3, H4, M3 | user: no DCS |

### C: catalog review

| Id | Deliverable | Done | Depends on | Verified by |
|---|---|---|---|---|
| C1 | Settle by screenshot the models a name and a box cannot judge on the seven terrains after Caucasus, listed in `data/places.md`, following the review below. Towns come from clusters of real buildings, so no terrain gets an override file, and what a screenshot finds goes in the rules every terrain shares | Each listed model is judged, and no model among the 100 most placed that counts towards a place is left unjudged | N7 | live: an agent drives the capture with the user at DCS and judges the screenshots |

How to review one terrain, with `data/scenery-review.md` beside you for the
capture:

1. Print the catalog with `dcsterrain check --terrain <id> --top 100000 --json`.
   List every placed model that is not plainly a marker or light.
2. From `scenery_in` choose a placement of each: clear of other boxes, fewest
   trees by `tree_density`.
3. The user starts a one-aircraft mission on the terrain, presses F11 and leaves
   the mouse alone. Shoot one frame of each model, 5 s a model, at 40°, and
   models with no box from about 300 m: a large one is a footprint the tool
   cannot see, so name it in the pull request.
4. Read every frame, in contact sheets. Retake what trees hide or what shows
   water, at 60° from another side. A neighbor in frame must not decide a class.
5. Judge each model by eye, looking for what the rules get wrong:
   - a large box that is not a building, which should be `misc`;
   - an industrial structure no name catches, which should be `industrial`;
   - a building the no-building rule took because a name pattern happens to
     match, such as `2-floor_house_clay_walls`;
   - a wrong family, where it changes whether the model counts towards a place.
6. Add what the frames show to the family rules in `classes.rs`, by a word that
   holds across terrains. Never name a `smoke#` model. No per-terrain file.
7. Run `every_installed_terrain_records_its_places` again, and describe what
   moved. The census must stay at 0.6 or more.
8. Send the user one list of the models you cannot judge, each with its
   screenshot. Do not guess those. Screenshots stay out of the repository.

## Milestones

| Milestone | Contains | Proof |
|---|---|---|
| MS1 Engine on the install | A1, N0–N7, N10, N12, X1 | `query --terrain Caucasus describe` and `sample` answer from the install with no step before; every decoder passes its opt-in checks; `Synth` passes every grid test |
| MS2 Queries | N8, N9, N11, Q1–Q13, Q15, Q17, Q18 | Every operation tested on `Synth`; opt-in benchmarks within the targets on Caucasus |
| MS3 Assistant and release | S1, V1, R1, R2, C1 | An assistant with the skill answers the FARP example; live validation within target; release binaries; a guide written against real output and reviewed by the user |
| MS4 Interfaces | M1–M3, H1–H5 | An assistant answers the FARP example through MCP alone; the user answers it with the commands for people and the guide alone |
| MS5 Off the road | M4, Q20, Q21, Q19 | An assistant with no skill answers the FARP example through MCP; a route from `airport:Kobuleti` answers off the road; a large answer's memory cut and measured |

MS5 runs in series, M4, Q20, Q21, then Q19: M4 first so every later change a
user touches is written once, in the skill the server cuts; Q19 last, since a
user sees nothing of it and its figures then cover the final answers. M4 and
Q19 share `mcp.rs` and the skill, Q19 and Q20–Q21 the GeoJSON writer, so none
runs beside another. C1, live, shares no file with them.

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
