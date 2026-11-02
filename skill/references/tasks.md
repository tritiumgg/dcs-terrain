# Worked tasks

Each task takes a mission designer's question to requests and an answer. The
numbers are Caucasus around Kutaisi. Every line of a `jsonl` block runs as it
stands; change the positions for the question at hand. Kutaisi is airdrome 25, at
`x -284887.375, z 683858.71875`.

Two habits hold throughout. Look positions up rather than guessing them, and
give the terms and the numbers behind a choice, so the designer can overrule it.

## FARP for a helicopter forward base

*Find a FARP site near Kutaisi.* Unless the user says otherwise, take these
defaults: a box 20 km square centered on the airdrome's exact position, a
footprint of 120 × 80 m at two orientations, a height span of at most 2.5 m,
land, 200 m to 3 km from a road, at least 3 km from the airfield, nothing within
100 m, every 100 m, 2 km apart, eight candidates.

**First pass.** The airdrome band is a fence, so its weight is 0.

```jsonl
{"id": "farp", "op": "find_sites", "request": {"region": {"Box": {"min_x": -294887.375, "min_z": 673858.71875, "max_x": -274887.375, "max_z": 693858.71875}}, "footprint": {"width": 120, "depth": 80, "orientations_deg": [0, 90]}, "criteria": [{"name": "surface_in", "params": {"classes": ["land"]}}, {"name": "span_max", "params": {"m": 2.5}}, {"name": "dist_road_band", "params": {"min": 200, "max": 3000}}, {"name": "dist_airdrome_band", "params": {"min": 3000, "max": 1000000, "airdrome": 25}}, {"name": "clearing_max", "params": {"radius": 100, "max": 0}}], "step": 100, "spacing": 2000, "limit": 8, "weights": {"dist_airdrome_band": 0}}}
```

**Second pass, for each candidate.** The plain is flat, so these terms tie. What
separates the candidates is what stands around them and who can see them.

- Scenery within 500 m: `scenery_in` with a `Circle` of radius 500 around the
  candidate. Report `counts_by_class`.
- Ground exposure: the share of 36 observers that see it. The observers stand on
  a ring around the candidate, 12 bearings every 30° at 3, 6 and 10 km, 2 m above
  the ground, and the site is judged 2 m above the ground. A criterion's observers
  are fixed points, so this is 36 `visible` lines per candidate.

An observer at bearing `b` and range `r` from `(x, z)` stands at
`(x + r cos b, z + r sin b)`:

In Git Bash or any POSIX shell:

```bash
awk 'BEGIN { pi = atan2(0, -1) }
{
  n = split("3000 6000 10000", rs, " ")
  for (i = 1; i <= n; i++) for (b = 0; b < 360; b += 30) {
    px = $1 + rs[i] * cos(b * pi / 180); pz = $2 + rs[i] * sin(b * pi / 180)
    printf "{\"id\":\"c%d\",\"op\":\"visible\",\"request\":{\"a\":{\"x\":%.3f,\"z\":%.3f,\"alt_agl\":2},\"b\":{\"x\":%s,\"z\":%s,\"alt_agl\":2}}}\n", NR, px, pz, $1, $2
  }
}' candidates.txt > exposure.jsonl
```

`candidates.txt` holds `x z` per line. Count the lines answering `"visible":true`
per id; the share is over the lines that answered, so say so if any are
`off_terrain`.

**Report** a table: position, `orientation_deg`, span, road distance, scenery
within 500 m, exposure. Recommend from it, and reject a candidate by name with the
reason. Candidates with nothing within 500 m and low exposure are the prudent
choice; one in a village is easier to reach and easier to find.

**Back in DCS:** a FARP static at `(x, z)` with the heading `orientation_deg`. A
FARP spawn removes scenery near it, as far as the 150 m `clearing_max` takes by
default, so give the count from `scenery_in` within 150 m as the expected
clearing. The 100 m test above does not say that. Use `geo` for the position in
latitude, longitude and MGRS.

## SAM site covering an airfield against low-level ingress

*Where should a SAM go to cover Kutaisi and stay masked from the west?* Sites 4 to
15 km from the field near a road. `masked_from` keeps only positions with a horizon
in that sector, and its weight ranks by how high the horizon is, so masking does
the sorting. A second pass over tied candidates would only sample them. 100 m at
10 km stands about 0.5° above the site, hence `min_elev_deg`.

