# Using the data

What every number the query operations return means, how to read it as
a mission designer, and how to turn a ranked list into an easy, medium,
hard or unfair mission. The operations and their exact request and
response fields are in `query-operations.md`; this document is
about judgement. Every distance is metres, every angle degrees, every
altitude either absolute (`alt`) or above ground (`alt_agl`), and
everything is terrain only: DCS's own line of sight ignores buildings
and has no trees, and so does this data (`design-and-facts.md`).

## One rule

The file never chooses. `find_sites` returns candidates ranked by a
weighted score, but the score is a convenience for sorting and the
`terms` array beside it is the real product. Selection happens in the
campaign, from the terms, against the player and the mission you are
building. Taking rank 1 every time produces the most defensible site,
which is usually the least interesting one to attack and the one the
player learns to expect.

## The metrics

### Geometry of the site itself

| Metric | Source | Reads as |
|---|---|---|
| `span` | `span_max` term | Height difference across the footprint. Under 1 m is a parking lot; 2 to 3 m is fine for a FARP; a helicopter pad tolerates 1 m. |
| `slope` | `slope_max` term, `slope` grid | Steepness in degrees. Vehicles park to 5°, a FARP wants 2°, infantry and MANPADS do not care to 20°. |
| `tpi_300`, `tpi_2000` | grids, `tpi_class_in` | Topographic position in metres above or below the mean height within the radius: positive on ridges and hilltops, negative in valleys and hollows, near zero on flats and mid-slopes. `tpi_2000` is the regional setting, `tpi_300` the local one; a site with `tpi_2000 < 0` and `tpi_300 > 0` is a knoll inside a valley, the classic hide-and-see position. `tpi_class_in` cuts both at ±10 m by default (`query-operations.md`, "Criteria"). |
| `enclosure` | grid, `enclosure_min` | Mean horizon elevation angle over all directions. Near 0° is an open plain; 5° is a broad valley; 15°+ is a gorge. High enclosure hides from the sides but also blinds a radar and traps a helicopter. |
| `horizon` bins | grid, `masked_from` | Horizon elevation per azimuth. A ridge in one bin masks that direction only. This is the direction-aware version of enclosure. |
| `dist_road`, `dist_rail`, `dist_water`, `dist_builtup` | grids, `*_band` and `*_min` terms | Straight-line distance to the nearest road, railway, water and built-up area. Road distance stands in for resupply plausibility and for how likely a ground patrol wanders past. |
| `scenery` counts by class, `clearing` | `scenery_max`, `scenery_in_footprint_max`, `clearing_max`, `scenery_in` | Buildings, industrial structures, walls and poles nearby. For a FARP the objects inside 150 m are what DCS deletes when it spawns; for a building static they are what it would sit inside. |

### Visibility between points

