//! `landscape5::Scene5File`, versions 8 and 9: every model a map places, as
//! instancer nodes, a group placed one record at a time, and three
//! collision pools naming which instances are objects, lines and trees.
//!
//! Like `.surface5`, the file is a table of contents up to the data base
//! the container header states, and every array offset is relative to that
//! base. The model table is walked; the rest is found by pattern, since the
//! framing between nodes is not understood:
//!
//! ```text
//! after the version   u32 M, M × str      model names, lowercase
//!                     u32 M, M × f32[6]   model boxes, model-local
//!                     min x,y,z, max x,y,z; all zero for a model with none
//! a node, at p        p-160 f64[16] matrix, row-major: identity rotation,
//!                     row 3 the translation; p-28 f32[6] world box
//!                     p: u32 2, u32 n, u32 nattr, nattr attributes
//! an attribute        u32 1, u32 len, len bytes of name, u16 format,
//!                     u16 stride, u32 flag, u64 size, u48 offset
//! the group, after    u32 h, u32 count, count × 140-byte record:
//! the last node       u32 0, u32 model, f64[16] matrix, f32
//! a pool, in the      three entries of u64 count, u48 offset: the ids,
//! section table       the instances' world boxes, a hierarchy not read
//! ```
//!
//! An instance id is `node << S | index`, S being 13 in version 9 and 15 in
//! version 8, whose nodes hold more than 8 191 instances. Group record `i`
//! takes the id that follows the last node's, `(nodes << S) + i`.

use std::fmt;
use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use super::container::{self, MAX_HEAD};
use super::{Decoder, dispatch};

/// The version 9 model index is 16 bits wide.
const MAX_MODELS: u32 = 1 << 16;
const MAX_NAME_LEN: u32 = 256;
/// A node's header: the marker, then its instance and attribute counts.
const NODE_MARKER: u32 = 2;
const MAX_ATTRS: u32 = 8;
/// Attribute names are short and uppercase; a longer one is bytes that
/// happen to follow a marker.
const MAX_ATTR_NAME: usize = 32;
/// After the name: format, stride, flag, size and a 6-byte offset.
const ATTR_TAIL: usize = 2 + 2 + 4 + 8 + 6;
/// The matrix sits this far before the node's header.
const MATRIX_BEFORE: usize = 160;
/// How far past the last node's attributes the group may start.
const GROUP_SEARCH: usize = 4096;
const GROUP_RECORD: usize = 140;
const MAX_GROUP: u32 = 10_000_000;
/// A matrix entry of a rotation, scaled by at most rounding.
const UNIT: f64 = 1.0001;
/// The bytes of a pool's three entries.
const POOL_ENTRIES: usize = 3 * 14;
const AABB_BYTES: u64 = 24;

/// What makes a scene file unreadable, in words that name the part.
#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Malformed(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "cannot be read: {e}"),
            Error::Malformed(m) => f.write_str(m),
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

fn malformed(s: String) -> Error {
    Error::Malformed(s)
}

/// Which pool an instance is in, in the order the section table lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Pool {
    Objects,
    Lines,
    Trees,
}

impl Pool {
    pub const ALL: [Pool; 3] = [Pool::Objects, Pool::Lines, Pool::Trees];

    pub fn name(self) -> &'static str {
        match self {
            Pool::Objects => "objects",
            Pool::Lines => "lines",
            Pool::Trees => "trees",
        }
    }
}

/// Where each version keeps an instance's model and heading, and how wide
/// its index is in an id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Layout {
    /// v9's low 16 bits are the model, the high 16 options; v8's whole word.
    model: &'static str,
    /// v9: 6 × u16 offset-binary, the local X axis then the local Z axis.
    /// v8: f32 (x, z) of the local X axis.
    axis: &'static str,
    axis_stride: u16,
    shift: u32,
}

