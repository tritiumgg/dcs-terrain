# Places on every installed terrain

How many places of each size class and form does each terrain make, what
does the `places` entry cost to build, what do `towns` with outlines and
`point_info`'s place cost, and how well do places rank against real towns'
census populations? Measured 2026-11-02 on DCS build 2.9.31.29434
(`autoupdate.cfg` timestamp `20261027-141234`), Windows 11 Pro 10.0.26200,
release build, one test at a time on an otherwise idle machine, but with the
terrain files mostly out of the OS cache: judge by cycles, the process's CPU
cycles across every thread. Times are upper bounds.

## Method

The opt-in test `every_installed_terrain_records_its_places` in
`dcsterrain-core/tests/places.rs`, run with `DCSTERRAIN_INSTALL` set and a
scratch cache:

```sh
mise exec -- cargo test --release -p dcsterrain-core --test places -- --nocapture --test-threads=1
```

The install holds 9 terrain folders; Kola's has no `entry.lua`, so 8 are
terrains. For each:

- **Build:** the tables and the scene index first, then the `places` entry
  alone: the walk of every object and the grouping, on one thread at low
  priority. Then a fresh process-like open reads it back.
- **`towns`:** the 20 places with most buildings, outlined.
- **`point_info`:** 1 000 points drawn uniformly over those 20 places'
  boxes, the same each run. Every answer's place is checked against a
  brute force: each place near the point read and linked in full, and the
  point's cell its place's when the circle of a member that counts, grown by
  100 m, holds the cell's center. The test fails on any difference. The time
  is the whole request, its ground, road, town and airfield fields included.
- **Census, Caucasus only:** every town of the terrain's list carrying a name
  that has a census figure, Georgia's of 2014 (Geostat) and Russia's of
  1 October 2021 (Rosstat): 72 towns. Abkhazia and South Ossetia are left
  out, having no census either office stands behind, and so is a name the
  list holds twice. Each town takes the place it names; Spearman's rank
  correlation, ties at their mean rank, between census population and the
  place's floor. The test fails under 0.6.

## Size classes and forms

| terrain | buildings placed | places | hamlet | village | large_village | town | city | large_city | compact | dispersed | agglomeration | build s / Gcycles | read ms | bytes |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Afghanistan | 1694531 | 27552 | 23693 | 3059 | 643 | 132 | 20 | 5 | 27378 | 64 | 110 | 201.3 / 32.0 | 2.1 | 2151816 |
| Caucasus | 388011 | 4948 | 3221 | 1352 | 123 | 182 | 67 | 3 | 4855 | 0 | 93 | 4.3 / 3.7 | 0.8 | 392544 |
| GermanyCW | 6656905 | 19372 | 3952 | 9568 | 4082 | 1514 | 227 | 29 | 19372 | 0 | 0 | 50.9 / 53.2 | 1.3 | 1511524 |
| MarianaIslands | 44235 | 218 | 181 | 19 | 9 | 6 | 3 | 0 | 207 | 0 | 11 | 1.2 / 1.0 | 0.6 | 17268 |
| MarianaIslandsWWII | 8132 | 503 | 484 | 14 | 4 | 1 | 0 | 0 | 498 | 1 | 4 | 0.6 / 0.4 | 0.5 | 39330 |
| Nevada | 429604 | 502 | 328 | 79 | 51 | 34 | 8 | 2 | 489 | 0 | 13 | 1.5 / 2.8 | 0.7 | 39796 |
| PersianGulf | 1616799 | 13625 | 10828 | 2015 | 390 | 310 | 72 | 10 | 13589 | 0 | 36 | 12.0 / 15.8 | 1.3 | 1064118 |
| Syria | 4342260 | 13962 | 8370 | 3366 | 1159 | 901 | 140 | 26 | 13929 | 3 | 30 | 32.4 / 48.2 | 1.1 | 1093400 |

Today's rule, by name, before this one, on the same build:

| terrain | places | hamlet | village | large_village | town | city | large_city | entry bytes |
|---|---|---|---|---|---|---|---|---|
| Afghanistan | 12666 | 12314 | 265 | 87 | 0 | 0 | 0 | 1243674 |
| Caucasus | 5103 | 3098 | 1500 | 345 | 111 | 47 | 2 | 507157 |
| GermanyCW | 21788 | 5933 | 9873 | 4124 | 1622 | 208 | 28 | 2136224 |
| MarianaIslands | 242 | 235 | 5 | 2 | 0 | 0 | 0 | about 24 000 |
| MarianaIslandsWWII | 123 | 116 | 7 | 0 | 0 | 0 | 0 | about 12 000 |
| Nevada | 602 | 433 | 106 | 43 | 19 | 0 | 1 | about 60 000 |
| PersianGulf | 7006 | 5771 | 789 | 144 | 267 | 28 | 7 | about 690 000 |
| Syria | 15084 | 8834 | 3743 | 70 | 2279 | 132 | 26 | about 1 480 000 |

