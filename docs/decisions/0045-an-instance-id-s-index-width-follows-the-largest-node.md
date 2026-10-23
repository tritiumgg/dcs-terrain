# ADR 0045: An instance id's index width follows the largest node, not the version

## Status

Accepted

## Context

**Affects:** ADR 0035 Context (the version 8 difference "shifts an instance
id by 15 bits instead of 13"); ADR 0039 "scenery.jsonl" (`id`,
`node << S | index`); `plan.md` E4.

ADR 0035 lists the instance id's shift among what distinguishes the two
scene versions:

> Nevada's version 8 scene file [...] shifts an instance id by 15 bits
> instead of 13.

and `docs/data/file-formats.md` gave **S = 13 for version 9 and 15 for
version 8**. E4 decoded the scene file of all eight maps installed on DCS
**2.9.30.28738** and checked each map's group ids against its
`NotInstances.lua`.

PersianGulf is version 9 and splits its ids at bit 15. Four of its 104 793
nodes hold more than 8 191 instances, the largest 14 872. No id in its three
pools sets bit 13 or 14, and its `NotInstances.lua` lists ids from
3 433 857 024, which is 104 793 << 15. Read at bit 13, its ids name nodes
that do not exist.

| Map | Version | Largest node | Shift |
|---|---|---|---|
| Nevada | 8 | 8 808 | 15 |
| PersianGulf | 9 | 14 872 | 15 |
| Afghanistan, Caucasus, GermanyColdWar, MarianaIslands, MarianasWWII, Syria | 9 | 7 745 to 8 189 | 13 |

The shift is the narrowest of 13 and 15 that numbers every node's
instances, on every map. The version does not decide it.

## Decision

The scene decoder splits an id at bit 13 when every node holds at most
8 192 instances, and at bit 15 when the largest holds at most 32 768; a file
with a larger node is refused, naming the node. A group record's id is
`(nodes << S) + i` at the same S. Before writing anything, the decoder reads
up to 1 000 ids spread through each pool and refuses the file unless at
least 99 % of them, split at S, put their instance within 0.5 m of the
pool's own box for that id in x and z.

### Alternatives considered

**By version, as ADR 0035 states.** Wrong on PersianGulf, where it names
nodes past the end of the file.

**Probe shifts 12 to 17 per pool by box fit.** The reference decode chose S
this way, and it reads every map correctly. It lost because it guesses from
the data where a rule over the file's own node sizes explains every map, and
ADR 0035 prefers what the file states over a probe. The box fit stays, as
the check on the rule.

## Consequences

Every installed map decodes, PersianGulf included, and the version no
longer carries the shift. ADR 0035's sentence on the version 8 shift reads
false as a version difference: the two versions still differ in attribute
count, model index and heading.

The rule rests on two maps that use 15. A map whose nodes fit 13 bits but
whose ids use 15 would be read at 13, and is refused rather than written
with wrong ids: its ids name nodes past the last, or the box check fails.
A node of more than 32 768 instances stops the run; supporting one is a new
record.

The live id is still unchecked against a running sim's `getName()`, as ADR
0039 notes; E9 confirms it.
