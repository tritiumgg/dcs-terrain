# State

Where the work is. Updated at the end of every working session, before handing
back. This file holds progress; `plan.md` holds the task graph and
`decisions/` holds what has diverged from the frozen documents.

**Last updated:** 2026-10-22T21:57Z

## Done

Newest first. Max 5 entries — drop the oldest when adding a sixth.

1. **The extract is decoded from the terrain files.** Read-only decoding of
   every Caucasus map file reproduced the live extract with no DCS running:
   heights 99.95 % exact, water all but 67 of 141 M cells, 900 of 900 stands,
   roads within 1.5 m of the router. The same decoders ran on Nevada (scene
   format v8) and Syria. ADRs **0034** to **0038**: the files are the source,
   decoders dispatch on class and version, 50 m on every map, the hook keeps
   only an optional tables pass, the projection is fitted from the beacons.
   Plan rewritten: E, F3, X15, C15, Q10. *Verified by:* the maintainer's
   session on 2.9.30.28738, against the live extract from 2.9.29.27468.
2. **X8b: the scenery sweep**, merged as built and reviewed (PR **55**). Its
   live half never ran: ADR 0034 retires it.
3. **X8a: the mission-pass transport and `surface`.** `server_call` runs a
   chunk through `net.dostring_in("server", ...)` and refuses any answer not
   framed as its own length; the surface sweep asks 64 rows a step, one
   printable character a cell, and writes a tile when its last band is in.
   *Verified by:* an agent offline, then live on Caucasus 2.9.29.27468: ten
   cells of the Kutaisi crop re-read through the bridge equal the file, a
   150 km crop stopped twice mid-surface resumed without sweeping a tile
   again (501 lines, all distinct), about 73 ms a tile. PR **54**.
4. **X7: roads and railroads.** Four hook jobs, `<kind>:seeds` then
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
5. **X6: the pre-sweep, `water` and `height`.** Prepare is `identity`,
   `presweep`, `grid`: the pre-sweep walks a 5 km lattice one cell a step, a
   road snap first and a 2 km line of heights only where a road lies 5 to
   25 km off (ADR **0027**, after the spec's rule authored Caucasus's roadless
   mountains), every far snap kept as the disc it clears; it skips itself on
   a crop run and on a directory whose manifest carries the rectangle. The
   hook pass is `config`, `tables`, then `water+height` as one sweep with both
   calls at each cell (ADR **0028**: measured half the cost of two passes),
   one tile a step; it builds `run.skip`. *Verified by:* the maintainer on
   Caucasus 2.9.29.27468: crop cells equal the bridge's, the whole map's
   rectangle is 470 x 785 km in 19 s, and the fused sweep took 100 s for
   2 294 tiles against 189 s as two passes. PR **52**.

## Next

One task. The thing to pick up immediately.

**F3** — extract format v2, as an ADR: the layers the files add, the file
classes and versions, the authored rectangle read from the files, which
`config.json` fields survive without the sim, beacon `latlon_samples`. Walk
every field to its decoder and to the C task that reads it. The format
reference `docs/data/file-formats.md` describes each structure. *Verified
by:* the maintainer reading the ADR; nothing runs.

## Then

Max 5 entries, in dependency order. Task ids from `plan.md`.

1. **C1 / C2** — workspace scaffold, then `synth` writing v2 on the F2 constants.
2. **C3 / E1** — `check-extract` on v2, and the container reader that classifies
   every file of an install and refuses an unknown version.
3. **E2** — `surface5`: heights, water, surface to 50 m tiles.
4. **E3 / E4 / E5 / E6** — roads, scenery, airfields, Lua tables: independent,
   each against its opt-in Caucasus figures.
5. **C4** — `pack` reads the extract; where carry 1 is discharged.

Milestone in view: **MS0 Contract** (P1, P2, F1, F2, F3, C1, C2, C3, E1).
Proof is `check-extract` accepting the Rust synthetic v2 extract, and E1
classifying every terrain file of an install.

## Carries

Things a later task must not lose. Max 10 — `CLAUDE.md` has the rules.

| # | Carry | Discharged by |
|---|---|---|
| 1 | `synth` produces no all-fill tile at any size: the fill margin is 2 km and a tile is 12.8 km, so no tile is fill throughout. Test the absent-fill-tile read against a hand-built manifest, not against a generated extract. The all-sea case is generated, as one interior tile. | C4 |
