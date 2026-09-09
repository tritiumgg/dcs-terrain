# ADR 0022: Fingerprint files are found by extension, and a directory without entry.lua is not a theatre

## Status

Accepted

## Context

**Affects:** `extract-format.md` "manifest.json", the sentence "where `<id>`
is the theatre id and `<terrain_dir>` is the directory name from the
extractor config … the file base names use the id"; ADR 0011's provisional
paragraph on the `entry.lua` scan; `plan.md` X2b.

`extract-format.md` names the three fingerprinted files as
`Mods/terrains/<terrain_dir>/Surface/<id>.surface5`,
`Mods/terrains/<terrain_dir>/roads/<id>.rn4` and
`Mods/terrains/<terrain_dir>/Scenes/<id>.scn5`, and says "the file base names
use the id". ADR 0011 derives `<terrain_dir>` by scanning
`Mods/terrains/*/entry.lua` for the id, and notes that a theatre whose
`entry.lua` does not set `id` "would reopen the question of a fallback".

Measured on the install at DCS 2.9.29.27468 (`timestamp` 20260902-093323),
listing `Mods/terrains` read-only:

- `Mods/terrains` holds ten directories. Eight carry an `entry.lua` with
  `['id'] = "…"` at line 23. **`Kola` and `Nevada` hold a `radio.lua` each and
  nothing else**: no `entry.lua`, no data files.
- The base names follow no one rule. `Sinai` (id `SinaiMap`) ships
  `SinaiMap.surface5`, `SinaiMap.rn4`, `SinaiMap.scn5` — the id.
  `GermanyColdWar` (id `GermanyCW`) ships `GermanyColdWar.*` and `MarianasWWII`
  (id `MarianaIslandsWWII`) ships `MarianasWWII.*` — the directory name. The
  other five agree with both because id and directory are the same word.
- Each directory holds exactly one file with the extension in question, beside
  others: `Surface/` holds `<name>.ng5`, `<name>.onlay.sup4`, `<name>.surface5`
  and `<name>.tile`; `roads/` holds `<name>.rn4` and `<name>.routes`; `Scenes/`
  holds `<name>.scn5` alone. The airfield `.rn4` files live under
  `AirfieldsTaxiways/`, not `roads/`.
- The surface directory is spelled `Surface` on Caucasus and `surface` on the
  other seven. Windows resolves either.
- `lfs.dir` exists in the hook state and ED iterates it as
  `for name in lfs.dir(path)`, filtering `.` and `..`.

## Decision

Each fingerprinted file is the one file in its directory whose name ends in
its extension: `.surface5` under the theatre's `Surface` directory, `.rn4`
under `roads`, `.scn5` under `Scenes`. The directory is matched without regard
to case and the extension likewise, and the recorded `path` spells both as the
disk does. A directory with no such file, or with more than one, refuses the
run rather than guessing. When resolving `terrain_dir`, a directory under
`Mods/terrains` with no `entry.lua` is not a theatre and is passed over
without a message; a theatre whose `entry.lua` matches nothing is looked for
once more as a directory literally named for the id, which is ADR 0011's
fallback, before the run refuses.

### Alternatives considered

**Compose `<id>.<ext>` as specified, then try `<terrain_dir>.<ext>`.** Covers
all eight installed theatres today. Rejected because it encodes two rules where
the disk shows there is none, and a ninth theatre could follow a third.

**Compose the path with the spec's spelling `Surface` and let Windows resolve
it.** Rejected: the manifest would record a path that does not exist as written
on seven of eight theatres, and a reader on another platform comparing paths
would see a file that is not there.

**Treat a `Mods/terrains` entry without `entry.lua` as an error.** Rejected: the
install ships two, they are ED's leftovers rather than the user's, and a
warning about them would fire on every Start on every install.

## Consequences

The three files are found on every installed theatre, including the two whose
data files carry the directory name and the one whose data files carry the id.

**A theatre shipping two files of one extension in one directory refuses the
run.** None does today. If ED ever ships one, the refusal names both files, and
the fix is a rule for choosing rather than a silent pick.

The recorded `path` is now the disk's spelling, so the same theatre records
`Surface` on Caucasus and `surface` elsewhere. Nothing derives anything from
the path; it is a record of what was read.

`extract-format.md`'s clause "the file base names use the id" reads false, and
ADR 0011's provisional paragraph is settled: the fallback it anticipated is the
one implemented. X2b's done test keeps its second half, that the directory is
found for a theatre whose id is not its directory name, and gains that each
data file is found where its base name is neither.
