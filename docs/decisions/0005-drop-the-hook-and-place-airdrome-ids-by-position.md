# ADR 0005: Drop the hook, and give an airdrome its id by its beacons' positions

## Status

Accepted

## Context

**Affects:** `design.md` §3.4 (an airdrome's id), §4.2 (`tables.json`
rebuilt on every update), §4.3 (the hook's tables), §6 (`check`);
`queries.md` §1 (`AirdromeBand`), `nearest`, `airfield_info`,
`dist_airdrome_band`; `plan.md` X1, R2, MS1 and a new N12.

The optional GameGUI hook in `extractor/` exists to write six airdrome
fields only the running sim knows: `code`, `civilian`, `abandoned`,
`class`, the game's `display_name`, and the numeric `id` where `Radio.lua`
labels none. `index --hook` merges them into `tables.json`. Keeping it
costs a Lua 5.1 toolchain, offline tests, a CI workflow, a live run per
terrain and build, and X1's stripping work.

No operation in `queries.md` reads `code`, `civilian`, `abandoned` or
`class`. The display name already comes from the airdrome's first beacon.
The id is how a caller names an airdrome to `AirdromeBand`,
`dist_airdrome_band` and `airfield_info`, and how a mission file places a
unit at one (`airdromeId`).

The ids live in `terrain.cfg.lua.pak.crypt`, which the tool never opens.
They follow no order of the terrain's files: Nevada's run Creech, Groom,
Las Vegas, Nellis, then roughly alphabetical with gaps; PersianGulf's run
alphabetical by English name to 17, then by release.

Measured on 2.9.30.28738. As ground truth, the `airdromeId` of every
take-off point in the 870 missions the install ships for six terrains, all
but Caucasus, each matched to the airdrome nearest it. 126 ids the tool
takes from `Radio.lua` today agree with it; two disagree, 167 and 259 on
GermanyCW, each from a single take-off point, and are left as `Radio.lua`
gives them. `design.md` said about half of GermanyCW's and Syria's
airdromes lacked a `Radio.lua` id; on this build all of GermanyCW's and 78
of Syria's 80 have one. Where the name join fails, the beacon
files still label every airfield beacon `airfieldN_k` and give its
position:

| Terrain | Airdromes | Id today | Id with the beacon position join | Agree with missions | Still none |
|---|---|---|---|---|---|
| Nevada | 17 | 0 | 6 | 6 of 6 | 11 |
| PersianGulf | 30 | 0 | 27 | 25 of 25 | 3 |
| Syria | 80 | 78 | 78 | — | 2 |
| Afghanistan, Caucasus, GermanyCW, MarianaIslands | 170 | 170 | 170 | — | 0 |

The join gives every beacon `N` to the airdrome nearest it within 15 km and
keeps `N` only when all of `N`'s beacons agree. It contradicts no id the
name join gives. Nevada's `Radio.lua` also labels Boulder City, Henderson
and Laughlin as `airfieldN`, each tied to a scene object, a tower, not yet
tested.

## Decision

The tool drops the hook. `index --hook`, the import into `tables.json`,
the `code`, `civilian`, `abandoned` and `class` fields and `extractor/` are
removed. An airdrome's id is the one `Radio.lua`'s comment labels it with,
as before; failing that, the `N` of the `airfieldN_k` beacons nearest it
within 15 km when every beacon of `N` lies nearest it; failing that, the
`N` of a `Radio.lua` entry whose scene object lies within the airdrome's
extent, kept only if it agrees with the missions on every airdrome they
name. An airdrome with none has a null id and is named by its `name_id`.
`AirdromeBand` and `dist_airdrome_band` take an `airdrome` field in place
of `airdrome_id`, and `airfield_info` an airdrome: a number is an id; a
string is a `name_id`, or failing any, an id written in digits. `nearest`
keeps answering an airdrome's `id` as its id in digits, or its `name_id`
where it has none, so its answer can be passed back as is. The shipped
missions become an opt-in test oracle for the ids, never a source.

### Alternatives considered

- **Keep the hook for the ids** (X1 as planned). It fills every id, at the
  cost of a live run per terrain and build and a Lua toolchain kept for six
  fields, four of which nothing reads.
- **Read the ids from the shipped missions.** Each id appears only if some
  mission takes off from that airdrome, which ED does not promise; it reads
  about 2 000 archives that are not terrain files, and it would still miss
  Beatty, Mesquite, Qasoura, Gulechoba and Nicosia.
- **Derive the id from file order.** No order fits, as above.

## Consequences

- About 16 airdromes across the seven terrains have no id: 11 on Nevada,
  3 on PersianGulf and 2 on Syria on 2.9.30.28738, fewer if the tower join
  holds. A caller names them by `name_id`, and cannot give their
  `airdromeId` to a mission. An airdrome's beacons, radio and display name
  follow from its id, so these report no navaids or radio frequencies and
  their `name_id` as their name; most have no beacon at all, which is why
  the position join finds no id for them.
- Requests that named an airdrome by `airdrome_id` must say `airdrome`.
- `code`, `civilian`, `abandoned` and `class` are gone: no answer reports
  an airfield's ICAO code or whether it is abandoned.
- The Lua 5.1 toolchain, the hook's tests and their CI workflow go with
  X1; the live hook run leaves MS1.
- `tables.json` no longer depends on a build-bound import, so after an
  update it is re-stamped like the other entries.
- Reopen it if an operation comes to need one of the dropped fields, or if
  a new terrain's beacons leave most of its airdromes without an id.
