# ADR 0021: The terrain fingerprint records sizes and modification times, not a hash

## Status

Accepted

## Context

**Affects:** `extract-format.md` "manifest.json" (the `terrain_fingerprint`
entries, the `digest` rule and the SHA-256 paragraph); `extractor-hook.md`
"Hook-pass sweeps" (the **prepare** paragraph's 1 MiB read and sliced
SHA-256); `design-and-facts.md` "Design decisions", the "Two version stamps"
paragraph; ADR 0007's `digest` paragraph and the fingerprint values in its
manifest example; `plan.md` X2b, C3, C12c.

The fingerprint exists so that a resume refuses when ED rebuilt a terrain
under a half-finished extract, and so that a packed file and every query
response carry a short id of the terrain data they describe. `extract-format.md`
specifies it as, per file, "the file size from `lfs.attributes(path, "size")`
… the little-endian `u64` at bytes 8–15 of the container header … and the
SHA-256 of the first 1 MiB", with `digest` "the first 8 hex characters of the
SHA-256 over the three `head_sha256` values concatenated", and says "The hook
needs a SHA-256 in pure Lua 5.1: the hook state has no `bit` library and no
`string.pack`; use the arithmetic implementation". `extractor-hook.md` adds
that the hash runs "as a sliced iterator under the frame budget, because the
hook state runs about 17 million simple Lua operations per second and the
three hashes take several seconds in total".

Measured while planning X2b on DCS 2.9.29.27468 (`timestamp` 20260902-093323):

- A table-driven SHA-256 in Lua 5.1 doubles costs about 0.22 ms per 64-byte
  block under the MSVC interpreter `mise run lua51` builds, and by the probe
  log's operation rate up to 1.3 ms in the hook state. Three 1 MiB heads are
  49 152 blocks: between 11 s and 64 s of CPU, and at 5 ms of frame budget in
  a 16.7 ms frame between 36 s and 3.5 min of wall time in **prepare**, paid on
  every Start, since a Stop and a Start re-enter prepare.
- The implementation is about 150 lines of arithmetic standing in for bit
  operations, plus its test vectors, for a label nobody attacks: the only
  property asked of it is that it changes when the file does.
- The hook state has `lfs.md5sum`, with no measured signature and no call site
  in ED's own Lua. If it takes a path it hashes a whole file, and
  `Caucasus.surface5` is 6 293 651 208 bytes.
- `lfs.attributes(path, "modification")` answers in the hook state: ED's own
  `Scripts/Hooks/webGUI.lua` reads `attr.modification` off `lfs.attributes`
  there. It reports whole seconds since the epoch.
- `string.format("%d", 6293651208)` prints `-2147483648` on this build's Lua,
  and `%x` goes through the same 32-bit `long`.

The user decided in planning that hashing is not worth its cost here.

## Decision

The terrain fingerprint hashes nothing. For each of the three files the hook
records `path`, `size` from `lfs.attributes(path, "size")`, `payload_size` as
the little-endian u64 at bytes 8–15 of a sixteen-byte read of the head, and
`modified`, the file's modification time from `lfs.attributes(path,
"modification")` as an integer number of seconds since the Unix epoch. There is
no `head_sha256`. `digest` is the largest `modified` of the three files
written as eight lowercase hexadecimal digits, computed by arithmetic rather
than `%x`. It keeps the eight-character shape `core.md`, `query-operations.md`
and `mcp-server.md` rely on for `terrain_digest` and changes whenever any of
the three files is rebuilt, which is what those documents ask of it. The
manifest example's fingerprint therefore reads, in shape:

```json
"terrain_fingerprint": {
  "surface5": { "path": "Mods/terrains/Caucasus/Surface/Caucasus.surface5", "size": 6293651208, "payload_size": 43964216, "modified": 1763100000 },
  "rn4":      { "path": "Mods/terrains/Caucasus/roads/Caucasus.rn4",       "size": 203468160,  "payload_size": 203468160, "modified": 1763100000 },
  "scn5":     { "path": "Mods/terrains/Caucasus/Scenes/Caucasus.scn5",     "size": 1140069192, "payload_size": 11888248,  "modified": 1763100000 },
  "digest": "6916c560"
}
```

with the times and digest measured, not copied from here. `Theatre::matches_install`
in `core.md` compares size, payload size and modification time truncated to
whole seconds, because that is the resolution the hook records.

### Alternatives considered

**SHA-256 as specified.** Rejected for the cost above: minutes of frame time
per Start and 150 lines of hand-rolled arithmetic, against a requirement that
is only "changes when the file changes".

**`lfs.md5sum`, ED's own function.** It would have cost nothing to write if it
hashes a string. Rejected because its signature is unmeasured and its whole-file
form would freeze DCS on a 6 GB file; probing it needs a running DCS, and the
decision did not wait for one.

**CRC-32 over the first MiB.** About thirty lines, twenty times faster than
SHA-256 in this Lua, checkable against `cksum`. Rejected as still a hash by
hand for the same requirement.

**A small checksum over the nine numbers for `digest`.** Rejected: a hash by
another name, and the newest time already moves whenever any file does.

**Sizes only, no time.** Rejected: `rn4`'s payload field equals its size, and a
rebuild that keeps a size is plausible; the time is what makes a rebuilt file
read as new.

## Consequences

Prepare costs a few file reads instead of tens of seconds to minutes of hashing,
and `extractor-hook.md`'s sliced iterator is never written.

**The fingerprint is not content-derived.** A repair, reinstall or copy of the
DCS install re-stamps the three files without changing a byte of them. That
refuses a resume of an extract that was fine, and gives the same data a new
digest and so a new packed file name. Accepted with the decision.

**`digest` reads as a time.** `6916c560` is 1763100000 in hex, which a reader
can decode to the date the newest terrain file was written; it is no longer
opaque. Two installs holding identical terrain data at different times carry
different digests.

The frozen text now reads false where `extract-format.md` names `head_sha256`,
defines `digest` over SHA-256 or asks for "a SHA-256 in pure Lua 5.1"; where
`extractor-hook.md` reads "one `read` of 1 MiB (9 ms), then SHA-256 in pure Lua
as a sliced iterator under the frame budget"; and where `design-and-facts.md`
reads "SHA-256 of the first MiB". ADR 0007's paragraph beginning
"`terrain_fingerprint.digest` is the first 8 hex characters of the SHA-256"
and the `head_sha256` and `digest` values in its manifest example no longer
hold; the rest of that record stands.

**Done tests that change.** X2b's done test is no longer a `sha256sum`
comparison: each recorded `size`, `payload_size` and `modified` equals what
`stat` and `od` report for the real file. C3's `check-extract` validates the
new shape; C12c's default file name carries the new digest. The synthetic
constants in `synth_constants.lua` and `synth.rs` carry a `MODIFIED` per file
and the digest that rule gives.

**Provisional in one respect.** `lfs.attributes(path, "modification")` is
taken from ED's own hook rather than measured here; the live run under X2b
reads it and compares against `stat`. A value in anything other than whole
seconds since the epoch would reopen the digest's definition, not the decision
to record a time.
