# State

Where the work is. Updated at the end of every working session, before handing
back. This file holds progress; `plan.md` holds the task graph and
`design.md` and `queries.md` hold the design.

**Last updated:** 2026-10-30T06:00Z

## Done

Newest first. Max 5 entries — drop the oldest when adding a sixth.

1. **Q11: `airfield_info`.** Runways from the tables, true courses by the
   normal section on WGS84, widths the median of nine stations across the
   mesh's runway paint; helipads answer their stands, airdromes with no id
   their `name_id`. *Verified by:* an agent, through `mise run check`, and
   opt-in on the 8 terrains of 2.9.30.28738: all 565 airdromes answer, 363
   runways equal the tables, widths and elevations a scan's; 0.6–5.1 ms warm
   p50 (`data/airfield-info.md`).
2. **Q10: `point_info`.** Ground from the mesh triangle, water depth now
   carried through point mode; the nearest town by a scan of every town,
   with true and grid bearings and "6 km NE of" its name; road, airfield,
   lat/lon and MGRS. *Verified by:* an agent, through `mise run check`
   (each field on `Synth`, the town against brute force, the true bearing
   against Vincenty), and opt-in on the 8 terrains of 2.9.30.28738: all
   24 000 random points agree, 0.4–3.9 ms a point (`data/point-info.md`).
3. **Q12: `exposure`.** A multiple viewshed: each site's radar floor
   sweep, sites out of the region's reach skipped, reduced over the
   region's lattice to the least floor and the count of sites seeing a
   target height. *Verified by:* an agent, through `mise run check`
   (bit for bit against each site's floor, the count against `visible`),
   and opt-in on the 8 terrains of 2.9.30.28738: 0 of 12.5 M cells off
   the sites' floors, count agrees with `visible` at 1 587 of 1 600 cells,
   7 sites 0.24–1.2 s (`data/exposure.md`).
4. **N12: airdrome ids by position (ADR 0005).** Where no label names one,
   the beacons nearest an airdrome, then its tower through the scene index,
   give its id when every station of the id agrees; `AirdromeBand` and
   `dist_airdrome_band` take `airdrome`, an id or a `name_id`. *Verified
   by:* an agent, through `mise run check`, and opt-in on 2.9.30.28738:
   Nevada 9 of 17, PersianGulf 28 of 30, every one the shipped missions
   name equal to theirs; held out, 262 of 263 labels (`data/airdrome-ids.md`).
5. **Q4: `score_site`, `find_sites`.** A weighted overlay on the lattice:
   hard tests first, each position at its best orientation by the
   weighted score, ties by x then z; greedy non-maximum suppression over a
   spatial hash. *Verified by:* an agent, through `mise run check` (plane
   accepted, lake and hill flank rejected, equal to a brute-force search),
   and opt-in on the 8 terrains of 2.9.30.28738: a FARP search rescored
   alone, spaced, equal to brute force, 57–205 ms (`data/find-sites.md`).
## Next

One task. The thing to pick up immediately.

**Q7** — `dcsterrain query` over every operation, `schema`.
*Verified by:* an agent.

## Then

Max 5 entries, in dependency order. Task ids from `plan.md`.

1. **Q9** — the FARP and SAM replay on Caucasus.
2. **Q8** — benchmarks on `Synth` and, opt-in, the install.
3. **Q13** — `validate_placements`.
4. **Q15** — `towns` and the `places` entry.
5. **S1** — the skill an assistant loads.

Milestone in view: **MS1 Engine on the install**.

## Carries

Things a later task must not lose. Max 10 — `CLAUDE.md` has the rules.

| # | Carry | Discharged by |
|---|---|---|
