# State

Where the work is. Updated at the end of every working session, before handing
back. This file holds progress; `plan.md` holds the task graph and
`decisions/` holds what has diverged from the frozen documents.

**Last updated:** 2026-09-25T18:19Z

## Done

Newest first. Max 5 entries — drop the oldest when adding a sixth.

1. **X7: roads and railroads.** Four hook jobs, `<kind>:seeds` then
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
2. **X6: the pre-sweep, `water` and `height`.** Prepare is `identity`,
   `presweep`, `grid`: the pre-sweep walks a 5 km lattice one cell a step, a
   road snap first and a 2 km line of heights only where a road lies 5 to
   25 km off (ADR **0027**, after the spec's rule authored Caucasus's roadless
   mountains), every far snap kept as the disc it clears; it skips itself on
   a crop run and on a directory whose manifest carries the rectangle. The
   hook pass is `config`, `tables`, then `water+height` as one sweep with both
   calls at each cell (ADR **0028**: measured half the cost of two passes),
   one tile a step; it builds `run.skip` and a resumed sweep reads journalled
   tiles back for the sea ones. *Verified by:* an agent offline, then the
   maintainer on Caucasus 2.9.29.27468: a 5 km Kutaisi crop's cells equal the
   bridge's, the whole map's rectangle is 470 x 785 km in 19 s (38 km past
   the hull on the north where roads run, 14 km and 56 km short of it over
   roadless mountains and sea), and Stop then Start resumed without measuring
   again; Marianas found Pagan lost to a far snap's disc, fixed. The fused
   sweep on the call floor: Caucasus 100 s for 2 294 tiles against 189 s as
   two passes, Marianas 40 s against 47 s, same bytes. PR **52**.
3. **X5: the grid job, `config.json` and the seven tables.** Prepare is
   `identity` then `grid`; the hook pass is `config` (the theatre's facts,
   twenty lat/lon samples, the fill triple off three corners 500 km out) then
   `tables`, one file a step with `progress()`. No rectangle is read from a
   theatre file: ADR **0026**. Every shaper is terrain agnostic and never
   raises. *Verified by:* an agent offline, then live on Caucasus: all eight
   files re-read through the bridge matched byte for byte.
4. **X12: the run says where it is, and claims no more.** A job's `start`
   may return `progress()` beside its step; the run keeps a record refreshed
   every second that the window's line reads, logs a `heartbeat` line every
   ten seconds and one `totals` line at done. ADRs **0024**, **0025**.
5. **X2b: the run knows what it is extracting, and the build is the terrain's
   version.** `dcs_build` off `autoupdate.cfg`, `terrain_dir` by the
   shallowest id in an `entry.lua`, the three data files by path, size and
   payload field. ADRs **0022**, **0023**. Verified live on Caucasus and Sinai.
## Next

One task. The thing to pick up immediately.

**X8a** — the mission-pass transport and the `surface` sweep: built,
reviewed and offline-verified on `task/X8a-mission-transport`, PR **54**. The chunk
answers one printable character a cell under a length frame. What remains is
the live half: on the Kutaisi 5 km crop, a surface tile's bytes equal
`land.getSurfaceType` re-read through the bridge, the band's time against
the spec's 40 ms, and a Stop and Start mid-sweep. *Verified by:* the bridge
steps from a session with the maintainer's DCS.

## Then

Max 5 entries, in dependency order. Task ids from `plan.md`.

1. **X8b / X8c / X9** — scenery, the model catalogue and failure handling.
   X ends here until `check-extract` exists.
2. **C1 / C2 / C3** — the Rust interlude X10 needs: scaffold, `synth` on the F2
   constants, `check-extract`. Closes P2 and MS0 past.
3. **X10** — the 10 x 10 km Kutaisi crop that `check-extract` accepts; its
   authored pair is null (ADR 0009, ADR 0026).
4. **C4** — `pack` reads the extract: the first consumer of the X10 crop, and
   where carry 1 is discharged.
5. **C5a** — `pack` skeleton: schema, `meta` with the authored rectangle
   (ADR 0008), one transaction.

Milestone in view: **MS0 Contract** (P1, P2, F1, F2, C1, C2, C3, X1, X3).
Proof is `check-extract` accepting the Rust synthetic extract and the hook's
encoders and grid computation reproducing the F2 constants byte for byte.

First real data: **X10**, a 10 x 10 km Kutaisi extract that `check-extract`
accepts, which is where X ends for now. X11 still waits for C4 onward to read
that crop, so a field missing from the frozen format costs a crop re-run and
not a theatre. Everything from C4 on is built on macOS against the crop.

## Carries

Things a later task must not lose. Max 10 — `CLAUDE.md` has the rules.

| # | Carry | Discharged by |
|---|---|---|
| 1 | `synth` produces no all-fill tile at any size: the fill margin is 2 km and a tile is 12.8 km, so no tile is fill throughout. Test the absent-fill-tile read against a hand-built manifest, not against a generated extract. The all-sea case is generated, as one interior tile. | C4 |
| 2 | Measured authored rectangles: Afghanistan 855 x 1105 km, 945 000 km², its roads reaching the bounds rectangle's west edge; Cold War Germany 810 x 750 km, 607 500 km², roads within 5 km of 11 036 of its 11 063 authored cells. The size rule packs both at 100 m, where the design's 176 M-cell estimate for Afghanistan assumed 440 000 km². Whether the 500 000 km² threshold stands is C5b's to decide, as an ADR if it moves. | C5b |
| 3 | The router's first route point is 0.5 to 2 m from the seed's snap on both networks (Caucasus, six routes checked), so the design's 1 m vertex snap in the graph build would not join a route to its seed's road point; snap seeds and route ends with a few metres of tolerance. | C9 |
