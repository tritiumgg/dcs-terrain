//! What a box query needs of a scene file's head, small enough to keep:
//! where each instancer node's arrays lie, the model table, the group and
//! the pools. Parsing the head costs 2 s and 559 MiB on GermanyColdWar;
//! this costs 25 MiB there and is read in milliseconds.
//!
//! Encoded little-endian:
//!
//! ```text
//! u32 version, u32 shift, u64 data base, u64 file length
//! 3 × pool        u64 count, ids, boxes, tree, tree nodes
//! u32 M, M ×      u32 len, name, f32[6] box
//! u32 N, N ×      u32 instances, f64[3] translation, u64 P, model, axis
//! u32 G, G ×      u32 model, f64[3] position, f64 heading
//! ```

use std::io::{self, Read, Seek};

use super::{
    Error, GroupRecord, MAX_MODELS, Model, Pool, PoolToc, Scene, layout, locate, malformed, placed,
};
use crate::decode::surface5::ReadAt;

/// One instancer node: its instance count, translation, and where its
/// position, model and axis arrays start, from the data base.
#[derive(Debug, Clone, PartialEq)]
pub struct Placed {
    pub n: u32,
    pub t: [f64; 3],
    pub p: u64,
    pub model: u64,
    pub axis: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Index {
    pub version: u32,
    pub shift: u32,
    pub data_base: u64,
    /// The scene file's length, to tell a file that changed since.
    pub len: u64,
    pub models: Vec<Model>,
    pub nodes: Vec<Placed>,
    pub group: Vec<GroupRecord>,
    /// Objects, lines, trees.
    pub pools: [PoolToc; 3],
}

impl Scene {
    /// The index of this file, once a sample of each pool's ids has been
    /// checked against their own boxes: an id split at the wrong shift
    /// would name the wrong instance on every query.
    pub fn index<R: Read + Seek>(&self, r: &mut R) -> Result<Index, Error> {
        self.check_ids(r)?;
        let l = self.layout;
        let nodes = self
            .nodes
            .iter()
            .map(|n| {
                // Opening checked that each node names all three.
                let at = |name: &str| n.attr(name).unwrap().offset;
                Placed {
                    n: n.n,
                    t: n.t,
                    p: at("P"),
                    model: at(l.model),
                    axis: at(l.axis),
                }
            })
            .collect();
        Ok(Index {
            version: self.version,
            shift: self.shift,
            data_base: self.data_base,
            len: self.len,
            models: self.models.clone(),
            nodes,
            group: self.group.clone(),
            pools: self.pools,
        })
    }
}

struct Writer(Vec<u8>);

impl Writer {
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f32(&mut self, v: f32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f64(&mut self, v: f64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

fn le32<const N: usize>(c: &[u8; N], at: usize) -> u32 {
    u32::from_le_bytes(c[at..at + 4].try_into().unwrap())
}

fn le64<const N: usize>(c: &[u8; N], at: usize) -> u64 {
    u64::from_le_bytes(c[at..at + 8].try_into().unwrap())
}

impl<'a> Reader<'a> {
    /// A count of records of `N` bytes each, then the records.
    fn records<const N: usize>(&mut self) -> Result<&'a [[u8; N]], Error> {
        let n = self.count(N)?;
        let (records, _) = self.b[self.at..self.at + n * N].as_chunks::<N>();
        self.at += n * N;
        Ok(records)
    }

    fn take<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        let s = self
            .b
            .get(self.at..self.at + N)
            .ok_or_else(|| malformed(format!("ends at byte {} of {}", self.at, self.b.len())))?;
        self.at += N;
        Ok(s.try_into().unwrap())
    }
    fn u32(&mut self) -> Result<u32, Error> {
        self.take().map(u32::from_le_bytes)
    }
    fn u64(&mut self) -> Result<u64, Error> {
        self.take().map(u64::from_le_bytes)
    }
    fn f32(&mut self) -> Result<f32, Error> {
        self.take().map(f32::from_le_bytes)
    }
    /// A count of records of at least `bytes` each, refused when the rest
    /// cannot hold that many, so a corrupt count allocates nothing.
    fn count(&mut self, bytes: usize) -> Result<usize, Error> {
        let n = self.u32()? as usize;
        if n.saturating_mul(bytes) > self.b.len() - self.at {
            return Err(malformed(format!(
                "names {n} records at byte {} where the rest holds fewer",
                self.at - 4
            )));
        }
        Ok(n)
    }
}

impl Index {
    /// The world position of instance `id`, a pooled instance or a group
    /// record, read from the scene file `r`; `None` when the file holds no
    /// instance of that id.
    pub fn position<R: ReadAt + ?Sized>(&self, r: &R, id: u64) -> io::Result<Option<[f64; 3]>> {
        let first = (self.nodes.len() as u64) << self.shift;
        if let Some(g) = id.checked_sub(first) {
            let g = usize::try_from(g).ok().and_then(|g| self.group.get(g));
            return Ok(g.map(|g| g.pos));
        }
        let Some((k, index)) = locate(id, self.shift, |k| self.nodes.get(k).map(|n| n.n)) else {
            return Ok(None);
        };
        let n = &self.nodes[k];
        let mut p = [0u8; 12];
        r.read_exact_at(&mut p, self.data_base + n.p + 12 * index)?;
        Ok(Some(placed(n.t, &p, 0)))
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer(Vec::new());
        w.u32(self.version);
        w.u32(self.shift);
        w.u64(self.data_base);
        w.u64(self.len);
        for p in &self.pools {
            for v in [p.count, p.ids, p.aabbs, p.tree, p.tree_nodes] {
                w.u64(v);
            }
        }
        w.u32(self.models.len() as u32);
        for m in &self.models {
            w.u32(m.name.len() as u32);
            w.0.extend_from_slice(m.name.as_bytes());
            m.bbox.iter().for_each(|&v| w.f32(v));
        }
        w.u32(self.nodes.len() as u32);
        for n in &self.nodes {
            w.u32(n.n);
            n.t.iter().for_each(|&v| w.f64(v));
            for v in [n.p, n.model, n.axis] {
                w.u64(v);
            }
        }
        w.u32(self.group.len() as u32);
        for g in &self.group {
            w.u32(g.model);
            g.pos.iter().for_each(|&v| w.f64(v));
            w.f64(g.rotation);
        }
        w.0
    }

