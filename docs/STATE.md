# State

Where the work is. Updated at the end of every working session, before handing
back. This file holds progress; `plan.md` holds the task graph and
`design.md` and `queries.md` hold the design.

**Last updated:** 2026-10-27T20:05Z

## Done

Newest first. Max 5 entries — drop the oldest when adding a sixth.

1. **N2: the cache.** `cache` keeps raw entries under
   `<cache>/<terrain>/<build>/`, keyed on the build and a fresh stat and
   header read of the files of each entry's roles, written by rename;
   `index`, `cache list|prune|clear`, and a cache column in `list-terrains`.
   No entry kind exists yet: `cache::entries()` is empty. *Verified by:* an
   agent, through `mise run check`.
2. **N1: Synth as the Source.** `Source` with `Synth` in memory, a patch
   of each cover class; `grid` reads through it and its tests run on `Synth`;
   the synthetic extract writer is gone. An install terrain is not a `Source`
   until N3. A terrain opens by its
   `entry.lua` id; `query --terrain <id|synth> describe` and `list-terrains`
   work, `--install` from the registry on Windows. *Verified by:* an agent,
   through `mise run check`, and the opt-in install test on 2.9.30.28738.
3. **A1: the reset's code.** The CLI keeps only `schema`; `pack/`,
   `rusqlite`, the MCP crate and the theatre probe are gone; no code cites a
   pre-reset ADR. The extract writers stay until N1, N3 and N4 replace the
   tests that build fixtures with them. *Verified by:* an agent, through
   `mise run check` and `mise run lua-test`.
4. **The reset (documents).** The tool reads the install in place with a
   rebuildable cache; the packed file, SQLite and the MCP server are gone from
   the design. `design.md`, `queries.md` and a new `plan.md` replace the nine
   frozen specs, their ledgers and the 49 ADRs, which are deleted (git keeps
   them, last at `4b85bd0`). ADRs restart at 0001. *Verified by:* an agent,
   through `mise run docs`.
5. **The `grid` module** (pre-reset task C4). Cell and tile addressing;
   `Window` reads a rectangle across tiles and past the grid; `sample`
   bilinear on height. *Verified by:* an agent, through `mise run check`.

## Next

One task. The thing to pick up immediately.

**N3** — the mesh in place: `mesh.toc` as the first cache entry, tiles on
demand, point mode. *Verified by:* an agent, opt-in against the install.
**N0** is still open and live: it needs DCS on a terrain, so only somebody
at the install can do it, and no agent task waits on it.

## Then

Max 5 entries, in dependency order. Task ids from `plan.md`.

1. **N0** — live gate: read a terrain's files while DCS has it loaded.
2. **N4** — scenery in place: the pool-tree reader.
3. **N5** — `tables.json`, the projection check and `index --hook`.
4. **N6** — the road graph: junctions, contraction, betweenness.
5. **N7** — scenery classes over the model table.

Milestone in view: **MS1 Engine on the install**.

## Carries

Things a later task must not lose. Max 10 — `CLAUDE.md` has the rules.

| # | Carry | Discharged by |
|---|---|---|
| 1 | The hook still writes v1, so `extractor/test/constants.lua` pins `FORMAT_VERSION` to 1 rather than the shared 2; restore it, and drop the fill and road-seed constants if nothing else reads them. | X1 |
| 2 | The synthetic road network is a tree, so the junction-to-bridge edge outranks the bridge in betweenness and the contracted arms tie: "betweenness peaks at the bridge" fails as stated. Change the layout or the test, and say which in `design.md`. | N6 |
| 3 | `lua::tables::decode` returns `included`, the shared scripts each `dofile` ran, and `unresolved`, each name nothing assigns and each module the sandbox stood in for: report both in `check`. | N10 |
| 4 | The live extract's tiles are 256 cells and carry `trees` and `lines` where `Source` tiles are 64 and carry `surface` and `cover`; `Extract::tile` rebuilds the ones the extract left out. Compare install tiles against it cell by cell, not tile by tile. | N3 |
| 5 | `tables.json` cannot open with the binary entry header and stay JSON, so `cache` keys raw entries only; key it inside the JSON, from `TerrainCache::key`. | N5 |
