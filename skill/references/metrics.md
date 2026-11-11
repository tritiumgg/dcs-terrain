# What the numbers mean

How to read each metric as a mission designer, and how to turn a ranked list
into an easy, medium, hard or unfair mission. Everything is terrain only: DCS's
own sightline ignores buildings and trees, and so does this data.

## One rule

The tool never chooses. `find_sites`'s score is a convenience for sorting, and the
`terms` beside it are the product. Choosing happens in the campaign, against the
player and the mission. Rank 1 every time gives the most defensible site, usually
the least interesting to attack, and one the player learns to expect.

## The site itself

| Metric | Source | Reads as |
|---|---|---|
| span | `span_max` term | Height difference across the footprint. Under 1 m is a parking lot; 2 to 3 m is fine for a FARP; a helicopter pad tolerates 1 m |
| slope | `slope_max` or `slope_band` term, `slope` layer | Degrees. Vehicles park to 5°, a FARP wants 2°, infantry and MANPADS do not care to 20°. `slope_band` is the whole footprint as one plane, with the way it falls and how rough it is about it: under 0.5 m is smooth ground |
| `tpi_300`, `tpi_2000` | layers, `tpi_class_in` | Meters above or below the mean height within the radius: positive on ridges, negative in valleys, near zero on flats and mid-slopes. `tpi_2000` is the regional setting, `tpi_300` the local one. `tpi_2000 < 0` with `tpi_300 > 0` is a knoll inside a valley, the classic hide-and-see position |
| `enclosure` | layer, `enclosure_min` | Mean horizon angle in all directions. Near 0° is an open plain, 5° a broad valley, 15° or more a gorge. High enclosure hides from the sides, but it blinds a radar and traps a helicopter |
| `horizon_*` | layers, `masked_from` | Horizon angle per direction. A ridge in one bin masks that direction only |
| `dist_road`, `dist_rail`, `dist_water`, `dist_builtup` | layers, band criteria | Straight-line distance. Road distance stands in for resupply and for how likely a patrol wanders past |
| scenery counts, clearing | `scenery_max`, `scenery_in_footprint_max`, `clearing_max`, `scenery_in` | Objects nearby. For a FARP, those close by are what DCS removes when it spawns; for a building static, what it would sit inside |

## Sightlines

| Metric | Source | Reads as |
|---|---|---|
| `visible` | `visible` | Whether the terrain blocks the line. Whether a sensor detects anything is another question, of range and DCS's own model |
| `clearance_m` | `visible` | The least gap between the line and the ground anywhere, the ends included, so it never exceeds the lower end's height above its ground. Negative when blocked. Under 10 m either way with both ends well above that is the edge of the horizon: try ±5 m of altitude before calling it masked or seen |
| `blocking` | `visible` | Where the ground first cuts the line: the ridge the player will use. It is the first ground above the line, which may be near the site, not the crest that does most of the masking |
| `visible_fraction` | `viewshed` | The share of ground within the radius an observer sees. From a hilltop at 2 m, 0.6 is commanding; on a plain, 0.9 or more is normal; in a valley, 0.1 to 0.3 |
| `covered_fraction` per band | `coverage` | The same per target altitude. At 100 m AGL it is the low-level threat a SAM poses; at 3 000 m it is nearly always 1 |
| `dead_zones` | `coverage` | Polygons inside the radius not seen at that band: the routes in. Their count and size say how many ways in there are |
| `asset_covered_fraction` | `coverage` with `asset` | How much of the defended thing the site sees. Against `covered_fraction`, it tells a well placed site from a merely tall one |
| `blind_approach_exists` | `coverage` | Some radial stays masked from the radius in to `min_masked_range`. False means no terrain-masked way in at that altitude: only standoff, SEAD or altitude will do |
| `radials[].unmask_dist`, `blind` | `approach_spectrum` | Per bearing, the range a target at that band first comes into view, and whether it stays masked in to `min_masked_range`. The whole picture for a player who may come from anywhere |
| `blind_arc_fraction`, `widest_blind_arc_deg`, `blind_arcs` | `approach_spectrum` | The share of the compass with a masked way in, the widest, and where they are. A 40° arc is found by looking at the terrain; a 5° arc only a briefing reveals |
| `unmask_dist_min`, `_median`, `_max` | `approach_spectrum` | The median is an average approach, the min the sneakiest, the max the worst. A wide spread means direction matters, which makes a site interesting |
| `sector_summary` | `approach_spectrum` with `sector` | The same over the bearings the player is likely to use |
| `first_seen_dist_from_site` | `unmask_profile` | The number a player feels: 20 km is "I saw it on the RWR and had time"; 4 km is "it was already shooting" |
| `masked_segments` | `unmask_profile` | Stretches hidden from the site: places to break lock |
| `floor_agl` | `radar_floor`, `exposure` | The lowest height above ground at which a target is seen there: how low the player must fly |
| `seen_count`, `unseen_fraction` | `exposure` | How many sites see a cell, and the share of a band no site sees: gaps in an air defense |

