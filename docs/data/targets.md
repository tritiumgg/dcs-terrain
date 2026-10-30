# The targets on every installed terrain

Every target of `design.md` §8 against the shipped build, on the 8 terrains
of 2.9.30.28738 (`autoupdate.cfg` timestamp `20261006-075422`) and `Synth`.
Measured 2026-10-30 on a Ryzen 7 7800X3D (8 cores, 16 threads), 32 GB, NVMe,
Windows 11, DCS not running, nothing else at work.

**The bound is on cycles; the wall is beside it for a reader.** Each figure
is cycles, then wall clock. The target is the design's time; its budget in
cycles is that time on one core at the counter's rate.

## Method

- **Figure.** The median of the rounds' medians. A warm-up round is dropped,
  and is the "first" figure of the JSON.
- **Error bound.** 6 rounds here, whose order statistics give a 96.9 %
  interval on the median with no assumption about the distribution. Over the
  188 rows with a target it averages 4.7 % of the median, 127 rows lie within
  5 %, and the widest is 28 %, `route_50km` on `Synth`, a case of 0.4 Mc.
- **Outliers.** A round beyond 3 scaled median absolute deviations is counted
  and kept, never dropped; the median already ignores it. 39 of the 188 rows
  have one.
- **Cold** opens each file unbuffered before each round, which drops it from
  the Windows file cache without privilege. **Warm** runs on what the
  previous round left. A target is judged only in the OS cache state it is
  stated in.
- **The cache** was built beforehand: only `index_empty_cache` builds, once,
  from an empty cache, without betweenness or places.
- **Requests** come from one seeded generator, the same each run, and start
  on land. A terrain may refuse one: `route` and `trafficability` count
  `no_route`, `too_far` or `off_terrain` as errors, the figure then including
  the refusal. On GermanyCW every `trafficability` request was refused, so
  that case times nothing else. `trafficability` has no target.
- **The bounds in CI** take 9 rounds on `Synth` and fail above 3× the budget,
  above 3× the memory target, on a failed request, or on no cycles counted.
  They catch a large regression, not a small one: `Synth` is small, so only
  `viewshed_20km`, `radar_floor_20km`, `exposure_10_sites`,
  `find_sites_los_36` and the horizon's first touch stand within 5× of their
  bound, and `open` or `route_50km` lie thousands of times under theirs.
  On Linux the memory bound sees a child only above the bench's own peak,
  because the OS seeds a child's peak from its parent's.

```bash
DCSTERRAIN_INSTALL="<install root>" \
  DCSTERRAIN_BENCH_TERRAINS=Afghanistan,Caucasus,GermanyCW,MarianaIslands,MarianaIslandsWWII,Nevada,PersianGulf,Syria \
  DCSTERRAIN_BENCH_ROUNDS=6 \
  mise exec -- cargo bench -p dcsterrain-cli --bench query
```

## What a cycle is

Cycles are **reference cycles** of the CPU time a thread was running, summed
over the process's threads. They are not core clock cycles, which need a
privileged performance counter that GitHub's runners do not offer.

| Platform | Counted by | Ticks at |
|---|---|---|
| Windows | `QueryThreadCycleTime`, `QueryProcessCycleTime` | the time-stamp counter |
| Linux, macOS on x86_64 | CPU-time clock × the counter's rate | `rdtsc`, calibrated against the clock |
| Linux, macOS on aarch64 | CPU-time clock × the counter's rate | `cntfrq_el0` |
| any other | CPU-time clock | 1 GHz |

Measured here, a chain of one-cycle adds against both counters, GHz:

| Threads | Core clock | `rdtsc` | `QueryThreadCycleTime` |
|---|---|---|---|
| 1 | 5.03 | 4.19 | 4.19 |
| 8 | 4.96 | 4.19 | 4.18 |
| 16 | 4.89 | 4.19 | 4.10 |

What follows from that:

- **Frequency and turbo.** The counter does not follow the core. A core at
  5.03 GHz does a fifth more work per counted cycle than the counter's
  4.19 GHz implies, so a run with less turbo counts more cycles for the same
  work. Between machines, cycles carry the clock ratio; they are a fair
  comparison on one machine, or against a 3× bound.
- **SMT.** Both siblings of a core count. Work spread over 16 threads can
  count up to twice what it would over 8 cores.
