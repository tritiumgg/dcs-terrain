//! `towns`: the places of a terrain or a region, filtered and ranked, and
//! the ground each covers, which outlines it and tells whether a point
//! stands in it.

use serde_json::json;

use super::geojson::{Features, props};
use super::nearest::town_display_name;
use super::outline::polygons;
use super::region::resolve;
use super::{Terrain, refused};
use std::collections::{HashMap, HashSet};

use crate::places::{
    Building, GAP_M, MIN_BUILDINGS, Place, Places, RADIUS_CAP_M, buildings_in, link,
};
use crate::source::{Source, Tables};
use crate::types::{
    BoundingBox, Issue, PlaceInfo, PlaceRef, TownsRequest, TownsResponse, TownsSort,
};

/// The most places one request returns.
const LIMIT_MAX: u32 = 1000;
/// The most places one request outlines: each reads its box's scenery
/// again, about 0.1 s for a large city.
const OUTLINES_MAX: u32 = 100;
/// A building's circle is buffered by half the linking gap, so the buffers
/// of two linked buildings meet.
const BUFFER_M: f64 = GAP_M / 2.0;
/// The cells a place's ground is rasterized in.
const CELL_M: f64 = 50.0;
/// How far a simplified outline may stray from the cells' edges: half a
/// cell.
const TOLERANCE_M: f64 = CELL_M / 2.0;

/// The places of `src`, or the issue saying they are still being built.
pub(super) fn places_of(src: &dyn Source) -> Result<&Places, Vec<Issue>> {
    src.places().map_err(refused)?.ok_or_else(|| {
        vec![Issue::new(
            "not_ready",
            &format!(
                "places are still being built; `dcsterrain index --terrain {}` builds them",
                src.identity().terrain
            ),
            "",
        )]
    })
}

/// The buildings of `p`: those in its box that link to its least-id
/// building. No building outside the box belongs to it, and none inside
/// that does not link to it can, so linking the box alone is exact.
pub(super) fn members(src: &dyn Source, p: &Place) -> Result<Vec<Building>, Vec<Issue>> {
    let [x0, z0, x1, z1] = p.bbox;
    let mut b = buildings_in(src, p.bbox).map_err(refused)?;
    b.retain(|b| (x0..=x1).contains(&b.x) && (z0..=z1).contains(&b.z));
    let label = link(&b);
    let Some(first) = b.iter().position(|b| b.id == p.id) else {
        return Err(refused(crate::source::Refusal(format!(
            "place {} names a building the scenery does not hold; rebuild the cache's places",
            p.id
        ))));
    };
    let mine = label[first];
    Ok(b.into_iter()
        .zip(label)
        .filter(|(_, l)| *l == mine)
        .map(|(b, _)| b)
        .collect())
}

/// The ground a place covers: each 50 m cell, on the cells of DCS x and z
/// from 0, whose center lies within 100 m of one of its buildings' circles.
pub(super) struct Ground {
    row0: i64,
    col0: i64,
    rows: i64,
    cols: i64,
    set: Vec<bool>,
}

