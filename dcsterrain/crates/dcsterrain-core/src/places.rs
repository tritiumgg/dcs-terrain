//! Places: settlement buildings grouped by the gap between them, each
//! group sized, given a form and named from the terrain's towns.
//!
//! The grouping is single linkage: two buildings join when the gap between
//! their circles, each half its box's longer side, is at most 200 m, and a
//! place is a connected set of them, the UK statistics office's built-up
//! area rule. A union-find over a grid hash finds every such pair, and only
//! those: a cell is wider than the farthest two buildings can be and still
//! link, so the 3 × 3 cells about a building hold every partner it has, and
//! the result is exactly the brute-force linkage of every pair.

use std::collections::HashMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::classes::{Family, dims};
use crate::decode::scn5::Pool;
use crate::source::{Object, Refusal, Source};

/// The most a gap between two buildings' circles may be for them to join.
pub const GAP_M: f64 = 200.0;
/// A building's circle is half its box's longer side, never more than
/// this: a long hangar or a wall-like block should not bridge two places.
pub const RADIUS_CAP_M: f64 = 60.0;
/// A group of fewer buildings is no place.
pub const MIN_BUILDINGS: u32 = 3;
/// A town point names the place holding most buildings within this of it.
pub const NAME_REACH_M: f64 = 1000.0;
/// A storey's height, which turns a box's height into storeys.
const STOREY_M: f64 = 3.0;
/// The side of the cells occupied area is counted in: a hectare.
const HECTARE_M: f64 = 100.0;

/// One settlement building as places are made from it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Building {
    /// Its scenery object's id.
    pub id: u64,
    pub x: f64,
    pub z: f64,
    /// Half its box's longer side, capped at `RADIUS_CAP_M`.
    pub r: f32,
    pub family: Family,
    /// Footprint times storeys, a storey 3 m of its box's height and at
    /// least one, in square meters.
    pub floor: f32,
}

impl Building {
    /// The object as a building, or `None` when its family makes no place.
    pub fn of(o: &Object) -> Option<Building> {
        if !o.class.is_structure() {
            return None;
        }
        Building::new(o.id, o.pos[0], o.pos[2], o.family?, &o.bbox)
    }

    /// An object of `family` whose model's box is `bbox`, or `None` when
    /// the family makes no place.
    pub fn new(id: u64, x: f64, z: f64, family: Family, bbox: &[f32; 6]) -> Option<Building> {
        if !family.settles() {
            return None;
        }
        let (w, d, h) = dims(bbox);
        let (w, d) = (w.abs(), d.abs());
        let storeys = (h / STOREY_M).round().max(1.0);
        Some(Building {
            id,
            x,
            z,
            r: (w.max(d) / 2.0).min(RADIUS_CAP_M) as f32,
            family,
            floor: (w * d * storeys) as f32,
        })
    }

    /// Whether `self` and `o` join: the gap between their circles is at
    /// most `GAP_M`.
    pub fn links(&self, o: &Building) -> bool {
        (self.x - o.x).hypot(self.z - o.z) <= GAP_M + f64::from(self.r) + f64::from(o.r)
    }

    fn residential(&self) -> bool {
        matches!(self.family, Family::House | Family::Apartment)
    }
}

/// Union-find with path halving, each set named by its least member.
struct Sets(Vec<u32>);

impl Sets {
    fn new(n: usize) -> Sets {
        Sets((0..n as u32).collect())
    }

    fn find(&mut self, mut a: u32) -> u32 {
        while self.0[a as usize] != a {
            let up = self.0[self.0[a as usize] as usize];
            self.0[a as usize] = up;
            a = up;
        }
        a
    }

    fn union(&mut self, a: u32, b: u32) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            self.0[a.max(b) as usize] = a.min(b);
        }
    }
}

/// Points bucketed in square cells of side `cell`, each bucket's indices
/// ascending.
pub(crate) struct Buckets {
    cell: f64,
    of: HashMap<(i64, i64), Vec<u32>>,
}

