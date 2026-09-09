# ADR 0023: The DCS build is the terrain version, and the fingerprint is a record of the files

## Status

Accepted

## Context

**Affects:** `core.md` "Packed file" (`terrain_digest` in `meta`) and
"Versioning" (the default file name, `terrain_digest` in `describe` and every
response, `Theatre::matches_install`); `query-operations.md` "Common types",
the response envelope; `mcp-server.md` "Response discipline";
`extract-format.md` "manifest.json" (`digest`); `design-and-facts.md` "Design
decisions", the "Two version stamps" paragraph; ADR 0021, which this
supersedes; `plan.md` X2b, C3, C12c.

`design-and-facts.md` says "The DCS core build (`autoupdate.cfg`) and the
terrain module's data are versioned separately by ED: a terrain can be rebuilt
under an unchanged core build, and that is what moves heights, roads and
scenery." On that belief the fingerprint had to stand in for a terrain version,
first as a hash (`extract-format.md`) and then, in ADR 0021, as the files'
modification times with the newest as an eight-hex-digit `digest`. `core.md`
names the packed file `<theatre>-<dcs_build>-<terrain_digest>-<cell_size>m.sqlite`
and puts `terrain_digest` in `meta`, in `describe` and in "every operation's
response"; `query-operations.md` and `mcp-server.md` repeat the envelope.

What is true:

- ED ships terrain data only inside a DCS update, and every update, hotfixes
  included, changes the `version` string in `autoupdate.cfg`. That is a fact
  about ED's release process rather than about any file in the install, and
  the install cannot prove it; it is the user's knowledge of how DCS is
  distributed, and nothing measured contradicts it. A module installed or
  repaired between updates receives the data for the build already installed.
- The opposite direction does not hold: ADR 0007 measured the three Caucasus
  files byte-for-byte identical across builds 2.9.29.27278 and 2.9.29.27468.
  So the build identifies the data, but a build change does not mean the data
  changed.
- The resume check already refuses on `theatre`, `dcs_build` and
  `dcs_build_timestamp` before it reaches the fingerprint.
- The install holds no terrain version to read. `entry.lua` `version` is
  absent on Caucasus, `""` on three theatres, `"EA"` on three and `"2.8.0"` on
  Sinai, at 2.9.29.27468: a label, not a data version. `manifest.bin` is
  opaque and is re-stamped by the updater on every run. `autoupdate.cfg` lists
  the terrain modules by name with no version of their own.
- ADR 0021's `modified` field had one job, to change when a file did, and its
  accepted cost was that a repair or reinstall re-stamps the files and refuses
  a resume of an extract that was fine.

## Decision

The version of a theatre's data is the DCS build. A resume refuses when
`theatre`, `dcs_build` or `dcs_build_timestamp` differs from the existing
manifest, as it already did. The `terrain_fingerprint` is a record of the three
files and not a version: for each of `surface5`, `rn4` and `scn5` it holds
`path`, `size` from `lfs.attributes(path, "size")` and `payload_size` from the
little-endian u64 at bytes 8–15 of a sixteen-byte read of the head, and
nothing else. There is no `modified` and no `digest`. A resume also refuses
when any fingerprint entry differs, which catches an install that is damaged,
incomplete or not the one the extract was read from. `pack` names its output
`<theatre>-<dcs_build>-<cell_size>m.sqlite`; `terrain_digest` leaves `meta`,
`describe` and the response envelope, whose provenance fields are `theatre`,
`dcs_build` and `cell_size`; `Theatre::matches_install` compares each file's
path, size and payload size and reports which differ.

### Alternatives considered

**Keep ADR 0021 as it stands.** Rejected: under the fact above the
modification time adds no information to the build, and its cost, a refused
resume after a repair or reinstall, buys nothing.

**Stop comparing `modified` on resume but keep it, and the digest, as an
informational id.** Rejected: a field nothing compares and an id that
duplicates `dcs_build` in the file name are leftovers, and the same data would
still carry two file names after a reinstall.

**Derive `digest` from the build so the file name keeps its shape.** Rejected:
the build is already in the name.

**Drop the fingerprint entirely.** Rejected: the sizes and payload fields are
free to record, they catch a damaged or partial install without any hashing,
and they tell a reader of the manifest which files an extract was built from.

## Consequences

Prepare reads the build and sixteen bytes of each of three files. Nothing is
hashed and no time is recorded, so a repair or reinstall of the same build
reads the same and resumes.

**A build change refuses a resume even when the terrain data did not change,**
which ADR 0007 shows is the common case. That was already true before this
record, since the identity check compared the build first; what is lost is the
possibility of ever relaxing it by comparing the files instead, and that
possibility was never used.

**The decision rests on ED's release process.** A terrain update shipped
without a new build string would go unnoticed unless it changed a file's size
or payload field. Nothing in the install can detect that case, and nothing in
ED's practice suggests it happens.

**Frozen text that now reads false.** `design-and-facts.md` where it says a
terrain "can be rebuilt under an unchanged core build"; `extract-format.md`
where the manifest carries `digest`; `core.md` where the file name, `meta`,
`describe` and the responses carry `terrain_digest`; `query-operations.md` and
`mcp-server.md` where the envelope names `terrain_digest`. ADR 0021 is
superseded; its measurements of the SHA-256 cost and of `lfs.attributes` stand,
its decision does not.

**Done tests that change.** X2b: each recorded `size` and `payload_size`
equals what `stat` and `od` report for the real file, and the directory is
found for a theatre whose id is not its directory name. C3's `check-extract`
validates the three-key entries. C12c names the file without a digest. The
synthetic constants carry no `MODIFIED` and no `DIGEST`.
