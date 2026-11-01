# tools/validate

The live validation sortie. It compares the tool's answers with what a running
DCS reports for the same terrain — heights, water, sightlines, route lengths
and MGRS — and writes `validation.json`.

1. `plan` draws stratified samples, records the tool's answer to each, and
   writes `plan.json` and the Lua chunks under `--out`. It needs no DCS.
2. With DCS in the Mission Editor on that terrain, an agent evaluates each file
   in `chunks/` through the bridge, in the hook state, with an instruction
   budget of 50 000 000. Each chunk refuses another terrain or build, and
   writes its answers to `replies/`.
3. `report` scores the replies, writes `validation.json` under `--out` and to
   the terrain's cache, and exits with failure if a target is missed.

```sh
mise exec -- cargo run --manifest-path dcsterrain/Cargo.toml --release -p dcsterrain-validate -- plan --terrain Caucasus --out <dir>
mise exec -- cargo run --manifest-path dcsterrain/Cargo.toml --release -p dcsterrain-validate -- report --out <dir>
```

Routes are asked with no bound on the detour, and a route either side refuses
is kept in the sample: `pass` needs both to refuse the same ones, no route
longer than the game's by over 0.5 % of its length plus the distances its path
starts and stops off the ends asked, and a shorter one to take only passages the
road file lists. A plan drawn before routes recorded that is redrawn first.

`pass` judges the random sightlines; the marginal ones, within 1 m of grazing,
and each stratum are reported beside it. Planning clears `replies/`, so a plan
is run whole.

`--install` names the install where there is no registry, and `--cache` the
cache. It reads no data file from this repository.
