# State

Where the work is. Updated at the end of every working session, before handing
back. This file holds progress; `plan.md` holds the task graph and
`design.md` and `queries.md` hold the design.

**Last updated:** 2026-10-27T16:58Z

## Done

Newest first. Max 5 entries — drop the oldest when adding a sixth.

1. **The reset (documents).** The tool reads the install in place with a
   rebuildable cache; the packed file, SQLite and the MCP server are gone from
   the design. `design.md`, `queries.md` and a new `plan.md` replace the nine
   frozen specs, their ledgers and the 49 ADRs, which are deleted (git keeps
   them, last at `4b85bd0`). ADRs restart at 0001. *Verified by:* an agent,
   through `mise run docs`.
2. **The `grid` module** (pre-reset task C4). Cell and tile addressing; `Window` reads a
   rectangle across tiles and past the grid; `sample` bilinear on height.
   *Verified by:* an agent, through `mise run check`.
3. **The Lua tables** (pre-reset task E6). `Beacons.lua`, `Radio.lua`, `towns.lua` and
   `nodes.lua` run in a sandboxed Lua 5.1; the projection is fitted from the
   beacons. *Verified by:* an agent, opt-in against the live extract.

## Next

One task. The thing to pick up immediately.

**A1** — remove the pack pipeline and its subcommands, replace pre-reset ADR
citations in code and Lua with their reasons, and update the README.
*Verified by:* an agent, through `mise run check` and `mise run lua-test`.

## Then

Max 5 entries, in dependency order. Task ids from `plan.md`.

1. **N0** — live gate: read a map's files while DCS has it loaded. Needs DCS;
   only somebody at the install can run it, beside N1 onward.
2. **N1** — the `Source` trait and `Synth` in memory; `--map synth`.
3. **N2** — the cache directory, keys and atomic entries.
4. **N3** — the mesh in place: tiles on demand and point mode.
5. **N4** — scenery in place: the pool-tree reader.

Milestone in view: **MS1 Engine on the install**.

## Carries

Things a later task must not lose. Max 10 — `CLAUDE.md` has the rules.

| # | Carry | Discharged by |
|---|---|---|
| 1 | The hook still writes v1, so `extractor/test/constants.lua` pins `FORMAT_VERSION` to 1 rather than the shared 2; restore it, and drop the fill and road-seed constants if nothing else reads them. | X1 |
| 2 | The synthetic road network is a tree, so the junction-to-bridge edge outranks the bridge in betweenness and the contracted arms tie: "betweenness peaks at the bridge" fails as stated. Change the layout or the test, and say which in `design.md`. | N6 |
| 3 | `lua::tables::decode` returns `included`, the shared scripts each `dofile` ran, and `unresolved`, each name nothing assigns and each module the sandbox stood in for: report both in `check`. | N10 |
