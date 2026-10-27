# State

Where the work is. Updated at the end of every working session, before handing
back. This file holds progress; `plan.md` holds the task graph and
`design.md` and `queries.md` hold the design.

**Last updated:** 2026-10-27T21:10Z

## Done

Newest first. Max 5 entries — drop the oldest when adding a sixth.

1. **N3: the mesh in place.** An install terrain is a `Source`:
   `mesh.toc` is the first cache entry; blocks are read at their offsets,
   prepared with a triangle bin index into a byte-budgeted LRU; 64-cell
   tiles of height, water, surface and cover on demand, and point mode.
   `query --cache`; `describe` reports the extent. The extract tile writer
   is gone. *Verified by:* an agent, through `mise run check`, and opt-in on
   2.9.30.28738: Caucasus heights 99.9485 % exact, water off on 4 cells,
   open from the cache 8 ms.
2. **N2: the cache.** `cache` keeps raw entries under
   `<cache>/<terrain>/<build>/`, keyed on the build and a fresh stat and
   header read of the files of each entry's roles, written by rename;
   `index`, `cache list|prune|clear`, and a cache column in `list-terrains`.
   *Verified by:* an agent, through `mise run check`.
3. **N1: Synth as the Source.** `Source` with `Synth` in memory, a patch
   of each cover class; `grid` reads through it and its tests run on `Synth`;
   the synthetic extract writer is gone. A terrain opens by its
   `entry.lua` id; `query --terrain <id|synth> describe` and `list-terrains`
   work, `--install` from the registry on Windows. *Verified by:* an agent,
   through `mise run check`, and the opt-in install test on 2.9.30.28738.
4. **A1: the reset's code.** The CLI keeps only `schema`; `pack/`,
   `rusqlite`, the MCP crate and the theatre probe are gone; no code cites a
   pre-reset ADR. The extract writers stay until N1, N3 and N4 replace the
   tests that build fixtures with them. *Verified by:* an agent, through
   `mise run check` and `mise run lua-test`.
5. **The reset (documents).** The tool reads the install in place with a
   rebuildable cache; the packed file, SQLite and the MCP server are gone from
   the design. `design.md`, `queries.md` and a new `plan.md` replace the nine
   frozen specs, their ledgers and the 49 ADRs, which are deleted (git keeps
   them, last at `4b85bd0`). ADRs restart at 0001. *Verified by:* an agent,
   through `mise run docs`.

## Next

One task. The thing to pick up immediately.

**N4** — scenery in place: the pool-tree reader, `scene.nodes`, the trees
and lines pools; the scenery JSONL writer goes. *Verified by:* an agent,
opt-in against the install. **N0**'s Process Monitor and RAMMap captures
remain, for somebody at the install; no agent task waits.

## Then

Max 5 entries, in dependency order. Task ids from `plan.md`.

1. **N0** — the rest: DCS's open flags and its pages in the OS cache.
2. **N5** — `tables.json`, the projection check and `index --hook`.
3. **N6** — the road graph: junctions, contraction, betweenness.
4. **N7** — scenery classes over the model table.
5. **N9** — horizon tiles and `enclosure`.

Milestone in view: **MS1 Engine on the install**.

## Carries

Things a later task must not lose. Max 10 — `CLAUDE.md` has the rules.

| # | Carry | Discharged by |
|---|---|---|
| 1 | The hook still writes v1, so `extractor/test/constants.lua` pins `FORMAT_VERSION` to 1 rather than the shared 2; restore it, and drop the fill and road-seed constants if nothing else reads them. | X1 |
| 2 | The synthetic road network is a tree, so the junction-to-bridge edge outranks the bridge in betweenness and the contracted arms tie: "betweenness peaks at the bridge" fails as stated. Change the layout or the test, and say which in `design.md`. | N6 |
| 3 | `lua::tables::decode` returns `included`, the shared scripts each `dofile` ran, and `unresolved`, each name nothing assigns and each module the sandbox stood in for: report both in `check`. | N10 |
| 5 | `tables.json` cannot open with the binary entry header and stay JSON, so `cache` keys raw entries only; key it inside the JSON, from `TerrainCache::key`. | N5 |
