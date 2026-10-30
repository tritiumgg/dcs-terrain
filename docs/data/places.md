# Places on every installed terrain

How many places of each size class and form does each terrain make, what
does the `places` entry cost to build, what do `towns` with outlines and
`point_info`'s place cost, and how well do places rank against real towns'
census populations? Measured 2026-10-30 on DCS build 2.9.30.28738
(`autoupdate.cfg` timestamp `20261006-075422`), Windows 11 Pro 10.0.26200,
release build, with other builds and test runs sharing the machine: times
are upper bounds. Cycles are the process's CPU cycles over the same span,
across every thread.

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
  point's cell its place's when a member's circle, grown by 100 m, holds
  the cell's center. The test fails on any difference. The time is the whole
  request, its ground, road, town and airfield fields included.
- **Census, Caucasus only:** every town of the terrain's list carrying a name
  that has a census figure, Georgia's of 2014 (Geostat) and Russia's of
  1 October 2021 (Rosstat): 72 towns. Abkhazia and South Ossetia are left
  out, having no census either office stands behind, and so is a name the
  list holds twice. Each town takes the place it names; Spearman's rank
  correlation, ties at their mean rank, between census population and the
  place's residential floor. The test fails under 0.6.

## Size classes and forms

| terrain | buildings placed | places | hamlet | village | large_village | town | city | large_city | compact | dispersed | agglomeration | build s / Gcycles | read ms |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Afghanistan | 162386 | 12666 | 12314 | 265 | 87 | 0 | 0 | 0 | 12583 | 12 | 71 | 22.1 / 18.1 | 1.2 |
| Caucasus | 491443 | 5103 | 3098 | 1500 | 334 | 117 | 52 | 2 | 5012 | 1 | 90 | 2.9 / 3.1 | 0.9 |
| GermanyCW | 6961120 | 21788 | 5933 | 9873 | 4124 | 1622 | 208 | 28 | 21787 | 1 | 0 | 28.3 / 48.1 | 1.7 |
| MarianaIslands | 3194 | 242 | 235 | 5 | 2 | 0 | 0 | 0 | 235 | 0 | 7 | 1.2 / 0.8 | 0.6 |
| MarianaIslandsWWII | 1505 | 123 | 116 | 7 | 0 | 0 | 0 | 0 | 120 | 0 | 3 | 0.5 / 0.4 | 0.5 |
| Nevada | 357523 | 602 | 433 | 106 | 43 | 19 | 0 | 1 | 587 | 3 | 12 | 0.9 / 3.3 | 0.7 |
| PersianGulf | 804231 | 7006 | 5771 | 789 | 144 | 267 | 28 | 7 | 6960 | 14 | 32 | 7.6 / 8.3 | 0.8 |
| Syria | 4830018 | 15084 | 8834 | 3743 | 70 | 2279 | 132 | 26 | 15051 | 2 | 31 | 24.7 / 47.1 | 1.3 |

Kola: its folder has no `entry.lua`, so it is no terrain to open. Build is
the `places` entry alone, the scene index and tables read before it; read
is the entry decoded from the cache. GermanyCW lists 127 towns, each far from the next, so no place
there holds two names.

The grouping matches the settlements research's prototype on Caucasus
exactly: 3 098 hamlets and 1 500 villages at the 200 m gap.

## Queries

| terrain | towns, 20 largest outlined, ms / Mcycles | outline vertices | point_info, 1 000 points in their boxes, ms / Mcycles | in a place |
|---|---|---|---|---|
| Afghanistan | 182 / 762 | 9650 | 477 / 2645 | 515 |
| Caucasus | 111 / 467 | 35458 | 781 / 3640 | 439 |
| GermanyCW | 2299 / 9626 | 59914 | 2733 / 14998 | 513 |
| MarianaIslands | 17 / 71 | 1742 | 426 / 2552 | 704 |
| MarianaIslandsWWII | 6 / 27 | 842 | 274 / 1771 | 796 |
| Nevada | 799 / 3347 | 26418 | 236 / 1262 | 523 |
| PersianGulf | 624 / 2601 | 38864 | 1717 / 7747 | 497 |
| Syria | 3997 / 16733 | 47330 | 1071 / 5260 | 473 |

Every one of the 8 000 points' places equals the brute force's. The
`point_info` times are the whole request, warm, after the road graph is
read: 0.2–2.7 ms a point, `point_info`'s own cost without places
(`point-info.md`). Outlining reads each place's box again: about 0.1 s for
a GermanyCW or Syria city of tens of thousands of buildings, which is why a
request outlines at most 100.

## Census

Spearman's rank correlation by residential floor is **0.629** over the 70
towns that name a place (by building count 0.570); Anapa and Gardabani name
none. By country, 0.77 over the 40 Russian towns and 0.58 over the 30
Georgian. The Georgian under-ranking is the western lowlands: Samtredia,
Martvili, Abasha and Khoni each name a place of thousands of buildings,
villages chained into one by the 200 m gap, which holds four to nine town
names. Mtskheta and Chiatura are modeled thinly. The settlements research
measured 0.73 over 47 towns, a list it did not keep.