impl Ground {
    pub(super) fn of(buildings: &[Building]) -> Ground {
        let reach = |b: &Building| BUFFER_M + f64::from(b.r);
        let cell = |v: f64| (v / CELL_M).floor() as i64;
        let (mut r0, mut c0, mut r1, mut c1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
        for b in buildings {
            let e = reach(b);
            (r0, c0) = (r0.min(cell(b.x - e)), c0.min(cell(b.z - e)));
            (r1, c1) = (r1.max(cell(b.x + e)), c1.max(cell(b.z + e)));
        }
        if buildings.is_empty() {
            (r0, c0, r1, c1) = (0, 0, -1, -1);
        }
        let (rows, cols) = (r1 - r0 + 1, c1 - c0 + 1);
        let mut set = vec![false; (rows * cols) as usize];
        for b in buildings {
            let e = reach(b);
            for r in cell(b.x - e)..=cell(b.x + e) {
                let dx = (r as f64 + 0.5) * CELL_M - b.x;
                for c in cell(b.z - e)..=cell(b.z + e) {
                    let dz = (c as f64 + 0.5) * CELL_M - b.z;
                    if dx.hypot(dz) <= e {
                        set[((r - r0) * cols + c - c0) as usize] = true;
                    }
                }
            }
        }
        Ground {
            row0: r0,
            col0: c0,
            rows,
            cols,
            set,
        }
    }

    fn at(&self, r: i64, c: i64) -> bool {
        (0..self.rows).contains(&r)
            && (0..self.cols).contains(&c)
            && self.set[(r * self.cols + c) as usize]
    }

    /// Whether the cell holding `(x, z)` is the place's: what a point's
    /// lookup must agree with, which reads far less to answer it.
    #[cfg(test)]
    fn contains(&self, x: f64, z: f64) -> bool {
        let cell = |v: f64| (v / CELL_M).floor() as i64;
        self.at(cell(x) - self.row0, cell(z) - self.col0)
    }

    /// The ground's outline: polygons along the cells' edges, simplified to
    /// within half a cell.
    pub(super) fn outline(&self) -> Vec<Vec<Vec<[f64; 2]>>> {
        let corner = |r: f64, c: f64| {
            [
                (self.row0 as f64 + r) * CELL_M,
                (self.col0 as f64 + c) * CELL_M,
            ]
        };
        polygons(
            &|r, c| self.at(r, c),
            self.rows,
            self.cols,
            &corner,
            TOLERANCE_M,
        )
    }
}

/// A place's name and its others, by the towns' display names.
fn names(tables: &Tables, p: &Place) -> (Option<String>, Vec<String>) {
    let mut all = p
        .names
        .iter()
        .filter_map(|&n| tables.towns.get(n as usize))
        .map(town_display_name);
    (all.next(), all.collect())
}

/// The place `p` as a point in it hears of it.
pub(super) fn place_ref(tables: &Tables, p: &Place) -> PlaceRef {
    PlaceRef {
        id: p.id,
        name: names(tables, p).0,
        size: p.size,
        form: p.form,
    }
}

pub fn towns(terrain: &Terrain, q: TownsRequest) -> Result<TownsResponse, Vec<Issue>> {
    if !(1..=LIMIT_MAX).contains(&q.limit) {
        return Err(vec![Issue::new(
            "invalid_request",
            &format!("must be 1 to {LIMIT_MAX}"),
            "limit",
        )]);
    }
    if q.outlines && q.limit > OUTLINES_MAX {
        return Err(vec![Issue::new(
            "invalid_request",
            &format!("with outlines, must be 1 to {OUTLINES_MAX}"),
            "limit",
        )]);
    }
    let src = terrain.source();
    let area = q
        .region
        .as_ref()
        .map(|r| resolve(src, r, "region"))
        .transpose()?;
    let tables = src.tables().map_err(refused)?;
    let all = places_of(src)?;
    let mut found: Vec<&Place> = all
        .places
        .iter()
        .filter(|p| area.as_ref().is_none_or(|a| a.contains(p.x, p.z)))
        .filter(|p| q.size.is_empty() || q.size.contains(&p.size))
        .filter(|p| q.form.is_empty() || q.form.contains(&p.form))
        .filter(|p| p.buildings >= q.min_buildings)
        .collect();
    let key = |p: &Place| match q.sort {
        TownsSort::Buildings => f64::from(p.buildings),
        TownsSort::ResidentialFloor => p.residential_floor,
        TownsSort::ApartmentShare => p.apartment_share(),
    };
    found.sort_by(|a, b| key(b).total_cmp(&key(a)).then(a.id.cmp(&b.id)));
    let matched = found.len() as u32;
    found.truncate(q.limit as usize);

    let mut out = Vec::with_capacity(found.len());
    for p in found {
        let outline = if q.outlines {
            Some(Ground::of(&members(src, p)?).outline())
        } else {
            None
        };
        let (name, other_names) = names(tables, p);
        out.push(PlaceInfo {
            id: p.id,
            name,
            other_names,
            x: p.x,
            z: p.z,
            bbox: BoundingBox {
                min_x: p.bbox[0],
                min_z: p.bbox[1],
                max_x: p.bbox[2],
                max_z: p.bbox[3],
            },
            size: p.size,
            form: p.form,
            buildings: p.buildings,
            houses: p.houses,
            apartments: p.apartments,
            shops: p.shops,
            civic: p.civic,
            industrial: p.industrial,
            residential_floor: p.residential_floor,
            apartment_share: p.apartment_share(),
            hectares: p.hectares,
            outline,
        });
    }

    let geojson = if q.geojson {
        let mut f = Features::new(terrain)?;
        for p in &out {
            let about = || {
                props([
                    ("id", json!(p.id)),
                    ("name", json!(p.name)),
                    ("size", json!(p.size)),
                    ("form", json!(p.form)),
                    ("buildings", json!(p.buildings)),
                ])
            };
            if let Some(o) = &p.outline {
                f.polygons(o, about());
            }
            f.point([p.x, p.z], about());
        }
        Some(f.finish())
    } else {
        None
    };
    Ok(TownsResponse {
        provenance: terrain.provenance(),
        matched,
        places: out,
        geojson,
    })
}

/// Which place each of a request's points stands in. A point stands in the
/// place whose ground holds its cell; that cell's center lies within 100 m
/// of one of the place's buildings' circles, so only the buildings about
/// the point are read, and a place's own are read in full only to tell it
/// from another whose box also holds the building found.
pub(super) struct Lookup<'a> {
    places: &'a Places,
    /// The places whose box meets each 2 km cell.
    cells: HashMap<(i64, i64), Vec<u32>>,
    /// Each place's buildings' ids, read once for the request.
    members: HashMap<u32, HashSet<u64>>,
    /// Each 50 m cell's answer, so points sharing a cell read once.
    answered: HashMap<(i64, i64), Option<&'a Place>>,
}

