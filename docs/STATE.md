# State

Where the work is. Updated at the end of every working session, before handing
back. This file holds progress; `plan.md` holds the task graph and
`design.md` and `queries.md` hold the design.

**Last updated:** 2026-11-06T05:06Z

## Done

Newest first. Max 5 entries — drop the oldest when adding a sixth.

1. **Q19: each answer encoded as it is written.** `serde_json` keeps keys in
   the order a response declares them; every answer, `schema` and the MCP
   tools parse equal to before. Then each answer is written from its typed
   response, and each GeoJSON feature encoded to text when drawn: bytes
   unchanged. At the row caps `sample` of 57 layers peaks at 73 and 98 MiB,
   `point_info` 26 and 39, `scenery_in` 14 and 18 (ADR 0012: 133, 293, 76,
   187, 18, 47). *Verified by:* an agent, through `mise run check` and the
   answers compared on `Synth` and Caucasus of 2.9.31.29434.
2. **Q21: `route` off the road.** `offroad` plans each end's way across
   country over Q20's search, every road cell reached a start or goal of the
   one road search; a way off the road the whole way competes; people's page
   lists every turn. `max_offroad` is 5 km, default and limit, with the ground
   searched in few tiles. *Verified by:* an agent, through `mise run check` on
   `Synth`; Kobuleti to Batumi answers, V1's samples unchanged, GermanyCW
   peaks at 61.4 MiB, on 2.9.31.29434.
3. **Q20: the off-road search, and `trafficability` with it.** The least-cost
   way over the 50 m cells to the first road reached, 16 neighbors, straightened;
   `trafficability` answers `way` and `straight` within `max_distance`, 20 km,
   and prints a table. *Verified by:* an agent, through `mise run check` on
   `Synth` (Bellman–Ford, the hill, the river, two table snapshots); GermanyCW
   peaks at 46.8 MiB searching the whole 20 km, on 2.9.31.29434.
4. **Dropped M4: the skill stays the guidance.** Claude Code keeps only
   4 096 characters of a server's instructions and of each tool
   description, 2 048 before v2.1.296, and the guidance comes to about
   8 500; `plan.md` records why. *Verified by:* the user decided; the
   limits read in Claude Code's MCP documentation and changelog.
5. **Planned MS5: off the road, MCP guidance, smaller answers.** M4, Q20
   and Q21 added to `plan.md`, Q19 amended to turn on `preserve_order`
   first; ADR 0013 plans a route's off-road ends over the off-road cost;
   `queries.md` marks `route` and `trafficability`'s planned fields.
   *Verified by:* the user approved the design; `mise run docs`.

## Next

One task. The thing to pick up immediately.

**C1** — the models a name and a box cannot judge, from screenshots.
*Live: an agent drives the capture with the user at DCS.*

## Then

Max 5 entries, in dependency order. Task ids from `plan.md`.

None: no task in `plan.md` is left after C1, which closes **MS3 Assistant
and release**.

## Carries

Things a later task must not lose. Max 10 — `CLAUDE.md` has the rules.

| # | Carry | Discharged by |
|---|---|---|
