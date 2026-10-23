//! Every pooled instance and group record, with its id, model, position and
//! heading, a node at a time.
//!
//! The pools list ids, not instances, so each pool's ids are first spread
//! into a tag per instance. Instances in no pool, light halos and small
//! plants, are not scenery and are skipped.

use std::io::{self, Read, Seek, SeekFrom};

use super::{Error, Pool, Scene, heading, malformed};

/// How many of a pool's ids are checked against their own boxes.
const SAMPLE: u64 = 1000;
/// How far outside its box an instance may lie and still be inside.
const SLACK_M: f64 = 0.5;
/// Every map seen puts over 99.9 % of its ids inside their own boxes; a
/// wrong shift puts almost none.
const MIN_INSIDE: f64 = 0.99;
/// Ids are read this many at a time.
const CHUNK: u64 = 1 << 20;

#[derive(Debug, Clone, PartialEq)]
pub struct Instance {
    /// `node << shift | index`.
    pub id: u64,
    pub pool: Pool,
    /// An index into the model table.
    pub model: u32,
    /// World x, y (altitude), z.
    pub pos: [f64; 3],
    /// Of the local X axis, radians from north towards east, in `[0, 2π)`.
    pub rotation: f64,
}

fn read_at<R: Read + Seek>(r: &mut R, at: u64, len: usize) -> io::Result<Vec<u8>> {
    r.seek(SeekFrom::Start(at))?;
    let mut b = vec![0u8; len];
    r.read_exact(&mut b)?;
    Ok(b)
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn f32_at(b: &[u8], at: usize) -> f64 {
    f64::from(f32::from_le_bytes(b[at..at + 4].try_into().unwrap()))
}

impl Scene {
    /// The first global instance number of each node.
    fn starts(&self) -> Vec<u64> {
        let mut s = 0;
        self.nodes
            .iter()
            .map(|n| {
                let at = s;
                s += u64::from(n.n);
                at
            })
            .collect()
    }

    /// The node and index an id names, when the file holds that instance.
    fn locate(&self, id: u64) -> Option<(usize, u64)> {
        let node = usize::try_from(id >> self.shift()).ok()?;
        let index = id & ((1 << self.shift()) - 1);
        (index < u64::from(self.nodes.get(node)?.n)).then_some((node, index))
    }

    fn world(&self, node: usize, p: &[u8], index: usize) -> [f64; 3] {
        let t = self.nodes[node].t;
        [0, 1, 2].map(|k| t[k] + f32_at(p, 12 * index + 4 * k))
    }

    /// Each instance's pools, bit k set for `Pool::ALL[k]`. An instance may
    /// be in more than one: in-pavement lights on Caucasus and leaf walls on
    /// Nevada are both objects and lines.
    fn tags<R: Read + Seek>(&self, r: &mut R, starts: &[u64]) -> Result<Vec<u8>, Error> {
        let mut tags = vec![0u8; self.instances() as usize];
        for (k, (pool, toc)) in Pool::ALL.iter().zip(&self.pools).enumerate() {
            let mut i = 0;
            while i < toc.count {
                let n = CHUNK.min(toc.count - i);
                let ids = read_at(r, self.data_base + toc.ids + 8 * i, 8 * n as usize)?;
                for (j, id) in ids.chunks_exact(8).enumerate() {
                    let id = u64::from_le_bytes(id.try_into().unwrap());
                    let Some((node, index)) = self.locate(id) else {
                        return Err(malformed(format!(
                            "id {id}, entry {} of the {} pool, names an instance the nodes do not hold",
                            i + j as u64,
                            pool.name()
                        )));
                    };
                    let g = (starts[node] + index) as usize;
                    if tags[g] & 1 << k != 0 {
                        return Err(malformed(format!(
                            "instance {id} is listed twice by the {} pool",
                            pool.name()
                        )));
                    }
                    tags[g] |= 1 << k;
                }
                i += n;
            }
        }
        Ok(tags)
    }

    /// Checks that a sample of each pool's ids land in their own boxes: an id
    /// split at the wrong shift names another instance, far from the box.
    fn check_ids<R: Read + Seek>(&self, r: &mut R) -> Result<(), Error> {
        for (pool, toc) in Pool::ALL.iter().zip(&self.pools) {
            let step = (toc.count / SAMPLE).max(1);
            let (mut seen, mut inside) = (0u64, 0u64);
            for i in (0..toc.count).step_by(step as usize) {
                let id = read_at(r, self.data_base + toc.ids + 8 * i, 8)?;
                let id = u64::from_le_bytes(id.try_into().unwrap());
                let b = read_at(r, self.data_base + toc.aabbs + 24 * i, 24)?;
                let (node, index) = self.locate(id).expect("checked by tags");
                let p = self.nodes[node].attr("P").unwrap();
                let p = read_at(r, self.data_base + p.offset + 12 * index, 12)?;
                let [x, _, z] = self.world(node, &p, 0);
                let within = |v: f64, lo: usize, hi: usize| {
                    v >= f32_at(&b, 4 * lo) - SLACK_M && v <= f32_at(&b, 4 * hi) + SLACK_M
                };
                seen += 1;
                inside += u64::from(within(x, 0, 3) && within(z, 2, 5));
            }
            let share = inside as f64 / seen as f64;
            if share < MIN_INSIDE {
                return Err(malformed(format!(
                    "{:.1} % of the {} pool's sampled ids, split at bit {}, lie inside their own boxes; \
                     a version {} file is read with every one inside",
                    100.0 * share,
                    pool.name(),
                    self.shift(),
                    self.version
                )));
            }
        }
        Ok(())
    }

    /// Calls `f` with every pooled instance in id order, once for each pool
    /// that lists it, then with every group record as an object, its id
    /// following the last node's.
    pub fn walk<R, F>(&self, r: &mut R, mut f: F) -> Result<(), Error>
    where
        R: Read + Seek,
        F: FnMut(&Instance) -> Result<(), Error>,
    {
        let starts = self.starts();
        let tags = self.tags(r, &starts)?;
        self.check_ids(r)?;
        let l = self.layout;
        let models = self.models.len() as u32;
        for (k, node) in self.nodes.iter().enumerate() {
            let n = node.n as usize;
            let s = starts[k] as usize;
            let here = &tags[s..s + n];
            if here.iter().all(|&t| t == 0) {
                continue;
            }
            let read = |r: &mut R, name: &str| {
                let a = node.attr(name).unwrap();
                read_at(r, self.data_base + a.offset, a.size as usize)
            };
            let p = read(r, "P")?;
            let ty = read(r, l.model)?;
            let ax = read(r, l.axis)?;
            for (i, &tag) in here.iter().enumerate().filter(|(_, t)| **t != 0) {
                let word = u32_at(&ty, 4 * i);
                // Version 9 keeps options in the high half of the word.
                let model = if self.version == 8 {
                    word
                } else {
                    word & 0xffff
                };
                let id = (k as u64) << self.shift() | i as u64;
                if model >= models {
                    return Err(malformed(format!(
                        "instance {id} names model {model}, past the {models} the table holds"
                    )));
                }
                let rotation = if self.version == 8 {
                    heading(f32_at(&ax, 8 * i), f32_at(&ax, 8 * i + 4))
                } else {
                    let c = |j: usize| {
                        let u = u16::from_le_bytes([ax[12 * i + 2 * j], ax[12 * i + 2 * j + 1]]);
                        (f64::from(u) - 32768.0) / 32767.0
                    };
                    heading(c(0), c(2))
                };
                let pos = self.world(k, &p, i);
                if !(rotation.is_finite() && pos.iter().all(|v| v.is_finite())) {
                    return Err(malformed(format!(
                        "instance {id} has a position or axis that is not a number"
                    )));
                }
                for (b, &pool) in Pool::ALL.iter().enumerate() {
                    if tag & 1 << b != 0 {
                        f(&Instance {
                            id,
                            pool,
                            model,
                            pos,
                            rotation,
                        })?;
                    }
                }
            }
        }
        let first = (self.nodes.len() as u64) << self.shift();
        for (i, g) in self.group.iter().enumerate() {
            f(&Instance {
                id: first + i as u64,
                pool: Pool::Objects,
                model: g.model,
                pos: g.pos,
                rotation: g.rotation,
            })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::super::tests::scene;
    use super::*;

    fn walk(b: &[u8]) -> Result<Vec<Instance>, Error> {
        let mut r = Cursor::new(b);
        let s = Scene::open(&mut r, "X.scn5")?;
        let mut out = Vec::new();
        s.walk(&mut r, |i| {
            out.push(i.clone());
            Ok(())
        })?;
        Ok(out)
    }

    #[test]
    fn walks_every_pooled_instance_in_id_order_then_the_group() {
        for version in [8, 9] {
            let built = scene(version);
            let got = walk(&built.file()).unwrap();
            let s = 13;
            let ids: Vec<u64> = got.iter().map(|i| i.id).collect();
            // Node 0 holds three instances, node 1 two; all five are pooled.
            assert_eq!(
                ids,
                [0, 1, 2, 1 << s, 1 << s | 1, 2 << s, (2 << s) + 1],
                "v{version}"
            );
            let pools: Vec<Pool> = got.iter().map(|i| i.pool).collect();
            use Pool::*;
            assert_eq!(
                pools,
                [Objects, Trees, Lines, Trees, Objects, Objects, Objects]
            );
            let models: Vec<u32> = got.iter().map(|i| i.model).collect();
            assert_eq!(models, [0, 1, 2, 1, 0, 2, 0]);
            assert_eq!(got[0].pos, [-150_000.0 + 10.5, 22.25, 450_000.0 - 3.0]);
            assert_eq!(got[4].pos, [-140_000.0 + 100.0, 31.0, 460_000.0 + 200.0]);
            assert_eq!(got[5].pos, [-130_000.25, 40.5, 470_000.75]);
            let rot: Vec<f64> = got.iter().map(|i| i.rotation).collect();
            let want = [
                0.0,
                0.8f64.atan2(0.6),
                1.5 * std::f64::consts::PI,
                0.0,
                std::f64::consts::PI,
                1.0,
                4.0,
            ];
            for (g, w) in rot.iter().zip(want) {
                // Version 9 stores an axis in 16 bits.
                assert!((g - w).abs() < 1e-4, "v{version}: {rot:?}");
            }
        }
    }

    #[test]
    fn skips_an_instance_in_no_pool() {
        let mut built = scene(9);
        built.pools[2].retain(|&k| k != (0, 1));
        let ids: Vec<u64> = walk(&built.file()).unwrap().iter().map(|i| i.id).collect();
        assert!(!ids.contains(&1), "{ids:?}");
        assert_eq!(ids.len(), 6);
    }

    #[test]
    fn visits_an_instance_once_for_each_pool_that_lists_it() {
        let mut built = scene(9);
        built.pools[1].push((0, 0));
        let got = walk(&built.file()).unwrap();
        let first: Vec<(u64, Pool)> = got.iter().take(3).map(|i| (i.id, i.pool)).collect();
        assert_eq!(
            first,
            [(0, Pool::Objects), (0, Pool::Lines), (1, Pool::Trees)]
        );
        assert_eq!(got[0].pos, got[1].pos);
    }

    #[test]
    fn refuses_an_id_a_pool_lists_twice() {
        let mut built = scene(9);
        built.pools[1].push((0, 2));
        let e = walk(&built.file()).unwrap_err().to_string();
        assert_eq!(e, "instance 2 is listed twice by the lines pool");
    }

    #[test]
    fn refuses_an_id_naming_no_instance() {
        let mut built = scene(8);
        built.pools[0].push((1, 2));
        let e = walk(&built.file()).unwrap_err().to_string();
        assert_eq!(
            e,
            format!(
                "id {}, entry 2 of the objects pool, names an instance the nodes do not hold",
                1 << 13 | 2
            )
        );
    }

    #[test]
    fn refuses_ids_split_at_another_versions_shift() {
        // Ids split at bit 15 in a file whose nodes give 13 name instances
        // that exist, far from their boxes: node 4 is the one node 1 becomes.
        let mut built = scene(9);
        built.shift = Some(15);
        for _ in 0..3 {
            built.nodes.push(super::super::build::Node {
                t: [0.0; 3],
                p: vec![[0.0; 3]; 3],
                model: vec![0; 3],
                axis: vec![[1.0, 0.0]; 3],
            });
        }
        built.pools = [vec![(1, 0)], vec![(1, 1)], vec![(0, 0)]];
        let e = walk(&built.file()).unwrap_err().to_string();
        assert!(
            e.contains("split at bit 13, lie inside their own boxes"),
            "{e}"
        );
    }

    #[test]
    fn refuses_an_axis_that_is_not_a_number() {
        let mut built = scene(8);
        built.nodes[0].axis[1] = [f32::NAN, 0.0];
        let e = walk(&built.file()).unwrap_err().to_string();
        assert_eq!(e, "instance 1 has a position or axis that is not a number");
    }

    #[test]
    fn refuses_an_instance_naming_a_model_past_the_table() {
        let mut built = scene(9);
        built.nodes[0].model[2] = 7;
        let e = walk(&built.file()).unwrap_err().to_string();
        assert_eq!(e, "instance 2 names model 7, past the 3 the table holds");
    }
}