fn layout(version: u32) -> Layout {
    if version == 8 {
        Layout {
            model: "TYPE",
            axis: "AXISX",
            axis_stride: 8,
            shift: 15,
        }
    } else {
        Layout {
            model: "TYPE_OPTIONS",
            axis: "AXIS",
            axis_stride: 12,
            shift: 13,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    /// As the file spells it, lowercase.
    pub name: String,
    /// Min x, y, z, then max x, y, z, model-local.
    pub bbox: [f32; 6],
}

/// One data array a node names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attr {
    pub name: String,
    pub format: u16,
    pub stride: u16,
    pub size: u64,
    /// From the data base.
    pub offset: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    /// Where the node's header sits in the file.
    pub at: usize,
    pub n: u32,
    /// Added to every `P` to give world coordinates.
    pub t: [f64; 3],
    pub attrs: Vec<Attr>,
    /// Where the attributes end.
    end: usize,
}

impl Node {
    pub fn attr(&self, name: &str) -> Option<&Attr> {
        self.attrs.iter().find(|a| a.name == name)
    }
}

/// A model placed by its own matrix rather than by an instancer.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupRecord {
    pub model: u32,
    pub pos: [f64; 3],
    /// Of the local X axis, radians from north towards east, in `[0, 2π)`.
    pub rotation: f64,
}

/// A pool as the section table describes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoolToc {
    pub count: u64,
    /// From the data base.
    pub ids: u64,
    /// 8 for the instancer pools, 4 for the group's.
    pub id_width: u8,
    pub aabbs: u64,
}

