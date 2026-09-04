# tools/probe-theatre

The per-theatre measurement, packaged so a new map or a new DCS build is
measured in one step: the hook chunk that samples the map, and `fit.py`, which
fits the projection constants from those samples.

The samples themselves are not kept. The fitted constants are, in the probe log
under `docs/spec/`.