Towns of the list that name a place, today and now: Afghanistan 40.3 → 57.5 %,
Caucasus 97.6 → 97.5, GermanyCW 99.2 → 98.4, MarianaIslands 71.4 → 83.1,
MarianaIslandsWWII 90.9 → 100, Nevada 70.8 → 73.1, PersianGulf 82.9 → 88.3,
Syria 94.9 → 94.6. A town that names a place in both builds keeps its class or
moves one: Caucasus 98 %, GermanyCW 100, Syria 100, Nevada 97, PersianGulf 78.
These shares were scored before the last name rules, which move only models
of the Marianas, Nevada, PersianGulf and Afghanistan.

## Why these rules

Each from a scratch build of the variants on all 8 terrains.

- **Class and box alone fail.** Census 0.58, under the bar; `rock1` was 45 % of
  Guam's largest place, reefs up to 445 × 920 m gave 62 objects 321 M m²,
  `solar_power_plant_01` made an unnamed place of 28 264 objects on Nevada, and
  `greenhouse1` made 61 towns or larger on Syria. Hence the exclusions.
- **60 m².** At 30 m² Kabul is 1.1 M objects across 49 × 45 km and Helmand's farm
  belt an unnamed place of 200 000. At 60 it is 292 000. The figure is a judgment
  from one terrain, and costs Afghanistan 4 % of its named towns.
- **Industrial buildings only link.** Left out altogether they break towns: 58
  Caucasus towns fell two classes or more, Dzhukmur 606 buildings to 40. Linking
  brought back 35 of the 58 and lost none, and hamlets 4 327 → 3 222. It costs
  the census: 0.661 → 0.631, still over the bar, under the 0.644 of the rule by
  name, because more villages chain.
- **The 200 m gap stays.** 150 m gives 0.53 and 100 m 0.50, and neither stops
  Kabul chaining.
- **An airfield mask is not needed.** 800 m about runways and 300 m about stands
  left the census alone and moved 7 or fewer places of large village or above on
  each terrain. Without the industrial class little of a base remains.
- **Size by floor.** By count GermanyCW has 1 343 cities, Afghan houses being
  several models each. Floor per person, single-name towns: Caucasus median 73
  m² against census; against rough populations from general knowledge,
  Afghanistan 57, PersianGulf 87, Syria 99, GermanyCW 155. So about 100 m² a
  person, and 5 000, 30 000 and 200 000 people make the lines 0.5, 3 and 20 M m².
  None is fit to a terrain's class counts. GermanyCW and Syria keep 29 and 26
  large cities, and Syria's towns, 2 279 by a 5 % apartment test the box split
  passed too often, come to 901. 57 of the 70 census towns read town or larger.

## Models for a screenshot

Named rules took what a name says plainly, on every terrain: rocks and reefs,
`american_truck`, `mv_krait`, `pipe_construction`, `power_trans_hub_*`, dishes,
`chimney`, airfield telebridges, and as military `military_installation_*`,
`undegraund_shelter*`, `airfield_building_tech_*`, `k_troop_housing_*`, `b_hut_*`.
Still counted, unjudged, house-sized, with objects placed:

| terrain | model | objects | box m | why |
|---|---|---|---|---|
| PersianGulf, Nevada | `concreet_blocks` | 24 358, 2 520 | 11 × 7 × 3–5 | a stack of blocks or a shed |
| PersianGulf, Nevada | `shader_01`–`04`, `shader_big` | about 1 000 | up to 123 × 24 | sunshades, by the name; 180 make an unnamed town by Liwa_Airbase |
| Afghanistan | `herat_canopy_small` | 2 241 | 14 × 13 × 10 | a roof or a building |
| Afghanistan | `herat_storage_01`, `02` | 811 | 33 × 13 × 13 | a base depot or a town's |
| PersianGulf, Nevada | `constr_build_*` | 1 454 | 41 × 42 × 15 | a site, not yet a building |
| Syria, GermanyCW | `construction*`, `constraction_big_*` | 3 123 | up to 31 × 88 × 53 | a site, not yet a building |
| PersianGulf, Nevada | `apex_concreate_*` | about 550 | up to 145 × 89 | a pad with a box |
| Syria | `cg_building07`, `triangle1` | 1 886 | 39 × 55, 41 × 41 | no name tells |
| Afghanistan | unnamed block at x 33302, z 265924 | 27 832 | 5 × 5 km | a city no town names, 11 km from Mohammad Agha |

## Queries