```jsonl
{"id": "sam", "op": "find_sites", "request": {"region": {"AirdromeBand": {"airdrome": 25, "min_dist": 4000, "max_dist": 15000}}, "footprint": {"width": 30, "depth": 30, "orientations_deg": [0]}, "criteria": [{"name": "surface_in", "params": {"classes": ["land"]}}, {"name": "dist_road_band", "params": {"min": 0, "max": 500}}, {"name": "masked_from", "params": {"bearing_deg": 270, "half_width_deg": 45, "min_elev_deg": 0.5}}], "step": 300, "spacing": 5000, "limit": 5, "weights": {"masked_from": 1, "surface_in": 0, "dist_road_band": 0}}}
```

Then prove each candidate with `visible` from a 6 m mast: to the field and a 5 km
ring of 24 points around it at 100 m AGL, and to the western arc about that site,
7 points 10 km out at bearings 225 to 315 every 15° at 100 m AGL. On this plain
every site covers the ring, so masking decides. The horizon is judged from the
ground and the raster, so a candidate can still be seen from a point in the arc.

Then on the chosen site:

```jsonl
{"id": "cover", "op": "coverage", "request": {"site": {"x": -293887, "z": 682059, "alt_agl": 6}, "envelope": {"radius": 30000, "alt_bands_agl": [60, 100, 300]}, "asset": {"AirdromeBand": {"airdrome": 25, "min_dist": 0, "max_dist": 5000}}}}
{"id": "spectrum", "op": "approach_spectrum", "request": {"site": {"x": -293887, "z": 682059, "alt_agl": 6}, "radius": 30000, "alt_bands_agl": [60, 200, 1500], "min_masked_range": 8000, "sector": {"from_deg": 225, "to_deg": 315}}}
```

`coverage` gives the dead zones for the briefing. `approach_spectrum` gives the
blind arcs, and with `sector` the same over the arc the player will come from.

## Unmasking point on an ingress

*Where does that SAM first see a flight at 100 m from 60 km due west?*

```jsonl
{"id": "ingress", "op": "unmask_profile", "request": {"site": {"x": -293887, "z": 682059, "alt_agl": 6}, "route": [{"x": -284887, "z": 623859, "alt_agl": 100}, {"x": -284887, "z": 683859, "alt_agl": 100}], "step": 100}}
```

`first_seen_dist_from_site` is the number a player feels. `masked_segments` are
the stretches the route is hidden, as distances along it.

## Several threats together

*Where can I fly at 100 m without any of these sites seeing me?*

```jsonl
{"id": "gaps", "op": "exposure", "request": {"sites": [{"observer": {"x": -293887, "z": 682059, "alt_agl": 6}, "radius": 30000}, {"observer": {"x": -277987, "z": 673059, "alt_agl": 6}, "radius": 30000}], "target_alt_agl": 100, "bands_agl": [30, 100, 300], "geojson": true}}
```

`seen_fraction` and each band's `unseen_fraction` are the numbers; the `geojson`
draws the gaps. It reports geometry only, with no clutter, Doppler notch or
minimum elevation.

## Helicopter pop-up positions

*Where can an attack helicopter hide from that SAM and still shoot it?* Positions
within weapon range, hidden from the radar at 5 m and seeing the target at 30 m:

```jsonl
{"id": "popup", "op": "find_sites", "request": {"region": {"Circle": {"x": -293887, "z": 682059, "radius": 6000}}, "footprint": {"width": 30, "depth": 30, "orientations_deg": [0]}, "criteria": [{"name": "not_visible_from", "params": {"observers": [{"x": -293887, "z": 682059, "alt_agl": 6}], "max_fraction": 0, "own_alt_agl": 5}}, {"name": "visible_to", "params": {"targets": [{"x": -293887, "z": 682059, "alt_agl": 2}], "min_fraction": 1, "own_alt_agl": 30}}], "step": 100, "spacing": 300, "limit": 20}}
```

`stats.cells_passed` is how much of it there is. Then `visible` from the radar to
each position at 30 m tells a free shot from a duel.

## Concealed command bunker