- **Waiting costs nothing.** A thread blocked on a disk or a lock, or
  preempted, counts no cycles. Cold wall is the only figure that sees a disk,
  which is why it stays beside the cycles.

A parallel operation can therefore miss its one-core budget while meeting
its time by far: `viewshed_20km` on `Synth` spends 748 Mc in 52 ms.

**Under noise.** `Synth` with 8 spinner processes against the idle machine, 5
rounds: cycles moved 0.77–1.32× (1.77× on one sub-millisecond case,
`airfield_info/fresh`) while wall moved up to 1.88×. The horizon's first
touch moved 0.98× in cycles and 1.88× in wall, `viewshed_20km` 1.02× and
1.37×. Nothing crossed a bound. The tightest case at rest,
`find_sites_horizon_first_touch`, stands at 36.2 Gc of its 62.7 Gc bound.

## The table

Mc (millions of cycles) / ms. The bound is on the Mc. Bold marks a figure
over the target; a bold wall alone is a target met on cycles and over on the
clock.

| Operation | OS cache | Target ms | Budget Mc | synth | Afghanistan | Caucasus | GermanyCW | MarianaIslands | MarianaIslandsWWII | Nevada | PersianGulf | Syria |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `open` | cold | 20 | 83.7 | 0.02 / 0.01 | 19.0 / 4.57 | 8.22 / 1.98 | 16.0 / 3.84 | 11.4 / 2.73 | 11.4 / 2.74 | 11.3 / 2.71 | 16.4 / 3.94 | 18.4 / 4.41 |
| `describe` | cold | 20 | 83.7 | 0.07 / 0.02 | 8.13 / 1.95 | 7.58 / 1.82 | 11.6 / 2.78 | 7.46 / 1.79 | 7.51 / 1.80 | 7.50 / 1.80 | 8.02 / 1.93 | 11.0 / 2.64 |
| `sample_10k_scattered` | cold | 3000 | 12549.6 | 52.3 / 12.5 | 2006 / 1598 | 1366 / 1040 | 3653 / 1794 | 382 / 95.4 | 393 / 97.3 | 1305 / 1239 | 1273 / 1145 | 2277 / 1706 |
| `viewshed_20km` | cold | 300 | 1255.0 | 748 / 52.1 | 594 / 16.2 | 718 / 27.7 | 622 / 16.7 | **4518** / **571** | **4414** / **544** | 467 / 12.6 | 603 / 15.3 | 830 / 47.4 |
| `sample_10k_lattice` | cold | 100 | 418.3 | 34.2 / 8.16 | 27.9 / 6.04 | 36.6 / 7.99 | 42.6 / 9.59 | 136 / 20.1 | 164 / 22.2 | 31.6 / 7.22 | 28.5 / 6.68 | 34.1 / 7.63 |
| `visible_10_40km` | warm | 1 | 4.2 | 0.42 / 0.10 | 1.67 / 0.16 | 2.27 / 0.28 | 2.22 / 0.22 | **58.2** / **10.7** | **61.7** / **12.1** | 2.46 / 0.19 | 1.60 / 0.16 | 1.83 / 0.20 |
| `visible_10_40km/fresh` | cold | 20 | 83.7 | 0.43 / 0.11 | 22.0 / 5.80 | 13.0 / 4.16 | 22.8 / 5.61 | 78.7 / 16.8 | 79.4 / 17.4 | 17.6 / 5.12 | 20.0 / 5.32 | 23.2 / 6.88 |
| `radar_floor_20km` | cold | 300 | 1255.0 | 827 / 74.6 | 742 / 47.9 | 780 / 59.6 | **1316** / 105 | **4638** / **594** | **4511** / **565** | 802 / 65.0 | 746 / 49.3 | 928 / 78.3 |
| `exposure_10_sites` | cold | 3000 | 12549.6 | 7635 / 515 | 6274 / 187 | 6018 / 183 | 9822 / 519 | **15303** / 2106 | **13162** / 1618 | 6347 / 202 | 6857 / 266 | 9106 / 452 |
| `find_sites_20km` | cold | 500 | 2091.6 | 143 / 34.2 | 329 / 51.7 | 82.9 / 14.7 | 624 / 93.9 | 323 / 48.7 | 358 / 55.4 | 130 / 27.2 | 128 / 20.8 | 472 / 70.9 |
| `find_sites_los_36` | cold | 10000 | 41832.0 | 23305 / 5563 | **115441** / **27559** | 5205 / 1238 | 6378 / 1515 | **293156** / **69950** | **343299** / **81917** | 27025 / 6449 | 4956 / 1178 | 6181 / 1474 |
| `find_sites_horizon_first_touch` | cold | 5000 | 20916.0 | **36223** / 618 | 19243 / 463 | **99474** / 1670 | 10036 / 378 | 18695 / 391 | 18986 / 398 | 13962 / 423 | **25585** / 544 | **24702** / 577 |
| `route_50km` | cold | 500 | 2091.6 | 0.38 / 0.10 | 15.2 / 4.40 | 5.06 / 2.06 | 25.8 / 6.61 | 3.23 / 0.78 | 131 / 31.6 | 24.3 / 8.71 | 3.90 / 1.59 | 36.7 / 8.83 |
| `point_info/fresh` | cold | 20 | 83.7 | 2.43 / 0.60 | **345** / **52.9** | 58.9 / 13.7 | **765** / **118** | **104** / **22.6** | **103** / **21.6** | **87.5** / **20.1** | **133** / **24.9** | **608** / **95.9** |
| `airfield_info/fresh` | cold | 20 | 83.7 | 0.41 / 0.10 | 64.6 / 16.7 | 31.1 / 8.77 | 70.9 / 18.0 | **122** / **32.4** | **90.7** / **22.7** | 42.8 / 11.6 | 38.6 / 10.6 | 55.2 / 14.9 |
| `validate_1000` | cold | 3000 | 12549.6 | 401 / 95.8 | 2831 / 1322 | 3652 / 1635 | 6852 / **3003** | 3039 / 754 | 3705 / 915 | 1905 / 898 | 5178 / 1803 | 4140 / 2224 |
| `cli_describe` | warm | 50 | 209.2 | 21.5 / 5.87 | 58.7 / 14.8 | 49.7 / 12.6 | 82.4 / 20.5 | 46.0 / 11.9 | 46.8 / 12.0 | 49.8 / 12.6 | 57.3 / 14.5 | 74.9 / 18.6 |
| `cli_sample` | warm | 50 | 209.2 | 21.6 / 5.80 | 41.4 / 10.7 | 32.1 / 8.47 | 40.4 / 10.4 | 47.6 / 12.1 | 48.7 / 12.4 | 36.1 / 9.39 | 40.9 / 10.5 | 43.8 / 11.2 |
| `cli_visible` | warm | 50 | 209.2 | 22.3 / 5.96 | 45.0 / 11.2 | 37.5 / 9.19 | 46.8 / 11.2 | 101 / 20.5 | 111 / 22.8 | 41.0 / 9.77 | 43.7 / 10.9 | 47.3 / 11.6 |
| `index_empty_cache` | warm | 10000 | 41832.0 | 2.05 / 0.51 | 22438 / 8538 | 2822 / 1307 | **43971** / **10502** | 1779 / 453 | 1765 / 998 | 4645 / 2505 | 6308 / 3993 | 28793 / **10112** |
| `find_sites_horizon` | cold | 500 | 2091.6 | — | 131 / 32.3 | 69.8 / 16.6 | 178 / 44.2 | 238 / 33.5 | 278 / 36.9 | 191 / 47.9 | 68.2 / 16.1 | 170 / 41.8 |