## Roads

| Metric | Source | Reads as |
|---|---|---|
| `length`, `straight_line` | `route` | Their ratio is the detour: 1.1 is a direct road, 2 a valley or river forcing the long way. Time is length over road speed; ground groups do roughly 50 to 70 km/h, so 23 km is 20 to 30 minutes |
| `ascent`, `descent`, `max_grade_pct` | `route` | 600 m of ascent over 20 km is a mountain road. DCS slows ground groups on grades, so widen the arrival window |
| `exposure` per leg | `route` | The share of a leg the observers see. A convoy is ambushable on exposed legs |
| `offroad_from`, `offroad_to` | `route` with `offroad` | Each end's way across country: its `length`, its `cost` in meters of open flat ground (a forest meter costs 4), its climb and `exposure`. A way that costs twice its length is slow going; the share of the route's `length` off the road is the share driven at cross-country speed |
| `stretch`, `overlap_with`, `shared_edges_with_all` | `route_alternatives` | How much longer than the shortest, what it shares with each other route, and the edges all must use. Stretch is the price of variety; shared edges are where a player can wait whichever route is taken |
| `betweenness` | `chokepoints` | How many shortest paths use an edge. Cut a high one and traffic reroutes far |
| `reachable`, `way`, `straight` | `trafficability` | Whether a group can drive between the road and a point round steep slopes and water, the way it takes (its `length`, climb and where it `meets` the road), and where the straight line fails |

## Turning metrics into difficulty

The bands below are starting values, reasoned from Caucasus examples and untested
against sorties. Say so if you use them. On a very flat or very broken terrain,
compute the metric over the top 50 and split by quantile, so every terrain still
gives all four bands.

Difficulty belongs to the pairing of a site with how the player approaches it.
When the mission prescribes a route, measure with `unmask_profile` on that route,
not a straight line. When the player chooses, measure with `approach_spectrum`,
over all 360° for the structure and with `sector` for the practice: the bearing
from the site to the player's base, ±45°. A site whose only blind arc faces away
from the player's base is hard however medium it reads.

The recipe:

1. `find_sites` with the hard constraints only. Take the top 50, not the top 5.
2. For each, `approach_spectrum` at the altitudes the player may fly and the radius
   of the defending weapon, with `sector`. Add `coverage` with the asset, if any.
3. Put each candidate in a band.
4. Draw at random from the band matching the player. Never take the best in band,
   or the generator becomes predictable.
5. Keep the metric vector with the placed object: a debrief can then say "you were
   seen at 6.2 km because you came in on 270, and the only blind arc was 300 to
   340".

### SAM against a fixed-wing strike, free approach

`approach_spectrum` at, say, 60, 200 and 1 500 m AGL, `radius` the SAM's range,
`min_masked_range` the player's weapon standoff, `sector` the player's arc.

| Band | Blind arc fraction | Widest arc | Blind arc in the player's sector | Median unmask | Unmask at the lowest band |
|---|---|---|---|---|---|
| easy | ≥ 0.4 at 200 m | ≥ 40° | yes, at 200 m | ≥ 15 km | any |
| medium | 0.15 to 0.4 at 200 m | 15° to 40° | yes, at ≤ 200 m | 6 to 15 km | ≥ 4 km |
| hard | 0.05 to 0.15, at 60 m only | 5° to 15° | only at 60 m, or none but ≥ 15° elsewhere | 2 to 6 km | ≥ 2 km |
| unfair | 0 at every band | 0° | no | < 2 km | — |

