# The targets on every installed terrain

Every target of `design.md` §8 against the shipped build, on the 8 terrains
of 2.9.30.28738 (`autoupdate.cfg` timestamp `20261006-075422`) and `Synth`.
Measured 2026-10-30 on a Ryzen 7 7800X3D (8 cores, 16 threads), 32 GB, NVMe,
Windows 11, DCS not running, nothing else at work. The two Mariana columns are
at the 64 MiB block budget; the rest were measured at 32 MiB, and their
verdicts recomputed against the budgets and targets of ADR 0007.
`find_sites_los_36` was measured at 64 MiB, over the pool, on every terrain.

**The bound is on cycles; the wall is beside it for a reader.** Each figure
is cycles, then wall clock. The target is the design's time; its budget in
cycles is that time on one core at the counter's rate, or on 8 for a parallel
case (Decided, below).

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
- **The bounds** are `mise run bench-bounds`, run by hand and not a CI check:
  shared runners differ from run to run, and the same work on the horizon's
  first touch cost 24.2 Gc on one and 36.9 Gc, 3.02× its one-core budget of the day, on the next,
  so bounds are judged on one machine. They take 9 rounds on `Synth` and fail
  above 3× the budget, above 3× the memory target, on a failed request, or on no cycles over
  10 ms of work, since a shorter case can read zero under the CPU clock's tick.
  They catch a large regression, not a small one: `Synth` is small, so only
  the horizon's first touch and `find_sites_los_36` stand within 15× of their
  bounds, and `open` or `route_50km` lie thousands of
  times under theirs.
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

A parallel case can therefore spend far more than its time on one core:
`viewshed_20km` on `Synth` spends 748 Mc in 52 ms. It can also meet its
budget and still be over its time, so the wall is read as well. The reverse
holds too: on all 16 threads a case can meet its time and be over a budget
of 8 cores, as `find_sites_los_36` is on the Marianas.

**Under noise.** `Synth` with 8 spinner processes against the idle machine, 5
rounds: cycles moved 0.77–1.32× (1.77× on one sub-millisecond case,
`airfield_info/fresh`) while wall moved up to 1.88×. The horizon's first
touch moved 0.98× in cycles and 1.88× in wall, `viewshed_20km` 1.02× and
1.37×. Nothing crossed a bound. The tightest case at rest is
`find_sites_horizon_first_touch`, 36.2 Gc of its 502 Gc bound. That run
predates the sightlines going parallel.

## The table

Mc (millions of cycles) / ms. The bound is on the Mc. Bold marks a figure
over the target; a bold wall alone is a target met on cycles and over on the
clock.