impl Buckets {
    pub(crate) fn new(cell: f64, points: impl Iterator<Item = (f64, f64)>) -> Buckets {
        let mut of: HashMap<(i64, i64), Vec<u32>> = HashMap::new();
        for (i, (x, z)) in points.enumerate() {
            of.entry(Self::key(cell, x, z)).or_default().push(i as u32);
        }
        Buckets { cell, of }
    }

    fn key(cell: f64, x: f64, z: f64) -> (i64, i64) {
        ((x / cell).floor() as i64, (z / cell).floor() as i64)
    }

    /// Calls `f` with every point in a cell meeting the square of half-side
    /// `r` about `(x, z)`, and perhaps a few beyond it.
    pub(crate) fn near(&self, x: f64, z: f64, r: f64, mut f: impl FnMut(u32)) {
        let (a, b) = (
            Self::key(self.cell, x - r, z - r),
            Self::key(self.cell, x + r, z + r),
        );
        for kx in a.0..=b.0 {
            for kz in a.1..=b.1 {
                if let Some(v) = self.of.get(&(kx, kz)) {
                    v.iter().for_each(|&i| f(i));
                }
            }
        }
    }
}

/// Each building's group under single linkage at `GAP_M`, as the index of
/// the group's first building.
pub fn link(b: &[Building]) -> Vec<u32> {
    // No two buildings farther apart than this link, so a building's
    // partners all lie in its own cell or the eight about it.
    let cell = GAP_M + 2.0 * RADIUS_CAP_M;
    let buckets = Buckets::new(cell, b.iter().map(|b| (b.x, b.z)));
    let mut sets = Sets::new(b.len());
    // Each pair of cells once: a cell with itself and with the four of the
    // eight about it that come after it.
    const AFTER: [(i64, i64); 4] = [(0, 1), (1, -1), (1, 0), (1, 1)];
    for (&(kx, kz), here) in &buckets.of {
        for (n, &i) in here.iter().enumerate() {
            for &j in &here[n + 1..] {
                if b[i as usize].links(&b[j as usize]) {
                    sets.union(i, j);
                }
            }
        }
        for (dx, dz) in AFTER {
            let Some(there) = buckets.of.get(&(kx + dx, kz + dz)) else {
                continue;
            };
            for &i in here {
                for &j in there {
                    if b[i as usize].links(&b[j as usize]) {
                        sets.union(i, j);
                    }
                }
            }
        }
    }
    (0..b.len() as u32).map(|i| sets.find(i)).collect()
}

/// A place's size, in the terrain's own units: buildings, residential
/// floor and the share of apartment blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SizeClass {
    Hamlet,
    Village,
    LargeVillage,
    Town,
    City,
    LargeCity,
}

impl SizeClass {
    pub const ALL: [SizeClass; 6] = [
        SizeClass::Hamlet,
        SizeClass::Village,
        SizeClass::LargeVillage,
        SizeClass::Town,
        SizeClass::City,
        SizeClass::LargeCity,
    ];

    pub fn name(self) -> &'static str {
        match self {
            SizeClass::Hamlet => "hamlet",
            SizeClass::Village => "village",
            SizeClass::LargeVillage => "large_village",
            SizeClass::Town => "town",
            SizeClass::City => "city",
            SizeClass::LargeCity => "large_city",
        }
    }

    /// Fitted on 47 Caucasus towns against their census populations.
    pub fn of(buildings: u32, apartment_share: f64, residential_floor: f64) -> SizeClass {
        if buildings < 50 {
            SizeClass::Hamlet
        } else if buildings < 200 {
            SizeClass::Village
        } else if residential_floor >= 20e6 {
            SizeClass::LargeCity
        } else if residential_floor >= 3e6 && apartment_share >= 0.10 {
            SizeClass::City
        } else if apartment_share >= 0.05 || residential_floor >= 1e6 {
            SizeClass::Town
        } else {
            SizeClass::LargeVillage
        }
    }
}