| terrain | towns, 20 largest outlined, ms / Mcycles | outline vertices | point_info, 1 000 points in their boxes, ms / Mcycles | in a place |
|---|---|---|---|---|
| Afghanistan | 5962 / 10592 | 47102 | 1402 / 6946 | 495 |
| Caucasus | 113 / 447 | 41382 | 369 / 1835 | 385 |
| GermanyCW | 6565 / 11484 | 62172 | 4059 / 14316 | 435 |
| MarianaIslands | 95 / 397 | 9588 | 584 / 3706 | 592 |
| MarianaIslandsWWII | 14 / 55 | 1548 | 321 / 1987 | 663 |
| Nevada | 1249 / 2752 | 26380 | 246 / 1269 | 505 |
| PersianGulf | 2381 / 5444 | 42804 | 772 / 3844 | 414 |
| Syria | 7809 / 17374 | 54148 | 1203 / 5715 | 433 |

The 201 s on Afghanistan is the disk: an earlier run took 19 s and the same 24–32
Gcycles. Every one of the 8 000 points' places equals the brute force's. Outlining
reads each place's whole box, so its cost goes with the largest places: Afghanistan
took 762 Mcycles before Kabul existed and takes 10 592 now.

Against this rule's parent commit, three terrains, three rounds, warm and cold,
median of a request; `towns` is 50 places, unoutlined:

| case | now ms / Mcycles | before ms / Mcycles |
|---|---|---|
| Caucasus `towns` warm | 0.11 / 0.45 | 0.13 / 0.52 |
| Caucasus `towns` cold | 0.11 / 0.43 | 0.13 / 0.49 |
| GermanyCW `towns` warm | 0.20 / 0.82 | 0.23 / 0.95 |
| GermanyCW `towns` cold | 0.19 / 0.78 | 0.25 / 1.03 |
| Afghanistan `towns` warm | 0.26 / 1.08 | 0.23 / 0.93 |
| Afghanistan `towns` cold | 0.26 / 1.06 | 0.21 / 0.87 |
| Caucasus `point_info` warm | 1.20 / 5.0 | 1.33 / 5.6 |
| GermanyCW `point_info` warm | 5.6 / 23.9 | 5.8 / 25.3 |
| Afghanistan `point_info` warm | 3.0 / 13.5 | 1.2 / 6.6 |
| Afghanistan `point_info` cold | 4.0 / 13.4 | 2.4 / 6.3 |

`point_info` on Afghanistan doubles: a point now stands among buildings it used
to miss. It is still far under its 20 ms.

Peak commit, by the benchmark's `DCSTERRAIN_BENCH_PEAK`, MiB, `towns` then
`point_info`: Afghanistan 13.7, 22.5; Caucasus 5.5, 15.4; GermanyCW 11.8, 31.9;
MarianaIslands 6.5, 19.3; MarianaIslandsWWII 6.4, 20.1; Nevada 5.9, 11.0;
PersianGulf 10.7, 25.5; Syria 10.8, 26.9. `towns` with 20 outlines, peak paged
memory by a poll of the process: Afghanistan 51.4 (20.8 before), GermanyCW 53.5,
Syria 54.0. All under the 64 MiB limit.

The cache: `places` grows on Afghanistan, 1.24 → 2.15 MB, and on MarianaIslandsWWII
and PersianGulf, and shrinks on the rest, a row being 20 bytes shorter.

## Census

Spearman's rank correlation by floor is **0.631** over the 70 towns that name a
place (by building count 0.571); Anapa and Gardabani name none.