/// The side of the cells places are looked up in.
const LOOKUP_CELL_M: f64 = 2000.0;

fn lookup_key(x: f64, z: f64) -> (i64, i64) {
    (
        (x / LOOKUP_CELL_M).floor() as i64,
        (z / LOOKUP_CELL_M).floor() as i64,
    )
}

impl<'a> Lookup<'a> {
    pub(super) fn new(places: &'a Places) -> Lookup<'a> {
        let mut cells: HashMap<(i64, i64), Vec<u32>> = HashMap::new();
        for (k, p) in places.places.iter().enumerate() {
            let [x0, z0, x1, z1] = p.bbox;
            let (a, b) = (lookup_key(x0, z0), lookup_key(x1, z1));
            for kx in a.0..=b.0 {
                for kz in a.1..=b.1 {
                    cells.entry((kx, kz)).or_default().push(k as u32);
                }
            }
        }
        Lookup {
            places,
            cells,
            members: HashMap::new(),
            answered: HashMap::new(),
        }
    }

    /// The place whose ground holds `(x, z)`.
    pub(super) fn at(
        &mut self,
        src: &dyn Source,
        x: f64,
        z: f64,
    ) -> Result<Option<&'a Place>, Vec<Issue>> {
        let cell = ((x / CELL_M).floor() as i64, (z / CELL_M).floor() as i64);
        if let Some(&p) = self.answered.get(&cell) {
            return Ok(p);
        }
        let p = self.find(src, x, z)?;
        self.answered.insert(cell, p);
        Ok(p)
    }

    fn find(&mut self, src: &dyn Source, x: f64, z: f64) -> Result<Option<&'a Place>, Vec<Issue>> {
        let center = |v: f64| ((v / CELL_M).floor() + 0.5) * CELL_M;
        let (cx, cz) = (center(x), center(z));
        let e = BUFFER_M + RADIUS_CAP_M;
        let near = buildings_in(src, [cx - e, cz - e, cx + e, cz + e]).map_err(refused)?;
        // A building whose buffer holds the cell's center. Every building
        // whose does is of one group, since two groups' buffers are apart.
        let Some(b) = near
            .into_iter()
            .find(|b| (b.x - cx).hypot(b.z - cz) <= BUFFER_M + f64::from(b.r))
        else {
            return Ok(None);
        };
        // Its place, if its group is one, is among those whose box holds it.
        let mut candidates: Vec<u32> = self
            .cells
            .get(&lookup_key(b.x, b.z))
            .into_iter()
            .flatten()
            .copied()
            .filter(|&k| {
                let [x0, z0, x1, z1] = self.places.places[k as usize].bbox;
                (x0..=x1).contains(&b.x) && (z0..=z1).contains(&b.z)
            })
            .collect();
        let size = |k: &u32| (self.places.places[*k as usize].buildings, *k);
        candidates.sort_by_key(size);
        let Some(largest) = candidates.pop() else {
            return Ok(None);
        };
        // The smaller ones by their members; the largest, read last, by
        // elimination, when the building's group is large enough to be a
        // place at all.
        for k in candidates {
            if !self.members.contains_key(&k) {
                let p = &self.places.places[k as usize];
                let ids = members(src, p)?.iter().map(|m| m.id).collect();
                self.members.insert(k, ids);
            }
            if self.members[&k].contains(&b.id) {
                return Ok(Some(&self.places.places[k as usize]));
            }
        }
        Ok(in_a_group_of(src, &b, MIN_BUILDINGS as usize)?
            .then(|| &self.places.places[largest as usize]))
    }
}