/// How a place is laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Form {
    Compact,
    /// A strung-out spread of houses no gap separates.
    Dispersed,
    /// Several named towns grown together.
    Agglomeration,
}

impl Form {
    pub const ALL: [Form; 3] = [Form::Compact, Form::Dispersed, Form::Agglomeration];

    pub fn name(self) -> &'static str {
        match self {
            Form::Compact => "compact",
            Form::Dispersed => "dispersed",
            Form::Agglomeration => "agglomeration",
        }
    }

    /// Dispersed is 1 000 buildings or more, under 8 % apartments and under
    /// 1 500 m² of residential floor per occupied hectare: the lowland
    /// villages of western Georgia, which run on for kilometers.
    pub fn of(names: usize, buildings: u32, apartment_share: f64, floor_per_ha: f64) -> Form {
        if names > 1 {
            Form::Agglomeration
        } else if buildings >= 1000 && apartment_share < 0.08 && floor_per_ha < 1500.0 {
            Form::Dispersed
        } else {
            Form::Compact
        }
    }
}

/// One place: a group of `MIN_BUILDINGS` or more settlement buildings.
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    /// The least scenery object id among its buildings: stable for as long
    /// as the scene file is.
    pub id: u64,
    /// The mean of its buildings' positions.
    pub x: f64,
    pub z: f64,
    /// Its buildings' positions' box, min x, min z, max x, max z.
    pub bbox: [f64; 4],
    pub buildings: u32,
    pub houses: u32,
    pub apartments: u32,
    pub shops: u32,
    pub civic: u32,
    pub industrial: u32,
    /// The floor of its houses and apartments, square meters.
    pub residential_floor: f64,
    /// The 100 m cells holding one of its buildings.
    pub hectares: u32,
    /// Indices into the terrain's towns, the one it is named by first.
    pub names: Vec<u32>,
    pub size: SizeClass,
    pub form: Form,
}

impl Place {
    pub fn apartment_share(&self) -> f64 {
        f64::from(self.apartments) / f64::from(self.buildings.max(1))
    }
}

/// Every place of a terrain, in id order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Places {
    pub places: Vec<Place>,
}

/// The places `buildings` make, named from `towns`, each town `(x, z)` in
/// the terrain's table order. A town point names the place holding most
/// buildings within `NAME_REACH_M` of it, ties to the place of least id; a
/// place several points name takes the name of the one with most of its
/// buildings within reach, ties to the first in table order, and lists the
/// others after it.
pub fn group(buildings: &[Building], towns: &[(f64, f64)]) -> Places {
    let label = link(buildings);
    // A group's members, its label first.
    let mut members: HashMap<u32, Vec<u32>> = HashMap::new();
    for (i, &l) in label.iter().enumerate() {
        members.entry(l).or_default().push(i as u32);
    }
    members.retain(|_, m| m.len() >= MIN_BUILDINGS as usize);

    // Buildings within reach of each town, by group.
    let buckets = Buckets::new(NAME_REACH_M, buildings.iter().map(|b| (b.x, b.z)));
    let mut named: HashMap<u32, Vec<(u32, u32)>> = HashMap::new();
    for (t, &(tx, tz)) in towns.iter().enumerate() {
        let mut count: HashMap<u32, u32> = HashMap::new();
        buckets.near(tx, tz, NAME_REACH_M, |i| {
            let b = &buildings[i as usize];
            if (b.x - tx).hypot(b.z - tz) <= NAME_REACH_M
                && members.contains_key(&label[i as usize])
            {
                *count.entry(label[i as usize]).or_default() += 1;
            }
        });
        let least_id = |l: u32| members[&l].iter().map(|&i| buildings[i as usize].id).min();
        let best = count
            .into_iter()
            .max_by(|a, b| a.1.cmp(&b.1).then(least_id(b.0).cmp(&least_id(a.0))));
        if let Some((l, n)) = best {
            named.entry(l).or_default().push((t as u32, n));
        }
    }

    let mut places: Vec<Place> = members
        .iter()
        .map(|(l, m)| {
            let mut names = named.remove(l).unwrap_or_default();
            // Most buildings first, then table order.
            names.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            summarize(
                m.iter().map(|&i| &buildings[i as usize]),
                names.iter().map(|n| n.0).collect(),
            )
        })
        .collect();
    places.sort_by_key(|p| p.id);
    Places { places }
}

