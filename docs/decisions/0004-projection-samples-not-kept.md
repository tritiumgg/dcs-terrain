# ADR-0004: The projection samples are not kept; `fit.py` moves to `tools/probe-theatre/`

- **Status:** Accepted
- **Date:** 2026-09-03
- **Affects:** `validation.md` "Measuring a new theatre or build", which names
  `projection-samples/fit.py`; `probe-log-2.9.29.27278.md` "Projection, bounds
  and fill for every installed theatre", which cites
  `projection-samples/<id>.txt`.

## Context

`docs/data/projection-samples/` held 20 measured `(x, z, lat, lon)` samples for
each of seven theatres — Afghanistan, Cold War Germany, Marianas, Marianas
WWII, Persian Gulf, Sinai and Syria — beside `fit.py`, which fits a transverse
Mercator to a sample set and prints the proj4 string.

Nothing in the build reads the sample files. The extractor writes
`latlon_samples` into the extract's `config.json`, `pack` fits the projection
from those, and task C10's done test uses synthetic samples generated from the
design document's own parameters. The fitted results are already recorded as a
table in the probe log, so the samples are the working-out behind a number that
is written down elsewhere.

The script is different. `validation.md` names it as the second step of
measuring a new theatre or a new build: one `dcs_eval_in` in the hook state
with the Mission Editor open on the map produces the 20 samples, then `fit.py`
fits them. A theatre with no measured projection has no `crs_proj4`, so the
step is not optional, and task V2 packages the script with the hook chunk into
`tools/probe-theatre/`.

Keeping the script inside a directory named for samples that no longer exist
would leave the name lying about the contents.

## Decision

The seven sample files are deleted. `fit.py` moves to
`tools/probe-theatre/fit.py`, the location task V2 packages it into, and
`docs/data/projection-samples/` is removed.

Re-measuring a theatre produces its samples again; they are an intermediate of
the probe, not an artifact worth carrying. Anyone who wants them runs the probe.

## Consequences

The repository stops carrying measurement working-out that nothing reads. The
probe log's projection table remains the record of the result.

Two frozen documents now name a path that does not exist:
`validation.md` says `projection-samples/fit.py`, and the probe log cites
`projection-samples/<id>.txt`. Read both as `tools/probe-theatre/fit.py` and as
samples that are no longer stored.

Re-deriving a projection for one of the seven theatres now needs that theatre
installed and a DCS session, where before it was a local script run. That is
the cost of the deletion, and it is the reason the fitted table in the probe log
matters more than it did.

`tools/probe-theatre/` exists before task V2 creates it. V2 adds the hook chunk
and the packaging; the script is already in place.

## Alternatives considered

**Keep the samples.** 36 KB, and irreplaceable without owning all seven
terrains and running a probe per map. Rejected: nothing reads them, the fitted
results are recorded in the probe log, and a repository that ships tools only
should not accumulate measurement intermediates.

**Delete `fit.py` too.** It is 26 lines of numpy and pyproj, and C10 implements
the same fit in Rust. Rejected: C10 fits from an extract, and the probe path
exists precisely for a theatre that has not been extracted yet. V2 needs the
script, and a frozen spec names it.

**Leave `fit.py` in `docs/data/projection-samples/`.** No frozen path breaks,
and no ADR is needed. Rejected: a directory named for samples it does not
contain, holding a script that is not documentation, inside `docs/`.
