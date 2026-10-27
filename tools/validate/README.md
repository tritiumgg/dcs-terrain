# tools/validate

The live validation sortie, planned and not built yet. It will compare the
tool's answers with what a running DCS reports for the same terrain —
heights, water, surface types, sightlines and routes — and write
`validation.json` to the terrain's cache.

It reads no data file from this repository; the DCS install is named on the
command line.