| town | census | residential floor m² | buildings | names | size | form |
|---|---|---|---|---|---|---|
| TBILISI | 1108717 | 56684669 | 9351 | 5 | large_city | agglomeration |
| KRASNODAR | 1099344 | 43703685 | 4327 | 3 | large_city | agglomeration |
| SOCHI | 448250 | 5878499 | 894 | 1 | city | compact |
| VLADIKAVKAZ | 295830 | 16290242 | 3106 | 2 | city | agglomeration |
| NOVOROSSIYSK | 262293 | 9489844 | 1977 | 1 | city | compact |
| NAL'CHIK | 247054 | 15849631 | 3210 | 2 | city | agglomeration |
| BATUMI | 152839 | 14828498 | 2631 | 8 | city | agglomeration |
| KUTAISI | 147635 | 16014561 | 3038 | 3 | city | agglomeration |
| PYATIGORSK | 146473 | 10612758 | 3667 | 2 | city | agglomeration |
| MAYKOP | 143385 | 10611727 | 2233 | 2 | city | agglomeration |
| KISLOVODSK | 127521 | 4824085 | 1489 | 2 | city | agglomeration |
| RUSTAVI | 125103 | 5067234 | 459 | 1 | city | compact |
| Nazran | 122350 | 4495158 | 1666 | 1 | city | compact |
| ESSENTUKI | 119658 | 5220175 | 1383 | 3 | city | agglomeration |
| CHERKESSK | 113226 | 10061171 | 3654 | 2 | city | agglomeration |
| GELENDZHIK | 80204 | 1074608 | 520 | 1 | town | compact |
| MINERAL'NYE VODY | 70485 | 6161440 | 1990 | 4 | city | agglomeration |
| GEORGIEVSK | 63221 | 11503639 | 3114 | 2 | city | agglomeration |
| SLAVYANSK-NA-KUBANI | 62985 | 13664257 | 1591 | 2 | city | agglomeration |
| Sunzha | 62078 | 1393953 | 367 | 1 | town | compact |
| TUAPSE | 61571 | 1864050 | 1607 | 4 | town | agglomeration |
| PROHLADNIY | 59938 | 866633 | 1073 | 1 | large_village | dispersed |
| BELORECHENSK | 55870 | 8676147 | 988 | 1 | city | compact |
| KRYMSK | 54597 | 10334575 | 1172 | 1 | city | compact |
| GORI | 48143 | 1699608 | 816 | 2 | town | agglomeration |
| KARABULAK | 43037 | 316159 | 421 | 1 | large_village | compact |
| ZUGDIDI | 42998 | 15931992 | 2934 | 4 | city | agglomeration |
| TEMRYUK | 41608 | 3492617 | 476 | 1 | city | compact |
| POTI | 41465 | 9684667 | 1234 | 1 | city | compact |
| Goryachiy Klyuch | 40903 | 5206243 | 897 | 1 | city | compact |
| APSHERONSK | 40289 | 3018478 | 848 | 1 | city | compact |
| UST'-LABINSK | 40158 | 4758655 | 559 | 1 | city | compact |
| BAKSAN | 39593 | 1757895 | 1329 | 1 | town | compact |
| ABINSK | 39511 | 3586665 | 598 | 1 | city | compact |
| MOZDOK | 36784 | 3864375 | 1037 | 1 | city | compact |
| MALGOBEK | 36480 | 320393 | 426 | 1 | large_village | compact |
| GORYACHEVODSKIY | 36032 | 705827 | 891 | 1 | large_village | compact |
| BESLAN | 35929 | 1270091 | 978 | 1 | town | compact |
| NARTKALA | 33203 | 1618936 | 953 | 2 | town | agglomeration |
| UST'-DZHEGUTA | 31137 | 2496353 | 787 | 1 | town | compact |
| KOBULETI | 27546 | 2919021 | 599 | 1 | town | compact |
| INOZEMTSEVO | 27500 | 995926 | 586 | 1 | town | compact |
| KHASHURI | 26135 | 3013584 | 500 | 1 | city | compact |
| SAMTREDIA | 25318 | 10980686 | 5986 | 9 | town | agglomeration |
| ZHELEZNOVODSK | 22863 | 470819 | 616 | 1 | large_village | compact |
| LERMONTOV | 22444 | 260611 | 355 | 2 | large_village | agglomeration |
| TYRNYAUZ | 22056 | 310092 | 213 | 1 | large_village | compact |
| SENAKI | 21596 | 5401094 | 1324 | 4 | city | agglomeration |
| ALAGIR | 21550 | 65092 | 92 | 1 | village | compact |
| ZESTAFONI | 20814 | 663262 | 253 | 1 | town | compact |
| NOVOPAVLOVSK | 20490 | 2034460 | 1029 | 1 | town | compact |
| Marneuli | 20211 | 670595 | 480 | 1 | large_village | compact |
| TEREK | 19948 | 421868 | 580 | 1 | large_village | compact |
| OZURGETI | 14785 | 3689248 | 830 | 1 | city | compact |
| KASPI | 13423 | 1150865 | 587 | 1 | town | compact |
| CHIATURA | 12803 | 104934 | 69 | 1 | village | compact |
| TSHALTUBO | 11281 | 3593129 | 1242 | 2 | city | agglomeration |
| Sagarejo | 10871 | 2576015 | 1228 | 1 | town | compact |
| TKIBULI | 9770 | 408154 | 294 | 1 | town | compact |
| HONI | 8987 | 7888768 | 2110 | 3 | city | agglomeration |
| MTSKHETA | 7940 | 166588 | 12 | 1 | hamlet | compact |
| KARELI | 6654 | 90449 | 131 | 1 | village | compact |
| LANCHHUTI | 6395 | 4186261 | 750 | 1 | city | compact |
| Dusheti | 6167 | 369655 | 275 | 1 | large_village | compact |
| SACHHERE | 6140 | 501982 | 208 | 1 | town | compact |
| ABASHA | 4941 | 6699477 | 1758 | 4 | city | agglomeration |
| MARTVILI | 4425 | 8918811 | 4639 | 8 | town | agglomeration |
| DZHVARI | 4361 | 694622 | 234 | 1 | town | compact |
| ONI | 2656 | 44294 | 63 | 1 | village | compact |
| AMBROLAURI | 2047 | 533337 | 223 | 1 | town | compact |
