//! A box answered from a pool's own tree: the nodes whose box meets it,
//! then the run of world boxes each owns, then one read into the instancer
//! node of each box that meets it. Every read is positional, so a query
//! touches only the bytes it needs.

use super::{
    Error, Index, Instance, Pool, f32_at, heading_of, layout, malformed, model_of, u32_at,
};
use crate::decode::surface5::ReadAt;

const NODE: usize = 48;
const NONE: u32 = u32::MAX;

/// Whether `[x0, z0, x1, z1]` boxes `a` and `b` meet, edges included.
fn meets(a: [f64; 4], b: [f64; 4]) -> bool {
    a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3]
}

/// The x, z of a world box stored as f32 min x, y, z, max x, y, z at `at`.
fn flat(b: &[u8], at: usize) -> [f64; 4] {
    [0, 2, 3, 5].map(|k| f64::from(f32_at(b, at + 4 * k)))
}

/// The ground a model covers, as a box: its model-local box turned by
/// `rotation` and moved to `pos`. Local X points along the heading, local Z
/// a right angle clockwise of it.
pub fn footprint(bbox: &[f32; 6], pos: [f64; 3], rotation: f64) -> [f64; 4] {
    let (s, c) = rotation.sin_cos();
    let mut out = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for (lx, lz) in [(0, 2), (3, 2), (0, 5), (3, 5)] {
        let (lx, lz) = (f64::from(bbox[lx]), f64::from(bbox[lz]));
        let x = pos[0] + lx * c - lz * s;
        let z = pos[2] + lx * s + lz * c;
        out = [out[0].min(x), out[1].min(z), out[2].max(x), out[3].max(z)];
    }
    out
}

impl Index {
    fn read<R: ReadAt + ?Sized>(&self, r: &R, at: u64, buf: &mut [u8]) -> Result<(), Error> {
        Ok(r.read_exact_at(buf, self.data_base + at)?)
    }

    /// The instance an id names, read from its node's arrays.
    fn instance<R: ReadAt + ?Sized>(&self, r: &R, pool: Pool, id: u64) -> Result<Instance, Error> {
        let index = id & ((1 << self.shift) - 1);
        let Some(node) = usize::try_from(id >> self.shift)
            .ok()
            .and_then(|k| self.nodes.get(k))
            .filter(|n| index < u64::from(n.n))
        else {
            return Err(malformed(format!(
                "id {id} in the {} pool names an instance the nodes do not hold",
                pool.name()
            )));
        };
        let stride = layout(self.version).axis_stride;
        let mut p = [0u8; 12];
        let mut word = [0u8; 4];
        let mut ax = [0u8; 12];
        let ax = &mut ax[..usize::from(stride)];
        self.read(r, node.p + 12 * index, &mut p)?;
        self.read(r, node.model + 4 * index, &mut word)?;
        self.read(r, node.axis + u64::from(stride) * index, ax)?;
        let model = model_of(self.version, u32::from_le_bytes(word));
        if model as usize >= self.models.len() {
            return Err(malformed(format!(
                "instance {id} names model {model}, past the {} the table holds",
                self.models.len()
            )));
        }
        let rotation = heading_of(self.version, ax);
        let pos: [f64; 3] = std::array::from_fn(|k| node.t[k] + f64::from(f32_at(&p, 4 * k)));
        if !(rotation.is_finite() && pos.iter().all(|v| v.is_finite())) {
            return Err(malformed(format!(
                "instance {id} has a position or axis that is not a number"
            )));
        }
        Ok(Instance {
            id,
            pool,
            model,
            pos,
            rotation,
        })
    }

