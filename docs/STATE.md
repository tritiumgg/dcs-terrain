# State

Where the work is. Updated at the end of every working session, before handing
back. This file holds progress; `plan.md` holds the task graph and
`decisions/` holds what has diverged from the frozen documents.

**Last updated:** 2026-10-24T08:25Z

## Done

Newest first. Max 5 entries — drop the oldest when adding a sixth.

1. **E6: the Lua tables.** `Beacons.lua`, `Radio.lua`, `towns.lua` and
   `nodes.lua` read as data, never run, with the constants their `dofile`d
   scripts assign; the projection fitted to the beacons, towns put through
   it, and airdrome ids from `Radio.lua`'s labels. *Verified by:* an agent,
   through `mise run check`, and the opt-in checks: Caucasus beacons, radio,
   nodes and towns equal the live extract field for field, town `x, z`
   within 2.2 mm; the fit within 0.01 m of the measured row on all 8 maps.
2. **E5: airfields.** Nodes found by their matrix; runway ends told from
   stands by coloring each linked group, and paired by designator, then
   length; each stand's class from the `.rn5`, numbered in its own order,
   and its parameters from `assets.assets`. ADR **0046** writes a named
   node as a stand only when its class has a stand record. *Verified by:*
   an agent, through `mise run check`, and the opt-in checks: Caucasus
   900/900 stands on every parameter and 21/21 runways, bit-exact.
3. **E4: the `scn5` scene decoder.** Model table, nodes, group and pools
   found; every pooled instance walked in id order; `scenery.jsonl`,
   `scenery_models.json` and the `trees` and `lines` tiles written: Caucasus
   in 5 s. ADR **0045** sets the id shift by the largest node, not the
   version. *Verified by:* an agent, through `mise run check`, and the
   opt-in checks: Caucasus and Nevada objects equal the reference decode row
   for row; 87.9 % of Nevada's runway lights within 2°; all eight maps walk.
4. **E3: the `rn4` road network.** Edges found by scanning point runs, with
   kind, class and node ids; roads and rail written to their tables. ADR
   **0043** sets the router check at 2 m roads, 3 m rail; ADR **0044** reads
   ids as junctions and writes each map's catch-all id null.
   *Verified by:* an agent, through `mise run check`, and the opt-in checks
   on all eight maps: Caucasus router p90 1.58 m roads, 2.42 m rail.
5. **E2: the `surface5` decoder.** The table of contents is scanned for
   blocks and fields, finest blocks read on demand with their `Runway`
   subsets, a point sampler, and `height`, `water` and `surface` tiles
   written band by band: Caucasus whole in 20 s. ADR **0041** puts water
   over land and river first; ADR **0042** keeps the mesh edge as coverage.
   *Verified by:* an agent, through `mise run check`, and the opt-in checks:
   all eight maps read; Caucasus 99.9485 % heights exact, 4 water cells off.

## Next

One task. The thing to pick up immediately.

**C4** — `grid`: tile addressing, cross-tile window reads, bilinear sample,
the absent-tile rules of ADR 0039, nodata handling; where carry 1 is
discharged. *Verified by:* an agent, through `mise run check`, on the
synthetic extract and a hand-built manifest; no DCS needed.

## Then

Max 5 entries, in dependency order. Task ids from `plan.md`.

1. **C5a / C5b / C5c** — the packed file's `meta`, base grids and tables.
2. **C8** — scenery classification from the model boxes.
3. **E7** — forest and built-up polygons from the mesh's materials.
4. **E8** — `dcsterrain extract` runs the decoders and writes the extract.
5. **C10** — `pack` checks `latlon_samples` against the extract's `crs`.

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
| 6 | `lua::tables::decode` returns `included`, the shared scripts each `dofile` names (read for their constants), and `unresolved`, each name nothing assigns: put the first in the manifest's `files` and the second in `notes`. GermanyCW's `BEACON_TYPE_AIRPORT_TACAN` is defined nowhere, so that beacon's `type` is null, as in the game. | E8 |