`find_sites_horizon` has no `Synth` figure: `Synth` keeps no horizons, so each
touch there is the first.

## One call end to end

`cli_describe`, `cli_sample` and `cli_visible` above are the shipped binary
started once per request; each meets 50 ms on every terrain, the slowest
22.8 ms. A batch of 100 lines (50 `visible`, 50 `sample`) through one process,
warm, with its peak memory:

| Terrain | Mc (no bound) | ms | Peak MiB (target 64) |
|---|---|---|---|
| `Synth` | 55.8 | 14.0 | 2.6 |
| Afghanistan | 529 | 86.7 | 45.0 |
| Caucasus | 483 | 88.1 | 40.7 |
| GermanyCW | 1 274 | 204 | **65.6** |
| MarianaIslands | 3 074 | 669 | **100** |
| MarianaIslandsWWII | 3 535 | 752 | **125** |
| Nevada | 471 | 76.7 | 26.3 |
| PersianGulf | 386 | 75.0 | 21.0 |
| Syria | 794 | 135 | 48.7 |

## Why each miss

Windows has no sampling profiler that runs here without elevation (`wpr`
needs it), so these come from counters and span timers put into the code for
the measurement and taken out again.

| Miss | Measured cause |
|---|---|
| The budget of a parallel operation: `find_sites_horizon_first_touch`, `radar_floor_20km` on GermanyCW, `exposure_10_sites` on the Marianas | The budget is one core, the work ran on 2–14 cores' worth (cycles ÷ rate ÷ wall). Each wall is inside its target: 0.38–1.7 s of 5 s, 105 ms of 300 ms, 1.6–2.1 s of 3 s. |
| `visible_10_40km` on the Marianas, 14× | Blocks are 4.5 MiB against GermanyCW's 0.7, and each line turns over 5 of them in the 32 MiB block budget, read and prepared again. Over 200 lines the blocks read fall from 985 to 0 with 512 MiB, 13.8 → 0.9 ms a line. |
| `viewshed_20km`, `radar_floor_20km` on the Marianas, 3.5–3.7× | Not the block budget: unchanged from 16 MiB to 1 GiB. They spread over 1.9 cores against 3–9 elsewhere. Those two facts are as far as this was taken. |
| `find_sites_los_36`: Afghanistan 2.8×, MarianaIslands 7×, MarianaIslandsWWII 8× | One thread: cycles ÷ rate equals the wall. Reads do not matter: 42–87 blocks a request, the same at 32 and 256 MiB. It is sightlines × triangles: 811 k lines of 231 triangles on Afghanistan, 215 k of 620 on MarianaIslands, against 323 k of 41 on Caucasus. |
| `point_info/fresh`, 1.05–9× | The first nearest road. Of 114 ms on GermanyCW, 91 ms are the per-point step, 47 of them loading the 144 MB road graph; of 52 ms on Afghanistan, 42 and 23. Warm and cold agree, so it is CPU, not the disk. Held, `point_info` takes 0.08–5.5 ms. |
| `airfield_info/fresh` on the Marianas, 1.1–1.5× | Not split further. `data/airfield-info.md` has the runway width as the cost on those terrains. |
| `index_empty_cache` on GermanyCW, 1.05× | The road build: `design.md` §8 has 9.8 s of it, against 10.5 s here. Syria is 1 % over on the wall alone. |
| `validate_1000` on GermanyCW, wall 0.1 % over | Cold: half a core's worth of cycles over the wall, so most of the time is the disk's. 6.9 Gc against 12.5 Gc. |
| Memory of the 100-line batch | Not the block budget, as below: on the Marianas a batch holds 100–125 MiB whatever the budget. GermanyCW is 1.6 MiB over. Not split further. |