Unfair means no terrain solution: the mission must supply SEAD, standoff or a
decoy. The spread matters as much as the median. Two sites with median unmask
8 km, one from 1 to 20 km and one from 7 to 9: the first rewards study and
punishes a careless heading, the second plays the same from everywhere.

### SAM or AAA against a helicopter attack

Helicopters fly the terrain and pop up, so run the same operations low.

- **Masked arcs at 15, 30 and 60 m AGL.** `approach_spectrum` with
  `min_masked_range` the helicopter's weapon range. `blind_arc_fraction` at 30 m is
  the number for Hellfire or Vikhr, at 15 m for rockets.
- **Pop-up positions.** The helicopter task in [tasks.md](tasks.md). Their count in
  weapon range is how many shots the player gets.
- **Free shots.** `visible` from the threat to each position at 30 m: seen is a
  duel, unseen a free shot. This applies when the target is not the threat itself.
  A position that sees a radar is nearly always seen by it, so count free shots
  against a separate target and call shots at the radar duels.
- **Egress.** The nearest cell masked from the threat at 5 m, from `viewshed` at
  `target_alt_agl` 5. Under 300 m is one dash; over 1 km a long exposed run.

| Band | Blind arc | Pop-ups in range | Free shots | Egress |
|---|---|---|---|---|
| easy | ≥ 0.3 at 60 m, widest ≥ 40° | ≥ 6 | ≥ 0.7 | ≤ 300 m |
| medium | ≥ 0.15 at 30 m, widest ≥ 15° | 3 to 5 | 0.4 to 0.7 | ≤ 600 m |
| hard | > 0 at 15 m only, or widest < 15° | 1 to 2 | < 0.4 | ≤ 1 km |
| unfair | 0 at ≤ 30 m | 0 | — | — |

### Hidden target for a search

Ground exposure: the share of road cells within 3 km with a sightline at 2 m. Air
exposure: the share of a 1 km lattice at the search altitude. Buildings from
`scenery_in`.

| Band | Ground exposure | Air exposure at 500 m | Buildings in 150 m |
|---|---|---|---|
| easy | ≥ 0.5 | ≥ 0.6 | 0 to 2 |
| medium | 0.2 to 0.5 | 0.3 to 0.6 | 3 to 15 |
| hard | < 0.2 | 0.1 to 0.3 | ≥ 15 |
| unfair | < 0.05 | < 0.1 | — |

### Convoy for an interdiction

Exposed share from `route` legs against the player's likely orbit, `chokepoints`
along it, and `masked_segments` from `unmask_profile` with the orbit as the site
and the route at 2 m AGL.

| Band | Exposed share of route | Chokepoints on route | Longest masked stretch |
|---|---|---|---|
| easy | ≥ 0.6 | ≥ 2 bridges or defiles | < 2 km |
| medium | 0.3 to 0.6 | 1 | 2 to 5 km |
| hard | < 0.3 | 0 | > 5 km |
| unfair | < 0.1 | 0 | the route is all in a gorge |

### A recurring convoy that should not be predictable

Plan once with `route_alternatives`, then draw a route per departure, weighted,
never the same twice running. Replan when a bridge is destroyed. An easy setting
draws mostly the exposed route and a hard one the masked. Three routes with 20 to
40 % overlap and stretch under 1.3 is a good default; cap `max_stretch` at 1.2 if
supplies must arrive on time. If every alternative crosses one bridge, that bridge
is the mission: report it.

### FARP for the player's own side

Difficulty here is logistics. Rank by terms and pick by policy: close to the front,
low ground exposure, and the clearing as the price in destroyed buildings. A FARP
with no clearing, exposure near 0.4 and a road 250 m away is the prudent choice.

## Filtering patterns

- **Hard constraint, then band, then draw.** Do not filter on the weighted score;
  it hides which term drove the rank.
- **Two candidates that differ in one term** are the interesting pair: same
  coverage, different unmask distance is a pure difficulty dial.
- **Blind arcs are the debrief.** "There was a masked approach between 300 and
  340" is a sentence to keep and show afterward.
