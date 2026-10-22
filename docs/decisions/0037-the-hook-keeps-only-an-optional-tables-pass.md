# ADR 0037: The hook keeps only an optional tables pass

## Status

Accepted

## Context

**Affects:** `extractor-hook.md` throughout, and its "Lifecycle" and
"airdromes" passages in particular; `extract-format.md` "airdromes.json",
"config.json"; `core.md` the `airdrome` table; `validation.md`; `plan.md`
component X; ADR 0034.

Under ADR 0034 the extract is decoded from the map's files. A search of the
whole install on DCS **2.9.30.28738**, reading only, found an unencrypted
source for every field the database stores except four columns of one table:

```
CREATE TABLE airdrome(id INT PRIMARY KEY, name_id TEXT, code TEXT, name TEXT,
  x REAL, z REAL, lat REAL, lon REAL, civilian INT, abandoned INT, class TEXT);
```

- `code` (ICAO), `civilian`, `abandoned` and `class` exist only in
  `terrain.cfg.lua.pak.crypt`, which is encrypted. The Mission Editor's own
  scripts read them only through `Terrain.GetTerrainConfig("Airdromes")`. A
  module's radio-preset comments give 18 of 21 Caucasus ICAO codes, with one
  wrong.
- `id` is readable from the map's `Radio.lua`, one block per airdrome, and
  `name_id` from the block's comment. That covers every Caucasus airdrome,
  21 of 21, but only 9 of Nevada's 17 airfields.

The hook's tables pass already reads all of these with
`GetTerrainConfig("Airdromes")` at the menu in under a minute, with no sweep.
The `id` is what a campaign script names an airfield by, so an airfield
without one cannot be matched to the game.

## Decision

The Lua hook is reduced to its tables pass, and running it is optional.
`dcsterrain extract` writes a complete extract without it. Each airdrome's
`code`, `civilian`, `abandoned` and `class` are null, and its `id` comes from
`Radio.lua` where the map lists one. When the user has also run the hook on
the same map and build, `extract` takes its `airdromes.json` and fills those
columns and the missing ids from it, joining on `name_id`. An airdrome no
source gives an id is keyed by its `name_id`, and its `id` stays null. The
hook's pre-sweep, water and height sweep, road sweeps and scenery sweep are
deleted.

### Alternatives considered

**Drop the hook entirely.** One language and one toolchain fewer, and no Lua
5.1 build on Windows. It lost because the airdrome id is the join key a
campaign uses, and on Nevada half the airfields would have none.

**Keep the hook as a live validator as well.** A bridge-driven comparison of a
decoded extract against the running sim, for each new build. It is not ruled
out, but it is a separate task with its own done test, and the tables pass
does not depend on it.

## Consequences

Extraction no longer needs a running DCS. Running the hook is how a user gets
ICAO codes and complete airdrome ids, and it takes seconds at the menu.

The Lua toolchain, the offline Lua tests and the hook's control window stay
in the project, sized down to one pass. ADRs 0010 to 0025 govern the hook's
lifecycle, window and logging, and keep governing what is left. Task X5
survives in reduced form. X12 to X14 are reviewed for what still applies to a
run of one pass.

The `airdrome` table has nullable columns that were never null before, and
every consumer of `code` must handle null. An airdrome keyed by `name_id`
with a null `id` cannot be named to `dist_airdrome_band` by id.

The join on `name_id` assumes the hook and the decoder read the same map at
the same build. `extract` checks both stamps and refuses a hook file from
another build.