| Metric | Source | Reads as |
|---|---|---|
| `visible` | `visible` | Terrain line of sight between two points at stated altitudes. True means the terrain does not block it; whether a sensor detects anything is a separate question of range and DCS's own model. |
| `clearance_m` | `visible` | How much the sightline clears the highest terrain along it (negative when blocked). Small positive values (under 10 m) are marginal: a slightly different altitude, or DCS's interpolation, flips them. Treat ±10 m as "edge of the horizon". |
| `blocking` | `visible` | Where the terrain cuts the line. For a blocked SAM sightline it tells you which ridge is doing the work, which is the ridge the player will use. |
| `visible_fraction` | `viewshed` | The share of ground within radius that an observer at a height sees. From a hilltop at 2 m AGL, 0.6 is commanding; on a plain, 0.9+ is normal; in a valley, 0.1 to 0.3. |
| `covered_fraction` per band | `coverage` | Same idea from a weapon system's point of view, per target altitude band. A SAM's covered fraction at 100 m AGL is the low-level threat it poses; at 3 000 m AGL it is nearly always 1.0 on a flat world. |
| `dead_zones` | `coverage` | Polygons inside the engagement radius where the band is not seen. These are the routes in. Their count and size are the "how many ways in" number. |
| `asset_covered_fraction` | `coverage` with `asset` | How much of the thing being defended the site can actually see at each band. The difference between this and `covered_fraction` is whether the site is well placed or merely tall. |
| `blind_approach_exists` | `coverage` | True when at least one radial stays masked from the engagement radius inward to `min_masked_range`. This is the solvability switch for a low-level attack: false means there is no terrain-masked way in at that altitude, and only standoff, SEAD or altitude will do. |
| `radials[].unmask_dist`, `blind` | `approach_spectrum` | For every bearing around the site, the range at which a target at the band's altitude first comes into view, and whether that bearing stays masked all the way in to `min_masked_range`. This is the whole picture for a player who may come from anywhere: the site's exposure as a function of direction. |
| `blind_arc_fraction`, `widest_blind_arc_deg`, `blind_arcs` | `approach_spectrum` | Share of the compass with a masked way in, the widest single one, and where they are. A 40° arc is something a pilot finds by looking at the map; a 5° arc is a trick that only a briefing reveals. |
| `unmask_dist_min`, `_median`, `_max` | `approach_spectrum` | The spread of unmask distances over all bearings. Median is what an average approach gets; min is the sneakiest bearing; max is the worst one. A large spread means direction matters, which is what makes the site interesting. |
| `sector_summary` | `approach_spectrum` with `sector` | The same summaries restricted to the bearings the player is likely to use (the arc facing their airbase). Free choice is still shaped by where they take off. |
| `first_seen`, `first_seen_dist_from_site` | `unmask_profile` | Where along a given route the site first gets line of sight, and how far from the site that is. This is the number a player feels: 20 km is "I saw it on the RWR and had time"; 4 km is "it was already shooting". |
| `masked_segments` | `unmask_profile` | Route stretches below the site's horizon after the first unmask. They are the places to break lock, and the number of them is how forgiving the route is. |
| `length`, `straight_line`, per-leg `length` | `route` | Planar road distance between the snapped endpoints, from the road geometry DCS's own router returned (probe log: 23 257 m for Kutaisi city to airfield), and the endpoint distance for comparison. `length / straight_line` is the detour: 1.1 is a direct road, 2 is a valley or a river forcing a long way round. Time is length over the group's road speed; DCS ground groups move at 50 to 70 km/h on roads, so 23 km is 20 to 30 minutes. |
| `ascent`, `descent`, `max_grade_pct` | `route` | Cumulative climb and descent along the route from the height grid, and the steepest 100 m. A route with 600 m of ascent over 20 km is a mountain road; DCS slows ground groups on grades, so use ascent to widen the arrival window rather than assuming a flat speed. |
| `stretch`, `overlap_with`, `shared_edges_with_all` | `route_alternatives` | For each alternative route: how much longer than the shortest it is, how much of it coincides with each other alternative, and the edges every alternative is forced through. Stretch is the price of variety; shared edges are where a player can wait no matter which route is taken. |
| `exposure` per leg | `route` | For a ground route, the fraction of each leg any observer sees. Sum length × exposure for total exposed metres; a convoy is ambushable on the exposed legs and safe on the masked ones. |

### Network metrics

| Metric | Source | Reads as |
|---|---|---|
| `betweenness` | `chokepoints`, `road_edge` | How many shortest paths use an edge. High values with `crosses_water = 1` are bridges; high values with negative `tpi_300` are defiles. Cut one and traffic reroutes far. |
| `reachable`, `blocking` | `trafficability` | Whether a ground group can get from the nearest road to a point without exceeding a slope or crossing water, and where it fails. |

## Turning metrics into difficulty

Everything in this section is guidance for a campaign author. The band
tables are initial values, reasoned from the live Caucasus examples and
untested against sorties; nothing in the repository encodes them, and a
campaign that adopts them owns them.

Difficulty is a property of the pairing of a site with the way the
player approaches it. When the mission prescribes a route,
`unmask_profile` on that route is the measure. When the player is free
to come from any direction, which is the normal case in a campaign, the
measure is `approach_spectrum`: the site's exposure as a function of
bearing, summarised as how much of the compass offers a masked way in,
how wide the best gap is, and how the unmask distance varies around the
clock. The prescribed-route case is the spectrum read at one bearing.

Free choice is not uniform choice. A player takes off from somewhere,
and most approaches fall inside the 90° or so facing their base. So
compute the spectrum over all 360° for the structural picture, and
`sector_summary` over the arc from the player's likely origin (the
bearing from the site to their airbase or FARP, ±45°) for the practical
one. A site whose only blind arc faces away from the player's base is
hard in practice even if it is medium on paper; one whose blind arc
faces the base is easier than its full-circle numbers say.

