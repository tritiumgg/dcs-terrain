# State

Where the work is. Updated at the end of every working session, before handing
back. This file holds progress; `plan.md` holds the task graph and
`design.md` and `queries.md` hold the design.

**Last updated:** 2026-11-11T06:13Z

## Done

Newest first. Max 5 entries — drop the oldest when adding a sixth.

1. **`slope_band`, and the `dist_sea` and `depth` layers.** A plane fitted
   over the footprint and its ring of cells, in band and within a residual,
   its term giving the downslope bearing; `dist_sea` passes over lakes and
   rivers by the sea bit; `depth` reads the mesh in both modes, null on
   land. *Verified by:* an agent, through `mise run check` on `Synth`;
   bearings within 6° of `aspect`, the sea at most 100 m, rivers 0, on
   Caucasus of 2.9.31.29434.
2. **Forest in `validate_placements`, and trees and lines in `scenery_in`.**
   A placement on land in forest is the `forest` issue, by the forest bit
   so forest under town counts, unless `allow_forest`; `scenery_in` lists
   `pools` with each row's `pool` and counts without a cap by
   `counts_only`. *Verified by:* an agent, through `mise run check` on
   `Synth`; the Bakhmaro pad raises `forest` and the whole of Caucasus's
   13 M trees count in 12.5 MiB, on 2.9.31.29434.
3. **`query --input`, `--output`, and a batch's progress.** A batch reads a
   file and answers to one in UTF-8, where Windows PowerShell 5.1's `>`
   writes UTF-16; stderr tells lines answered and failed at most once a
   second. *Verified by:* an agent, through `mise run check` on `Synth`, and
   the guide's commands in PowerShell 5.1 and 7 on Caucasus of 2.9.31.29434.
4. **The skill says how to read an answer.** Each `sample` layer has its
   unit and reach; `tree_density` is single trees per hectare, not forest;
   which layers the mesh answers at points; what `validate_placements` and
   `scenery_in` test; a scenery box's `obb_w` lies along `rotation_deg`.
   *Verified by:* an agent, every claim read against the code, `mise run
   check` and `mise run docs`.
5. **Q19: each answer encoded as it is written.** `serde_json` keeps keys in
   the order a response declares them; every answer, `schema` and the MCP
   tools parse equal to before. Then each answer is written from its typed
   response, and each GeoJSON feature encoded to text when drawn: bytes
   unchanged. At the row caps `sample` of 57 layers peaks at 73 and 98 MiB,
   `point_info` 26 and 39, `scenery_in` 14 and 18 (ADR 0012: 133, 293, 76,
   187, 18, 47). *Verified by:* an agent, through `mise run check` and the
   answers compared on `Synth` and Caucasus of 2.9.31.29434.

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
