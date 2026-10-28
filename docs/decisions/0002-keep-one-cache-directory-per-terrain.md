# ADR 0002: Keep one cache directory per terrain, and re-stamp an entry whose files survive an update

## Status

Accepted

## Context

**Affects:** `design.md` §4.1 (the cache's layout), §4.2 (when an entry is
rebuilt, and `cache prune`) and §5 (what the build in a key does);
`plan.md` N6, whose reuse across builds this replaces.

The cache kept one directory per terrain and DCS build,
`<cache>/<terrain>/<dcs_build>/`, and every entry's key holds the build, so
each DCS update rebuilt every entry of every terrain. ED ships terrain only
inside updates, but most updates leave a terrain's files alone. The road
graph's betweenness is the costly one: about 250 s of CPU on GermanyColdWar,
against about 5 s for the graph itself (§4.1 on 2.9.30.28738).

Keeping a directory per build only pays off for a user who switches back to an
older build, which few do, and they do it to join a multiplayer server that has
not updated yet. Until it was pruned, every old build's directory took space
for that case.

The build in a key also catches what a stat cannot: a terrain file whose header
changes while its size and modification time stay the same (ADR 0001).

## Decision

The cache keeps one directory per terrain, `<cache>/<terrain>/`, holding the
entries for whichever build was installed when each was last checked. An entry
still records in its key the build it was made on. When the build in an
entry's key is not the installed one, the engine reads the headers of the
entry's install files again and compares the whole key except the build. If
they match, it re-stamps the entry: it writes the same payload under the key
with the installed build, under a temporary name renamed into place, as a build
is written. If they differ, it rebuilds the entry. `tables.json` is always
rebuilt after a build change, because its hook import is tied to one build by
name. `validation.json` stays, and reports the build its figures were measured
on. `dcsterrain cache prune` is removed, since there are no other builds'
directories left for it to delete.

### Alternatives considered

- **Keep a directory per build and copy an entry from the previous one**, the
  first draft of this record. It kept a downgrade's old entries until
  `prune`, at the cost of searching sibling directories, a copy per build,
  and a `prune` command. Few users downgrade, and an entry whose files
  survived is re-stamped either way.
- **Drop the build from keys.** A header that changes while its file's size
  and time stay the same would then never be seen, and the hook import would
  survive a build it does not match.
- **Re-stamp from metadata alone, without reading headers.** Cheaper by the
  cost of the headers, at most about 40 ms on GermanyColdWar's 454 airfield
  files, once per update, and it would carry ADR 0001's blind spot from one
  build to the next.

## Consequences

- An update that leaves a terrain's files unchanged costs one header read per
  file and a copy per entry, under a second for GermanyColdWar's road graph,
  rather than minutes of rebuilding.
- Switching to an older build and back re-stamps or rebuilds each time.
  Nothing from the build left behind is kept for the return.
- A header changed under an unchanged size and time is still caught at the
  next update, because re-stamping reads the headers.
- A cache made before this change keeps its per-build directories under each
  terrain. They are left alone, since nothing is deleted without a command,
  and `cache clear` removes them.
- The cache's size no longer grows with each update.