/// Whether `b` links, directly or through others, to at least `n - 1`
/// other buildings: a walk out from it that stops at `n`.
fn in_a_group_of(src: &dyn Source, b: &Building, n: usize) -> Result<bool, Vec<Issue>> {
    let mut seen = vec![*b];
    let mut next = 0;
    while next < seen.len() {
        let at = seen[next];
        next += 1;
        let e = GAP_M + f64::from(at.r) + RADIUS_CAP_M;
        for o in buildings_in(src, [at.x - e, at.z - e, at.x + e, at.z + e]).map_err(refused)? {
            if at.links(&o) && !seen.iter().any(|s| s.id == o.id) {
                seen.push(o);
                if seen.len() >= n {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::super::answer;
    use super::super::geojson::tests::assert_valid;
    use super::super::tests::synth;
    use super::*;
    use serde_json::{Value, json};

    fn ask(t: &Terrain, req: Value) -> Value {
        answer(t, "towns", &req).unwrap()
    }

    /// The synthetic terrain's settlement buildings grouped by testing
    /// every pair, groups of three or more, each as its sorted ids.
    fn every_pair(t: &Terrain) -> Vec<Vec<u64>> {
        let b = buildings_in(t.source(), [-1e7, -1e7, 1e7, 1e7]).unwrap();
        let mut group: Vec<usize> = (0..b.len()).collect();
        // Merge until nothing changes: slow, and plainly right.
        loop {
            let mut changed = false;
            for i in 0..b.len() {
                for j in 0..b.len() {
                    if b[i].links(&b[j]) && group[j] > group[i] {
                        let (from, to) = (group[j], group[i]);
                        group
                            .iter_mut()
                            .filter(|g| **g == from)
                            .for_each(|g| *g = to);
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        let mut out: Vec<Vec<u64>> = Vec::new();
        for g in group
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
        {
            let ids: Vec<u64> = (0..b.len())
                .filter(|&i| group[i] == g)
                .map(|i| b[i].id)
                .collect();
            if ids.len() >= 3 {
                out.push(ids);
            }
        }
        out.sort();
        out
    }

    #[test]
    fn the_places_are_the_groups_every_pair_makes() {
        let t = synth();
        let want = every_pair(&t);
        assert!(!want.is_empty());
        let v = ask(&t, json!({"limit": 1000}));
        let places = v["places"].as_array().unwrap();
        assert_eq!(places.len(), want.len());
        assert_eq!(v["matched"], want.len());
        let src = t.source();
        for p in places {
            let id = p["id"].as_u64().unwrap();
            let group = want
                .iter()
                .find(|g| g[0] == id)
                .expect("a place's id is its least");
            assert_eq!(p["buildings"], group.len());
            let place = places_of(src)
                .unwrap()
                .places
                .iter()
                .find(|q| q.id == id)
                .unwrap();
            let mut got: Vec<u64> = members(src, place).unwrap().iter().map(|b| b.id).collect();
            got.sort();
            assert_eq!(&got, group);
        }
    }

    #[test]
    fn an_outline_holds_its_buildings_and_nothing_200_m_beyond() {
        let t = synth();
        let src = t.source();
        let v = ask(&t, json!({"outlines": true, "geojson": true}));
        assert_valid(&v["geojson"]);
        let p = &v["places"][0];
        assert!(!p["outline"].as_array().unwrap().is_empty());
        let place = &places_of(src).unwrap().places[0];
        let b = members(src, place).unwrap();
        let g = Ground::of(&b);
        for m in &b {
            assert!(g.contains(m.x, m.z), "{m:?}");
        }
        let far = |x: f64, z: f64| {
            b.iter()
                .all(|m| (m.x - x).hypot(m.z - z) > BUFFER_M + f64::from(m.r) + CELL_M)
        };
        let [x0, z0, x1, z1] = place.bbox;
        let mut beyond = 0;
        for k in 0..400 {
            let (x, z) = (
                x0 - 500.0 + (k % 20) as f64 * (x1 - x0 + 1000.0) / 19.0,
                z0 - 500.0 + (k / 20) as f64 * (z1 - z0 + 1000.0) / 19.0,
            );
            if far(x, z) {
                beyond += 1;
                assert!(!g.contains(x, z), "{x} {z}");
            }
        }
        assert!(beyond > 50);
    }

    #[test]
    fn filters_sort_and_limit() {
        let t = synth();
        let all = ask(&t, json!({}));
        let n = all["matched"].as_u64().unwrap();
        let p = &all["places"][0];
        let (x, z) = (p["x"].as_f64().unwrap(), p["z"].as_f64().unwrap());
        let near = json!({"Circle": {"x": x, "z": z, "radius": 10.0}});
        assert_eq!(ask(&t, json!({"region": near}))["places"][0]["id"], p["id"]);
        let far = json!({"Circle": {"x": x + 50_000.0, "z": z, "radius": 10.0}});
        assert_eq!(ask(&t, json!({"region": far}))["matched"], 0);
        let b = p["buildings"].as_u64().unwrap();
        // The first place has the most buildings.
        assert_eq!(ask(&t, json!({"min_buildings": b + 1}))["matched"], 0);
        assert!(n > 1);
        assert_eq!(
            ask(&t, json!({"limit": 1}))["places"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(ask(&t, json!({"size": ["city"]}))["matched"], 0);
        assert_eq!(
            ask(&t, json!({"size": [p["size"]], "form": [p["form"]]}))["places"][0]["id"],
            p["id"]
        );
        let ranked = ask(&t, json!({"sort": "residential_floor", "limit": 1000}));
        let f: Vec<f64> = ranked["places"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["residential_floor"].as_f64().unwrap())
            .collect();
        assert!(f.windows(2).all(|w| w[0] >= w[1]), "{f:?}");
        let issue = answer(&t, "towns", &json!({"limit": 0})).unwrap_err();
        assert_eq!(issue[0].code, "invalid_request");
        let issue = answer(&t, "towns", &json!({"limit": 101, "outlines": true})).unwrap_err();
        assert_eq!(issue[0].path, "limit");
    }

    #[test]
    fn the_lookup_agrees_with_every_places_ground_at_every_cell() {
        let t = synth();
        let src = t.source();
        let all = places_of(src).unwrap();
        let grounds: Vec<Ground> = all
            .places
            .iter()
            .map(|p| Ground::of(&members(src, p).unwrap()))
            .collect();
        let mut lookup = Lookup::new(all);
        let (mut inside, mut outside) = (0, 0);
        for p in &all.places {
            let [x0, z0, x1, z1] = p.bbox;
            // Every 25 m over the box and 400 m beyond, so each cell is
            // tried at more than one point.
            let (mut x, mut n) = (x0 - 400.0, 0);
            while x <= x1 + 400.0 {
                let mut z = z0 - 400.0;
                while z <= z1 + 400.0 {
                    let want = (all.places.iter().zip(&grounds))
                        .find(|(_, g)| g.contains(x, z))
                        .map(|(q, _)| q.id);
                    let got = lookup.at(src, x, z).unwrap().map(|q| q.id);
                    assert_eq!(got, want, "{x} {z}");
                    if got.is_some() {
                        inside += 1
                    } else {
                        outside += 1
                    }
                    z += 25.0;
                    n += 1;
                }
                x += 25.0;
            }
            assert!(n > 100);
        }
        assert!(inside > 100 && outside > 100, "{inside} {outside}");
    }

    #[test]
    fn a_point_stands_in_the_place_whose_ground_holds_it() {
        let t = synth();
        let src = t.source();
        let place = &places_of(src).unwrap().places[0];
        let m = members(src, place).unwrap()[0];
        let v = answer(
            &t,
            "point_info",
            &json!({"points": [
                {"x": m.x, "z": m.z}, {"x": m.x + 40_000.0, "z": m.z}
            ]}),
        )
        .unwrap();
        assert_eq!(v["places_built"], true);
        assert_eq!(v["points"][0]["place"]["id"], place.id);
        assert_eq!(v["points"][0]["place"]["size"], json!(place.size));
        assert!(v["points"][1]["place"].is_null());
        let near = answer(&t, "nearest", &json!({"kind": "place", "x": m.x, "z": m.z})).unwrap();
        let r = &near["results"][0];
        assert_eq!(r["id"], place.id.to_string());
        assert_eq!(r["kind"], "place");
        assert_eq!(r["class"], place.size.name());
        assert!(
            (r["distance"].as_f64().unwrap() - (place.x - m.x).hypot(place.z - m.z)).abs() < 1e-9
        );
    }
}