The recipe is the same for every task:

1. Run `find_sites` with the hard constraints only (surface, slope,
   road access, spacing). Take the top 50, not the top 5.
2. For each candidate run `approach_spectrum` at the altitude bands the
   player may fly and the radius of the weapon that will defend it,
   with `sector` set from the player's origin. Add `coverage` with the
   asset when there is one to defend, and `viewshed` at the player's
   likely release point.
3. Classify each candidate into a band with the thresholds below.
4. Draw from the band matching the player's rating, at random. Never
   take the best in band, or the generator becomes predictable.
5. Record why: keep the metric vector with the placed object, so a
   debrief can say "you were seen at 6.2 km because you came in on 270
   and the only blind arc was 300 to 340".

Player rating moves the band. A simple scheme: start at medium; after a
clean success move up one band; after a loss move down one; never enter
unfair automatically, only by explicit mission design (a set-piece that
provides SEAD or a wingman).

### SAM against a fixed-wing strike, free approach

Metrics from `approach_spectrum` at the ingress altitudes the player
may use (say 60, 200 and 1 500 m AGL), radius = the SAM's engagement
range, `min_masked_range` = the player's weapon standoff, `sector` =
the player's origin arc; plus `dead_zones` count from `coverage` at
100 m AGL.

| Band | Blind arc fraction (all 360°) | Widest blind arc | Blind arc inside player's sector | Median unmask distance | Unmask at the lowest band |
|---|---|---|---|---|---|
| easy | ≥ 0.4 at 200 m AGL | ≥ 40° | yes, at 200 m AGL | ≥ 15 km | any |
| medium | 0.15 to 0.4 at 200 m AGL | 15° to 40° | yes, at ≤ 200 m AGL | 6 to 15 km | ≥ 4 km |
| hard | 0.05 to 0.15 at 60 m AGL only | 5° to 15° | yes only at 60 m AGL, or no but a ≥ 15° arc exists elsewhere | 2 to 6 km | ≥ 2 km |
| unfair | 0 at every allowed band | 0° | no | < 2 km | — |

Reading the table: easy means terrain hides the approach from a good
share of directions and the player can find one without help; medium
means the masked approaches are few and low, but at least one faces the
player; hard means the site is nearly all-round exposed except for a
narrow low-level slot, or the slot is on the far side so the player has
to route around; unfair means there is no terrain solution and the
mission must supply another one (SEAD, standoff, a decoy). On the Rioni
plain nearly every site is unfair against a low-level attack, which is
why the live SAM example's masking column decided everything: the one
site with a full western blind arc was the only medium site in 20 km.

The spread matters as much as the median. Two sites with median unmask
8 km, one with min 1 km and max 20 km, the other with min 7 and max
9 km: the first rewards map study and punishes a careless heading; the
second plays the same from everywhere. Give the first to a player who
reads briefings and the second to one who wants a straight fight.

### SAM or AAA against a helicopter attack

Helicopters do not fly corridors at altitude; they fly the terrain and
pop up. So the useful quantities are different, and all of them come
from the same operations run at helicopter altitudes.

- **Masked approach arcs at 15, 30 and 60 m AGL**: `approach_spectrum`
  on the threat with `alt_bands_agl = [15, 30, 60]`, `radius` = the
  threat's engagement range, and `min_masked_range` = the helicopter's
  weapon range. `blind_arc_fraction` at 30 m AGL is the solvability
  number for an Apache or Ka-50 with Hellfire or Vikhr; at 15 m it is
  the number for a Huey or Gazelle with rockets. `widest_blind_arc_deg`
  says whether the way in is obvious or a needle to thread, and
  `sector_summary` from the FARP's bearing says whether it is on the
  player's side.
- **Pop-up positions**: `find_sites` around the target with
  `not_visible_from(observers = [threat at its mast height],
  max_fraction = 0, own_alt_agl = 5)` and `visible_to(targets =
  [target], min_fraction = 1, own_alt_agl = 30)`. A cell that passes both
  is hidden at hover height and sees the target at pop-up height. Count
  them inside weapon range; their number is how many shots the player
  gets before they run out of new positions.
