# ADR 0010: Tell places by clusters of buildings, and size them by floor

## Status

Accepted

## Context

**Affects:** `design.md` §6.3 and §6.4, the risks on class boundaries;
`queries.md`, `towns`; `plan.md`, C1, whose override file per terrain goes away.

A place was made of buildings whose family, read from the model's name, was
house, apartment, shop, civic or industrial, and its size came from residential
floor and the share of apartments. A building no name tells is `unknown`, and did
not settle. On 2.9.31.29434, `unknown` is 99.5 % of MarianaIslands' `building`
objects, 98.6 % of Afghanistan's, 93.3 % of MarianaIslandsWWII's, 70.9 % of
PersianGulf's, 32.7 % of Nevada's, and 0.1 % or less on Syria, GermanyCW and
Caucasus. Afghanistan placed 162 386 of its 9.26 M objects, in no place above a
large village: Kabul was 498 buildings. C1 meant to fix this with an override
file per terrain, but only 1 145 of 33 016 models appear on two or more terrains,
so that work would not carry from one terrain to the next.

An offline experiment built places from class and box alone, then with each
exclusion below (`data/places.md`). Class and box alone rank Caucasus towns at
0.58 against the census, under the 0.6 bar, and rocks, reefs, solar panels and
greenhouses read as towns. Only Caucasus has census proof: every other terrain
was judged by whether its named towns land in a place of a plausible size.

## Decision

A place is a cluster of buildings, told by the cluster. Any model with a family
counts, `unknown` included, unless it is of class `industrial`, of family farm,
outbuilding or military, or its box is under 60 m² or under 2 m tall; no terrain
has an override file for this. A class `industrial` model of that size only
links: it joins buildings across the 200 m gap and adds no count, floor, hectare,
id, name or outline. A place's size is the total floor of the buildings that
count: a town from 0.5 M m², a city from 3 M m², a large city from 20 M m². These
are 5 000, 30 000 and 200 000 people at about 100 m² each. Apartment share and
residential floor go away, and the `towns` reply carries `floor` in place of
`houses`, `apartments`, `shops`, `civic`, `industrial`, `residential_floor` and
`apartment_share`, which would read mostly 0 where families cannot name a
terrain's models. Its sort `residential_floor` becomes `floor`, and
`apartment_share` goes.

### Alternatives considered

- **An override file per terrain.** Hours of screenshots a terrain, for names that
  rarely carry to the next.
- **Class and box alone.** Fails the census and reads rocks as towns.
- **Leaving industrial buildings out altogether.** Census 0.661, the best figure,
  but 58 Caucasus towns fall two classes or more, because their factories held
  them together. Linking recovers 35 of the 58 and loses none, at a census of
  0.631; today's rule by name scores 0.644.
- **Size by building count.** Afghan houses are several part models each, so
  counts do not compare across terrains: GermanyCW would have 1 343 cities.
- **Size thresholds matched to Caucasus's class counts.** Halved the large cities
  of GermanyCW and Syria. The people lines keep 29 and 26.
- **An airfield mask.** Left the census alone and moved 7 or fewer places of large
  village or above on each terrain, once industrial models count for nothing.
- **A narrower gap.** 150 m ranks at 0.53 and does not stop Kabul chaining.

## Consequences

Afghanistan and the Marianas have towns and cities, and one rule serves every
terrain. Size classes are in people, not in the terrain's own units, so they read
the same across terrains.

The census falls from 0.644 to 0.631, still over the bar, and only Caucasus has
that test. About 100 m² a person is a rough figure: rough populations from general
knowledge give medians of 57 (Afghanistan) to 155 (GermanyCW), and 73 against
Caucasus's census. Syria's towns fall from 2 279 to 901 as the apartment test goes.
The 60 m² is a judgment from Afghanistan.

The names did not go: farm, outbuilding and military stay, and each new
non-building found needs a rule, though one that holds for every terrain. Family
stays in the catalog. The `towns` reply changes shape.

A town now costs more where it did not exist: outlining Afghanistan's 20 largest
places is 14 times the cycles, and `point_info` there twice. Peak commit stays
under 64 MiB.

A census for a second terrain, or screenshots of the models `data/places.md` lists
for one, would reopen the figures.
