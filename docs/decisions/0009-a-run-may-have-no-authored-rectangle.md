# ADR-0009: A run may have no authored rectangle, and both manifest keys are then null

- **Status:** Accepted
- **Date:** 2026-09-04
- **Affects:** `extract-format.md` "manifest.json", the `authored_bounds_m` and
  `authored_bounds_source` rules; `core.md` "Packed file", the `valid` row;
  ADR-0008; `plan.md` tasks X3, X10, C3, C4, C5a.

## Context

`extract-format.md` says `authored_bounds_m` "is `nodesMapBorders` from the
theatre's `entry.lua` when the extractor is given it in config
(`authored_bounds_source: "config"`), else the bounding rectangle, expanded by
10 km, of the authored cells of a 5 km pre-sweep over the bounds rectangle
(`authored_bounds_source: "presweep"`)". It names two sources and no third.

`extractor-hook.md` runs the pre-sweep only in one case: "when `crop_m` and
`authored_bounds_m` are both nil, run the pre-sweep". A user who gives a crop
and no `nodesMapBorders` therefore gets no pre-sweep and no authored rectangle
at all. The grid comes from the crop, and nothing in the run ever measures
where the terrain the author built begins.

That is not a corner case. Task X10 is exactly such a run: a 10 × 10 km crop
around Kutaisi, which is the first live extract and the input every C task
after C4 is built on.

The absent case is already contemplated downstream. `core.md` gives the packed
`valid` layer as 1 where "`height` is not nodata, `water` is not 2, and the
cell is inside `authored_bounds_m` **if known**". What no document settles is
what the extractor *writes* when it is not known, and what `pack` and `check`
then do with it.

ADR-0008 makes that gap a contradiction rather than an omission. It gives the
packed `meta` five new keys — `authored_bounds_min_x`, `authored_bounds_min_z`,
`authored_bounds_max_x`, `authored_bounds_max_z` and
`authored_bounds_source` — "copied from the manifest without conversion", and
says "`check` requires all five". A crop extract has none of them to copy, so
`check` as written refuses a packed file that is correct.

## Decision

**When a run has no authored rectangle, the extractor writes both
`authored_bounds_m` and `authored_bounds_source` as JSON `null`.** That is the
case where `crop_m` is given and config supplies no `authored_bounds_m`, so
the pre-sweep does not run. The two keys are always present and are null
together: a null `authored_bounds_source` with a rectangle, or a source naming
where a rectangle that is absent came from, is an error `check-extract`
reports.

**A null `authored_bounds_m` means the authored rectangle is unknown. It never
means nothing is authored.** No reader treats it as an empty rectangle, and no
reader substitutes the grid or `bounds_km` for it.

**`pack` writes SQL NULL into all five `meta` keys of ADR-0008 when the
manifest's `authored_bounds_m` is null, and `check` accepts NULL in the four
coordinates exactly when `authored_bounds_source` is NULL.** Any other
combination of NULL and non-NULL across the five is a `check` failure. This
replaces ADR-0008's "`check` requires all five".

**`valid` falls back when the rectangle is unknown**: a cell is valid where
`height` is not nodata and `water` is not 2, with no inside-the-rectangle
test. `core.md`'s "if known" is the rule; this record says what unknown looks
like on disk.

## Consequences

X10's extract carries a null rectangle, so C3's `check-extract` must accept
the null pair from the first live run onward. C3 gains one check it would
otherwise not have: the two keys are null together or neither is.

C4 must not use `authored_bounds_m` as a validity mask without testing for
null first. A reader that treats null as an empty rectangle marks the whole
crop invalid, and one that substitutes the grid marks fill cells valid — both
are silent, and both produce a packed file that looks finished.

C5a writes five NULLs rather than five numbers on a crop extract, and C5a's
done test — `check` reports every `meta` key — now covers a NULL as a value it
must print rather than skip.

The cost, and it is real: **a crop extract's `valid` layer is weaker than a
full theatre's.** Fill cells inside a crop are caught by the nodata test
alone, which is what the per-cell fill test already does, so nothing becomes
wrong — but a consumer comparing a cropped packed file with a full one is
comparing two different definitions of `valid`, and only `meta` says which is
which. That is the argument for `authored_bounds_source` being in `meta` at
all, and it now carries a third value.

ADR-0008's sentence "`check` requires all five" reads false and is superseded
by the clause above. ADR-0008 is otherwise unchanged and stays Accepted; the
`meta` keys, their units and their provenance are all still its decision.

`extract-format.md`'s `authored_bounds_m` rule reads incomplete rather than
false: its two sources are both correct, and neither covers a crop run.

## Alternatives considered

**Record the theatre bounds rectangle as `authored_bounds_m`, with a third
source naming it.** Every key stays non-null and `check` needs no change.
Rejected: it asserts the whole map is authored, which is false on every
theatre measured — on Caucasus 3.2 % of a 1 km lattice inside
`nodesMapBorders` already reads the fill value, and `bounds_km` is far larger
than `nodesMapBorders` again. `valid` would mark fill as terrain, which is the
one thing the fill rules exist to prevent.

**Run the pre-sweep even when a crop is given, so a rectangle always exists.**
No null anywhere, and the crop extract gains a real authored rectangle.
Rejected: the pre-sweep is a 5 km lattice over the whole bounds rectangle,
about a minute of frame-budgeted work on a large theatre, to answer a question
about terrain a 10 × 10 km crop will never touch. It would also make the
rectangle depend on which crop was asked for, since the grid is the crop's.

**Omit the two keys rather than writing them null.** The format's other absent
values are omitted rather than nulled in some places. Rejected: ADR-0007
already fixed the opposite rule — a key DCS omits is written as JSON `null` —
and an absent key cannot distinguish "this extractor did not know" from "this
extractor is older than the field". A null says which.

**Let `check` accept any mix of NULL across the five keys.** Simpler
validation. Rejected: the four coordinates and the source are one fact, and a
packed file carrying three coordinates and a source is corrupt in a way no
later reader can detect. Requiring them to agree is what makes the null
meaningful.
