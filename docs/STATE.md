# State

Where the work is. Updated at the end of every working session, before handing
back. This file holds progress; `plan.md` holds the task graph and
`decisions/` holds what has diverged from the frozen documents.

**Last updated:** 2026-10-23T17:10Z

## Done

Newest first. Max 5 entries — drop the oldest when adding a sixth.

1. **C3: `check-extract` validates a v2 extract.** Manifest keys and
   version first, then grid, layers, every tile's size, min/max and value
   domain, the absent-tile rules, unlisted tiles and stray `.tmp` files,
   and every table's keys; all failures listed with their path, exit 1.
   *Verified by:* an agent, through `mise run check`.
2. **C2: the synthetic theatre writes a v2 extract.** `dcsterrain synth`
   writes every layer, table and polygon of ADR 0039 from closed forms,
   manifest last and byte-identical between runs; roads, the runway and
   the power line sit on cell centres; a transverse Mercator gives towns
   and beacons positions that project back. *Verified by:* an agent,
   through `mise run check`.
3. **C1: workspace scaffold and types.** Every operation's request and
   response in `types`, with the closed criteria set; `OPERATIONS` is the
   one list of names; `dcsterrain schema` prints one draft 2020-12 document,
   held to a committed snapshot; every subcommand parses its flags and the
   rest say "not built yet"; stderr logging, debug with `--verbose`.
   *Verified by:* an agent, through `mise run check`.
4. **F3: extract format v2.** ADR **0039** walks every field to its decoder
   and reader: no journal or resume, a manifest written last, `files` with
   class and version, the mesh edge as coverage, `trees` and `lines` count
   layers, road edges with class and node ids, model boxes, forest and
   built-up polygons in `ground.jsonl`, hook-only fields null. ADR **0040**
   moves the projection fit into the extractor. *Verified by:* the maintainer
   reading the ADRs.
5. **The extract is decoded from the terrain files.** Read-only decoding of
   every Caucasus map file reproduced the live extract with no DCS running:
   heights 99.95 % exact, water all but 67 of 141 M cells, 900 of 900 stands,
   roads within 1.5 m of the router. The same decoders ran on Nevada (scene
   format v8) and Syria. ADRs **0034** to **0038**: the files are the source,
   decoders dispatch on class and version, 50 m on every map, the hook keeps
   only an optional tables pass, the projection is fitted from the beacons.
   Plan rewritten: E, F3, X15, C15, Q10. *Verified by:* the maintainer's
   session on 2.9.30.28738, against the live extract from 2.9.29.27468.

## Next

One task. The thing to pick up immediately.

**E1** — the container reader that classifies every file of an install,
refusing an unknown class and version. *Verified by:* an agent through
`mise run check` for the offline tests; the opt-in check needs an install's
files, so the maintainer on Windows.

## Then

Max 5 entries, in dependency order. Task ids from `plan.md`.

1. **E2** — `surface5`: heights, water, surface to 50 m tiles.
3. **E3 / E4 / E5 / E6** — roads, scenery, airfields, Lua tables: independent,
   each against its opt-in Caucasus figures.
4. **C4** — `pack` reads the extract; where carry 1 is discharged.
5. **C5a / C5b / C5c** — the packed file's `meta`, base grids and tables.
5. **C8** — scenery classification from the model boxes.

Milestone in view: **MS0 Contract** (P1, P2, F1, F2, F3, C1, C2, C3, E1).
Proof is `check-extract` accepting the Rust synthetic v2 extract, and E1
classifying every terrain file of an install.

## Carries

Things a later task must not lose. Max 10 — `CLAUDE.md` has the rules.

| # | Carry | Discharged by |
|---|---|---|
| 1 | `synth` produces no tile wholly outside the mesh at any size: its margin is 2 km and a tile is 12.8 km. Test that absent-tile read against a hand-built manifest, not a generated extract. The all-sea case is generated, as one interior tile. | C4 |
| 2 | `ViewshedResponse` requires `bits`, but the server returns polygons by default with the bitmask only on request: make the mask fields optional when the server needs to omit them. | M3 |
| 3 | The hook still writes v1, so `extractor/test/constants.lua` pins `FORMAT_VERSION` to 1 rather than the shared 2; restore it, and drop the fill and road-seed constants if nothing else reads them. | X15 |
| 4 | The synthetic road network is a tree, so the junction-to-bridge edge outranks the bridge in betweenness and the contracted arms tie: "bridge has max betweenness" fails as stated. Change the layout or the test, and record which in an ADR. | C9 |
