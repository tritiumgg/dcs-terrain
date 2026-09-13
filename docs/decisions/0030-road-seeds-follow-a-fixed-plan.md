# ADR 0030: Road seeds follow a fixed plan, and the file is its own journal

## Status

Accepted

## Context

**Affects:** `extractor-hook.md` "Hook-pass sweeps", the `roads, railroads`
entry, and "Lifecycle", the resumable-iterator paragraph; `extract-format.md`
"Tables", the `roads.jsonl` entry, and "manifest.json", the `timing_ms`
example; `plan.md` X7.

The frozen hook says what the roads sweep does and leaves how it numbers,
merges, pairs and resumes to the implementation:

> for each seed call `getClosestPointOnRoads(kind, x, z)` and write the
> `seed` line. Then for each seed find its `road_seed_neighbours` nearest
> seeds by snap point (a simple grid bucket lookup), and for each unordered
> pair not yet requested call `findPathOnRoads(kind, x1, z1, x2, z2)` with
> the snap points and write the `path` or `nopath` line. [...] Seeds whose
> snap points lie within 100 m of another seed's are merged before pairing

and the only resume the documents describe is the tile journal:

> Every sweep is a resumable iterator over tiles. [...] one line appended to
> `tiles.jsonl` [...] On resume, tiles in the journal are skipped.

The roads sweep writes no tiles. Caucasus is about 370 000 seeds and 700 000
route calls, the file they make is hundreds of megabytes, hook-state file
handles have no `seek`, and `write_file` holds a whole file as one string;
the sweep takes tens of minutes and a Stop or a kill in the middle of it has
to cost minutes, not the sweep. The format's `seed` line carries an `id` and
nothing says where an id comes from; the merge has no field, so the file
cannot say which seed absorbed which; and `timing_ms` shows one key,
`roads`, where the sweep is four jobs.

## Decision

Seed ids are positions in a fixed plan: the lattice at `road_seed_spacing`
anchored on the grid origin, row-major, then the airdromes in ascending id,
then the towns in name order, which is the order the tables sweep already
writes them in. A position whose seed cannot be placed, off the grid, with no
position, or in a tile the tile sweep left out, keeps its number and gets no
line. A resumed run therefore gives every seed the id it had by computing
it. The plan's shape is not pinned in the manifest: the grid is already
there, and the airdrome and town tables change only with the terrain data,
which ships in a DCS build, and a different build already refuses the resume
(ADR 0023).

Every placed seed that is asked gets its line, merged or not, so the file
holds every answer the router gave. After each snap the seed merges into the
lowest kept id whose snap lies within 100 m of its own; a merged seed gets no
paths and is never a merge target, so merging does not chain. Neighbours are
the four nearest kept seeds by snap point, ties broken by lower id, within
50 km; a seed with fewer takes the ones it has. A pair is requested from the
seed of lower index unless that seed does not list the higher, in which case
the higher requests it, which yields every unordered pair exactly once with
no record of pairs seen; the line is written with `from` the lower id and
`to` the higher.

The file is its own journal. On every Start the seeds sweep reads it back in
counted chunks, one a step: each seed line is replayed through the merge, so
the kept set is the earlier run's exactly, since `%.17g` round-trips a double;
the pair lines are counted, and the last is kept. Seeds are then asked from
the position after the last id read, and the paths sweep advances its cursor
past the counted pairs and refuses when the last counted pair is not the line
the file holds. A file whose ids do not climb, exceed the plan, or hold a
line of another kind refuses with the file's name and what to do. A last line
with no newline is dropped by copying the head into a fresh file, a chunk a
step, and swapping it in with the original renamed aside first, so no moment
has no file; a swap cut short is put back on the next Start. Lines reach the
file in batches of 64, so a Stop loses at most a batch, which the resume asks
again. Each of the four jobs has its own `timing_ms` key, `roads:seeds`,
`roads:paths`, `railroads:seeds` and `railroads:paths`.

### Alternatives considered

**A journal line per seed in `tiles.jsonl`.** The journal's entries are tile
entries with a layer, and a seed is neither. Rejected.

**A set of pairs seen.** What the spec's "not yet requested" reads as. A set
over 700 000 pairs is memory the hook state does not need to hold, and the
neighbour lists already say which side asks. Rejected.

**Merged seeds written with a `merged_into` field, or not written.** A new
field changes the frozen format, and an absent line loses an answer the
router gave. `pack` needs neither: it unions routes and counts seeds.
Rejected.

**Pin the plan's shape in the manifest.** Four numbers and a comparison,
suggested in review. Rejected as redundant with the build check, which is
the only way the tables can change under a resume, and with the read-back's
own check on ids.

**Repair a cut-short tail by rewriting the file, or by appending a newline
and having readers skip a line that does not parse.** The first holds
hundreds of megabytes in one string. The second is two lines of code and
puts a rule on every reader of the format; the user chose the copy.

## Consequences

A Stop and a Start into the same directory cost a read-back of the file and
at most a batch of calls made again; a kill costs the same plus one copy of
the file. The read-back is paid on every Start once paths have begun, and it
is tens of seconds on Caucasus, since the hook pass is re-entered on a
resume and the paths sweep needs the kept seeds in memory.

A reader of `timing_ms` finds four road keys where the spec example shows
`roads`. `pack` reads `from` and `to` in either order and is unaffected.

The neighbour cap of 50 km makes a lone road component with fewer than four
kept seeds within it route to nothing beyond, which the spec's "nearest
seeds" would have routed at any distance and cost.

The cost of the four sweeps is not measured here. The spec's 25 minutes on
Caucasus is roads only and counts no Lua; the live crop run at X10 records
call time against sweep time per seed and per route, and the whole-map run at
X11 records the total, and this record is revisited if either contradicts
the design's targets.