*Hide a bunker near the road in this area: little seen from roads or the air.*
Plausibility first, then exposure per candidate. These terms all tie, and ties go
to the lesser x, so take 200 candidates: 20 would all lie in the south of the
box. Exposure chooses among them.

```jsonl
{"id": "bunker", "op": "find_sites", "request": {"region": {"Box": {"min_x": -300000, "min_z": 690000, "max_x": -280000, "max_z": 710000}}, "footprint": {"width": 40, "depth": 40}, "criteria": [{"name": "slope_max", "params": {"deg": 8}}, {"name": "surface_in", "params": {"classes": ["land"]}}, {"name": "dist_road_band", "params": {"min": 50, "max": 2000}}, {"name": "dist_cover_band", "params": {"class": "town", "min": 0, "max": 5000}}], "step": 100, "spacing": 1000, "limit": 200, "weights": {"dist_road_band": 0, "dist_cover_band": 0, "slope_max": 0}}}
```

`radar_floor` with `points` is one call per candidate and judges every point as
`visible` does. Put the site at the bunker, 2 m up. Each point's `floor_agl` is the
lowest height above ground at which something there sees the bunker.

```jsonl
{"id": "bunker-air", "op": "radar_floor", "request": {"site": {"x": -290000, "z": 700000, "alt_agl": 2}, "radius": 15000, "points": [{"x": -285000, "z": 700000}, {"x": -290000, "z": 705000}, {"x": -295000, "z": 700000}, {"x": -290000, "z": 695000}]}}
```

Road cells come from `sample` over a `Circle` of 3 km, `step` 100, layer
`dist_road`, keeping those under 30 m. Ground exposure is the share of those with
`floor_agl` ≤ 2; air exposure is the share of a 1 km lattice within 15 km with
`floor_agl` ≤ the band. `scenery_in` within 150 m gives the built-up cover. Keep
masking and cover as separate columns: a valley floor and a town block hide a
bunker in different ways.

To judge a hand-picked position with the same criteria, use `score_site`.

## Convoy from depot to front

*Route a convoy from Kutaisi to here, avoiding that SAM's area and that outpost.*

```jsonl
{"id": "convoy", "op": "route", "request": {"network": "roads", "from": {"x": -284887, "z": 683859}, "to": {"x": -250000, "z": 650000}, "avoid": [{"Circle": {"x": -265000, "z": 665000, "radius": 3000}}], "exposure_observers": [{"x": -270000, "z": 660000, "alt_agl": 10}], "exposure_weight": 1}}
```

`length / straight_line` is the detour. Each leg has its `exposure`, the share of
it the observers see; sum `length × exposure` for the exposed meters. A route that
does not move at `exposure_weight` 1 is not proof it is the least exposed: raise
the weight, up to 1000, to see what avoiding the observers costs. Back in DCS,
each waypoint becomes a waypoint with the action `On Road`.

## Alternative routes for a recurring convoy

*The same convoy, but not the same road every time.*

```jsonl
{"id": "alts", "op": "route_alternatives", "request": {"network": "roads", "from": {"x": -284887, "z": 683859}, "to": {"x": -250000, "z": 650000}, "k": 3, "max_stretch": 1.3, "max_overlap": 0.4}}
```

`shared_edges_with_all` is the edges every alternative must use, and so where an
ambush works whichever route is drawn. Trust it over comparing `edge_id`s by hand:
two routes can hold opposite ends of one edge and share none of its road. The method promises nothing of the
alternatives it misses: fewer than `k` routes is an answer.

## Ambush against a route

*Where can a platoon ambush that road?* Take the route's waypoints from `route`.
The region is the route's box widened by 2 km. The targets are the convoy on the
road every 200 m at 2 m AGL. `masked_from` hides the site from the approach side:
the bearing from the ambush back along the road toward the convoy's approach.

```jsonl
{"id": "ambush", "op": "find_sites", "request": {"region": {"Box": {"min_x": -290000, "min_z": 672000, "max_x": -282000, "max_z": 684000}}, "footprint": {"width": 30, "depth": 30, "orientations_deg": [0]}, "criteria": [{"name": "slope_max", "params": {"deg": 15}}, {"name": "surface_in", "params": {"classes": ["land"]}}, {"name": "dist_road_band", "params": {"min": 100, "max": 600}}, {"name": "masked_from", "params": {"bearing_deg": 225, "half_width_deg": 30, "min_elev_deg": 0.5}}, {"name": "visible_to", "params": {"targets": [{"x": -288034, "z": 673167, "alt_agl": 2}, {"x": -285098, "z": 679940, "alt_agl": 2}], "min_fraction": 0.5, "own_alt_agl": 2}}], "step": 100, "spacing": 500, "limit": 5, "weights": {"dist_road_band": 0}}}
```

