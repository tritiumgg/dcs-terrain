# State

Where the work is. Updated at the end of every working session, before handing
back. This file holds progress; `plan.md` holds the task graph and
`decisions/` holds what has diverged from the frozen documents.

**Last updated:** 2026-10-22T22:59Z

## Done

Newest first. Max 5 entries — drop the oldest when adding a sixth.

1. **F3: extract format v2.** ADR **0039** walks every field to its decoder
   and reader: no journal or resume, a manifest written last, `files` with
   class and version, the mesh edge as coverage, `trees` and `lines` count
   layers, road edges with class and node ids, model boxes, forest and
   built-up polygons in `ground.jsonl`, hook-only fields null. ADR **0040**
   moves the projection fit into the extractor. *Verified by:* the maintainer
   reading the ADRs.
2. **The extract is decoded from the terrain files.** Read-only decoding of
   every Caucasus map file reproduced the live extract with no DCS running:
   heights 99.95 % exact, water all but 67 of 141 M cells, 900 of 900 stands,
   roads within 1.5 m of the router. The same decoders ran on Nevada (scene
   format v8) and Syria. ADRs **0034** to **0038**: the files are the source,
   decoders dispatch on class and version, 50 m on every map, the hook keeps
   only an optional tables pass, the projection is fitted from the beacons.
   Plan rewritten: E, F3, X15, C15, Q10. *Verified by:* the maintainer's
   session on 2.9.30.28738, against the live extract from 2.9.29.27468.
3. **X8b: the scenery sweep**, merged as built and reviewed (PR **55**). Its
   live half never ran: ADR 0034 retires it.
4. **X8a: the mission-pass transport and `surface`.** `server_call` runs a
   chunk through `net.dostring_in("server", ...)` and refuses any answer not
   framed as its own length; the surface sweep asks 64 rows a step, one
   printable character a cell, and writes a tile when its last band is in.
   *Verified by:* an agent offline, then live on Caucasus 2.9.29.27468: ten
   cells of the Kutaisi crop re-read through the bridge equal the file, a
   150 km crop stopped twice mid-surface resumed without sweeping a tile
   again (501 lines, all distinct), about 73 ms a tile. PR **54**.
5. **X7: roads and railroads.** Four hook jobs, `<kind>:seeds` then
   `<kind>:paths`: seeds numbered by a fixed plan, merged into the lowest kept
   id at 100 m, each unordered pair of four nearest neighbours asked once by
   a cursor, and the file its own journal, read back in chunks on every
   Start with a cut-short tail repaired by a streamed copy (ADR **0030**); a
   seed provably beyond 25 km from every road is neither asked nor written,
   tested per lattice row as disc spans (ADR **0031**). *Verified by:* an
   agent offline, then the maintainer on Caucasus 2.9.29.27468: on the 5 km
   Kutaisi crop six snaps and six routes re-read through the bridge are the
   file's bytes on both networks; the whole map's four sweeps took 220, 96,
   151 and 110 s (187 567 and 133 993 snaps, 121 368 and 39 691 routes,
   85 and 82 MB), against the spec's 25 min for roads alone, and a Stop
   mid-paths then Start read back 187 567 seeds and 7 744 pairs in 2.5 s
   and asked nothing again. PR **53**.

## Next

One task. The thing to pick up immediately.

**C1 / C2** — workspace scaffold, then `synth` writing a v2 directory per
ADR 0039, with F2's constants extended for road classes, tree and line
counts and ground polygons. *Verified by:* an agent, through `mise run
check`; no DCS.

## Then

Max 5 entries, in dependency order. Task ids from `plan.md`.

1. **C3 / E1** — `check-extract` on v2, and the container reader that classifies
   every file of an install and refuses an unknown version.
2. **E2** — `surface5`: heights, water, surface to 50 m tiles.
3. **E3 / E4 / E5 / E6** — roads, scenery, airfields, Lua tables: independent,
   each against its opt-in Caucasus figures.
4. **C4** — `pack` reads the extract; where carry 1 is discharged.
5. **C5a / C5b / C5c** — the packed file's `meta`, base grids and tables.

Milestone in view: **MS0 Contract** (P1, P2, F1, F2, F3, C1, C2, C3, E1).
Proof is `check-extract` accepting the Rust synthetic v2 extract, and E1
classifying every terrain file of an install.

## Carries

Things a later task must not lose. Max 10 — `CLAUDE.md` has the rules.

| # | Carry | Discharged by |
|---|---|---|
| 1 | `synth` produces no tile wholly outside the mesh at any size: its margin is 2 km and a tile is 12.8 km. Test that absent-tile read against a hand-built manifest, not a generated extract. The all-sea case is generated, as one interior tile. | C4 |
