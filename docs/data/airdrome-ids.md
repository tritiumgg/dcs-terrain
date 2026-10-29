# Airdrome ids by position: against the shipped missions

Measured on 2.9.30.28738 (`autoupdate.cfg` timestamp 20261006-075422),
Windows 11, DCS not running, a release build of
`cargo test --release -p dcsterrain-core --test airdrome_ids -- --nocapture`
with `DCSTERRAIN_INSTALL` set. Kola's folder holds no `entry.lua`, so the
install lists eight terrains. No `.crypt` file was opened.

## Method

Where `Radio.lua`'s comment labels an airdrome with no id, the tool joins one
by position (ADR 0005). It is a spatial join followed by record linkage with
a unanimity rule:

1. **Beacons.** Each `airfieldN_k` beacon votes for the airdrome whose
   reference point (the mean of its runway ends) is nearest it, within 15 km,
   or for none when none is that near or two are equally near.
2. **Towers.** Each `Radio.lua` station `airfieldN_k` names its scene object,
   `t:<id>`, an instance id of the terrain's `.scn5`; the scene places it.
   It votes for the airdrome whose box of runway ends and stands lies
   nearest it, within 1 km of the box, on the same terms; an object the
   scene does not place votes for none.
3. **Unanimity.** `N` goes to an airdrome only when every station of `N`
   votes for it, no other `N` does, and no airdrome or `Radio.lua` label
   holds `N` already. The label wins over both joins, and the beacons over
   the towers.

The nearest-neighbor search is an exact linear scan: at most 227 airdromes
and a few hundred stations per terrain, so an index (a k-d tree or a grid)
would cost more to build than the scan. Measured: both joins take at most
1.4 ms on a terrain, and placing the towers at most 10 ms, through the
scene's cached index (`scene.nodes`), which scenery builds anyway. Parsing
the scene file's head instead took up to 354 ms and hundreds of MiB. A
scene no decoder reads costs the tower ids; a scene a decoder reads but
finds malformed refuses the tables, as it refuses scenery.

Error bound. A station that votes for the wrong airdrome or for none costs
`N` its id; it never gives a wrong one, unless every station of `N` lies
nearest the same wrong airdrome. One station of `N` far from the rest is
enough to drop `N`. A unit test holds the join to a brute-force oracle over
500 random cases, ties included.

The oracle is the `airdromeId` of every route point in the 2 062 `.miz`
missions under the install, each point given to the airdrome whose
reference point lies nearest it within 5 km; an airdrome's id by the
missions is the one most of its points give. The missions are a check,
never a source.

## Per terrain

| Terrain | Airdromes | With a runway | Label | Beacons | Tower | None | None with a runway | Named by missions | Agree | Disagree |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| Afghanistan | 26 | 26 | 26 | 0 | 0 | 0 | 0 | 25 | 25 | 0 |
| Caucasus | 21 | 21 | 21 | 0 | 0 | 0 | 0 | 21 | 21 | 0 |
| GermanyCW | 227 | 118 | 119 | 0 | 0 | 108 | 0 | 55 | 54 | 1 |
| MarianaIslands | 8 | 5 | 5 | 0 | 0 | 3 | 0 | 5 | 5 | 0 |
| MarianaIslandsWWII | 11 | 11 | 11 | 0 | 0 | 0 | 0 | 5 | 3 | 2 |
| Nevada | 17 | 17 | 0 | 6 | 3 | 8 | 8 | 15 | 9 | 0 |
| PersianGulf | 30 | 30 | 0 | 27 | 1 | 2 | 2 | 27 | 26 | 0 |
| Syria | 225 | 80 | 81 | 0 | 0 | 144 | 2 | 42 | 42 | 0 |

"Airdromes" counts every taxiway file; the airdromes without a runway are
helipads. Every id the joins give agrees with the missions where they name
the airdrome: Nevada 9 of 9, PersianGulf 26 of 26; Tonb-e Kochak (16) and
Lavan Island (26) take an id no mission confirms. The towers add Boulder City
(6), Henderson (8) and Laughlin (10) on Nevada, and Sir Abu Nuair (17) on
PersianGulf. On this build no id on another terrain changes: the joins
fill none there, since every airdrome with a runway but two on Syria has a
label. The test holds that a joined id never contradicts a mission; that
the joins fill none elsewhere is this measurement, not an assertion.

The three disagreements are labels, each from a single route point:
Obermehler Schlotheim (label 51, one point 167) on GermanyCW, Gurguan Point
(label 5, one point 3) and Isley Field (label 6, one point 2) on
MarianaIslandsWWII. They are left as `Radio.lua` gives them.

Still none on Nevada: Echo Bay (missions 7), Jean (9), Lincoln County (11),
Mina (14), Pahute Mesa (16), Tonopah (17), Beatty and Mesquite (no mission).
On PersianGulf: Greater Tunb (missions 8) and Quasoura (no mission). No
station of theirs votes for them unanimously, if they have one at all.

## Held out

The joins run again over every airdrome a label names, as if none were
labeled, and each id they give is held to its label: a hold-out check of
the joins on the 263 airdromes where the truth is known.

| Terrain | Labeled | By beacons, equal to the label | By tower, equal | Not equal |
|---|---:|---:|---:|---:|
| Afghanistan | 26 | 7 | 19 | 0 |
| Caucasus | 21 | 18 | 3 | 0 |
| GermanyCW | 119 | 70 | 48 | 0 |
| MarianaIslands | 5 | 4 | 1 | 0 |
| MarianaIslandsWWII | 11 | 0 | 11 | 0 |
| Syria | 81 | 34 | 47 | 0 |

262 of 263 labeled airdromes take their label back by position, none another
id; one on GermanyCW takes none.

The tower reach was set from the same measurement: every labeled airdrome's
tower lies within 520 m of its own box, and a box of another
airdrome lies at least 1.5 km from it except where a helipad shares the
field, where the nearer box is still the right one.
