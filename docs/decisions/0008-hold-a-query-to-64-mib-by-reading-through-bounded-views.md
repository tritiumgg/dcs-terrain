# ADR 0008: Hold a query to 64 MiB by reading through bounded views

## Status

Accepted

## Context

**Affects:** `design.md` §3.3 (reading blocks), §4 (`roads.graph`, `scene.nodes`)
and §8 (the memory target, replacing ADR 0007's 150 MiB); `plan.md` Q18.

ADR 0007 raised the memory target of a query process to 150 MiB because the
Marianas held about 100 MiB whatever the block budget, a cause it left
untraced. Measured on 2.9.30.28738, every case of the benchmark through one
process of the binary, peak private commit:

- `nearest` scenery reached 961 MiB on Afghanistan: a doubling box made an
  answer with two strings for every object in it.
- GermanyCW held 295–305 MiB and Syria 214–220 MiB in any operation that read
  the road graph, which was decoded whole.
- The Marianas held 114–151 MiB. They have 458 blocks, most of them tiny, and
  one of 811 k triangles: 19 MiB to read and 27 MiB prepared. Raw and prepared
  copies of a few such blocks stand together, so no block budget bounds them.
  `viewshed`'s radials gathered the pieces of every radial over that mesh
  before reducing any.

`design.md` ruled out mapping a whole terrain file: on Windows a view is
charged about 2 MiB of commit per GiB, 123 MiB for GermanyCW's `.surface5`.
Shrinking the block budget to 32 MiB made `visible` on MarianaIslands 10×
slower.

Measured with a view open on NTFS: overwriting in place, extending, renaming
over the file, renaming it away and deleting it all succeed, and the view goes
on reading. Truncating fails, with os error 1224. On Linux and macOS truncation
under a view ends the process with SIGBUS.

## Decision

A query process holds at most 64 MiB, judged on peak commit, which is
`PeakPagefileUsage` on Windows, so what Windows charges for a view counts and
mapped file pages do not. A request that builds a cache entry is exempt and is
measured beside the target.

A block kept for repeated use is read through a read-only view of its own
arrays, through `memmap2`, and never a view of a whole file; a block of under
64 KiB of arrays, or one read once and dropped, is copied. Only a compact index
stays private: a four-way bounding volume hierarchy over triangles in Morton
order, about 7 bytes a triangle against the 33 the prepared form took. The
hierarchy is built in memory and adds nothing to the cache. `roads.graph`
becomes flat arrays with each network's search grid, and it and `scene.nodes`
are read through views of their cache entries. A terrain with no block asked for in a second drops every block
it holds, so a batch waiting on its caller holds no view of an install file.

### Alternatives considered

- **A smaller block budget alone.** It trades memory for block reads: 10× on
  `visible`, and the Marianas stay over the target.
- **Mapping whole terrain files.** The view's charge alone is over the target
  on GermanyCW.
- **The hierarchy or the bins kept in the cache.** A terrain's cache is as
  small as 5 MiB, and the bar a cache increase must clear allows 5 %.
- **Positional reads off Windows, where nothing guards against truncation.** It
  leaves those platforms over the target on the Marianas, and DCS, the one
  thing that rewrites these files, does not run there.

## Consequences

Peak commit is under 64 MiB on every case of every terrain
(`data/q18-performance.md`), `exposure` included: at the limit its answer is
47 MiB of text, so an answer written out is encoded as it goes rather than held.
Cold reads of a large block go through
`PrefetchVirtualMemory` or `madvise`, so a view is not read page by page.

A view of an install file stops an updater that truncates that file in place,
for at most a request and a second; an updater that replaces the file by rename
is unaffected. A file overwritten in place under a view shows through it, so a
request running across an update can see mixed data, as mixed positional reads
could; indices stay bounds-checked, so it fails or answers that one request
wrongly.

On Linux and macOS a terrain file truncated while a query reads it kills the
process with SIGBUS, with no error and no cache written. This is accepted and
documented in the README.

`roads.graph` grows by 4 bytes an edge and its grid, at most 3.8 % of a
terrain's cache, and rebuilds once. Its arrays must lie on 8 bytes in the entry's file, which its
head is padded for; an entry re-stamped with a key of another length is copied
into memory until it next rebuilds, which costs the target on that terrain only.

Supersedes ADR 0007's memory target; the rest of ADR 0007 stands.
