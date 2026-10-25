# State

Where the work is. Updated at the end of every working session, before handing
back. This file holds progress; `plan.md` holds the task graph and
`decisions/` holds what has diverged from the frozen documents.

**Last updated:** 2026-10-25T05:08Z

## Done

Newest first. Max 5 entries — drop the oldest when adding a sixth.

1. **C5a: the `pack` skeleton.** `pack` writes `meta`, `grid` and
   `grid_tile` at 8 KiB pages with no journal, each grid in one
   transaction, then `VACUUM`, under a `.partial` name renamed into place;
   `meta` carries ADR 0008's keys and ADR 0039's fill. `check` reports
   every `meta` key, NULLs included. *Verified by:* an agent, through
   `mise run check`: the synthetic extract packs at 50 and 100 m and passes.
2. **C4: the `grid` module.** Cell and tile addressing; `Window` reads a
   rectangle across tiles and past the grid, off-grid cells `nodata`;
   `sample` bilinear on `height`, `None` where a weighted corner is
   `nodata`, nearest elsewhere; unwritten tiles rebuilt by ADR 0039's
   three rules. *Verified by:* an agent, through `mise run check`: windows
   at every tile and grid edge and corner, the whole grid against the
   closed form, and an off-mesh tile from a hand-edited manifest.
3. **E6: the Lua tables.** `Beacons.lua`, `Radio.lua`, `towns.lua` and
   `nodes.lua` run in a sandboxed Lua 5.1 (ADR **0047**); the projection
   fitted to the beacons, towns put through it, and airdrome ids from
   `Radio.lua`'s labels. *Verified by:* an agent, through `mise run check`,
   and the opt-in checks: Caucasus beacons, radio, nodes and towns equal the
   live extract field for field, town `x, z` within 2.2 mm; the fit within
   0.01 m of the measured row on all 8 maps.
4. **E5: airfields.** Nodes found by their matrix; runway ends told from
   stands by coloring each linked group, and paired by designator, then
   length; each stand's class from the `.rn5`, numbered in its own order,
   and its parameters from `assets.assets`. ADR **0046** writes a named
   node as a stand only when its class has a stand record. *Verified by:*
   an agent, through `mise run check`, and the opt-in checks: Caucasus
   900/900 stands on every parameter and 21/21 runways, bit-exact.
5. **E4: the `scn5` scene decoder.** Model table, nodes, group and pools
   found; every pooled instance walked in id order; `scenery.jsonl`,
   `scenery_models.json` and the `trees` and `lines` tiles written: Caucasus
   in 5 s. ADR **0045** sets the id shift by the largest node, not the
   version. *Verified by:* an agent, through `mise run check`, and the
   opt-in checks: Caucasus and Nevada objects equal the reference decode row
   for row; 87.9 % of Nevada's runway lights within 2°; all eight maps walk.

## Next

One task. The thing to pick up immediately.

**C5b / C5c** — the packed file's base grids and tables. C5b re-tiles
through `grid` and writes each grid with `pack::write_grid`; C5c creates
its own tables, which the skeleton leaves out. *Verified by:* an agent,
through `mise run check`, on the synthetic extract; no DCS needed.

## Then

Max 5 entries, in dependency order. Task ids from `plan.md`.

1. **C8** — scenery classification from the model boxes.
2. **E7** — forest and built-up polygons from the mesh's materials.
3. **E8** — `dcsterrain extract` runs the decoders and writes the extract.
4. **C10** — `pack` checks `latlon_samples` against the extract's `crs`.
5. **C6** — `derive` `valid`, `tpi_2000` and the query-time window layers.

Milestone in view: **MS1 Packed file**. Proof is `pack` and `check` passing
on the synthetic extract, and every decoder passing its opt-in checks on
Caucasus.

## Carries

Things a later task must not lose. Max 10 — `CLAUDE.md` has the rules.

| # | Carry | Discharged by |
|---|---|---|
| 1 | `ViewshedResponse` requires `bits`, but the server returns polygons by default with the bitmask only on request: make the mask fields optional when the server needs to omit them. | M3 |
| 2 | The hook still writes v1, so `extractor/test/constants.lua` pins `FORMAT_VERSION` to 1 rather than the shared 2; restore it, and drop the fill and road-seed constants if nothing else reads them. | X15 |
| 3 | The synthetic road network is a tree, so the junction-to-bridge edge outranks the bridge in betweenness and the contracted arms tie: "bridge has max betweenness" fails as stated. Change the layout or the test, and record which in an ADR. | C9 |
| 4 | `Mesh::sample` tests every triangle of each block whose box holds the point, about 10 000 a block: fine for checking thousands of points, not for putting millions of road, scenery and tree positions on the ground. Index the triangles, or read heights from the written tiles, before those checks run. | E8 |
| 5 | `lua::tables::decode` returns `included`, the shared scripts each `dofile` ran, and `unresolved`, each name nothing assigns and each module the sandbox stood in for: put the first in the manifest's `files` and the second in `notes`. | E8 |