/// A place from its buildings and its names.
fn summarize<'a>(members: impl Iterator<Item = &'a Building>, names: Vec<u32>) -> Place {
    let mut p = Place {
        id: u64::MAX,
        x: 0.0,
        z: 0.0,
        bbox: [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ],
        buildings: 0,
        houses: 0,
        apartments: 0,
        shops: 0,
        civic: 0,
        industrial: 0,
        residential_floor: 0.0,
        hectares: 0,
        names,
        size: SizeClass::Hamlet,
        form: Form::Compact,
    };
    let mut cells = std::collections::HashSet::new();
    for b in members {
        p.id = p.id.min(b.id);
        p.x += b.x;
        p.z += b.z;
        p.bbox = [
            p.bbox[0].min(b.x),
            p.bbox[1].min(b.z),
            p.bbox[2].max(b.x),
            p.bbox[3].max(b.z),
        ];
        p.buildings += 1;
        match b.family {
            Family::House => p.houses += 1,
            Family::Apartment => p.apartments += 1,
            Family::Shop => p.shops += 1,
            Family::Civic => p.civic += 1,
            Family::Industrial => p.industrial += 1,
            _ => {}
        }
        if b.residential() {
            p.residential_floor += f64::from(b.floor);
        }
        cells.insert(Buckets::key(HECTARE_M, b.x, b.z));
    }
    p.x /= f64::from(p.buildings);
    p.z /= f64::from(p.buildings);
    p.hectares = cells.len() as u32;
    let share = p.apartment_share();
    p.size = SizeClass::of(p.buildings, share, p.residential_floor);
    let density = p.residential_floor / f64::from(p.hectares);
    p.form = Form::of(p.names.len(), p.buildings, share, density);
    p
}

/// Every settlement building of `src` within `b`, min x, min z, max x,
/// max z, once each, in id order.
pub fn buildings_in(src: &dyn Source, b: [f64; 4]) -> Result<Vec<Building>, Refusal> {
    let mut out = Vec::new();
    src.objects_in(Pool::Objects, b, &mut |o| {
        if let Some(x) = Building::of(o) {
            out.push(x);
        }
    })?;
    out.sort_by_key(|b| b.id);
    out.dedup_by_key(|b| b.id);
    Ok(out)
}

/// The places of the whole of `src`, named from its towns.
pub fn build(src: &dyn Source) -> Result<Places, Refusal> {
    let g = src.grid();
    let c = f64::from(g.cell_size);
    let (x1, z1) = (
        g.origin_x + f64::from(g.height) * c,
        g.origin_z + f64::from(g.width) * c,
    );
    // Scenery can stand past the mesh's last cell.
    let pad = 10_000.0;
    let all = buildings_in(
        src,
        [g.origin_x - pad, g.origin_z - pad, x1 + pad, z1 + pad],
    )?;
    let towns: Vec<(f64, f64)> = src.tables()?.towns.iter().map(|t| (t.x, t.z)).collect();
    Ok(group(&all, &towns))
}

const MAGIC: &[u8; 4] = b"PLC1";