    /// How many instances of each model `pool` lists, by model index, the
    /// group records counted as objects. Reads the pool's ids, then the
    /// model words of each node they name, a node at a time.
    pub fn counts<R: ReadAt + ?Sized>(&self, r: &R, pool: Pool) -> Result<Vec<u64>, Error> {
        const IDS: u64 = 1 << 16;
        let toc = &self.pools[Pool::ALL.iter().position(|&p| p == pool).unwrap()];
        let mut by_node: Vec<Vec<u32>> = vec![Vec::new(); self.nodes.len()];
        let mut ids = Vec::new();
        let mut i = 0;
        while i < toc.count {
            let n = IDS.min(toc.count - i);
            ids.resize(8 * n as usize, 0);
            self.read(r, toc.ids + 8 * i, &mut ids)?;
            for (j, id) in ids.as_chunks::<8>().0.iter().enumerate() {
                let id = u64::from_le_bytes(*id);
                let index = id & ((1 << self.shift) - 1);
                match usize::try_from(id >> self.shift)
                    .ok()
                    .filter(|&k| k < self.nodes.len() && index < u64::from(self.nodes[k].n))
                {
                    Some(k) => by_node[k].push(index as u32),
                    None => {
                        return Err(malformed(format!(
                            "id {id}, entry {} of the {} pool, names an instance the nodes do not hold",
                            i + j as u64,
                            pool.name()
                        )));
                    }
                }
            }
            i += n;
        }
        let mut counts = vec![0u64; self.models.len()];
        let mut words = Vec::new();
        for (node, here) in self.nodes.iter().zip(&by_node) {
            if here.is_empty() {
                continue;
            }
            words.resize(4 * node.n as usize, 0);
            self.read(r, node.model, &mut words)?;
            for &index in here {
                let at = 4 * index as usize;
                let word = u32::from_le_bytes(words[at..at + 4].try_into().unwrap());
                let model = model_of(self.version, word);
                let Some(c) = counts.get_mut(model as usize) else {
                    return Err(malformed(format!(
                        "an instance names model {model}, past the {} the table holds",
                        self.models.len()
                    )));
                };
                *c += 1;
            }
        }
        if pool == Pool::Objects {
            for g in &self.group {
                counts[g.model as usize] += 1;
            }
        }
        Ok(counts)
    }

