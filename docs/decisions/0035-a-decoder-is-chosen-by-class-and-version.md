# ADR 0035: A decoder is chosen by the file's class and version, and an unknown one is refused

## Status

Accepted

## Context

**Affects:** `design-and-facts.md` "Two version stamps, not one" (the
sentence that the install carries no readable terrain version); ADR 0034.

The design records the terrain as unversioned:

> The install carries no readable terrain
> version string (`entry.lua` `version` is empty, `manifest.bin` is
> opaque)

That holds for the terrain as a whole, but not for its files. Every map file
opens with the same container header: a u32 container version (2), a u32
header size (0x30), and at 0x20 a length-prefixed class name, followed by a
u32 format version. Read across the eight maps installed on DCS
**2.9.30.28738**:

| File | Class | Version |
|---|---|---|
| `.surface5` | `landscape5::Surface5File` | 5 on every map |
| `.scn5` | `landscape5::Scene5File` | 8 on Nevada, 9 on the other seven |
| `.rn4`, roads and airfields | `landscape4::lRoadNetwork` | 5 on every map |
| `.rn5` | `t5::rn5file` | 0 on every map |
| `.routes` | `landscape4::lRoutesFile` | 3 on every map |
| `.ng5` | `navGraph5File` | 10 on every map |
| `.gn4` | `landscape4::lGeoNamesFile` | 2 on every map |

The one version that differs is the one format that differs. Nevada's version
8 scene file carries five attributes per placement block where version 9
carries four, gives the model index directly instead of in the low bits of an
options word, stores heading as one f32 axis instead of two packed u16 axes,
and shifts an instance id by 15 bits instead of 13. A decoder written for
version 9 reads version 8 without error and returns wrong models and headings.

## Decision

Each decoder declares the (class name, format version) pairs it reads, and
`dcsterrain extract` reads every file's header before decoding and chooses the
decoder from that pair. A pair no decoder declares stops the run, naming the
file, its class and its version. After decoding, each decoder checks its own
output against the decoded ground and the map's extent: road vertices,
scenery, stands and runway ends lie on the ground and inside the mesh, and
counts are non-zero. A failed check stops the run the same way. The extract's
`manifest.json` records the class and version of every file it read.

### Alternatives considered

**Key decoders on the DCS build.** The build is readable from
`autoupdate.cfg`, but a build changes far more often than a format, so every
release would need a new entry for files that did not change. It also says
nothing about a map whose module updates apart from the core.

**Detect the format by probing the data.** Version 8 and 9 scene files can be
told apart by attribute count. But probing guesses where the file states the
answer, and a probe written today cannot recognize a version that does not
exist yet.

## Consequences

A new DCS build that changes a format stops extraction with a message naming
the file, rather than writing plausible but wrong terrain. Supporting it is
one new decoder entry, and the old one keeps reading older installs.

The version catches only the changes ED declares. A change made without
bumping the version passes the dispatch, and only the sanity checks can catch
it. They catch gross failures, not a field that is subtly misread. The
regression against a live extract stays the only exact test, and it is run
for one map.

The sanity checks need a ground model before the other layers can be checked,
so `surface5` is decoded first on every run.

The manifest gains a list of file classes and versions, which the extract
format revision must carry.