| town | census | floor m² | buildings | names | size | form |
|---|---|---|---|---|---|---|
| TBILISI | 1108717 | 70559376 | 8024 | 5 | large_city | agglomeration |
| KRASNODAR | 1099344 | 50987717 | 3723 | 3 | large_city | agglomeration |
| SOCHI | 448250 | 7845508 | 794 | 1 | city | compact |
| VLADIKAVKAZ | 295830 | 21338954 | 2557 | 2 | large_city | agglomeration |
| NOVOROSSIYSK | 262293 | 11091064 | 1498 | 1 | city | compact |
| NAL'CHIK | 247054 | 19146652 | 2451 | 2 | city | agglomeration |
| BATUMI | 152839 | 18034024 | 2081 | 8 | city | agglomeration |
| KUTAISI | 147635 | 18172248 | 2266 | 3 | city | agglomeration |
| PYATIGORSK | 146473 | 13585323 | 3434 | 3 | city | agglomeration |
| MAYKOP | 143385 | 12890626 | 1811 | 2 | city | agglomeration |
| KISLOVODSK | 127521 | 6212151 | 1243 | 2 | city | agglomeration |
| RUSTAVI | 125103 | 6203579 | 419 | 1 | city | compact |
| Nazran | 122350 | 5597821 | 1250 | 1 | city | compact |
| ESSENTUKI | 119658 | 6213480 | 1084 | 3 | city | agglomeration |
| CHERKESSK | 113226 | 11676749 | 2762 | 2 | city | agglomeration |
| GELENDZHIK | 80204 | 1344541 | 450 | 1 | town | compact |
| MINERAL'NYE VODY | 70485 | 7877990 | 1665 | 5 | city | agglomeration |
| GEORGIEVSK | 63221 | 14508889 | 2591 | 2 | city | agglomeration |
| SLAVYANSK-NA-KUBANI | 62985 | 16443411 | 1330 | 2 | city | agglomeration |
| Sunzha | 62078 | 1218485 | 242 | 1 | town | compact |
| TUAPSE | 61571 | 2529987 | 1094 | 4 | town | agglomeration |
| PROHLADNIY | 59938 | 1500972 | 826 | 1 | town | compact |
| BELORECHENSK | 55870 | 9720033 | 835 | 1 | city | compact |
| KRYMSK | 54597 | 12421228 | 1001 | 1 | city | compact |
| GORI | 48143 | 2160409 | 625 | 2 | town | agglomeration |
| KARABULAK | 43037 | 591752 | 342 | 1 | town | compact |
| ZUGDIDI | 42998 | 18448923 | 2214 | 4 | city | agglomeration |
| TEMRYUK | 41608 | 3752152 | 374 | 1 | city | compact |
| POTI | 41465 | 11539760 | 890 | 1 | city | compact |
| Goryachiy Klyuch | 40903 | 6000587 | 724 | 1 | city | compact |
| APSHERONSK | 40289 | 3422075 | 650 | 1 | city | compact |
| UST'-LABINSK | 40158 | 5485644 | 436 | 1 | city | compact |
| BAKSAN | 39593 | 2529920 | 1095 | 1 | town | compact |
| ABINSK | 39511 | 4275485 | 507 | 1 | city | compact |
| MOZDOK | 36784 | 4905937 | 892 | 1 | city | compact |
| MALGOBEK | 36480 | 575709 | 346 | 1 | town | compact |
| GORYACHEVODSKIY | 36032 | 13585323 | 3434 | 3 | city | agglomeration |
| BESLAN | 35929 | 1833686 | 745 | 1 | town | compact |
| NARTKALA | 33203 | 2022462 | 796 | 2 | town | agglomeration |
| UST'-DZHEGUTA | 31137 | 2864304 | 638 | 1 | town | compact |
| KOBULETI | 27546 | 3774129 | 499 | 1 | city | compact |
| INOZEMTSEVO | 27500 | 1228710 | 480 | 1 | town | compact |
| KHASHURI | 26135 | 2104368 | 236 | 1 | town | compact |
| SAMTREDIA | 25318 | 13520696 | 4679 | 9 | city | agglomeration |
| ZHELEZNOVODSK | 22863 | 728367 | 526 | 1 | town | compact |
| LERMONTOV | 22444 | 422446 | 299 | 2 | large_village | agglomeration |
| TYRNYAUZ | 22056 | 262713 | 187 | 1 | village | compact |
| SENAKI | 21596 | 5863507 | 882 | 3 | city | agglomeration |
| ALAGIR | 21550 | 75883 | 80 | 1 | village | compact |
| ZESTAFONI | 20814 | 4553530 | 669 | 1 | city | compact |
| NOVOPAVLOVSK | 20490 | 2134334 | 705 | 1 | town | compact |
| Marneuli | 20211 | 192400 | 150 | 1 | village | compact |
| TEREK | 19948 | 670325 | 477 | 1 | town | compact |
| OZURGETI | 14785 | 3860310 | 578 | 1 | city | compact |
| KASPI | 13423 | 1583417 | 502 | 1 | town | compact |
| CHIATURA | 12803 | 417299 | 212 | 1 | large_village | compact |
| TSHALTUBO | 11281 | 4104702 | 1051 | 2 | city | agglomeration |
| Sagarejo | 10871 | 4037125 | 1071 | 1 | city | compact |
| TKIBULI | 9770 | 697340 | 186 | 1 | village | compact |
| HONI | 8987 | 8721306 | 1492 | 3 | city | agglomeration |
| MTSKHETA | 7940 | 196044 | 12 | 1 | hamlet | compact |
| KARELI | 6654 | 229574 | 109 | 1 | village | compact |
| LANCHHUTI | 6395 | 4652077 | 523 | 1 | city | compact |
| Dusheti | 6167 | 451499 | 213 | 1 | large_village | compact |
| SACHHERE | 6140 | 669101 | 157 | 1 | village | compact |
| ABASHA | 4941 | 7281775 | 1281 | 4 | city | agglomeration |
| MARTVILI | 4425 | 11023231 | 3519 | 8 | city | agglomeration |
| DZHVARI | 4361 | 854647 | 157 | 1 | village | compact |
| ONI | 2656 | 52558 | 53 | 1 | village | compact |
| AMBROLAURI | 2047 | 804243 | 199 | 1 | village | compact |
