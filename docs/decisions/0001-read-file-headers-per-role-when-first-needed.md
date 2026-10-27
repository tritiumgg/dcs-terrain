# ADR 0001: Read a terrain's file headers per role, when the role is first needed

## Status

Accepted

## Context

**Affects:** `design.md` §3.2 (every file's header read at open), §4.1 and
§4.2 (how a cache key is checked) and §8 (the open target); `plan.md` N5,
and the new task N2b before it.

On 2.9.30.28738, `install::open` takes 42 ms on GermanyCW and 41 ms on Syria,
which hold 454 and 450 airfield files, against the 20 ms target for open and
`describe`; Caucasus takes 5.8 ms. `TerrainCache::key` then reopens and
rereads the same headers on every `get`, about 40 ms more once `tables.json`
keys on the airfield roles. No height, tile or sightline query reads an
airfield file. Listing `AirfieldsTaxiways` with size and mtime takes 0.2 ms.
The research's open figures (2.1, 7.2 and 14.2 ms) read only the mesh table
of contents.

## Decision

Opening a terrain reads the headers of the surface file only. Each other role
is classified the first time an entry or operation needs it: the airfield,
assets and Lua files when `tables.json` is keyed or built, the scene file for
scenery, the road file for the graph. A cache key is checked from metadata: a
stat for a single-file role, one directory listing with size and mtime for a
directory role, and the build. The class, version and payload size are read
when the entry is built and recorded in its key. An unknown (class, version)
pair still refuses everything that reads its role; it no longer refuses the
roles that do not.

### Alternatives considered

- Keep reading every header at open: simple, and it refuses early, but it
  misses the 20 ms target by 2× on the large terrains, for files most queries
  never read.
- Lazy classification only: no help to `describe`, which needs `tables.json`
  and would still reread every airfield header.
- The research's key, the size and mtime of one file: it misses a build
  change.

## Consequences

- Open on GermanyCW and Syria drops to about the cost of the table of
  contents; a cache check costs a listing, not about 450 opens.
- An unknown taxiway file no longer blocks heights or sightlines on that
  terrain; it refuses only what reads it, and `check` still reports it.
- A header that changes while size and mtime stay the same goes unseen until
  the build changes. No ED patch is known to do that.
- `list-terrains` builds its survey in one pass.