- **Pop-up margin**: at each pop-up position, `visible` from the threat
  to the cell at 30 m AGL. If the threat also sees the pop-up, the
  position is a duel; if not, it is a free shot. The fraction of free
  shots among pop-up positions is the second difficulty number.
- **Egress**: from the pop-up position, the nearest cell masked from the
  threat at 5 m AGL (`viewshed` from the threat at `target_alt_agl = 5`,
  take the nearest unseen cell). Under 300 m means one dash; over 1 km
  means a long exposed run.
- **LZ or FARP quality**, for insertions: `span` ≤ 1 m, `slope` ≤ 5°,
  `scenery_in_footprint_max = 0`, `clearing` count within 60 m = 0,
  and `not_visible_from(threats, own_alt_agl = 5)`.

| Band | Blind arc fraction, widest arc | Pop-up positions in range | Free shots | Egress |
|---|---|---|---|---|
| easy | ≥ 0.3 at 60 m AGL, widest ≥ 40° | ≥ 6 | ≥ 0.7 | ≤ 300 m |
| medium | ≥ 0.15 at 30 m AGL, widest ≥ 15° | 3 to 5 | 0.4 to 0.7 | ≤ 600 m |
| hard | > 0 at 15 m AGL only, or widest < 15° | 1 to 2 | < 0.4 | ≤ 1 km |
| unfair | 0 at ≤ 30 m AGL | 0 | — | — |

Worked example on the live Caucasus numbers in the design doc: the SAM
example's site 1 (−293887, 682059) on the Rioni plain is masked from the
west at 10 km by a 108 m rise 2 km from the site, and from the
south-west by a 369 m ridge at 6 km. Its spectrum at 15 m AGL has one
blind arc, roughly 225° to 315°, so `blind_arc_fraction` is about 0.25
and the widest arc about 90°: a Huey pilot who looks at the map sees
the rise and comes in behind it. For a Huey at 15 m AGL from Kutaisi
airfield (which lies to the north-east, so the blind arc is on the far
side and `sector_summary` from the airfield shows no blind arc: the
player has to swing around), `coverage` at 15 m AGL from that site has a
dead zone behind the rise; `find_sites` in the 3 km rocket ring
with the two LOS criteria above finds the cells on the far slope of the
rise, hidden at 5 m and seeing the site at 30 m; the free-shot fraction
there is high because the site is on the flat and the rise is between.
That is a medium site for a Huey with rockets and an easy one for an
Apache with Hellfire from 6 km. Move the same SAM 2 km west onto the rise
itself and the pop-up cells vanish: hard for the Apache, unfair for the
Huey, because at 15 m AGL nothing within 3 km is masked from a site on
the high ground.

### Hidden target for a search

Metrics: ground exposure (fraction of road cells within 3 km with LOS at
2 m AGL) and air exposure (fraction of a 1 km lattice at the search
altitude with LOS), from `not_visible_from` terms or a `viewshed` from
the target; building count within 150 m from `scenery_in`.

| Band | Ground exposure | Air exposure at 500 m AGL | Buildings in 150 m |
|---|---|---|---|
| easy | ≥ 0.5 | ≥ 0.6 | 0 to 2 (stands alone) |
| medium | 0.2 to 0.5 | 0.3 to 0.6 | 3 to 15 |
| hard | < 0.2 | 0.1 to 0.3 | ≥ 15 (one building among many) |
| unfair | < 0.05 | < 0.1 | — |

Air exposure below 0.1 at the search altitude means the target sits in
a hole the player cannot see into without overflying it; pair that with
a MANPADS and it is unfair, pair it with an intelligence marker and it
is a puzzle.

### Convoy for an interdiction

Metrics: total exposed metres from `route` legs against the player's
likely orbit; `chokepoints` along the route; `masked_segments` of the
route from the player's perspective (run `unmask_profile` with the
player's orbit as the site and the convoy route at 2 m AGL).

| Band | Exposed fraction of route | Chokepoints on route | Longest masked stretch |
|---|---|---|---|
| easy | ≥ 0.6 | ≥ 2 bridges or defiles | < 2 km |
| medium | 0.3 to 0.6 | 1 | 2 to 5 km |
| hard | < 0.3 | 0 | > 5 km |
| unfair | < 0.1 | 0 | route entirely in a gorge |