    pub fn decode(b: &[u8]) -> Result<Index, Error> {
        let mut r = Reader { b, at: 0 };
        let version = r.u32()?;
        let shift = r.u32()?;
        if !(version == 8 || version == 9) || !(shift == 13 || shift == 15) {
            return Err(malformed(format!(
                "states version {version} and shift {shift}"
            )));
        }
        let data_base = r.u64()?;
        let len = r.u64()?;
        let mut pools = [PoolToc {
            count: 0,
            ids: 0,
            id_width: 8,
            aabbs: 0,
            tree: 0,
            tree_nodes: 0,
        }; 3];
        for p in &mut pools {
            [p.count, p.ids, p.aabbs, p.tree, p.tree_nodes] =
                [r.u64()?, r.u64()?, r.u64()?, r.u64()?, r.u64()?];
        }
        let m = r.count(4 + 24)?;
        if m as u32 > MAX_MODELS {
            return Err(malformed(format!("names {m} models")));
        }
        let mut models = Vec::with_capacity(m);
        for _ in 0..m {
            let n = r.count(1)?;
            let name = r.b[r.at..r.at + n].to_vec();
            r.at += n;
            let name = String::from_utf8(name)
                .map_err(|_| malformed(format!("model {} is not UTF-8", models.len())))?;
            let mut bbox = [0.0; 6];
            for v in &mut bbox {
                *v = r.f32()?;
            }
            models.push(Model { name, bbox });
        }
        // The fixed-size records are read from one slice each, their count
        // already checked against the bytes left: half a million nodes on the
        // largest terrain, read on every first scenery request.
        let nodes = r
            .records::<{ 4 + 24 + 24 }>()?
            .iter()
            .map(|c| Placed {
                n: le32(c, 0),
                t: [le64(c, 4), le64(c, 12), le64(c, 20)].map(f64::from_bits),
                p: le64(c, 28),
                model: le64(c, 36),
                axis: le64(c, 44),
            })
            .collect::<Vec<_>>();
        let group = r
            .records::<{ 4 + 32 }>()?
            .iter()
            .map(|c| GroupRecord {
                model: le32(c, 0),
                pos: [le64(c, 4), le64(c, 12), le64(c, 20)].map(f64::from_bits),
                rotation: f64::from_bits(le64(c, 28)),
            })
            .collect::<Vec<_>>();
        if r.at != b.len() {
            return Err(malformed(format!(
                "holds {} bytes past its group",
                b.len() - r.at
            )));
        }
        // Every offset a query reads from must lie in the file, so a corrupt
        // index is refused here rather than overflowing, allocating a pool's
        // worth of nothing, or reading past the end later.
        let Some(data) = len.checked_sub(data_base) else {
            return Err(malformed(format!(
                "puts the data base {data_base} past the file's {len} bytes"
            )));
        };
        let within = |at: u64, n: u64, size: u64| {
            n.checked_mul(size)
                .and_then(|b| b.checked_add(at))
                .is_some_and(|e| e <= data)
        };
        for (pool, p) in Pool::ALL.iter().zip(&pools) {
            if p.tree_nodes == 0
                || !within(p.ids, p.count, 8)
                || !within(p.aabbs, p.count, 24)
                || !within(p.tree, p.tree_nodes, 48)
            {
                return Err(malformed(format!(
                    "puts the {} pool's arrays past the file",
                    pool.name()
                )));
            }
        }
        let stride = u64::from(layout(version).axis_stride);
        for (k, n) in nodes.iter().enumerate() {
            let c = u64::from(n.n);
            if !(within(n.p, c, 12) && within(n.model, c, 4) && within(n.axis, c, stride))
                || !n.t.iter().all(|v| v.is_finite())
            {
                return Err(malformed(format!(
                    "puts node {k}'s arrays past the file or its translation off the map"
                )));
            }
        }
        for (i, g) in group.iter().enumerate() {
            let finite = g.pos.iter().all(|v| v.is_finite()) && g.rotation.is_finite();
            if g.model as usize >= models.len() || !finite {
                return Err(malformed(format!(
                    "group record {i} names model {} of {}, or a position or heading that is not \
                     a number",
                    g.model,
                    models.len()
                )));
            }
        }
        Ok(Index {
            version,
            shift,
            data_base,
            len,
            models,
            nodes,
            group,
            pools,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::super::tests::scene;
    use super::*;

    fn index(version: u32) -> Index {
        let b = scene(version).file();
        let mut r = Cursor::new(&b[..]);
        let s = Scene::open(&mut r, "X.scn5").unwrap();
        s.index(&mut r).unwrap()
    }

    #[test]
    fn an_index_decodes_to_what_was_encoded() {
        for version in [8, 9] {
            let i = index(version);
            assert_eq!(i.nodes.len(), 2);
            assert_eq!(i.nodes[0].n, 3);
            assert_eq!(i.nodes[1].t, [-140_000.0, 0.0, 460_000.0]);
            assert_eq!(i.group.len(), 2);
            assert_eq!(i.models[2].name, "wire");
            assert_eq!(Index::decode(&i.encode()).unwrap(), i, "v{version}");
        }
    }

    #[test]
    fn refuses_a_truncated_or_padded_index() {
        let b = index(9).encode();
        for cut in [0, 7, 30, b.len() - 1] {
            assert!(Index::decode(&b[..cut]).is_err(), "cut at {cut}");
        }
        let mut long = b.clone();
        long.push(0);
        let e = Index::decode(&long).unwrap_err().to_string();
        assert_eq!(e, "holds 1 bytes past its group");
    }

    /// The index of a version 9 scene with `edit` applied, encoded and
    /// decoded again.
    fn edited(edit: impl Fn(&mut Index)) -> String {
        let mut i = index(9);
        edit(&mut i);
        Index::decode(&i.encode()).unwrap_err().to_string()
    }

    #[test]
    fn refuses_an_index_that_reads_past_the_file_or_past_its_tables() {
        let e = edited(|i| i.pools[1].count = 1 << 40);
        assert_eq!(e, "puts the lines pool's arrays past the file");
        let e = edited(|i| i.pools[0].tree = u64::MAX);
        assert_eq!(e, "puts the objects pool's arrays past the file");
        let e = edited(|i| i.pools[2].tree_nodes = 0);
        assert_eq!(e, "puts the trees pool's arrays past the file");
        let e = edited(|i| i.nodes[1].axis = u64::MAX - 3);
        assert!(e.starts_with("puts node 1's arrays past the file"), "{e}");
        let e = edited(|i| i.nodes[0].t[2] = f64::NAN);
        assert!(e.starts_with("puts node 0's arrays"), "{e}");
        let e = edited(|i| i.data_base = i.len + 1);
        assert!(e.starts_with("puts the data base"), "{e}");
        let e = edited(|i| i.group[1].model = 70_000);
        assert!(
            e.starts_with("group record 1 names model 70000 of 3"),
            "{e}"
        );
        let e = edited(|i| i.group[0].rotation = f64::INFINITY);
        assert!(e.starts_with("group record 0"), "{e}");
    }

    #[test]
    fn refuses_a_count_the_rest_cannot_hold() {
        let mut b = index(8).encode();
        // The model count follows the header and the three pools.
        let at = 4 + 4 + 8 + 8 + 3 * 40;
        b[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        let e = Index::decode(&b).unwrap_err().to_string();
        assert!(e.starts_with("names 4294967295 records"), "{e}");
    }

    #[test]
    fn building_an_index_checks_the_pools_ids() {
        let mut built = scene(9);
        built.pools[0].push((1, 2));
        let b = built.file();
        let mut r = Cursor::new(&b[..]);
        let s = Scene::open(&mut r, "X.scn5").unwrap();
        let e = s.index(&mut r).unwrap_err().to_string();
        assert_eq!(
            e,
            format!(
                "id {}, entry 2 of the objects pool, names an instance the nodes do not hold",
                1 << 13 | 2
            )
        );
    }
}
