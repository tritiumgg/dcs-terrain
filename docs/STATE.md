# State

Where the work is. Updated at the end of every working session, before handing
back. This file holds progress; `plan.md` holds the task graph and
`design.md` and `queries.md` hold the design.

**Last updated:** 2026-11-05T23:47Z

## Done

Newest first. Max 5 entries — drop the oldest when adding a sixth.

1. **Q21: `route` off the road.** `offroad` plans each end's way across
   country over Q20's search, every road cell reached a start or goal of the
   one road search; a way off the road the whole way competes; people's page
   lists every turn. `max_offroad` is 5 km, default and limit, with the ground
   searched in few tiles. *Verified by:* an agent, through `mise run check` on
   `Synth`; Kobuleti to Batumi answers, V1's samples unchanged, GermanyCW
   peaks at 61.4 MiB, on 2.9.31.29434.
2. **Q20: the off-road search, and `trafficability` with it.** The least-cost
   way over the 50 m cells to the first road reached, 16 neighbors, straightened;
   `trafficability` answers `way` and `straight` within `max_distance`, 20 km,
   and prints a table. *Verified by:* an agent, through `mise run check` on
   `Synth` (Bellman–Ford, the hill, the river, two table snapshots); GermanyCW
   peaks at 46.8 MiB searching the whole 20 km, on 2.9.31.29434.
3. **Dropped M4: the skill stays the guidance.** Claude Code keeps only
   4 096 characters of a server's instructions and of each tool
   description, 2 048 before v2.1.296, and the guidance comes to about
   8 500; `plan.md` records why. *Verified by:* the user decided; the
   limits read in Claude Code's MCP documentation and changelog.
4. **Planned MS5: off the road, MCP guidance, smaller answers.** M4, Q20
   and Q21 added to `plan.md`, Q19 amended to turn on `preserve_order`
   first; ADR 0013 plans a route's off-road ends over the off-road cost;
   `queries.md` marks `route` and `trafficability`'s planned fields.
   *Verified by:* the user approved the design; `mise run docs`.
5. **H5 fix: `geo` for a person with an MGRS.** `--to-xz` takes an MGRS,
   read to its square's center, and every latitude and longitude form a
   place takes; `geo` prints one row per point with x, z, MGRS, latitude
   and longitude; the guide's "Convert a position" shows it.
   *Verified by:* an agent, through `mise run check` on `Synth` (three `geo`
   snapshots); the new guide commands run in PowerShell 5.1 and 7 on
   Caucasus of 2.9.31.29434, output pasted as printed. The user's rerun of
   H5 is pending.

## Next

One task. The thing to pick up immediately.

**Q19** — encode each answer as it is written, to cut its memory.
*Verified by:* an agent on `Synth` and Caucasus, peak commit at the row caps.

## Then

Max 5 entries, in dependency order. Task ids from `plan.md`.

1. **C1** — the models a name and a box cannot judge, from screenshots.
   *Live: an agent drives the capture with the user at DCS.*

MS5 ends with Q19. C1 shares no file with it and runs whenever the user is
at DCS; it closes **MS3 Assistant and release**.

## Carries

Things a later task must not lose. Max 10 — `CLAUDE.md` has the rules.

| # | Carry | Discharged by |
|---|---|---|
