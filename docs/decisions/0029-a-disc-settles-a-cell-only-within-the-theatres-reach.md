# ADR 0029: A disc settles a cell only within the theatre's reach

## Status

Accepted

## Context

**Affects:** ADR 0027's Decision, the paragraph on the disc a snap clears;
`extractor-hook.md` "Lifecycle" step 2 and "Hook-pass sweeps", the roads
sweep's seeds; `plan.md` X6, X7.

ADR 0027 says of the pre-sweep:

> Every snap made is kept as the disc it clears, since the nearest road to a
> point d from its snap is at least d − r from any point r away, so a cell
> within d − 25 km of an earlier query is neither asked nor sampled.

The second Marianas run showed that sentence losing an island. The theatre's
road snap answers nothing beyond a reach of its own: on Marianas,
**DCS 2.9.29.27468**, it answered a road 242 km from Saipan's roads and
nothing at 266 km, in two directions; on Caucasus it answered 513 km away,
on Afghanistan past 800 km. ADR 0027 leaves a cell with no answer to its
breakpoints, so Pagan, 325 km from the nearest road and rough, is authored
when asked. A sea cell 240 km out answered a far road whose disc covered
Pagan, and Pagan was never asked.

Asking every rough cell inside a disc mends that and costs Afghanistan an
hour: every cleared cell outside its hull is rough mountain, and a snap far
from roads costs tens of milliseconds.

## Decision

A disc says how near a road can be, not whether the theatre would say so. A
snap that answered at distance d proves the theatre's reach is at least d. So
a cell inside a disc is settled with nothing read only when its nearest road
is provably within the farthest distance answered so far, which is its disc's
distance plus its offset from the disc's center; otherwise it reads its line
and, if rough, asks. No disc is made from an answer of nothing.

This takes the reach as a distance around the query point, which is how it
measured on Marianas in two directions. A theatre whose silence has another
shape would have to be measured, and this record revisited.

The same bound serves the roads sweep's seeds, which snap from a 1 km
lattice: a seed inside a disc from an earlier answer has no road within the
disc's radius, and where that radius exceeds the merge distance the seed can
be placed at its nearest known road without a snap of its own.

### Alternatives considered

**Ask every rough cleared cell.** Correct, and an hour on Afghanistan.

**Never ask a cleared cell.** ADR 0027 as written; loses Pagan.

**A fixed reach.** 250 km fits Marianas and not Caucasus; nothing measured
says what sets it.

## Consequences

Caucasus's pre-sweep takes 60 s rather than 19, since its cleared cells are
rough and near enough to the reach's edge to be asked; Afghanistan's takes
78 s rather than an hour; Marianas keeps Pagan.

ADR 0027's sentence quoted above is replaced by this record; its rule for a
cell is unchanged.

**Provisional:** the reach is measured on one build and three theatres, and
the assumption that it is a radius around the query point is measured on one.
