# State

Where the work is. Updated at the end of every working session, before handing
back. This file holds progress; `plan.md` holds the task graph and
`design.md` and `queries.md` hold the design.

**Last updated:** 2026-11-04T20:09Z

## Done

Newest first. Max 5 entries — drop the oldest when adding a sixth.

1. **Planned MS5: off the road, MCP guidance, smaller answers.** M4, Q20
   and Q21 added to `plan.md`, Q19 amended to turn on `preserve_order`
   first; ADR 0013 plans a route's off-road ends over the off-road cost;
   `queries.md` marks `route` and `trafficability`'s planned fields.
   *Verified by:* the user approved the design; `mise run docs`.
2. **H5 fix: `geo` for a person with an MGRS.** `--to-xz` takes an MGRS,
   read to its square's center, and every latitude and longitude form a
   place takes; `geo` prints one row per point with x, z, MGRS, latitude
   and longitude; the guide's "Convert a position" shows it.
   *Verified by:* an agent, through `mise run check` on `Synth` (three `geo`
   snapshots); the new guide commands run in PowerShell 5.1 and 7 on
   Caucasus of 2.9.31.29434, output pasted as printed. The user's rerun of
   H5 is pending.
3. **H5: the README and the guide for people.** The guide walks the FARP
   question through the commands for people, a hidden site found with
   `not-visible-from`, and keeps `query` and MCP as the paths for software
   and assistants; the download is release 0.2.0, the crate's version.
   *Verified by:* an agent: every guide command run in Windows PowerShell
   5.1 and 7 on Caucasus of 2.9.31.29434, output pasted as printed; a
   stand-in newcomer answered an unseen question. The user's run is pending.
4. **H3: tables for the main operations.** `find-sites`, `route`,
   `route-alternatives`, `nearest`, `point-info`, `airfield-info`, `towns`
   and `visible` print tables through one printer: units in each header,
   MGRS beside every x and z, at most 100 columns, what is left out named,
   color only on a terminal without `NO_COLOR`. *Verified by:* an agent,
   through `mise run check` on `Synth`: 15 snapshots within 100 columns
   with no escape when piped, each MGRS equal to `geo`'s for its point.
5. **H2: one subcommand per operation.** Its flags come from the request
   schema when it runs and its `--help` from the schema's descriptions,
   now required of every field a request reaches; `<place>@<m>` heights,
   objects in order or as `key=value`, a shape's name first. `--json`
   prints `query`'s answer, the default a plain list, `--geojson-out`
   the GeoJSON, `--show-request` the request. *Verified by:* an agent,
   through `mise run check` on `Synth`: for each of the 22 requests the
   CLI's tests send, the flags make the same request, `--json` matches
   `query` byte for byte but for the time taken, the plain list names
   every field and the GeoJSON file equals `query`'s.

## Next

One task. The thing to pick up immediately.

**M4** — the server's instructions and tool descriptions cut from the skill.
*Verified by:* an agent, through an MCP client test; then the user, in Claude
Desktop with no skill.

## Then

Max 5 entries, in dependency order. Task ids from `plan.md`.

1. **Q20** — the off-road search, and `trafficability` with it. *Agent, opt-in.*
2. **Q21** — `route` off the road. *Agent, opt-in.*
3. **Q19** — encode each answer as it is written, to cut its memory. *Agent.*
4. **C1** — the models a name and a box cannot judge, from screenshots.
   *Live: an agent drives the capture with the user at DCS.*

MS5 runs in series. C1 shares no file with it and runs whenever the user is
at DCS; it closes **MS3 Assistant and release**.

## Carries

Things a later task must not lose. Max 10 — `CLAUDE.md` has the rules.

| # | Carry | Discharged by |
|---|---|---|