/// An opened scene file: everything before the data base.
#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    pub version: u32,
    pub models: Vec<Model>,
    pub nodes: Vec<Node>,
    pub group: Vec<GroupRecord>,
    /// Objects, lines, trees.
    pub pools: [PoolToc; 3],
    layout: Layout,
    data_base: u64,
    len: u64,
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn f32_at(b: &[u8], at: usize) -> f32 {
    f32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn f64_at(b: &[u8], at: usize) -> f64 {
    f64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}

fn u64_at(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}

fn u48_at(b: &[u8], at: usize) -> u64 {
    let mut w = [0u8; 8];
    w[..6].copy_from_slice(&b[at..at + 6]);
    u64::from_le_bytes(w)
}

/// A heading in `[0, 2π)` from the (x, z) of an axis.
pub(crate) fn heading(x: f64, z: f64) -> f64 {
    let h = z.atan2(x).rem_euclid(std::f64::consts::TAU);
    // A tiny negative angle rounds up to 2π itself.
    if h >= std::f64::consts::TAU { 0.0 } else { h }
}

/// Reads the model names and boxes at `at`, returning them and where they
/// end.
fn models(b: &[u8], mut at: usize) -> Result<(Vec<Model>, usize), Error> {
    let short = || malformed("the model table runs past the data base".into());
    let count = u32_at(b, at).ok_or_else(short)?;
    if count == 0 || count > MAX_MODELS {
        return Err(malformed(format!("a model table of {count} entries")));
    }
    at += 4;
    let mut names = Vec::with_capacity(count as usize);
    for i in 0..count {
        let len = u32_at(b, at).ok_or_else(short)?;
        if len == 0 || len > MAX_NAME_LEN {
            return Err(malformed(format!("model {i} has a name of {len} bytes")));
        }
        let name = b.get(at + 4..at + 4 + len as usize).ok_or_else(short)?;
        // UTF-8, not ASCII: Syria spells some names with a Cyrillic letter,
        // and a GermanyColdWar name ends in a space.
        let name = std::str::from_utf8(name)
            .ok()
            .filter(|s| !s.chars().any(char::is_control))
            .ok_or_else(|| malformed(format!("model {i}'s name is not printable UTF-8")))?;
        names.push(name.to_string());
        at += 4 + len as usize;
    }
    let again = u32_at(b, at).ok_or_else(short)?;
    if again != count {
        return Err(malformed(format!(
            "the model table names {count} models and boxes {again}"
        )));
    }
    at += 4;
    if b.len() < at + 24 * count as usize {
        return Err(short());
    }
    let models = names
        .into_iter()
        .enumerate()
        .map(|(i, name)| Model {
            name,
            bbox: [0, 1, 2, 3, 4, 5].map(|k| f32_at(b, at + 24 * i + 4 * k)),
        })
        .collect();
    Ok((models, at + 24 * count as usize))
}

/// An attribute at `i`, and where it ends.
fn attr_at(b: &[u8], i: usize) -> Option<(Attr, usize)> {
    if u32_at(b, i)? != 1 {
        return None;
    }
    let len = u32_at(b, i + 4)? as usize;
    if !(1..=MAX_ATTR_NAME).contains(&len) {
        return None;
    }
    let name = b.get(i + 8..i + 8 + len)?;
    if !name[0].is_ascii_uppercase() || !name.iter().all(|c| c.is_ascii_uppercase() || *c == b'_') {
        return None;
    }
    let q = i + 8 + len;
    if b.len() < q + ATTR_TAIL {
        return None;
    }
    let attr = Attr {
        name: String::from_utf8(name.to_vec()).expect("checked ASCII"),
        format: u16_at(b, q),
        stride: u16_at(b, q + 2),
        size: u64_at(b, q + 8),
        offset: u48_at(b, q + 16),
    };
    Some((attr, q + ATTR_TAIL))
}

/// A node whose header is at `p`, when the marker, a non-zero count and
/// every attribute parse there.
fn node_at(b: &[u8], p: usize) -> Option<Node> {
    if p < MATRIX_BEFORE || u32_at(b, p)? != NODE_MARKER {
        return None;
    }
    let n = u32_at(b, p + 4)?;
    let nattr = u32_at(b, p + 8)?;
    if n == 0 || !(1..=MAX_ATTRS).contains(&nattr) {
        return None;
    }
    let mut q = p + 12;
    let mut attrs = Vec::with_capacity(nattr as usize);
    for _ in 0..nattr {
        let (a, end) = attr_at(b, q)?;
        attrs.push(a);
        q = end;
    }
    let m = p - MATRIX_BEFORE;
    Some(Node {
        at: p,
        n,
        t: [12, 13, 14].map(|k| f64_at(b, m + 8 * k)),
        attrs,
        end: q,
    })
}

/// Every node between the model table and the data base. A node's matrix
/// must be a translation: a rotation would turn its instances, and nothing
/// here applies one.
fn nodes(b: &[u8], from: usize) -> Result<Vec<Node>, Error> {
    let mut out = Vec::new();
    let mut i = from + MATRIX_BEFORE;
    while i + 12 <= b.len() {
        let Some(node) = node_at(b, i) else {
            i += 1;
            continue;
        };
        let m = i - MATRIX_BEFORE;
        let entry = |k: usize| f64_at(b, m + 8 * k);
        let identity = (0..16).all(|k| {
            let want = if k % 5 == 0 { 1.0 } else { 0.0 };
            // Row 3 is the translation and w.
            (12..15).contains(&k) || (entry(k) - want).abs() <= 1e-6
        });
        if !identity {
            return Err(malformed(format!(
                "node {} at byte {i} has a matrix that is not a translation",
                out.len()
            )));
        }
        i = node.end;
        out.push(node);
    }
    Ok(out)
}

/// A group's 16 matrix entries as f64, from its record at `r`.
fn record_matrix(b: &[u8], r: usize) -> [f64; 16] {
    std::array::from_fn(|k| f64_at(b, r + 8 + 8 * k))
}

fn is_placement(m: &[f64; 16]) -> bool {
    m[15] == 1.0
        && m[3] == 0.0
        && m[7] == 0.0
        && m[11] == 0.0
        && m[..12].iter().all(|v| v.abs() <= UNIT)
}

/// The group after the last node, if there is one: its records and where
/// they end.
fn group(b: &[u8], from: usize, models: usize) -> Option<(Vec<GroupRecord>, usize)> {
    for q in from..from + GROUP_SEARCH {
        let (Some(h), Some(count)) = (u32_at(b, q), u32_at(b, q + 4)) else {
            return None;
        };
        if h as usize >= models || count == 0 || count >= MAX_GROUP || u32_at(b, q + 8) != Some(0) {
            continue;
        }
        let first = q + 8;
        let end = first + GROUP_RECORD * count as usize;
        if end > b.len() || !is_placement(&record_matrix(b, first)) {
            continue;
        }
        if record_matrix(b, end - GROUP_RECORD)[15] != 1.0 {
            continue;
        }
        let records = (0..count as usize)
            .map(|i| {
                let r = first + GROUP_RECORD * i;
                let m = record_matrix(b, r);
                GroupRecord {
                    model: u32_at(b, r + 4).unwrap(),
                    pos: [m[12], m[13], m[14]],
                    rotation: heading(m[0], m[2]),
                }
            })
            .collect();
        return Some((records, end));
    }
    None
}

/// Every pool descriptor between `from` and the data base, whose arrays lie
/// within `data` bytes after it. A pool is found by its structure: two
/// entries of one count whose offsets differ by the width of its ids.
fn pools(b: &[u8], from: usize, data: u64) -> Vec<PoolToc> {
    let mut out = Vec::new();
    let mut i = from;
    while i + POOL_ENTRIES <= b.len() {
        let (c1, o1) = (u64_at(b, i), u48_at(b, i + 8));
        let (c2, o2) = (u64_at(b, i + 14), u48_at(b, i + 22));
        let fits = c1 == c2
            && c1 > 0
            && c1 < 1 << 32
            && o2 > o1
            && o2.checked_add(c1 * AABB_BYTES).is_some_and(|e| e <= data);
        let width = match o2.wrapping_sub(o1) {
            d if fits && d == 8 * c1 => Some(8),
            d if fits && (d == 4 * c1 || d == (4 * c1).div_ceil(8) * 8) => Some(4),
            _ => None,
        };
        let Some(id_width) = width else {
            i += 1;
            continue;
        };
        out.push(PoolToc {
            count: c1,
            ids: o1,
            id_width,
            aabbs: o2,
        });
        i += POOL_ENTRIES;
    }
    out
}

impl Scene {
    /// Reads everything before the data base of a file whose header names
    /// this decoder. `name` is how an error names the file.
    pub fn open<R: Read + Seek>(r: &mut R, name: &str) -> Result<Scene, Error> {
        let len = r.seek(SeekFrom::End(0))?;
        r.seek(SeekFrom::Start(0))?;
        let mut first = Vec::with_capacity(MAX_HEAD);
        r.take(MAX_HEAD as u64).read_to_end(&mut first)?;
        let h = container::parse(&first).map_err(|e| malformed(format!("not a container: {e}")))?;
        match dispatch(name, &h) {
            Ok(Decoder::Scene5) => {}
            Ok(d) => {
                return Err(malformed(format!(
                    "{} version {} is read by the {d:?} decoder, not this one",
                    h.class, h.version
                )));
            }
            Err(e) => return Err(malformed(e.to_string())),
        }
        if h.data_base > len {
            return Err(malformed(format!(
                "the data base {} lies past the end of the file, {len} bytes",
                h.data_base
            )));
        }
        r.seek(SeekFrom::Start(0))?;
        let mut head = vec![0u8; h.data_base as usize];
        r.read_exact(&mut head)?;

        let layout = layout(h.version);
        let (models, at) = models(&head, h.body())?;
        let nodes = nodes(&head, at)?;
        let Some(last) = nodes.last() else {
            return Err(malformed("no instancer node was found".into()));
        };
        let (group, after) = group(&head, last.end, models.len()).unwrap_or((Vec::new(), last.end));
        let found = pools(&head, after, len - h.data_base);
        let pools = match (group.len(), found.as_slice()) {
            (0, [o, l, t]) | (_, [_, o, l, t]) if [o, l, t].iter().all(|p| p.id_width == 8) => {
                [*o, *l, *t]
            }
            _ => {
                let widths: Vec<String> = found
                    .iter()
                    .map(|p| format!("u{}", 8 * p.id_width))
                    .collect();
                return Err(malformed(format!(
                    "the section table holds pools of [{}] ids, where a scene has an optional u32 pool for its \
                     group, then objects, lines and trees of u64",
                    widths.join(", ")
                )));
            }
        };
        // The group's width is not checked: one u32 id padded to 8 bytes
        // reads as one u64.
        if let [g, _, _, _] = found.as_slice() {
            if g.count != group.len() as u64 {
                return Err(malformed(format!(
                    "the group places {} models, and its pool lists {}",
                    group.len(),
                    g.count
                )));
            }
        }
        let scene = Scene {
            version: h.version,
            models,
            nodes,
            group,
            pools,
            layout,
            data_base: h.data_base,
            len,
        };
        scene.check_attrs()?;
        Ok(scene)
    }

    /// Bits of an id that hold the instance's index in its node.
    pub fn shift(&self) -> u32 {
        self.layout.shift
    }

    pub fn instances(&self) -> u64 {
        self.nodes.iter().map(|n| u64::from(n.n)).sum()
    }

    /// Every node names `P`, a model and an axis at the stride this version
    /// gives them, with one element an instance, inside the file; and every
    /// model a group record names is in the table.
    fn check_attrs(&self) -> Result<(), Error> {
        let l = self.layout;
        let need = [("P", 12), (l.model, 4), (l.axis, l.axis_stride)];
        let data = self.len - self.data_base;
        for (k, node) in self.nodes.iter().enumerate() {
            for (name, stride) in need {
                let Some(a) = node.attr(name) else {
                    return Err(malformed(format!(
                        "node {k} has no {name}, which version {} places by",
                        self.version
                    )));
                };
                if a.stride != stride || a.size != u64::from(node.n) * u64::from(stride) {
                    return Err(malformed(format!(
                        "node {k}'s {name} has stride {} and {} bytes for {} instances, where version {} \
                         stores {stride} bytes each",
                        a.stride, a.size, node.n, self.version
                    )));
                }
                if a.offset.checked_add(a.size).is_none_or(|e| e > data) {
                    return Err(malformed(format!(
                        "node {k}'s {name} runs past the end of the file"
                    )));
                }
            }
        }
        if let Some((i, g)) = self
            .group
            .iter()
            .enumerate()
            .find(|(_, g)| g.model as usize >= self.models.len())
        {
            return Err(malformed(format!(
                "group record {i} names model {}, past the {} the table holds",
                g.model,
                self.models.len()
            )));
        }
        Ok(())
    }
}

pub fn reader(path: &Path) -> io::Result<BufReader<File>> {
    Ok(BufReader::new(File::open(path)?))
}

/// Opens the scene file at `path`.
pub fn open(path: &Path) -> Result<Scene, Error> {
    Scene::open(&mut reader(path)?, &path.to_string_lossy())
}

/// Builds scene files for tests: the parts a decoder finds, with framing
/// between them that it must step over.
#[cfg(test)]
pub mod build {
    use super::super::container;
    use super::layout;

    pub struct Node {
        pub t: [f64; 3],
        /// Relative to `t`.
        pub p: Vec<[f32; 3]>,
        pub model: Vec<u32>,
        /// (x, z) of each instance's local X axis, a unit vector.
        pub axis: Vec<[f32; 2]>,
    }

    pub struct Record {
        pub model: u32,
        pub pos: [f64; 3],
        /// Radians.
        pub heading: f64,
    }

    pub struct Scene {
        pub version: u32,
        pub models: Vec<(&'static str, [f32; 6])>,
        pub nodes: Vec<Node>,
        pub group: Vec<Record>,
        /// Objects, lines, trees: each instance as (node, index), in the
        /// order the pool lists them.
        pub pools: [Vec<(u64, u64)>; 3],
        /// The shift ids are written with, when not the version's own.
        pub shift: Option<u32>,
    }

    fn str_(out: &mut Vec<u8>, s: &str) {
        out.extend_from_slice(&(s.len() as u32).to_le_bytes());
        out.extend_from_slice(s.as_bytes());
    }

    fn put<T: Copy, const N: usize>(out: &mut Vec<u8>, v: &[T], f: impl Fn(T) -> [u8; N]) {
        for &x in v {
            out.extend_from_slice(&f(x));
        }
    }

    fn attr(
        toc: &mut Vec<u8>,
        data: &mut Vec<u8>,
        name: &str,
        format: u16,
        stride: u16,
        bytes: &[u8],
    ) {
        toc.extend_from_slice(&1u32.to_le_bytes());
        str_(toc, name);
        toc.extend_from_slice(&format.to_le_bytes());
        toc.extend_from_slice(&stride.to_le_bytes());
        toc.extend_from_slice(&0u32.to_le_bytes());
        toc.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        toc.extend_from_slice(&(data.len() as u64).to_le_bytes()[..6]);
        data.extend_from_slice(bytes);
    }

    fn entry(toc: &mut Vec<u8>, count: u64, offset: u64) {
        toc.extend_from_slice(&count.to_le_bytes());
        toc.extend_from_slice(&offset.to_le_bytes()[..6]);
    }

    /// The offset-binary u16 a version 9 axis stores for `v`.
    pub fn offset_binary(v: f32) -> u16 {
        (f64::from(v) * 32767.0 + 32768.0).round() as u16
    }

    fn matrix(out: &mut Vec<u8>, m: [f64; 16]) {
        put(out, &m, f64::to_le_bytes);
    }

    impl Scene {
        pub fn world(&self, (node, i): (u64, u64)) -> [f64; 3] {
            let n = &self.nodes[node as usize];
            let p = n.p[i as usize];
            [0, 1, 2].map(|k| n.t[k] + f64::from(p[k]))
        }

        pub fn id(&self, (node, i): (u64, u64)) -> u64 {
            node << self.shift.unwrap_or(layout(self.version).shift) | i
        }

        pub fn file(&self) -> Vec<u8> {
            let v9 = self.version == 9;
            let mut toc = Vec::new();
            let mut data = Vec::new();
            toc.extend_from_slice(&(self.models.len() as u32).to_le_bytes());
            for (name, _) in &self.models {
                str_(&mut toc, name);
            }
            toc.extend_from_slice(&(self.models.len() as u32).to_le_bytes());
            for (_, b) in &self.models {
                put(&mut toc, b, f32::to_le_bytes);
            }
            // A material descriptor, which the decoder does not read.
            toc.extend_from_slice(&1u32.to_le_bytes());
            toc.extend_from_slice(&2u32.to_le_bytes());
            str_(&mut toc, "HwInstancer5.2");
            str_(&mut toc, "Undef");
            toc.extend_from_slice(&0u32.to_le_bytes());
            toc.extend_from_slice(&(-1i32).to_le_bytes());

            for n in &self.nodes {
                toc.extend_from_slice(&[0xEE; 4]);
                let mut m = [0.0; 16];
                (m[0], m[5], m[10], m[15]) = (1.0, 1.0, 1.0, 1.0);
                (m[12], m[13], m[14]) = (n.t[0], n.t[1], n.t[2]);
                matrix(&mut toc, m);
                toc.extend_from_slice(&147_829.0f32.to_le_bytes());
                put(
                    &mut toc,
                    &[-1e6f32, 0.0, -1e6, 1e6, 3000.0, 1e6],
                    f32::to_le_bytes,
                );
                toc.extend_from_slice(&0u32.to_le_bytes());
                toc.extend_from_slice(&2u32.to_le_bytes());
                toc.extend_from_slice(&(n.p.len() as u32).to_le_bytes());
                toc.extend_from_slice(&(if v9 { 4u32 } else { 5 }).to_le_bytes());
                let mut p = Vec::new();
                put(&mut p, &n.p.concat(), f32::to_le_bytes);
                let seed: Vec<u8> =
                    n.p.iter()
                        .flat_map(|_| 0x3F80_0000u32.to_le_bytes())
                        .collect();
                if v9 {
                    let mut ty = Vec::new();
                    put(&mut ty, &n.model, |m| (m | 0x30 << 16).to_le_bytes());
                    let mut ax = Vec::new();
                    for a in &n.axis {
                        // The local Z axis is the X axis turned a right angle.
                        for c in [a[0], 0.0, a[1], -a[1], 0.0, a[0]] {
                            ax.extend_from_slice(&offset_binary(c).to_le_bytes());
                        }
                    }
                    attr(&mut toc, &mut data, "P", 2, 12, &p);
                    attr(&mut toc, &mut data, "TYPE_OPTIONS", 4, 4, &ty);
                    attr(&mut toc, &mut data, "SEED_SCALE", 4, 4, &seed);
                    attr(&mut toc, &mut data, "AXIS", 6, 12, &ax);
                } else {
                    let mut ty = Vec::new();
                    put(&mut ty, &n.model, u32::to_le_bytes);
                    let mut ax = Vec::new();
                    put(&mut ax, &n.axis.concat(), f32::to_le_bytes);
                    let options: Vec<u8> = n.p.iter().flat_map(|_| 0x30u32.to_le_bytes()).collect();
                    attr(&mut toc, &mut data, "TYPE", 4, 4, &ty);
                    attr(&mut toc, &mut data, "P", 2, 12, &p);
                    attr(&mut toc, &mut data, "AXISX", 1, 8, &ax);
                    attr(&mut toc, &mut data, "SEED", 0, 4, &seed);
                    attr(&mut toc, &mut data, "OPTIONS", 4, 4, &options);
                }
                // The models the node uses, then framing.
                toc.extend_from_slice(&0u32.to_le_bytes());
                toc.extend_from_slice(&(n.model.len() as u32).to_le_bytes());
                put(&mut toc, &n.model, u32::to_le_bytes);
                toc.extend_from_slice(&3u32.to_le_bytes());
                toc.extend_from_slice(&(-1i32).to_le_bytes());
            }

            if !self.group.is_empty() {
                toc.extend_from_slice(&0u32.to_le_bytes());
                toc.extend_from_slice(&(self.group.len() as u32).to_le_bytes());
                for g in &self.group {
                    toc.extend_from_slice(&0u32.to_le_bytes());
                    toc.extend_from_slice(&g.model.to_le_bytes());
                    let (s, c) = g.heading.sin_cos();
                    let [x, y, z] = g.pos;
                    matrix(
                        &mut toc,
                        [
                            c, 0.0, s, 0.0, 0.0, 1.0, 0.0, 0.0, -s, 0.0, c, 0.0, x, y, z, 1.0,
                        ],
                    );
                    toc.extend_from_slice(&0.5f32.to_le_bytes());
                }
            }
            toc.extend_from_slice(&0u32.to_le_bytes());

            let aabb = |w: [f64; 3]| {
                [
                    w[0] - 1.0,
                    w[1] - 1.0,
                    w[2] - 1.0,
                    w[0] + 1.0,
                    w[1] + 1.0,
                    w[2] + 1.0,
                ]
            };
            if !self.group.is_empty() {
                let ids = data.len() as u64;
                put(
                    &mut data,
                    &(0..self.group.len() as u32).collect::<Vec<_>>(),
                    u32::to_le_bytes,
                );
                data.resize(data.len().div_ceil(8) * 8, 0);
                let boxes = data.len() as u64;
                for g in &self.group {
                    put(&mut data, &aabb(g.pos).map(|v| v as f32), f32::to_le_bytes);
                }
                entry(&mut toc, self.group.len() as u64, ids);
                entry(&mut toc, self.group.len() as u64, boxes);
                entry(&mut toc, 1, 0);
            }
            for pool in &self.pools {
                toc.extend_from_slice(&[0xEE; 8]);
                let ids = data.len() as u64;
                put(
                    &mut data,
                    &pool.iter().map(|&k| self.id(k)).collect::<Vec<_>>(),
                    u64::to_le_bytes,
                );
                let boxes = data.len() as u64;
                for &k in pool {
                    put(
                        &mut data,
                        &aabb(self.world(k)).map(|v| v as f32),
                        f32::to_le_bytes,
                    );
                }
                entry(&mut toc, pool.len() as u64, ids);
                entry(&mut toc, pool.len() as u64, boxes);
                entry(&mut toc, 1, 0);
            }
            // The destroyed-model map.
            put(&mut toc, &vec![-1i32; self.models.len()], i32::to_le_bytes);

            let class = "landscape5::Scene5File";
            let head = container::build(class, self.version, 0).len();
            let base = (head + toc.len()) as u64;
            let mut out = container::build(class, self.version, base);
            out.extend_from_slice(&toc);
            out.extend_from_slice(&data);
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::build::{self, Record};
    use super::*;

    const BOX: [f32; 6] = [-5.0, 0.0, -10.0, 5.0, 8.0, 10.0];

    fn scene(version: u32) -> build::Scene {
        let (c, s) = (0.6f32, 0.8f32);
        build::Scene {
            version,
            models: vec![
                ("sklad_new", BOX),
                ("oak_1", [0.0; 6]),
                ("wire", [-1.0, 0.0, -1.0, 1.0, 9.0, 1.0]),
            ],
            nodes: vec![
                build::Node {
                    t: [-150_000.0, 0.0, 450_000.0],
                    p: vec![[10.5, 22.25, -3.0], [20.0, 23.0, 4.0], [30.0, 24.0, 5.0]],
                    model: vec![0, 1, 2],
                    axis: vec![[1.0, 0.0], [c, s], [0.0, -1.0]],
                },
                build::Node {
                    t: [-140_000.0, 0.0, 460_000.0],
                    p: vec![[1.0, 30.0, 2.0], [100.0, 31.0, 200.0]],
                    model: vec![1, 0],
                    axis: vec![[1.0, 0.0], [-1.0, 0.0]],
                },
            ],
            group: vec![
                Record {
                    model: 2,
                    pos: [-130_000.25, 40.5, 470_000.75],
                    heading: 1.0,
                },
                Record {
                    model: 0,
                    pos: [-130_100.0, 41.0, 470_100.0],
                    heading: 4.0,
                },
            ],
            pools: [vec![(1, 1), (0, 0)], vec![(0, 2)], vec![(1, 0), (0, 1)]],
            shift: None,
        }
    }

    fn open_bytes(b: &[u8]) -> Result<Scene, Error> {
        Scene::open(&mut Cursor::new(b), "X.scn5")
    }

    #[test]
    fn reads_the_model_table_nodes_and_pools_of_each_version() {
        for version in [8, 9] {
            let built = scene(version);
            let s = open_bytes(&built.file()).unwrap();
            assert_eq!(s.version, version);
            assert_eq!(s.shift(), if version == 8 { 15 } else { 13 });
            let names: Vec<&str> = s.models.iter().map(|m| m.name.as_str()).collect();
            assert_eq!(names, ["sklad_new", "oak_1", "wire"]);
            assert_eq!(s.models[0].bbox, BOX);
            assert_eq!(s.nodes.len(), 2);
            assert_eq!(s.nodes[1].t, [-140_000.0, 0.0, 460_000.0]);
            assert_eq!(s.nodes[0].n, 3);
            assert_eq!(s.instances(), 5);
            let counts: Vec<u64> = s.pools.iter().map(|p| p.count).collect();
            assert_eq!(counts, [2, 1, 2]);
            assert!(s.pools.iter().all(|p| p.id_width == 8));
            assert_eq!(s.group.len(), 2);
            assert_eq!(s.group[0].model, 2);
            assert_eq!(s.group[0].pos, [-130_000.25, 40.5, 470_000.75]);
            assert!((s.group[0].rotation - 1.0).abs() < 1e-12);
        }
    }

    #[test]
    fn reads_a_file_with_no_group() {
        let mut built = scene(8);
        built.group.clear();
        let s = open_bytes(&built.file()).unwrap();
        assert!(s.group.is_empty());
        assert_eq!(s.pools[2].count, 2);
    }

    #[test]
    fn refuses_a_node_without_its_versions_attributes() {
        // A version 8 file's nodes, declared as version 9.
        let mut b = scene(8).file();
        let body = container::parse(&b).unwrap().body();
        b[body - 4] = 9;
        let e = open_bytes(&b).unwrap_err().to_string();
        assert_eq!(e, "node 0 has no TYPE_OPTIONS, which version 9 places by");
    }

    #[test]
    fn refuses_a_section_table_without_three_pools() {
        let mut built = scene(9);
        built.pools[2].clear();
        // An empty pool has no descriptor the scan can find.
        let e = open_bytes(&built.file()).unwrap_err().to_string();
        assert!(
            e.starts_with("the section table holds pools of [u32, u64, u64]"),
            "{e}"
        );
    }

    #[test]
    fn refuses_a_version_this_build_does_not_read() {
        let mut b = scene(9).file();
        let body = container::parse(&b).unwrap().body();
        b[body - 4] = 10;
        let e = open_bytes(&b).unwrap_err().to_string();
        assert!(e.ends_with("reads version 8, 9"), "{e}");
    }

    #[test]
    fn a_heading_is_in_one_turn() {
        use std::f64::consts::{FRAC_PI_2, PI};
        assert_eq!(heading(1.0, 0.0), 0.0);
        assert!((heading(0.0, 1.0) - FRAC_PI_2).abs() < 1e-15);
        assert!((heading(0.0, -1.0) - 3.0 * FRAC_PI_2).abs() < 1e-15);
        assert!((heading(-1.0, 0.0) - PI).abs() < 1e-15);
        assert_eq!(heading(1.0, -1e-300), 0.0);
    }
}
