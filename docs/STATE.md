# State

Where the work is. Updated at the end of every working session, before handing
back. This file holds progress; `plan.md` holds the task graph and
`design.md` and `queries.md` hold the design.

**Last updated:** 2026-10-29T22:54Z

## Done

Newest first. Max 5 entries — drop the oldest when adding a sixth.

1. **N12: airdrome ids by position (ADR 0005).** Where no label names one,
   the beacons nearest an airdrome, then its tower through the scene index,
   give its id when every station of the id agrees; `AirdromeBand` and
   `dist_airdrome_band` take `airdrome`, an id or a `name_id`. *Verified
   by:* an agent, through `mise run check`, and opt-in on 2.9.30.28738:
   Nevada 9 of 17, PersianGulf 28 of 30, every one the shipped missions
   name equal to theirs; held out, 262 of 263 labels (`data/airdrome-ids.md`).
2. **Q4: `score_site`, `find_sites`.** A weighted overlay on the lattice:
   hard tests first, each position at its best orientation by the
   weighted score, ties by x then z; greedy non-maximum suppression over a
   spatial hash. *Verified by:* an agent, through `mise run check` (plane
   accepted, lake and hill flank rejected, equal to a brute-force search),
   and opt-in on the 8 terrains of 2.9.30.28738: a FARP search rescored
   alone, spaced, equal to brute force, 57–205 ms (`data/find-sites.md`).
3. **ADR 0006: the sweep crosses the mesh near the site.** Out to 1 km
   each radial takes its steepest slope over the triangles, every block
   read once for all radials (`Source::alongs`); the raster beyond.
   *Verified by:* an agent, through `mise run check`, and opt-in on the
   8 terrains of 2.9.30.28738: floor p99 error from up to 330 m to 41 m for
   0.1–0.9 Gcycles; the mesh all the way cost up to 1.2 s (`data/radar-floor.md`).
4. **Q6: routing.** A* over any graph of neighbors; `route` snaps each
   end part way along an edge and searches within the detour's ellipse,
   avoided regions cut, exposure costed; alternatives by the penalty
   method; `chokepoints` by betweenness, `not_ready` until `index` builds
   it; `trafficability` by an off-road cost field from slope and cover;
   a loop road in the synthetic terrain. *Verified by:* an agent, through
   `mise run check` (bridge, avoid box, the two alternatives, A* against
   Dijkstra and Bellman–Ford), and opt-in on the 8 terrains of
   2.9.30.28738: A* equal to Dijkstra on all 712 joined pairs
   (`data/route-search.md`).
5. **Q5: `radar_floor`; `coverage`, `approach_spectrum`, `unmask_profile`.**
   The viewshed sweep keeps each cell's floor; bands nest by construction;
   points and routes judged over the triangles, as `visible` judges.
   *Verified by:* an agent, through `mise run check`, and opt-in on
   2.9.30.28738 on all 8 terrains: bands nest, 99.7 % of cell-bands agree
   with `visible`, floor 96–153 ms at 20 km; the raster runs up to p99
   330 m low where the ground falls from the site (`data/radar-floor.md`).
## Next

One task. The thing to pick up immediately.

**Q11** — `airfield_info`.
*Verified by:* an agent, opt-in.

## Then

Max 5 entries, in dependency order. Task ids from `plan.md`.

1. **Q10** — `point_info`.
2. **Q12** — `exposure`.
3. **Q7** — `dcsterrain query` over every operation, `schema`.
4. **Q9** — the FARP and SAM replay on Caucasus.
5. **Q8** — benchmarks on `Synth` and, opt-in, the install.

Milestone in view: **MS1 Engine on the install**.

## Carries

Things a later task must not lose. Max 10 — `CLAUDE.md` has the rules.

| # | Carry | Discharged by |
|---|---|---|