| Operation | OS cache | Target ms | Budget Mc | synth | Afghanistan | Caucasus | GermanyCW | MarianaIslands | MarianaIslandsWWII | Nevada | PersianGulf | Syria |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `open` | cold | 20 | 83.7 | 0.02 / 0.01 | 19.0 / 4.57 | 8.22 / 1.98 | 16.0 / 3.84 | 11.4 / 2.74 | 11.5 / 2.75 | 11.3 / 2.71 | 16.4 / 3.94 | 18.4 / 4.41 |
| `describe` | cold | 20 | 83.7 | 0.07 / 0.02 | 8.13 / 1.95 | 7.58 / 1.82 | 11.6 / 2.78 | 6.96 / 1.67 | 6.94 / 1.66 | 7.50 / 1.80 | 8.02 / 1.93 | 11.0 / 2.64 |
| `sample_10k_scattered` | cold | 3000 | 12549.6 | 52.3 / 12.5 | 2006 / 1598 | 1366 / 1040 | 3653 / 1794 | 378 / 95.0 | 397 / 98.8 | 1305 / 1239 | 1273 / 1145 | 2277 / 1706 |
| `viewshed_20km` | cold | 300 | 10039.7 | 748 / 52.1 | 594 / 16.2 | 718 / 27.7 | 622 / 16.7 | 4375 / **541** | 4456 / **553** | 467 / 12.6 | 603 / 15.3 | 830 / 47.4 |
| `sample_10k_lattice` | cold | 100 | 418.3 | 34.2 / 8.16 | 27.9 / 6.04 | 36.6 / 7.99 | 42.6 / 9.59 | 133 / 19.9 | 159 / 20.6 | 31.6 / 7.22 | 28.5 / 6.68 | 34.1 / 7.63 |
| `visible_10_40km` | warm | 1 | 4.2 | 0.42 / 0.10 | 1.67 / 0.16 | 2.27 / 0.28 | 2.22 / 0.22 | **5.80** / **1.12** | **8.87** / **1.95** | 2.46 / 0.19 | 1.60 / 0.16 | 1.83 / 0.20 |
| `visible_10_40km/fresh` | cold | 20 | 83.7 | 0.43 / 0.11 | 22.0 / 5.80 | 13.0 / 4.16 | 22.8 / 5.61 | 77.5 / 16.5 | 78.6 / 17.1 | 17.6 / 5.12 | 20.0 / 5.32 | 23.2 / 6.88 |
| `radar_floor_20km` | cold | 300 | 10039.7 | 827 / 74.6 | 742 / 47.9 | 780 / 59.6 | 1316 / 105 | 4499 / **569** | 4501 / **569** | 802 / 65.0 | 746 / 49.3 | 928 / 78.3 |
| `exposure_10_sites` | cold | 3000 | 100396.8 | 7635 / 515 | 6274 / 187 | 6018 / 183 | 9822 / 519 | 14337 / 1933 | 12159 / 1432 | 6347 / 202 | 6857 / 266 | 9106 / 452 |
| `find_sites_20km` | cold | 500 | 2091.6 | 143 / 34.2 | 329 / 51.7 | 82.9 / 14.7 | 624 / 93.9 | 320 / 49.6 | 344 / 52.6 | 130 / 27.2 | 128 / 20.8 | 472 / 70.9 |
| `find_sites_los_36` | cold | 10000 | 334656.0 | 70263 / 1801 | 152329 / 2533 | 8590 / 183 | 14409 / 267 | **381490** / 6317 | **443244** / 7332 | 46239 / 803 | 9613 / 181 | 14695 / 275 |
| `find_sites_horizon_first_touch` | cold | 5000 | 167328.0 | 36223 / 618 | 19243 / 463 | 99474 / 1670 | 10036 / 378 | 18655 / 391 | 18913 / 398 | 13962 / 423 | 25585 / 544 | 24702 / 577 |
| `route_50km` | cold | 500 | 2091.6 | 0.38 / 0.10 | 15.2 / 4.40 | 5.06 / 2.06 | 25.8 / 6.61 | 3.15 / 0.76 | 51.1 / 12.2 | 24.3 / 8.71 | 3.90 / 1.59 | 36.7 / 8.83 |
| `point_info/fresh` | cold | 150 | 627.5 | 2.43 / 0.60 | 345 / 52.9 | 58.9 / 13.7 | **765** / 118 | 96.1 / 20.7 | 96.4 / 20.5 | 87.5 / 20.1 | 133 / 24.9 | 608 / 95.9 |
| `airfield_info/fresh` | cold | 20 | 83.7 | 0.41 / 0.10 | 64.6 / 16.7 | 31.1 / 8.77 | 70.9 / 18.0 | **123** / **33.1** | **86.0** / **21.2** | 42.8 / 11.6 | 38.6 / 10.6 | 55.2 / 14.9 |
| `validate_1000` | cold | 3000 | 12549.6 | 401 / 95.8 | 2831 / 1322 | 3652 / 1635 | 6852 / **3003** | 3067 / 761 | 3732 / 922 | 1905 / 898 | 5178 / 1803 | 4140 / 2224 |
| `cli_describe` | warm | 50 | 209.2 | 21.5 / 5.87 | 58.7 / 14.8 | 49.7 / 12.6 | 82.4 / 20.5 | 44.0 / 11.3 | 45.2 / 11.6 | 49.8 / 12.6 | 57.3 / 14.5 | 74.9 / 18.6 |
| `cli_sample` | warm | 50 | 209.2 | 21.6 / 5.80 | 41.4 / 10.7 | 32.1 / 8.47 | 40.4 / 10.4 | 48.7 / 12.5 | 48.3 / 12.3 | 36.1 / 9.39 | 40.9 / 10.5 | 43.8 / 11.2 |
| `cli_visible` | warm | 50 | 209.2 | 22.3 / 5.96 | 45.0 / 11.2 | 37.5 / 9.19 | 46.8 / 11.2 | 102 / 20.7 | 105 / 21.5 | 41.0 / 9.77 | 43.7 / 10.9 | 47.3 / 11.6 |
| `index_empty_cache` | warm | 10000 | 41832.0 | 2.05 / 0.51 | 22438 / 8538 | 2822 / 1307 | **43971** / **10502** | 1772 / 449 | 1437 / 369 | 4645 / 2505 | 6308 / 3993 | 28793 / **10112** |
| `find_sites_horizon` | cold | 500 | 2091.6 | — | 131 / 32.3 | 69.8 / 16.6 | 178 / 44.2 | 243 / 34.4 | 271 / 35.1 | 191 / 47.9 | 68.2 / 16.1 | 170 / 41.8 |

`find_sites_los_36` on `Synth` is 53 082 Mc / 911 ms when it runs alone. Run
after another parallel case in one process it holds 9 threads busy against 14,
repeatably; this was not split further. Afghanistan and MarianaIslands do not
move in the same order.

`find_sites_horizon` has no `Synth` figure: `Synth` keeps no horizons, so each
touch there is the first.

## One call end to end

`cli_describe`, `cli_sample` and `cli_visible` above are the shipped binary
started once per request; each meets 50 ms on every terrain, the slowest
22.8 ms. A batch of 100 lines (50 `visible`, 50 `sample`) through one process,
warm, with its peak memory:

| Terrain | Mc (no bound) | ms | Peak MiB (target 150) |
|---|---|---|---|
| `Synth` | 55.8 | 14.0 | 2.6 |
| Afghanistan | 529 | 86.7 | 45.0 |
| Caucasus | 483 | 88.1 | 40.7 |
| GermanyCW | 1 274 | 204 | 65.6 |
| MarianaIslands | 1 011 | 221 | 115 |
| MarianaIslandsWWII | 1 078 | 235 | 118 |
| Nevada | 471 | 76.7 | 26.3 |
| PersianGulf | 386 | 75.0 | 21.0 |
| Syria | 794 | 135 | 48.7 |

## Why each miss

Windows has no sampling profiler that runs here without elevation (`wpr`
needs it), so these come from counters and span timers put into the code for
the measurement and taken out again.

| Miss | Measured cause |
|---|---|
| The wall of a parallel operation: `viewshed_20km`, `radar_floor_20km` on the Marianas, 1.8–1.9× | Within the 8-core budget. Not the block budget: unchanged from 16 MiB to 1 GiB. They spread over 1.9 cores against 3–9 elsewhere. Those two facts are as far as this was taken. |
| `visible_10_40km` on the Marianas, 1.4–2.1× | Blocks are 4.5 MiB against GermanyCW's 0.7, so each line still turns over blocks at 64 MiB. Over 200 lines the blocks read fell from 985 at 32 MiB to 0 with 512 MiB, 13.8 → 0.9 ms a line. |
| `find_sites_los_36` on the Marianas, 1.14–1.32× on cycles alone | The budget counts 8 cores and the work runs on all 16 threads: cycles ÷ rate ÷ wall is 14.4 threads busy, and SMT siblings count in full, so the same answer costs 1.3× the cycles of one thread (293.7 and 344.5 Gc) in an eleventh of the wall. It is sightlines × triangles: 215 k lines of 620 triangles, against 323 k of 41 on Caucasus. |
| `point_info/fresh` on GermanyCW, 1.2× on cycles alone | The first nearest road. Of 114 ms on GermanyCW, 91 ms are the per-point step, 47 of them loading the 144 MB road graph; of 52 ms on Afghanistan, 42 and 23. Warm and cold agree, so it is CPU, not the disk. Held, `point_info` takes 0.08–5.5 ms. |
| `airfield_info/fresh` on the Marianas, 1.03–1.5× | Not split further. `data/airfield-info.md` has the runway width as the cost on those terrains. |
| `index_empty_cache` on GermanyCW, 1.05× | The road build: `design.md` §8 has 9.8 s of it, against 10.5 s here. Syria is 1 % over on the wall alone. |
| `validate_1000` on GermanyCW, wall 0.1 % over | Cold: half a core's worth of cycles over the wall, so most of the time is the disk's. 6.9 Gc against 12.5 Gc. |

The block budget against the Mariana misses, MarianaIslands, warm, 2 rounds:

| Budget MiB | `visible_10_40km` Mc / ms | `viewshed_20km` Mc / ms | `exposure_10_sites` Mc / ms | 100-line batch Mc / ms | Batch peak MiB |
|---|---|---|---|---|---|
| 16 | 62.1 / 11.7 | 4 513 / 544 | 15 624 / 2 165 | 3 917 / 777 | 109 |
| 32 | 59.2 / 10.8 | 4 448 / 535 | 15 137 / 2 071 | 3 098 / 674 | 100 |
| 64 | 5.5 / 1.2 | 4 335 / 520 | 14 351 / 1 902 | 999 / 219 | 115 |
| 256 | 3.6 / 0.81 | 4 562 / 541 | 14 274 / 1 895 | 769 / 168 | 108 |
| 1024 | 3.7 / 0.80 | 4 392 / 526 | 14 327 / 1 898 | 767 / 168 | 107 |

## Decided

ADR 0007 has the reasoning of 1, 4 and 5.

1. **A cycle budget** is the target's time on 8 cores for `viewshed_20km`,
   `radar_floor_20km`, `exposure_10_sites`, `find_sites_los_36` and
   `find_sites_horizon_first_touch`, whose answers are computed over the pool
   on any source, and on one core for every other case. Where only an
   install's reads of blocks fan out, as in `sample`, `visible` or
   `point_info`, the case stays on one core. So does `index_empty_cache`:
   two steps of its road build fan out, and it averaged at most one core.
2. **The block budget** is 64 MiB, from the table above. The rerun at 64 MiB
   took `visible_10_40km` warm from 14× to 1.4× on MarianaIslands and to 2.1×
   on MarianaIslandsWWII.
3. **`find_sites` with sightlines** runs over the pool, each site on a free
   thread and the sites gathered in candidate order. The wall fell 6–11× and
   every terrain meets 10 s, Afghanistan at 2.5 s and the Marianas at 6.3 and
   7.3 s; the Marianas are over the 8-core cycle budget, for the reason above.
   A pool thread takes a line's blocks one at a time, and one thread reads at
   a time. Over one 12 km square of 36 observers on MarianaIslands, each thread
   reading its own held 295 MiB; this holds 112 MiB against 84 on one thread,
   in the same 5.0 s. Answers on Afghanistan and the Marianas match
   `--threads 1` byte for byte.
4. **`point_info` in a fresh process** has a target of 150 ms. Held, it keeps
   its target.
5. **The memory target** of a query process is 150 MiB. The Mariana cause
   stays not split further.