### A recurring convoy that should not be predictable

A supply run from depot to base that always takes the shortest road is
a fixed target after the player has seen it once. `route_alternatives`
returns `k` routes that differ from each other by at least
`1 − max_overlap` of their length and are no more than `max_stretch`
longer than the shortest, each with the same per-leg exposure and
chokepoint data as a single route, plus `shared_edges_with_all`: the
edges every alternative must use.

How to use them:

- **Plan once, rotate per departure.** Keep the alternative set with
  the movement in the campaign overlay and pick a route each time the
  convoy leaves. Weighted random, never the same route twice running.
  Re-plan when a bridge is destroyed, a threat appears inside a route's
  box, or the overlay changes.
- **Weight by difficulty, not uniformly.** Each alternative has its own
  exposed fraction and chokepoint count, so the set spans the band
  table above. An easy setting draws mostly the exposed route; a hard
  setting draws the masked ones; the rotation itself is what makes a
  hard setting fair, because a player who studied one route is not
  wrong, just early.
- **`shared_edges_with_all` is the mission.** If all alternatives cross
  one bridge, the bridge is where the interdiction happens regardless
  of rotation. Report it in the planning output: a designer either
  accepts it as the intended chokepoint, or adds a ford or a second
  bridge to the campaign state to open the map up.
- **Stretch is time.** Each alternative carries its own `length`,
  `ascent` and `max_grade_pct`, so the arrival window is computed, not
  guessed: a 1.5× route takes 1.5× as long on the flat and more with
  climb. If the campaign needs the
  supplies at a fixed time, cap `max_stretch` at 1.2 and accept fewer
  alternatives.
- **Overlap is the tell.** Two routes that share 40 % of their length
  share the exposed legs on that 40 %. A player who ambushes the shared
  part catches both; the campaign can reward that by making the shared
  segment the higher-exposure one, or punish it by choosing a `k` whose
  routes only share the depot and the base.

Which to draw is a campaign decision; the file supplies the set and the
numbers to weight it. Rotation among three routes with 20 % to 40 %
overlap and stretch under 1.3 is a good default for a supply line the
player will see many times.

### FARP or forward base for the player's own side

Here "difficulty" is about the campaign's logistics rather than the
player's danger. Rank by `terms` and pick by policy: close to the front
(`dist_airdrome_band` with `airdrome_id` set to the friendly airfield,
distance to the enemy line as a custom box), low ground exposure for concealment, and
`clearing` count as the price in destroyed civilian buildings the
campaign is willing to pay. A FARP with `clearing = 0`, exposure 0.4 and
a road 250 m away (candidate 2 in the design doc's FARP example) is the
prudent choice; a FARP in a town with `clearing = 14` is faster to reach
by road and easier to find.

## Filtering patterns

- **Hard constraint, then band, then draw.** Never filter on the
  weighted score; it hides which term drove the rank.
- **Two candidates that differ in one term** are the interesting pair
  for a designer: same coverage, different unmask distance, is a pure
  difficulty dial.
- **Quantiles, not thresholds, across theatres.** The bands above are
  initial values reasoned from the Caucasus examples, not measured
  calibrations; on a plain like Sinai's Nile delta or a gorge
  like Afghanistan's Panjshir, compute the metric over the top 50 and
  split by quantile so every theatre still yields all four bands.
- **Check `clearance_m` on any decision that depends on one `visible`
  call.** Under 10 m either way, re-evaluate at ±5 m altitude before
  calling it masked or seen.
- **Spectrum for free approach, profile for a briefed route.** When
  the player chooses the heading, classify on `approach_spectrum` and
  its `sector_summary` from their origin; when the mission gives a
  route, run `unmask_profile` on the route they will actually get, not
  on a straight line, because the masked segments are where a waypoint
  can reward a good pilot.
- **Blind arcs are the debrief.** The arc list is a sentence: "there
  was a masked approach between 300 and 340". Keep it with the placed
  object and show it after the mission whether or not the player found
  it.
- **Keep the metric vector with the placed object** in the campaign
  overlay, both for debriefs and so the next mission can reuse or avoid
  the same terrain trick.