impl Places {
    /// Little-endian rows: the count, then each place's fixed fields and
    /// its names' town indices.
    pub fn encode(&self) -> Vec<u8> {
        let mut o = Vec::with_capacity(16 + self.places.len() * 96);
        o.extend_from_slice(MAGIC);
        o.extend_from_slice(&(self.places.len() as u32).to_le_bytes());
        for p in &self.places {
            o.extend_from_slice(&p.id.to_le_bytes());
            for v in [
                p.x,
                p.z,
                p.bbox[0],
                p.bbox[1],
                p.bbox[2],
                p.bbox[3],
                p.residential_floor,
            ] {
                o.extend_from_slice(&v.to_le_bytes());
            }
            for v in [
                p.buildings,
                p.houses,
                p.apartments,
                p.shops,
                p.civic,
                p.industrial,
                p.hectares,
                p.names.len() as u32,
            ] {
                o.extend_from_slice(&v.to_le_bytes());
            }
            for &n in &p.names {
                o.extend_from_slice(&n.to_le_bytes());
            }
            let size = SizeClass::ALL.iter().position(|&s| s == p.size).unwrap() as u8;
            let form = Form::ALL.iter().position(|&f| f == p.form).unwrap() as u8;
            o.extend_from_slice(&[size, form]);
        }
        o
    }

