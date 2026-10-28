# State

Where the work is. Updated at the end of every working session, before handing
back. This file holds progress; `plan.md` holds the task graph and
`design.md` and `queries.md` hold the design.

**Last updated:** 2026-10-28T06:45Z

## Done

Newest first. Max 5 entries — drop the oldest when adding a sixth.

1. **N5: `tables.json` and the hook import.** One JSON cache entry, keyed
   inside the JSON, holds the airdromes, runways, stands, beacons, radio,
   towns, nodes and the fit; `index --hook` merges the airdrome fields only
   the sim knows, refusing another build's directory by name, and `index`
   refuses a fit over 2 m without `--allow-crs-residual`. *Verified by:* an
   agent, through `mise run check`, and opt-in on 2.9.30.28738: every
   Caucasus table, and with the hook every airdrome field, equals the live
   extract; all eight terrains build in 54–560 ms, up to 2.4 MB.
2. **N4: scenery in place.** `scene.nodes` keeps each node's array
   offsets, the models, the group and the pools; a box descends each
   pool's own quadtree and reads its hits by position through 4 MiB of
   chunks, not a map (the user's choice). The JSONL writer is gone.
   *Verified by:* an agent, through `mise run check`, and opt-in on
   2.9.30.28738: 400 boxes each on Caucasus and Nevada equal a scan of
   every pool box and the walk; the index builds on all eight terrains.
3. **N3b: runway bins and a runway-only `mesh.toc`.** A prepared block
   bins its runway triangles like its triangle list, so a point tests
   the few near it; `mesh.toc` keeps only the runway subsets, version 2.
   *Verified by:* an agent, through `mise run check`, and opt-in on
   2.9.30.28738: 10 k points over GermanyCW's busiest airfield block
   978 → 13 ms, its runway test 1.1 ms; `mesh.toc` 8.25 → 0.75 MB;
   GermanyCW opens in 4.2 ms.
4. **N2b: per-role classification and metadata keys (ADR 0001).** Opening
   reads the surface header alone; other roles are classified when an entry
   reading them is built, so an unknown pair refuses only its role. Keys are
   checked from a stat or a directory listing and the build. `list-terrains`
   surveys in one pass. *Verified by:* an agent, through `mise run check`,
   and opt-in on 2.9.30.28738: GermanyCW opens in 14.8 ms, Syria 9.8 ms,
   `list-terrains` 78 ms.
5. **The cache guard.** A cache inside the install is refused however its
   path is spelled — case, 8.3 name, relative, link, junction, loopback
   share, `..` — by resolving both paths and comparing file identity,
   and before any entry is built. A bind mount of a directory inside the
   install is not caught. *Verified by:* an agent, through `mise run check`
   on Windows.

## Next

One task. The thing to pick up immediately.

**N6** — the road graph: junction-id graph, contraction, `crosses_water`,
sampled betweenness at low priority, raw entry; reuse across builds.
*Verified by:* an agent, opt-in against the install.

## Then

Max 5 entries, in dependency order. Task ids from `plan.md`.

1. **N7** — scenery classes over the model table.
2. **N8** — per-query layers: slope, TPIs, distances, cover.
3. **N9** — horizon tiles, and `enclosure` from them.
4. **N10** — `dcsterrain check --install --terrain`.
5. **Q1** — `describe`, `sample`, `geo`, `nearest`, `scenery_in`; `query
   --allow-crs-residual`.

Milestone in view: **MS1 Engine on the install**.

## Carries

Things a later task must not lose. Max 10 — `CLAUDE.md` has the rules.

| # | Carry | Discharged by |
|---|---|---|
| 1 | The hook still writes v1, so `extractor/test/constants.lua` pins `FORMAT_VERSION` to 1 rather than the shared 2; restore it, and drop the fill and road-seed constants if nothing else reads them. | X1 |
| 2 | The synthetic road network is a tree, so the junction-to-bridge edge outranks the bridge in betweenness and the contracted arms tie: "betweenness peaks at the bridge" fails as stated. Change the layout or the test, and say which in `design.md`. | N6 |
| 3 | `tables.json` keeps `lua_included`, the shared scripts each `dofile` ran, and `lua_unresolved`, each name nothing assigns and each module the sandbox stood in for: report both in `check`, with the hook import's `dropped`. | N10 |
| 6 | On MarianaIslands the blocks under one point outgrow the 32 MiB budget: 10 k lattice points over a single block take 250–290 ms, and 35–41 ms with every block held (2.9.30.28738). Answer every `sample` block by block, not only scattered points. | Q1 |
