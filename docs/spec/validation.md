# Spec: probes and validation

Everything that touches a running DCS other than the extractor itself:
the probes still open, and the validation sortie that stamps a packed
file. Runs on the Windows machine through the `dcs-api-bridge` tools.
Measured results so far: `probe-log-2.9.29.27278.md`. The safety
rules in `design-and-facts.md` ("Sources you have") apply to every
call here; in particular, check `dcs_bridge_status` shows phase `sim`
before any `server`-state call, and clear the bridge transport
directory before restarting DCS.

## Measuring a new theatre or build

Scenery clearing and every installed theatre's projection, bounds
and fill sentinel are answered in the probe log; no probe is open. A new
theatre (Kola, Nevada) or a new DCS build repeats the per-map chunk
recorded there:
one `dcs_eval_in` in the hook state with the Mission Editor open on the
map, then `projection-samples/fit.py` on the 20 samples. Results are
appended to the probe log with the build string, and the design doc is
updated in the same change. `tools/probe-theatre/` packages that chunk
and `fit.py` so the measurement is one step.

## Validation sortie

Run once per packed file, after `pack`, before the file is used by a
campaign. Inputs: the packed file, the same theatre loaded in a running
mission, the bridge.

1. `dcsterrain query <file> sample` with `points` set to 200 random
   `valid` cells across the grid, layers `height`, `water`, `surface`.
2. For the same cells, live `terrain.GetHeight`, `terrain.GetSurfaceType`
   and `land.getSurfaceType` through the bridge, in one chunk each.
3. Compute height RMSE and max error (expected: RMSE under 0.5 m from
   `i16` rounding, max under 1 m; anything larger means a grid offset
   bug), water class agreement (expected 100 %), surface enum agreement
   (expected 100 %).
4. 300 random point pairs 0.5 to 15 km apart at 2 m AGL among the
   `valid` cells: `dcsterrain query visible` versus live
   `terrain.isVisible`. Expected agreement ≥ 99 %; record the
   disagreement cases with their clearance values, since they measure
   the half-cell sampling rule.
5. 20 road routes between random airdrome pairs: `dcsterrain query route`
   length versus live `terrain.findPathOnRoads` polyline length. The
   route ratio is file length divided by live length per route;
   `validation_route_ratio` is the largest `|ratio − 1|` over the 20.
   Expected within 5 %; larger gaps point at graph merge errors.
6. `dcsterrain stamp <file> validation_height_rmse=... validation_los_
   agreement=... validation_route_ratio=... validation_at=...`.

The whole sortie is one script, `validate.lua`, run through
`dcs_eval_in` in chunks, plus one shell script that drives the
`dcsterrain` side and the comparison; both live in the repository under
`tools/validate/`. It reads no data file from the repository.

## Acceptance

1. A Caucasus file at 50 m stamps with height RMSE under 0.5 m, LOS
   agreement ≥ 99 %, route ratio within 5 %.
2. `tools/probe-theatre` reproduces the Caucasus projection row.
