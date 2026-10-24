# ADR 0046: A named node is a stand only when its class has a stand record

## Status

Accepted

## Context

**Affects:** ADR 0039 "stands.json"; `extract-format.md` "stands.json";
`plan.md` E5, C5c.

ADR 0039 takes the stands from the airfield files and their parameters from
`assets.assets`:

> **stands.json**: v1's keys plus `airdrome_name_id`, `stand_class` and
> `heading`, from the airfield `.rn4` and `.rn5` (E5). [...] `params` come
> from the stand class's record in `misc/assets.assets`

It does not say which named nodes are stands. The airfield `.rn4` names its
runway ends and its stands; E5 takes every named node that is not a runway
end as a stand, and its `.rn5` gives each a class. On Caucasus that is
exactly the 900 stands the game lists. Decoded across the eight maps
installed on DCS **2.9.30.28738**, every named node gets a class, and on two
maps some classes are not stands:

| Map | Class | Nodes |
|---|---|---|
| Syria | `taxi_point_holding_position` | 43 |
| MarianaIslands | `rwy_point_takeoff_landing` | 4 |

`assets.assets` marks a stand class's record with the pair `stand = true`,
and neither class has one, so the file gives these nodes no size and no
permissions. There is no live extract of Syria or MarianaIslands to show
whether the game lists them as stands. `docs/data/file-formats.md` records
the figures.

## Decision

The airfield decoder writes a named node that is not a runway end to
`stands.json` only when its `.rn5` class has a stand record in the map's
`assets.assets`. A node whose class has no record is not written. A node
that has no class at all, because its airfield has no `.rn5` or the `.rn5`
does not list it, is written with `stand_class`, `flag` and every `params`
value null; no installed map has one.

### Alternatives considered

**Write every named node, with null parameters where its class has no
record.** It keeps whatever the game might list. It lost because a reader
asking for parking spots would be handed holding points, and every reader of
`stands.json` would have to handle a stand with no size.

**Refuse a map that has such a node.** Nothing wrong is written, but Syria
and MarianaIslands could not be extracted at all.

## Consequences

`stands.json` holds only nodes with a known size and permissions, apart from
the unclassed case no map has today. Syria loses 43 named nodes and
MarianaIslands 4, which is right if the game does not list them as stands
and a loss of rows if it does. E9's live Syria check settles it: a holding
point the game lists as a stand reopens this decision.

The rule leans on `assets.assets` naming every stand class. A stand class a
future build adds to an `.rn5` without a record would drop its stands
silently; the opt-in check on Caucasus, which compares counts with the live
extract, is what would catch it there.