The `visible_to` term's `value` gives the share of road the site sees; multiplied
by the sampled length, that is the road it covers.

## Choke points on a road network

*Which roads and bridges matter most around Kutaisi?*

```jsonl
{"id": "choke", "op": "chokepoints", "request": {"region": {"Circle": {"x": -284887, "z": 683859, "radius": 20000}}, "network": "roads", "limit": 10}}
```

High `betweenness` with `crosses_water` is a bridge; with a strongly negative
`tpi_300_mean`, a defile. Report each `midpoint`.

## Spawn visibility for fog of war

*Can the player at this position see a group spawning there?*

```jsonl
{"id": "los", "op": "visible", "request": {"a": {"x": -284000, "z": 690000, "alt": 1500}, "b": {"x": -293887, "z": 682059, "alt_agl": 2}}}
{"id": "around", "op": "scenery_in", "request": {"region": {"Circle": {"x": -293887, "z": 682059, "radius": 100}}}}
```

`clearance_m` never exceeds the lower end's own height above its ground, so a
small value at a low target can mean a clear line. Blocked, or clear by under
10 m with both ends well above that, is marginal: try the height ±5 m before
saying seen or masked. Buildings do not block the line; `counts_by_class` says
whether the group stands among them.

## Recovery landing sites along a route

*Find places to set a helicopter down along this route.* The region is a corridor
3 km either side of the route, as a `Polygon`, or a `Box` per stretch.

```jsonl
{"id": "lz-chain", "op": "find_sites", "request": {"region": {"Polygon": {"points": [[-288000, 660000], [-282000, 660000], [-282000, 690000], [-288000, 690000]]}}, "footprint": {"width": 30, "depth": 30}, "criteria": [{"name": "slope_max", "params": {"deg": 5}}, {"name": "surface_in", "params": {"classes": ["land"]}}, {"name": "clearing_max", "params": {"radius": 60, "max": 0}}], "step": 100, "spacing": 5000, "limit": 12}}
```

## Trafficability of a point

*Can a ground group get to this point from the road?*

```jsonl
{"id": "reach", "op": "trafficability", "request": {"x": -293887, "z": 682059, "max_slope_deg": 20, "network": "roads"}}
```

`reachable`, `blocking` and `reason` are the answer. `cost` is a coarse
cross-country weight of this tool's own, not measured against the game's ground
units.

## Checking and describing placements

*Will these fit?* The tool knows nothing of units, so the caller gives each size
and what it needs.

```jsonl
{"id": "check", "op": "validate_placements", "request": {"placements": [{"id": "sa-radar", "x": -293887, "z": 682059, "heading": 90, "shape": {"Rect": {"width": 30, "depth": 60}}, "on": "land", "max_slope_deg": 5}, {"id": "ship", "x": -300000, "z": 600000, "heading": 0, "shape": {"Circle": {"radius": 100}}, "on": "water", "min_depth": 15}]}}
```

A clean placement has an empty `issues` list. Each issue names the kind (`water`,
`land`, `shallow`, `slope`, `no_ground`, `scenery`, `pavement`) and what it found.

*Can a C-130 park at Kutaisi?* The caller supplies the airframe's size:

```jsonl
{"id": "field", "op": "airfield_info", "request": {"airdrome": "Kutaisi", "fits": {"width": 40.4, "length": 29.8, "height": 11.8, "helicopter": false}}}
```

`course_true_deg` is the true course. `course_grid_deg` is from DCS x, which the
designator follows. A stand's `fits` is null where its size is unknown.

*What is at this position, in a briefing's words?*

```jsonl
{"id": "where", "op": "point_info", "request": {"points": [{"x": -293887, "z": 682059}]}}
```

*Which are the big towns?* This needs the cache entry `index` builds:

```jsonl
{"id": "cities", "op": "towns", "request": {"size": ["city"], "sort": "floor", "limit": 10}}
```