    pub fn decode(b: &[u8]) -> Result<Places, String> {
        let mut at = 0usize;
        let mut take = |n: usize| -> Result<&[u8], String> {
            let s = b.get(at..at + n).ok_or("the places end early")?;
            at += n;
            Ok(s)
        };
        if take(4)? != MAGIC {
            return Err("not a places payload".into());
        }
        let u32_ = |s: &[u8]| u32::from_le_bytes(s.try_into().unwrap());
        let n = u32_(take(4)?);
        let mut places = Vec::with_capacity(n.min(1 << 20) as usize);
        for _ in 0..n {
            let id = u64::from_le_bytes(take(8)?.try_into().unwrap());
            let mut f = [0f64; 7];
            for v in &mut f {
                *v = f64::from_le_bytes(take(8)?.try_into().unwrap());
            }
            let mut c = [0u32; 8];
            for v in &mut c {
                *v = u32_(take(4)?);
            }
            let names = (0..c[7])
                .map(|_| take(4).map(u32_))
                .collect::<Result<_, _>>()?;
            let k = take(2)?;
            let size = *SizeClass::ALL
                .get(k[0] as usize)
                .ok_or("an unknown size class")?;
            let form = *Form::ALL.get(k[1] as usize).ok_or("an unknown form")?;
            places.push(Place {
                id,
                x: f[0],
                z: f[1],
                bbox: [f[2], f[3], f[4], f[5]],
                residential_floor: f[6],
                buildings: c[0],
                houses: c[1],
                apartments: c[2],
                shops: c[3],
                civic: c[4],
                industrial: c[5],
                hectares: c[6],
                names,
                size,
                form,
            });
        }
        if at != b.len() {
            return Err("the places run on past their count".into());
        }
        Ok(Places { places })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small generator, so each test's buildings are the same each run.
    struct Rng(u64);
    impl Rng {
        fn unit(&mut self) -> f64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 >> 11) as f64 / (1u64 << 53) as f64
        }
    }

    fn house(id: u64, x: f64, z: f64, r: f32) -> Building {
        Building {
            id,
            x,
            z,
            r,
            family: Family::House,
            floor: 100.0,
        }
    }

    /// Single linkage by testing every pair: the oracle the grid hash must
    /// equal, as the least index of each building's group.
    fn every_pair(b: &[Building]) -> Vec<u32> {
        let mut sets = Sets::new(b.len());
        for i in 0..b.len() {
            for j in i + 1..b.len() {
                if b[i].links(&b[j]) {
                    sets.union(i as u32, j as u32);
                }
            }
        }
        (0..b.len() as u32).map(|i| sets.find(i)).collect()
    }

    /// Villages of random size and spread, scattered houses between them,
    /// and pairs placed at exactly the linking distance, so ties are tested.
    fn scatter(seed: u64, n: usize) -> Vec<Building> {
        let mut g = Rng(seed);
        let mut out = Vec::new();
        while out.len() < n {
            let (cx, cz) = (
                g.unit() * 20_000.0 - 10_000.0,
                g.unit() * 20_000.0 - 10_000.0,
            );
            let spread = 50.0 + g.unit() * 1500.0;
            for _ in 0..(1 + (g.unit() * 60.0) as usize) {
                let r = if g.unit() < 0.1 {
                    60.0
                } else {
                    (g.unit() * 20.0) as f32
                };
                let id = 1000 + out.len() as u64;
                out.push(house(
                    id,
                    cx + (g.unit() - 0.5) * spread,
                    cz + (g.unit() - 0.5) * spread,
                    r,
                ));
            }
            let last = *out.last().unwrap();
            let d = GAP_M + f64::from(last.r) + 5.0;
            out.push(house(1000 + out.len() as u64, last.x + d, last.z, 5.0));
        }
        out
    }

    #[test]
    fn the_grid_hash_links_exactly_the_pairs_every_pair_does() {
        for seed in 1..=20u64 {
            let b = scatter(0x9e37_79b9_7f4a_7c15 ^ seed, 600);
            assert_eq!(link(&b), every_pair(&b), "seed {seed}");
        }
    }

    #[test]
    fn a_gap_of_exactly_200_m_links_and_a_centimeter_more_does_not() {
        let at = |gap: f64| vec![house(1, 0.0, 0.0, 10.0), house(2, 0.0, gap + 20.0, 10.0)];
        assert_eq!(link(&at(200.0)), [0, 0]);
        assert_eq!(link(&at(200.01)), [0, 1]);
        // The cell edge is no barrier: a pair across it, and one three
        // cells apart in a chain.
        let chain: Vec<_> = (0..5)
            .map(|k| house(k, 319.0 + 300.0 * k as f64, 0.0, 50.0))
            .collect();
        assert_eq!(link(&chain), [0; 5]);
    }

    #[test]
    fn a_place_needs_three_buildings_and_its_id_is_its_least_object_id() {
        let mut b = vec![house(50, 0.0, 0.0, 5.0), house(7, 100.0, 0.0, 5.0)];
        assert!(group(&b, &[]).places.is_empty());
        b.push(house(30, 200.0, 0.0, 5.0));
        let p = group(&b, &[]);
        assert_eq!(p.places.len(), 1);
        assert_eq!(p.places[0].id, 7);
        assert_eq!(p.places[0].buildings, 3);
        assert!((p.places[0].x - 100.0).abs() < 1e-9);
    }

    #[test]
    fn a_town_names_the_place_with_most_buildings_within_reach() {
        // A big place 900 m east of the point and a small one 100 m west:
        // the point names the big one, which has more within 1 km.
        let mut b: Vec<_> = (0..10)
            .map(|k| house(100 + k, 0.0, 900.0 + 10.0 * k as f64, 2.0))
            .collect();
        b.extend((0..3).map(|k| house(200 + k, 0.0, -100.0 - 5.0 * k as f64, 2.0)));
        let p = group(&b, &[(0.0, 0.0)]);
        let named: Vec<_> = p.places.iter().filter(|q| !q.names.is_empty()).collect();
        assert_eq!(named.len(), 1);
        assert_eq!(named[0].id, 100);
        // Two places with the same count within reach: the least id wins.
        let b: Vec<_> = [
            (300, 500.0),
            (301, 510.0),
            (302, 520.0),
            (10, -500.0),
            (11, -510.0),
            (12, -520.0),
        ]
        .into_iter()
        .map(|(id, z)| house(id, 0.0, z, 2.0))
        .collect();
        let p = group(&b, &[(0.0, 0.0)]);
        assert_eq!(
            p.places.iter().find(|q| !q.names.is_empty()).unwrap().id,
            10
        );
    }

    #[test]
    fn several_names_make_an_agglomeration_named_by_its_largest_member() {
        // A row of houses 2 km long; the second town has more of it within
        // reach, the first comes first in the table.
        let b: Vec<_> = (0..41)
            .map(|k| house(k, 0.0, 50.0 * k as f64, 2.0))
            .collect();
        let towns = [(0.0, -500.0), (0.0, 1000.0)];
        let p = group(&b, &towns);
        assert_eq!(p.places.len(), 1);
        assert_eq!(p.places[0].names, [1, 0]);
        assert_eq!(p.places[0].form, Form::Agglomeration);
        // Equal counts keep table order.
        let p = group(&b, &[(0.0, 2500.0), (0.0, -500.0)]);
        assert_eq!(p.places[0].names, [0, 1]);
    }

    #[test]
    fn size_classes_follow_buildings_floor_and_apartments() {
        use SizeClass::*;
        assert_eq!(SizeClass::of(3, 0.0, 0.0), Hamlet);
        assert_eq!(SizeClass::of(49, 1.0, 1e9), Hamlet);
        assert_eq!(SizeClass::of(50, 0.0, 0.0), Village);
        assert_eq!(SizeClass::of(200, 0.04, 0.9e6), LargeVillage);
        assert_eq!(SizeClass::of(200, 0.05, 0.0), Town);
        assert_eq!(SizeClass::of(200, 0.0, 1e6), Town);
        assert_eq!(SizeClass::of(200, 0.09, 5e6), Town);
        assert_eq!(SizeClass::of(200, 0.10, 3e6), City);
        assert_eq!(SizeClass::of(200, 0.0, 20e6), LargeCity);
        assert_eq!(Form::of(1, 1000, 0.07, 1499.0), Form::Dispersed);
        assert_eq!(Form::of(1, 999, 0.07, 1499.0), Form::Compact);
        assert_eq!(Form::of(0, 1000, 0.08, 1499.0), Form::Compact);
        assert_eq!(Form::of(1, 1000, 0.0, 1500.0), Form::Compact);
        assert_eq!(Form::of(2, 3, 0.0, 0.0), Form::Agglomeration);
    }

    #[test]
    fn a_place_counts_its_families_and_residential_floor() {
        let mut b: Vec<_> = (0..4)
            .map(|k| house(k, 0.0, 30.0 * k as f64, 5.0))
            .collect();
        for (k, f) in [
            Family::Apartment,
            Family::Shop,
            Family::Civic,
            Family::Industrial,
        ]
        .into_iter()
        .enumerate()
        {
            b.push(Building {
                family: f,
                floor: 1000.0,
                ..house(10 + k as u64, 130.0, 30.0 * k as f64, 5.0)
            });
        }
        let p = &group(&b, &[]).places[0];
        assert_eq!(
            (
                p.buildings,
                p.houses,
                p.apartments,
                p.shops,
                p.civic,
                p.industrial
            ),
            (8, 4, 1, 1, 1, 1)
        );
        assert_eq!(p.residential_floor, 4.0 * 100.0 + 1000.0);
        assert_eq!(p.hectares, 2);
        assert_eq!(p.bbox, [0.0, 0.0, 130.0, 90.0]);
    }

    #[test]
    fn places_round_trip_through_their_encoding() {
        let b = scatter(7, 800);
        let towns: Vec<_> = b.iter().step_by(37).map(|b| (b.x + 300.0, b.z)).collect();
        let p = group(&b, &towns);
        assert!(p.places.iter().any(|q| q.names.len() > 1));
        let e = p.encode();
        assert_eq!(Places::decode(&e).unwrap(), p);
        assert!(Places::decode(&e[..e.len() - 1]).is_err());
        let mut long = e.clone();
        long.push(0);
        assert!(Places::decode(&long).is_err());
    }
}