The block budget against the Mariana misses, MarianaIslands, warm, 2 rounds:

| Budget MiB | `visible_10_40km` Mc / ms | `viewshed_20km` Mc / ms | `exposure_10_sites` Mc / ms | 100-line batch Mc / ms | Batch peak MiB |
|---|---|---|---|---|---|
| 16 | 62.1 / 11.7 | 4 513 / 544 | 15 624 / 2 165 | 3 917 / 777 | 109 |
| 32 | 59.2 / 10.8 | 4 448 / 535 | 15 137 / 2 071 | 3 098 / 674 | 100 |
| 64 | 5.5 / 1.2 | 4 335 / 520 | 14 351 / 1 902 | 999 / 219 | 115 |
| 256 | 3.6 / 0.81 | 4 562 / 541 | 14 274 / 1 895 | 769 / 168 | 108 |
| 1024 | 3.7 / 0.80 | 4 392 / 526 | 14 327 / 1 898 | 767 / 168 | 107 |

## Decisions open

None of these is settled here, and no target has moved.

1. **Whether a cycle budget is one core.** The targets are stated in time.
   One core is the strict reading, and it marks parallel work as a miss that
   no user would see.
2. **The block budget.** 64 MiB takes the Mariana `visible` miss from 14× to
   1.3× and speeds the batch 3×; peak memory stays 100–115 MiB at every
   budget.
3. **`find_sites` with sightlines runs on one thread.** Three terrains miss
   10 s, the Marianas by 7–8×.
4. **`point_info` in a fresh process** cannot meet 20 ms where the road graph
   is large, while the first nearest road loads it.
5. **The 64 MiB memory target** on the Marianas.
