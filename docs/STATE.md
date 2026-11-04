# State

Where the work is. Updated at the end of every working session, before handing
back. This file holds progress; `plan.md` holds the task graph and
`design.md` and `queries.md` hold the design.

**Last updated:** 2026-11-04T00:23Z

## Done

Newest first. Max 5 entries — drop the oldest when adding a sixth.

1. **H3: tables for the main operations.** `find-sites`, `route`,
   `route-alternatives`, `nearest`, `point-info`, `airfield-info`, `towns`
   and `visible` print tables through one printer: units in each header,
   MGRS beside every x and z, at most 100 columns, what is left out named,
   color only on a terminal without `NO_COLOR`. *Verified by:* an agent,
   through `mise run check` on `Synth`: 15 snapshots within 100 columns
   with no escape when piped, each MGRS equal to `geo`'s for its point.
2. **H2: one subcommand per operation.** Its flags come from the request
   schema when it runs and its `--help` from the schema's descriptions,
   now required of every field a request reaches; `<place>@<m>` heights,
   objects in order or as `key=value`, a shape's name first. `--json`
   prints `query`'s answer, the default a plain list, `--geojson-out`
   the GeoJSON, `--show-request` the request. *Verified by:* an agent,
   through `mise run check` on `Synth`: for each of the 22 requests the
   CLI's tests send, the flags make the same request, `--json` matches
   `query` byte for byte but for the time taken, the plain list names
   every field and the GeoJSON file equals `query`'s.
3. **H1: one parser for every place.** A place flag takes DCS `x,z`, MGRS
   at 2 to 10 digits (to the center of its square, or of the part its zone
   and band hold), latitude and longitude with N/S/E/W, or
   `airport:<name>` and `town:<name>`; a bare name, two small numbers and a
   name two towns share are refused with what to type. *Verified by:* an
   agent, through `mise run check` on `Synth`, and opt-in on Caucasus of
   2.9.31.29434: 21 airports by each name and 67 towns round-trip every form
   against `geo`; `town:Mayskiy` is refused with both towns.
4. **H4: GeoJSON on `sample`, `nearest` and `scenery_in`.** A point per
   sample row; a point per `nearest` result and each road or rail result's
   edge; each object's turned box, a point where it has none, and the
   region or footprint. A shape with no area once rounded is not drawn.
   *Verified by:* an agent, through `mise run check` on `Synth`. At the caps,
   `scenery_in` holds 46.9 MiB on Caucasus of 2.9.31.29434; `sample`
   breaks the 64 MiB limit (64.0 to 293 MiB), as it and `point_info` already
   did without GeoJSON.
5. **M3: setup and the skill through MCP.** One server answers every
   terrain: each tool takes `terrain`, `--terrain` only the default, one
   terrain open at a time, and `list_terrains` lists them without `files`.
   The skill calls the tools when present, a `score_site` over the ring for
   the FARP exposure; the guide sets up Claude Code and Claude Desktop.
   *Verified by:* an agent: on Caucasus of 2.9.31.29434 every FARP call
   through `mcp` equals `query`, each ring share its 36 `visible` calls;
   100 calls alternating Caucasus and Syria peak at 58.4 MiB. The user's
   run, an assistant with no shell, is pending.

## Next

One task. The thing to pick up immediately.

**C1** — the models a name and a box cannot judge, from screenshots.
*Verified by:* live: an agent drives the capture with the user at DCS.

## Then

Max 5 entries, in dependency order. Task ids from `plan.md`.

1. **H5** — the README and the guide for people. *User: no DCS.*
2. **Q19** — encode each answer as it is written, to cut its memory. *Agent.*

M and H run in parallel beside C1, which is live (ADR 0011). Milestone in
view: **MS3 Assistant and release**, which C1 closes; then **MS4 Interfaces**.

## Carries

Things a later task must not lose. Max 10 — `CLAUDE.md` has the rules.

| # | Carry | Discharged by |
|---|---|---|
