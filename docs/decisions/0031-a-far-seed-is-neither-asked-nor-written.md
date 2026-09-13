# ADR 0031: A far seed is neither asked nor written

## Status

Accepted

## Context

**Affects:** ADR 0029's Decision, the paragraph on the roads sweep's seeds;
`extractor-hook.md` "Hook-pass sweeps", the `roads, railroads` entry;
`extract-format.md` "Tables", the `roads.jsonl` entry; `plan.md` X7.

The frozen hook asks the router for every seed:

> seeds = lattice at `road_seed_spacing` over the grid, plus airdrome
> reference points and towns; for each seed call
> `getClosestPointOnRoads(kind, x, z)` and write the `seed` line.

A snap costs by how far it has to look: 0 ms beside a road, 14 ms mid-sea
103 km from one, 84 ms in a fill corner 513 km from one, measured on Caucasus
**DCS 2.9.29.27468** (ADR 0027). The seeds go only in the rectangle's tiles
that are not fill or sea, but the rectangle holds roadless ground: Turkey's
mountains on Caucasus at 85 to 139 km from a road, and on Afghanistan, whose
rectangle is 945 000 km², deserts 50 to 100 km from any road. A seed there
snaps to the same road stretch that the seeds beside that road already
found, since every road point has a lattice seed within 707 m of it, so its
answer adds nothing to the routes and its cost is tens of minutes over a
whole map.

ADR 0029 saw this coming and said, of its disc bound:

> The same bound serves the roads sweep's seeds, which snap from a 1 km
> lattice: a seed inside a disc from an earlier answer has no road within
> the disc's radius, and where that radius exceeds the merge distance the
> seed can be placed at its nearest known road without a snap of its own.

A seed placed that way would carry a snap the router never gave for it: a
real road point, but not necessarily its nearest, with a `snap_dist` that
means nothing.

## Decision

A seed provably farther than 25 km from every road is neither asked nor
written. The proof is the sweep's own answers: a snap that answered a road d
away is kept as a disc of radius d − 25 km around its query point, since no
road lies within d − r of any point r from the query, and a seed inside a disc
is passed over with no call and no line; a disc too small to reach the next
lattice seed is not kept. Discs read back from the file count too. A seed
that is asked and answers a road farther than 25 km is written, because the
router said it, and is not kept: it gets no neighbours and no paths. The
distance is the pre-sweep's own line for ground that is not built (ADR
0027). On the lattice the discs are tested per row, as the spans of z each
disc cuts on the row's x, merged and walked by a cursor, so the test costs a
few instructions a seed rather than a scan of the discs.

### Alternatives considered

**Place the seed at its nearest known road, ADR 0029 as written.** Records
an answer the router did not give. Rejected.

**Ask every seed, the spec as written.** Caucasus barely notices;
Afghanistan spends tens of minutes on answers that add nothing. Rejected.

**Write the seed with a null snap.** Null means the router answered nothing,
which it did not. Rejected.

**Test discs per seed, as the pre-sweep does.** The pre-sweep has 66 000
cells and tens of discs; the roads sweep has up to a million seeds and, on a
roadless theatre, thousands of small discs, and the scan would cost more than
the calls it saves. Rejected.

## Consequences

A seed within 25 km of a road is never passed over: a point on a road is at
least d from a query whose nearest road is d away, so it lies outside every
disc. Nothing a route would have traversed is lost.

The pack report's coverage fraction, the share of seeds whose snap lies on
the final graph, is over fewer seeds than the lattice has, and the seeds it
lacks are the ones that would have snapped to a road the graph reaches
anyway. Railroads are sparse, so `railroads.jsonl` holds a seed line for a
fraction of the lattice.

ADR 0029's paragraph on the roads sweep's seeds is replaced by this record;
its rule for the pre-sweep is unchanged, and its reach caveat does not apply
here, because a seed the router would answer nothing for and a seed passed
over both get no paths.

**Provisional** in one number: 25 km is ADR 0027's, measured on one build
and three theatres, and moves with it.
