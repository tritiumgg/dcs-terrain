# ADR 0049: An airdrome is keyed by its `name_id`, and runways and stands carry it

## Status

Accepted

## Context

**Affects:** `core.md` "Packed file" (the `airdrome`, `runway` and `stand`
tables); ADR 0037's consequence on `dist_airdrome_band`; ADR 0039
"airdromes.json", "stands.json" and Consequences; `plan.md` C5c.

The core spec keys the packed airdrome by DCS's own number, and points runways
and stands at it:

> CREATE TABLE airdrome(id INT PRIMARY KEY, name_id TEXT, code TEXT, name TEXT,

> CREATE TABLE runway(airdrome INT, name TEXT, x1 REAL, z1 REAL, x2 REAL,

Since ADR 0039 the file decoders write the extract, and `id` comes from
`Radio.lua`'s `airfield<ID>_0` label, else from the hook, else null. Without
the hook about half of Nevada's airdromes have a null `id`. Every airdrome has
`name_id`, the airfield file's basename, and `runways.json` and `stands.json`
carry both `airdrome_id` and `airdrome_name_id`. ADR 0039 leaves the packed
key open:

> How the packed `airdrome`, `runway` and `stand` tables key an airdrome whose
> `id` is null is C5c's decision, recorded as an ADR when it is built.

## Decision

`pack` creates `airdrome` with `id INT UNIQUE, name_id TEXT PRIMARY KEY NOT
NULL` and the spec's other columns unchanged. `id` is DCS's number where the
extract gives one and NULL where it does not; UNIQUE refuses a repeated number
and allows any count of NULLs. `runway` and `stand` each gain
`airdrome_name_id TEXT` after `airdrome`, filled from the extract's
`airdrome_name_id`; `airdrome` keeps DCS's number, NULL when unknown. A runway
or stand joins its airdrome on `name_id` always, and on `id` when the number
is known.

### Alternatives considered

**Keep `id` as the primary key and let it be NULL.** SQLite accepts a NULL in
a non-rowid primary key. The runways and stands of those airdromes would then
point at NULL and join to nothing, and half of Nevada's runways would be lost
to every query.

**Store `name_id` in `runway.airdrome` and `stand.airdrome`.** One column
fewer, but the column's meaning changes from the spec's, and a caller holding
DCS's number needs a join to use it.

**Invent a number for each airdrome that lacks one**, negative or above the
largest real one. The schema stays the spec's exactly. An invented number
looks like a real one, and a mission script that passes it to DCS gets the
wrong airbase or none.

**Leave out airdromes with no `id`.** Simplest, and it drops half of Nevada.

## Consequences

Every airdrome the extract lists is in the packed file, with its runways and
stands joined to it, whether or not the hook ran.

The packed schema differs from `core.md` in three places: `airdrome`'s primary
key and the extra column in `runway` and `stand`. A reader written against the
spec's text joins on `airdrome = id` and silently misses the airdromes with no
number.

A caller that names an airdrome by number cannot name one whose number is
unknown. ADR 0037 already says so of `dist_airdrome_band`; Q1 and Q3, which
build `nearest` on airdromes and the airdrome criteria, must accept `name_id`
as well.