    /// Calls `f` with every instance of `pool` whose box in that pool meets
    /// `b`, min x, min z, max x, max z in DCS meters, edges included, in the
    /// tree's order. For objects, then every group record whose footprint
    /// meets it, its id following the last node's.
    pub fn each_in<R, F>(&self, r: &R, pool: Pool, b: [f64; 4], mut f: F) -> Result<(), Error>
    where
        R: ReadAt + ?Sized,
        F: FnMut(&Instance),
    {
        let toc = &self.pools[Pool::ALL.iter().position(|&p| p == pool).unwrap()];
        let mut stack = vec![0u32];
        let mut visited = 0u64;
        let mut node = [0u8; NODE];
        let (mut boxes, mut ids) = (Vec::new(), Vec::new());
        while let Some(k) = stack.pop() {
            visited += 1;
            // A tree visits each node once; a corrupt one could loop.
            if visited > toc.tree_nodes {
                return Err(malformed(format!(
                    "the {} pool's tree reaches more nodes than its {}",
                    pool.name(),
                    toc.tree_nodes
                )));
            }
            self.read(r, toc.tree + NODE as u64 * u64::from(k), &mut node)?;
            if !meets(flat(&node, 0), b) {
                continue;
            }
            let first = u64::from(u32_at(&node, 24).unwrap());
            let count = u64::from(u32_at(&node, 28).unwrap());
            if first + count > toc.count {
                return Err(malformed(format!(
                    "node {k} of the {} pool's tree owns entries {first}..{}, past the {} the pool holds",
                    pool.name(),
                    first + count,
                    toc.count
                )));
            }
            if count > 0 {
                boxes.resize(24 * count as usize, 0);
                self.read(r, toc.aabbs + 24 * first, &mut boxes)?;
                ids.clear();
                for j in 0..count as usize {
                    if !meets(flat(&boxes, 24 * j), b) {
                        continue;
                    }
                    // The ids are read only for a node with a hit.
                    if ids.is_empty() {
                        ids.resize(8 * count as usize, 0);
                        self.read(r, toc.ids + 8 * first, &mut ids)?;
                    }
                    let id = u64::from_le_bytes(ids[8 * j..8 * j + 8].try_into().unwrap());
                    f(&self.instance(r, pool, id)?);
                }
            }
            for c in 0..4 {
                let child = u32_at(&node, 32 + 4 * c).unwrap();
                if child == NONE {
                    continue;
                }
                if u64::from(child) >= toc.tree_nodes {
                    return Err(malformed(format!(
                        "node {k} of the {} pool's tree names child {child}, past its {} nodes",
                        pool.name(),
                        toc.tree_nodes
                    )));
                }
                stack.push(child);
            }
        }
        if pool == Pool::Objects {
            let first = (self.nodes.len() as u64) << self.shift;
            for (i, g) in self.group.iter().enumerate() {
                let bbox = &self.models[g.model as usize].bbox;
                if meets(footprint(bbox, g.pos, g.rotation), b) {
                    f(&Instance {
                        id: first + i as u64,
                        pool,
                        model: g.model,
                        pos: g.pos,
                        rotation: g.rotation,
                    });
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::io::Cursor;

    use super::super::build::{self, Record};
    use super::super::tests::scene;
    use super::super::{Scene, TREE_NODE};
    use super::*;

    fn open(built: &build::Scene) -> (Vec<u8>, Scene, Index) {
        let b = built.file();
        let mut r = Cursor::new(&b[..]);
        let s = Scene::open(&mut r, "X.scn5").unwrap();
        let i = s.index(&mut r).unwrap();
        (b, s, i)
    }

    /// Every instance the walk gives, by pool and id.
    fn walked(b: &[u8], s: &Scene) -> BTreeMap<(u8, u64), Instance> {
        let mut out = BTreeMap::new();
        s.walk(&mut Cursor::new(b), |i| {
            out.insert((i.pool as u8, i.id), i.clone());
            Ok(())
        })
        .unwrap();
        out
    }

    fn query(b: &[u8], i: &Index, pool: Pool, q: [f64; 4]) -> Vec<Instance> {
        let mut out = Vec::new();
        i.each_in(b, pool, q, |x| out.push(x.clone())).unwrap();
        out
    }

    /// A scene of four nodes of 150 instances scattered over 8 km, every
    /// third instance in each pool, so each pool's tree is a few levels deep.
    fn scattered(version: u32) -> build::Scene {
        let mut s = scene(version);
        let mut seed = 7u64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        s.nodes = (0..4)
            .map(|k| build::Node {
                t: [-150_000.0 + 2000.0 * f64::from(k), 10.0, 450_000.0],
                p: (0..150)
                    .map(|_| [(8000.0 * next()) as f32, 5.0, (8000.0 * next()) as f32])
                    .collect(),
                model: (0..150).map(|i| i % 3).collect(),
                axis: vec![[0.6, 0.8]; 150],
            })
            .collect();
        s.pools = [0u64, 1, 2].map(|p| {
            (0..4u64)
                .flat_map(|n| (0..150u64).map(move |i| (n, i)))
                .filter(|(n, i)| (n + i) % 3 == p)
                .collect()
        });
        s
    }

    #[test]
    fn counts_each_model_as_the_walk_does() {
        for version in [8, 9] {
            let built = scattered(version);
            let (b, s, idx) = open(&built);
            for pool in Pool::ALL {
                let mut want = vec![0u64; s.models.len()];
                for ((p, _), i) in walked(&b, &s) {
                    if p == pool as u8 {
                        want[i.model as usize] += 1;
                    }
                }
                assert_eq!(idx.counts(&b[..], pool).unwrap(), want, "{pool:?}");
            }
        }
    }

    #[test]
    fn a_box_finds_what_a_scan_of_every_pool_box_finds() {
        for version in [8, 9] {
            let built = scattered(version);
            let (b, s, idx) = open(&built);
            let all = walked(&b, &s);
            for (k, pool) in Pool::ALL.into_iter().enumerate() {
                for q in 0..60 {
                    let x = -150_500.0 + 220.0 * f64::from(q);
                    let z = 449_500.0 + 150.0 * f64::from(q % 50);
                    let qb = [x, z, x + 900.0 + 40.0 * f64::from(q), z + 700.0];
                    let mut want: Vec<u64> = built.pools[k]
                        .iter()
                        .filter(|&&e| {
                            let [wx, _, wz] = built.world(e);
                            // The builder's boxes reach a meter each way.
                            meets([wx - 1.0, wz - 1.0, wx + 1.0, wz + 1.0], qb)
                        })
                        .map(|&e| built.id(e))
                        .collect();
                    if pool == Pool::Objects {
                        let first = 4u64 << 13;
                        for (i, g) in built.group.iter().enumerate() {
                            let bbox = &s.models[g.model as usize].bbox;
                            if meets(footprint(bbox, g.pos, g.heading), qb) {
                                want.push(first + i as u64);
                            }
                        }
                    }
                    want.sort_unstable();
                    let got = query(&b, &idx, pool, qb);
                    for g in &got {
                        assert_eq!(Some(g), all.get(&(pool as u8, g.id)), "v{version}");
                    }
                    let mut ids: Vec<u64> = got.iter().map(|g| g.id).collect();
                    ids.sort_unstable();
                    assert_eq!(ids, want, "v{version} {} {qb:?}", pool.name());
                }
            }
        }
    }

    #[test]
    fn a_box_over_everything_gives_the_walk() {
        for version in [8, 9] {
            let (b, s, idx) = open(&scattered(version));
            let all = walked(&b, &s);
            let mut got = BTreeMap::new();
            for pool in Pool::ALL {
                for i in query(&b, &idx, pool, [-1e7, -1e7, 1e7, 1e7]) {
                    assert!(got.insert((pool as u8, i.id), i).is_none());
                }
            }
            assert_eq!(got, all, "v{version}");
            assert!(query(&b, &idx, Pool::Trees, [0.0, 0.0, 1.0, 1.0]).is_empty());
        }
    }

    #[test]
    fn a_group_records_footprint_turns_with_it() {
        // A 10 by 20 m box turned a right angle is 20 by 10.
        let b = footprint(
            &[-5.0, 0.0, -10.0, 5.0, 8.0, 10.0],
            [100.0, 0.0, 200.0],
            std::f64::consts::FRAC_PI_2,
        );
        let want = [90.0, 195.0, 110.0, 205.0];
        for (g, w) in b.iter().zip(want) {
            assert!((g - w).abs() < 1e-9, "{b:?}");
        }
        let mut built = scene(9);
        built.group = vec![Record {
            model: 0,
            pos: [-130_000.0, 40.0, 470_000.0],
            heading: std::f64::consts::FRAC_PI_2,
        }];
        let (bytes, _, idx) = open(&built);
        let hit = |x: f64, z: f64| !query(&bytes, &idx, Pool::Objects, [x, z, x, z]).is_empty();
        assert!(hit(-130_009.0, 470_004.0));
        assert!(!hit(-130_004.0, 470_009.0));
    }

    /// The file with the trees pool's tree changed by `edit`.
    fn with_node(edit: impl Fn(&mut [u8])) -> (Vec<u8>, Index) {
        let built = scattered(9);
        let (mut b, s, idx) = open(&built);
        let at = (s.data_base + s.pools[2].tree) as usize;
        edit(&mut b[at..at + (TREE_NODE * s.pools[2].tree_nodes) as usize]);
        (b, idx)
    }

    fn refusal(b: &[u8], idx: &Index) -> String {
        idx.each_in(b, Pool::Trees, [-1e7, -1e7, 1e7, 1e7], |_| {})
            .unwrap_err()
            .to_string()
    }

    #[test]
    fn refuses_a_tree_naming_a_child_past_its_nodes() {
        let (b, idx) = with_node(|t| t[32..36].copy_from_slice(&9999u32.to_le_bytes()));
        let e = refusal(&b, &idx);
        assert!(
            e.starts_with("node 0 of the trees pool's tree names child 9999"),
            "{e}"
        );
    }

    #[test]
    fn refuses_a_tree_that_loops() {
        let (b, idx) = with_node(|t| t[48 + 32..48 + 36].copy_from_slice(&0u32.to_le_bytes()));
        let e = refusal(&b, &idx);
        assert!(
            e.starts_with("the trees pool's tree reaches more nodes"),
            "{e}"
        );
    }

    #[test]
    fn refuses_a_node_owning_entries_past_the_pool() {
        let (b, idx) = with_node(|t| t[28..32].copy_from_slice(&5000u32.to_le_bytes()));
        let e = refusal(&b, &idx);
        assert!(
            e.starts_with("node 0 of the trees pool's tree owns entries 0..5000"),
            "{e}"
        );
    }
}
