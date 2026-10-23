# State

Where the work is. Updated at the end of every working session, before handing
back. This file holds progress; `plan.md` holds the task graph and
`decisions/` holds what has diverged from the frozen documents.

**Last updated:** 2026-10-23T23:30Z

## Done

Newest first. Max 5 entries — drop the oldest when adding a sixth.

1. **E4: the `scn5` scene decoder.** Model table, nodes, group and pools
   found; every pooled instance walked in id order; `scenery.jsonl`,
   `scenery_models.json` and the `trees` and `lines` tiles written: Caucasus
   in 5 s. ADR **0045** sets the id shift by the largest node, not the
   version. *Verified by:* an agent, through `mise run check`, and the
   opt-in checks: Caucasus and Nevada objects equal the reference decode row
   for row; 87.9 % of Nevada's runway lights within 2°; all eight maps walk.
2. **E3: the `rn4` road network.** Edges found by scanning point runs, with
   kind, class and node ids; roads and rail written to their tables. ADR
   **0043** sets the router check at 2 m roads, 3 m rail; ADR **0044** reads
   ids as junctions and writes each map's catch-all id null.
   *Verified by:* an agent, through `mise run check`, and the opt-in checks
   on all eight maps: Caucasus router p90 1.58 m roads, 2.42 m rail.
3. **E2: the `surface5` decoder.** The table of contents is scanned for
   blocks and fields, finest blocks read on demand with their `Runway`
   subsets, a point sampler, and `height`, `water` and `surface` tiles
   written band by band: Caucasus whole in 20 s. ADR **0041** puts water
   over land and river first; ADR **0042** keeps the mesh edge as coverage.
   *Verified by:* an agent, through `mise run check`, and the opt-in checks:
   all eight maps read; Caucasus 99.9485 % heights exact, 4 water cells off.
4. **E1: the container reader.** Every map file's header gives class and
   version; a decoder is chosen by the pair, and an unknown one is refused
   naming file, class and version. A map's files are found by role and
   listed for the manifest, with the build and the `entry.lua` id.
   *Verified by:* an agent, through `mise run check`, and the opt-in check
   on all eight installed maps of 2.9.30.28738: Nevada's scene is v8.
5. **C3: `check-extract` validates a v2 extract.** Manifest keys and
   version first, then grid, layers, every tile's size, min/max and value
   domain, the absent-tile rules, unlisted tiles and stray `.tmp` files,
   and every table's keys; all failures listed with their path, exit 1.
   *Verified by:* an agent, through `mise run check`.

## Next

One task. The thing to pick up immediately.

**E5** — airfields: the airfield `rn4` and `rn5`, runway ends and their
pairing, stands with class, stand parameters from `misc/assets.assets`.
*Verified by:* an agent on the Windows machine, with `DCSTERRAIN_INSTALL`
and `DCSTERRAIN_EXTRACT` set, against the live Caucasus stands and runways.

## Then

Max 5 entries, in dependency order. Task ids from `plan.md`.

1. **E6** — Lua tables, independent of E5, against its opt-in Caucasus
   figures.
2. **C4** — `pack` reads the extract; where carry 1 is discharged.
3. **C5a / C5b / C5c** — the packed file's `meta`, base grids and tables.
4. **C8** — scenery classification from the model boxes.
5. **E7** — forest and built-up polygons from the mesh's materials.

Milestone in view: **MS1 Packed file**. Proof is `pack` and `check` passing
on the synthetic extract, and every decoder passing its opt-in checks on
Caucasus.

## Carries

Things a later task must not lose. Max 10 — `CLAUDE.md` has the rules.

| # | Carry | Discharged by |
|---|---|---|
| 1 | `synth` produces no tile wholly outside the mesh at any size: its margin is 2 km and a tile is 12.8 km. Test that absent-tile read against a hand-built manifest, not a generated extract. The all-sea case is generated, as one interior tile. | C4 |
| 2 | `ViewshedResponse` requires `bits`, but the server returns polygons by default with the bitmask only on request: make the mask fields optional when the server needs to omit them. | M3 |
| 3 | The hook still writes v1, so `extractor/test/constants.lua` pins `FORMAT_VERSION` to 1 rather than the shared 2; restore it, and drop the fill and road-seed constants if nothing else reads them. | X15 |
| 4 | The synthetic road network is a tree, so the junction-to-bridge edge outranks the bridge in betweenness and the contracted arms tie: "bridge has max betweenness" fails as stated. Change the layout or the test, and record which in an ADR. | C9 |
| 5 | `Mesh::sample` tests every triangle of each block whose box holds the point, about 10 000 a block: fine for checking thousands of points, not for putting millions of road, scenery and tree positions on the ground. Index the triangles, or read heights from the written tiles, before those checks run. | E8 |
| 6 | The opt-in Nevada runway-light check in `tests/scn5.rs` takes runway ends from the reference decode's `airfields.json` (`DCSTERRAIN_SCENE_REFERENCE`): point it at the decoded runways. The mesh's `Runway` material is no substitute: on Nevada it gives nine strips, mostly lit as taxiways. | E5 |
