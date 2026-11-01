//! `landscape4::lRoadNetwork`, version 5, the map-wide `roads/<Map>.rn4`:
//! the road and rail network the game routes over.
//!
//! The file opens with the class table, then the router's table of edge ends,
//! the edges, the connectors and a spatial index, packed with no alignment:
//!
//! ```text
//! after the version   u32 count, count × (u32 len, len bytes)   classes
//! the table           u32 count, count × {
//!                       u32 1, u32 junction, u32 edge, u32 k,
//!                       k × (u32 end, u32 cost) }
//! the edges           u32 count, count × {
//!                       u32 6, u8 kind, u8 class, u32 node a, u32 node b,
//!                       u32 p, p × (str key, str value),
//!                       u32 n, n × f64 (x, y, z),
//!                       unit tangents and more, read past }
//! kind                2 road, 1 rail, 0 airfield line
//! a connector         u32 3, u32 edge a, u32 edge b, u32 n, n × f64 (x, y, z)
//! ```
//!
//! An end of the table is one edge at one junction, and an edge has two. A
//! link to the edge's other end runs along it, and a link to an end of the
//! same junction is a passage through it, which a connector draws. A one-way
//! edge has its link one way only. What follows an edge's points is not
//! decoded, so the next edge is found by its head, which the table's ids make
//! unmistakable, and a connector by its frame and points lying within reach.

use std::collections::HashMap;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, BufWriter, Read, Write};
use std::path::Path;

use serde_json::json;

use super::container::{self, MAX_HEAD};
use super::surface5::Surface5;
use super::{Decoder, dispatch};

pub const KIND_AIRFIELD: u8 = 0;
pub const KIND_RAIL: u8 = 1;
pub const KIND_ROAD: u8 = 2;

pub const ROADS_FILE: &str = "roads.jsonl";
pub const RAILROADS_FILE: &str = "railroads.jsonl";

/// A run is at least a segment, and no road is longer than this many
/// vertices: a larger count is bytes that happen to precede plausible
/// floats.
const MIN_POINTS: u32 = 2;
const MAX_POINTS: u32 = 100_000;
/// What opens an edge, and a connector.
const EDGE_TAG: u32 = 6;
const CONNECTOR_TAG: u32 = 3;
/// The most points of a connector: they run to 5.
const MAX_CONNECTOR_POINTS: u32 = 64;
/// The most properties of an edge and the longest of their strings: they
/// are short words, so a larger count is not one.
const MAX_PROPERTIES: u32 = 16;
const MAX_PROPERTY_LEN: u32 = 64;
/// The most links of an end of the table.
const MAX_LINKS: u32 = 1024;
/// How far past the mesh's extent a connector's point may lie, sideways and
/// in height. Wide, because the box only has to refuse bytes that are not
/// coordinates.
const REACH_XZ_M: f64 = 20_000.0;
const REACH_Y_M: f64 = 500.0;
/// The class table is short; a larger count is not one.
const MAX_CLASSES: u32 = 256;
const MAX_CLASS_LEN: u32 = 64;
/// The value an end takes where the file names no junction.
const NO_NODE: u32 = u32::MAX;
/// How far from their mean the ends of one junction may lie. The widest real
/// junction on any map measured spans 2.2 km; each catch-all, hundreds.
const JUNCTION_REACH_M: f64 = 10_000.0;

/// What makes a road network unreadable, in words that name the part.
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

/// The box a vertex must lie in: the mesh's extent widened by a reach.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reach {
    pub min_x: f64,
    pub max_x: f64,
    pub min_y: f64,
    pub max_y: f64,
    pub min_z: f64,
    pub max_z: f64,
}

impl Reach {
    /// The finest blocks' boxes, widened.
    pub fn around(s: &Surface5) -> Reach {
        let b = s.bounds();
        let (mut min_y, mut max_y) = (f64::INFINITY, f64::NEG_INFINITY);
        for blk in &s.blocks {
            let ty = f64::from(blk.t[1]);
            min_y = min_y.min(ty + f64::from(blk.bbox[1]));
            max_y = max_y.max(ty + f64::from(blk.bbox[4]));
        }
        Reach {
            min_x: b.min_x - REACH_XZ_M,
            max_x: b.max_x + REACH_XZ_M,
            min_y: min_y - REACH_Y_M,
            max_y: max_y + REACH_Y_M,
            min_z: b.min_z - REACH_XZ_M,
            max_z: b.max_z + REACH_XZ_M,
        }
    }

    #[inline(always)]
    fn holds(&self, [x, y, z]: [f64; 3]) -> bool {
        // Zeroed bytes read as a triple of zeros.
        (x != 0.0 || y != 0.0 || z != 0.0)
            && (self.min_x..=self.max_x).contains(&x)
            && (self.min_z..=self.max_z).contains(&z)
            && y > self.min_y
            && y < self.max_y
    }
}

/// One edge as the file holds it. `points` are `[x, y, z]`, `y` up.
#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    /// Its number in the file, every kind counted, by which the table and
    /// the connectors name it.
    pub id: u32,
    pub kind: u8,
    pub class: u8,
    /// The junction at the first vertex, and at the last; none where the
    /// file names no junction.
    pub a: Option<u32>,
    pub b: Option<u32>,
    pub points: Vec<[f64; 3]>,
}

/// One connector, drawn from a point of edge `a` to a point of edge `b`.
#[derive(Debug, Clone, PartialEq)]
pub struct Connector {
    pub a: u32,
    pub b: u32,
    /// Its first and last points, `[x, z]`.
    pub ends: [[f64; 2]; 2],
    /// Meters along it.
    pub length: f64,
}

/// The router's table: end `i` is edge `edge[i]` at junction `junction[i]`,
/// linked to the ends `to[off[i]..off[i + 1]]`, each at a whole number of
/// meters.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Table {
    pub junction: Vec<u32>,
    pub edge: Vec<u32>,
    off: Vec<u32>,
    to: Vec<u32>,
    cost: Vec<u32>,
}

impl Table {
    /// The ends `end` links to, each with its cost.
    pub fn links(&self, end: usize) -> impl Iterator<Item = (usize, u32)> + '_ {
        (self.off[end] as usize..self.off[end + 1] as usize)
            .map(|k| (self.to[k] as usize, self.cost[k]))
    }

    /// The two ends of each of `edges` edges, refused unless each has two.
    pub fn ends(&self, edges: usize) -> Result<Vec<[u32; 2]>, Error> {
        let mut out = vec![[u32::MAX; 2]; edges];
        for (i, &e) in self.edge.iter().enumerate() {
            let slot = out
                .get_mut(e as usize)
                .ok_or_else(|| malformed(format!("the table names edge {e} of {edges}")))?;
            let free = slot.iter_mut().find(|s| **s == u32::MAX);
            *free.ok_or_else(|| malformed(format!("edge {e} has over two table ends")))? = i as u32;
        }
        match out.iter().position(|s| s[1] == u32::MAX) {
            Some(e) => Err(malformed(format!("edge {e} has under two table ends"))),
            None => Ok(out),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Network {
    pub classes: Vec<String>,
    pub edges: Vec<Edge>,
    /// The ids whose ends spread across the map, written as no junction.
    pub catch_all: Vec<u32>,
    pub table: Table,
    pub connectors: Vec<Connector>,
}

impl Network {
    pub fn class(&self, e: &Edge) -> &str {
        &self.classes[usize::from(e.class)]
    }
}

pub(super) fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

pub(super) fn f64_at(b: &[u8], at: usize) -> f64 {
    f64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}

/// Reads the class table at `at`, returning it and where it ends.
pub(super) fn classes(b: &[u8], mut at: usize) -> Result<(Vec<String>, usize), Error> {
    let short = || malformed("the class table runs past the end of the file".into());
    let count = u32_at(b, at).ok_or_else(short)?;
    if count == 0 || count > MAX_CLASSES {
        return Err(malformed(format!("a class table of {count} entries")));
    }
    at += 4;
    let mut out = Vec::with_capacity(count as usize);
    for i in 0..count {
        let len = u32_at(b, at).ok_or_else(short)?;
        if len == 0 || len > MAX_CLASS_LEN {
            return Err(malformed(format!("class {i} has a name of {len} bytes")));
        }
        let name = b.get(at + 4..at + 4 + len as usize).ok_or_else(short)?;
        if !name.iter().all(|c| c.is_ascii_graphic()) {
            return Err(malformed(format!(
                "class {i}'s name is not printable ASCII"
            )));
        }
        out.push(String::from_utf8(name.to_vec()).expect("checked ASCII"));
        at += 4 + len as usize;
    }
    Ok((out, at))
}

/// The `n` points that start at `start`, if all lie in `reach`.
// Inlined into the scan: called at every byte, a call left out of line
// doubles it.
#[inline(always)]
fn run(b: &[u8], start: usize, n: u32, reach: &Reach) -> Option<Vec<[f64; 3]>> {
    if start + 24 * n as usize > b.len() {
        return None;
    }
    let point = |k: usize| {
        let at = start + 24 * k;
        [f64_at(b, at), f64_at(b, at + 8), f64_at(b, at + 16)]
    };
    // Nearly every offset fails on its first point, so nothing is allocated
    // until the whole run has passed.
    (0..n as usize)
        .all(|k| reach.holds(point(k)))
        .then(|| points_at(b, start, n as usize))
}

/// The `n` points that start at `start`, which the caller knows are there.
fn points_at(b: &[u8], start: usize, n: usize) -> Vec<[f64; 3]> {
    (0..n)
        .map(|k| {
            let at = start + 24 * k;
            [f64_at(b, at), f64_at(b, at + 8), f64_at(b, at + 16)]
        })
        .collect()
}

/// The header of `b`, refused unless it names `want`. `name` is how an error
/// names the file.
pub(super) fn header(b: &[u8], name: &str, want: Decoder) -> Result<container::Header, Error> {
    let head = &b[..b.len().min(MAX_HEAD)];
    let h = container::parse(head).map_err(|e| malformed(format!("not a container: {e}")))?;
    match dispatch(name, &h) {
        Ok(d) if d == want => Ok(h),
        Ok(d) => Err(malformed(format!(
            "{} version {} is read by the {d:?} decoder, not this one",
            h.class, h.version
        ))),
        Err(e) => Err(malformed(e.to_string())),
    }
}

/// Decodes a whole road network file. `name` is how an error names it.
pub fn parse(b: &[u8], name: &str, reach: &Reach) -> Result<Network, Error> {
    collect(b, name, reach)
}

fn collect(r: impl Read, name: &str, reach: &Reach) -> Result<Network, Error> {
    let mut edges = Vec::new();
    let mut connectors = Vec::new();
    let (classes, table) = scan(r, name, reach, |e| edges.push(e), |c| connectors.push(c))?;
    let catch_all = unjoin_catch_alls(&mut edges);
    Ok(Network {
        classes,
        edges,
        catch_all,
        table,
        connectors,
    })
}

/// Bytes of the file held at a time while it is scanned.
const WINDOW: usize = 16 << 20;
/// What the scan needs past a candidate: its head and the longest run's
/// points.
const AHEAD: usize = (1 << 12) + 4 + 24 * MAX_POINTS as usize;

/// The file a window at a time, `at` being how far it is read in `b`.
struct Bytes<R> {
    r: R,
    b: Vec<u8>,
    at: usize,
    eof: bool,
}

impl<R: Read> Bytes<R> {
    fn new(mut r: R) -> io::Result<Bytes<R>> {
        let mut b = Vec::with_capacity(WINDOW);
        let eof = fill(&mut r, &mut b)?;
        Ok(Bytes { r, b, at: 0, eof })
    }

    /// Makes `n` bytes past `at` available if the file has them, and says
    /// whether it did.
    #[inline(always)]
    fn ahead(&mut self, n: usize) -> io::Result<bool> {
        if !self.eof && self.b.len() - self.at < n {
            self.b.drain(..self.at);
            self.at = 0;
            self.eof = fill(&mut self.r, &mut self.b)?;
        }
        Ok(self.b.len() - self.at >= n)
    }

    /// The u32 at `at`, moving past it. `what` names it in the error.
    fn count(&mut self, what: &str) -> Result<u32, Error> {
        self.ahead(4)?;
        let n = u32_at(&self.b, self.at)
            .ok_or_else(|| malformed(format!("the file ends before its {what}")))?;
        self.at += 4;
        Ok(n)
    }
}

/// Reads the table of `count` ends at `s.at`, moving past it, a window at a
/// time: it runs to 110 MiB on GermanyColdWar.
fn table<R: Read>(s: &mut Bytes<R>, count: u32) -> Result<Table, Error> {
    let short = || malformed("the table runs past the end of the file".into());
    let mut t = Table::default();
    t.off.push(0);
    for i in 0..count {
        if !s.ahead(16)? {
            return Err(short());
        }
        let word = |k: usize| u32_at(&s.b, s.at + 4 * k).unwrap();
        let (open, junction, edge, k) = (word(0), word(1), word(2), word(3));
        if open != 1 || k > MAX_LINKS {
            return Err(malformed(format!(
                "table end {i} opens {open} and has {k} links"
            )));
        }
        t.junction.push(junction);
        t.edge.push(edge);
        s.at += 16;
        if !s.ahead(8 * k as usize)? {
            return Err(short());
        }
        for l in 0..k as usize {
            let to = u32_at(&s.b, s.at + 8 * l).unwrap();
            if to >= count {
                return Err(malformed(format!("table end {i} links to end {to}")));
            }
            t.to.push(to);
            t.cost.push(u32_at(&s.b, s.at + 8 * l + 4).unwrap());
        }
        s.at += 8 * k as usize;
        t.off.push(t.to.len() as u32);
    }
    Ok(t)
}

/// Where the point count lies of the edge whose head is at `q`, if a head is
/// there. `fits` tells whether two ids are those of the edge looked for.
#[inline(always)]
fn head(b: &[u8], q: usize, classes: usize, fits: impl Fn(u32, u32) -> bool) -> Option<usize> {
    if u32_at(b, q)? != EDGE_TAG || *b.get(q + 4)? > KIND_ROAD {
        return None;
    }
    if usize::from(*b.get(q + 5)?) >= classes || !fits(u32_at(b, q + 6)?, u32_at(b, q + 10)?) {
        return None;
    }
    let properties = u32_at(b, q + 14)?;
    if properties > MAX_PROPERTIES {
        return None;
    }
    let mut at = q + 18;
    for _ in 0..properties * 2 {
        let len = u32_at(b, at)?;
        if len > MAX_PROPERTY_LEN {
            return None;
        }
        at += 4 + len as usize;
    }
    Some(at)
}

/// Scans the road network `r` holds from its start, a window at a time,
/// handing each edge to `edge` in file order with its ends as the file names
/// them and each connector to `connector`, and returns the class table and
/// the router's table. Holds `WINDOW` bytes of the file, which is 2.6 GiB on
/// GermanyColdWar. `name` is how an error names it.
pub fn scan<R: Read>(
    r: R,
    name: &str,
    reach: &Reach,
    mut edge: impl FnMut(Edge),
    mut connector: impl FnMut(Connector),
) -> Result<(Vec<String>, Table), Error> {
    let mut s = Bytes::new(r)?;
    let h = header(&s.b, name, Decoder::RoadNetwork)?;
    let (classes, body) = classes(&s.b, h.body())?;
    s.at = body;
    let count = s.count("table")?;
    let t = table(&mut s, count)?;
    let edges = s.count("edges")?;
    let ends = t.ends(edges as usize)?;
    let mut any = false;
    for id in 0..edges {
        let [e0, e1] = ends[id as usize].map(|e| t.junction[e as usize]);
        // An end that names no junction is at the table's catch-all.
        let named = |file: u32, end: u32| file == NO_NODE || file == end;
        let fits =
            |a: u32, b: u32| (named(a, e0) && named(b, e1)) || (named(a, e1) && named(b, e0));
        let (q, n) = loop {
            let more = s.ahead(AHEAD)?;
            if !more && s.at + 4 > s.b.len() {
                return Err(malformed(format!("edge {id} of {edges} was not found")));
            }
            let found = head(&s.b, s.at, classes.len(), fits).and_then(|q| {
                let n = u32_at(&s.b, q)?;
                let held = q + 4 + 24 * n as usize <= s.b.len();
                ((MIN_POINTS..=MAX_POINTS).contains(&n) && held).then_some((q, n as usize))
            });
            if let Some(found) = found {
                break found;
            }
            s.at += 1;
        };
        let word = |k: usize| u32_at(&s.b, s.at + k).unwrap();
        let (kind, class, a, b) = (s.b[s.at + 4], s.b[s.at + 5], word(6), word(10));
        any |= kind != KIND_AIRFIELD;
        edge(Edge {
            id,
            kind,
            class,
            a: node(a),
            b: node(b),
            points: points_at(&s.b, q + 4, n),
        });
        s.at = q + 4 + 24 * n;
    }
    // No map has no roads, so that is an error, not an empty table.
    if !any {
        return Err(malformed("no road or rail edge was found".into()));
    }
    loop {
        let more = s.ahead(AHEAD)?;
        if !more && s.at + 16 > s.b.len() {
            break;
        }
        let word = |k: usize| u32_at(&s.b, s.at + k).unwrap();
        let (tag, a, b, n) = (word(0), word(4), word(8), word(12));
        let framed = tag == CONNECTOR_TAG
            && a < edges
            && b < edges
            && (MIN_POINTS..=MAX_CONNECTOR_POINTS).contains(&n);
        match framed.then(|| run(&s.b, s.at + 16, n, reach)).flatten() {
            Some(p) => {
                let flat: Vec<[f64; 2]> = p.iter().map(|q| [q[0], q[2]]).collect();
                let length = flat
                    .windows(2)
                    .map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]))
                    .sum();
                connector(Connector {
                    a,
                    b,
                    ends: [flat[0], flat[flat.len() - 1]],
                    length,
                });
                s.at += 16 + 24 * p.len();
            }
            None => s.at += 1,
        }
    }
    Ok((classes, t))
}

/// Reads from `r` until `b` holds `WINDOW` bytes or the file ends, and says
/// whether it ended.
fn fill(r: &mut impl Read, b: &mut Vec<u8>) -> io::Result<bool> {
    let want = WINDOW.saturating_sub(b.len()) as u64;
    let got = r.by_ref().take(want).read_to_end(b)? as u64;
    Ok(got < want)
}

fn node(id: u32) -> Option<u32> {
    (id != NO_NODE).then_some(id)
}

/// Clears every end naming an id whose road and rail ends do not all lie
/// within reach of their mean, and returns those ids. Each map has
/// one id whose ends are spread across it, and joined through one node they
/// would weld its dead ends together.
fn unjoin_catch_alls(edges: &mut [Edge]) -> Vec<u32> {
    let wide = catch_alls(
        edges
            .iter()
            .filter(|e| e.kind != KIND_AIRFIELD)
            .flat_map(|e| ends(e.a, e.b, &e.points)),
    );
    for e in edges.iter_mut().filter(|e| e.kind != KIND_AIRFIELD) {
        for end in [&mut e.a, &mut e.b] {
            if end.is_some_and(|id| wide.binary_search(&id).is_ok()) {
                *end = None;
            }
        }
    }
    wide
}

/// The named ends of an edge with `points`, as `(id, [x, z])`.
pub fn ends(
    a: Option<u32>,
    b: Option<u32>,
    points: &[[f64; 3]],
) -> impl Iterator<Item = (u32, [f64; 2])> {
    let (first, last) = (points[0], points[points.len() - 1]);
    [(a, first), (b, last)]
        .into_iter()
        .filter_map(|(id, p)| id.map(|id| (id, [p[0], p[2]])))
}

/// The ids, sorted, among `ends` of the road and rail edges whose ends do not
/// all lie within reach of their mean: each map's catch-all, which names no
/// junction.
pub fn catch_alls(ends: impl IntoIterator<Item = (u32, [f64; 2])>) -> Vec<u32> {
    let mut by_id: HashMap<u32, Vec<[f64; 2]>> = HashMap::new();
    for (id, p) in ends {
        by_id.entry(id).or_default().push(p);
    }
    let mut wide: Vec<u32> = by_id
        .into_iter()
        .filter(|(_, l)| spread(l) > JUNCTION_REACH_M)
        .map(|(id, _)| id)
        .collect();
    wide.sort_unstable();
    wide
}

/// How far the farthest of `points` lies from their mean.
pub fn spread(points: &[[f64; 2]]) -> f64 {
    let n = points.len() as f64;
    let mx = points.iter().map(|p| p[0]).sum::<f64>() / n;
    let mz = points.iter().map(|p| p[1]).sum::<f64>() / n;
    points
        .iter()
        .map(|p| (p[0] - mx).hypot(p[1] - mz))
        .fold(0.0, f64::max)
}

/// Reads and decodes the road network at `path`, a window at a time.
pub fn read(path: &Path, reach: &Reach) -> Result<Network, Error> {
    collect(File::open(path)?, &path.to_string_lossy(), reach)
}

/// Writes the road and rail edges under `dir`, one JSON object a line, each
/// file as a `.tmp` renamed into place, and returns how many of each. An
/// airfield line is not written: its `y` is not a height.
pub fn write_tables(net: &Network, dir: &Path) -> io::Result<(usize, usize)> {
    let roads = write_kind(net, KIND_ROAD, &dir.join(ROADS_FILE))?;
    let rail = write_kind(net, KIND_RAIL, &dir.join(RAILROADS_FILE))?;
    Ok((roads, rail))
}

fn write_kind(net: &Network, kind: u8, path: &Path) -> io::Result<usize> {
    let tmp = path.with_file_name(format!(
        "{}.tmp",
        path.file_name().unwrap().to_string_lossy()
    ));
    let mut out = BufWriter::new(File::create(&tmp)?);
    let mut n = 0;
    for e in net.edges.iter().filter(|e| e.kind == kind) {
        // The extract's points are `[x, z, alt]`; the file's are `[x, y, z]`.
        let points: Vec<[f64; 3]> = e.points.iter().map(|&[x, y, z]| [x, z, y]).collect();
        let row = json!({
            "id": e.id, "class": net.class(e), "a": e.a, "b": e.b, "points": points,
        });
        serde_json::to_writer(&mut out, &row)?;
        out.write_all(b"\n")?;
        n += 1;
    }
    out.into_inner().map_err(|e| e.into_error())?.sync_all()?;
    fs::rename(&tmp, path)?;
    Ok(n)
}

#[cfg(any(test, feature = "testing"))]
pub mod build {
    //! Road network files as the decoder reads them, for tests.

    use super::super::container;

    pub struct Edge {
        pub kind: u8,
        pub class: u8,
        pub a: u32,
        pub b: u32,
        pub points: Vec<[f64; 3]>,
    }

    fn run(out: &mut Vec<u8>, points: &[[f64; 3]]) {
        out.extend_from_slice(&(points.len() as u32).to_le_bytes());
        for p in points {
            for v in p {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
    }

    /// One edge as the file holds it, its unit tangents after it, with
    /// `properties` pairs of a key and a value.
    pub fn edge_with(e: &Edge, properties: &[(&str, &str)]) -> Vec<u8> {
        let mut b = 6u32.to_le_bytes().to_vec();
        b.push(e.kind);
        b.push(e.class);
        b.extend_from_slice(&e.a.to_le_bytes());
        b.extend_from_slice(&e.b.to_le_bytes());
        b.extend_from_slice(&(properties.len() as u32).to_le_bytes());
        for s in properties.iter().flat_map(|(k, v)| [k, v]) {
            b.extend_from_slice(&(s.len() as u32).to_le_bytes());
            b.extend_from_slice(s.as_bytes());
        }
        run(&mut b, &e.points);
        let tangents: Vec<[f64; 3]> = e.points.iter().map(|_| [1.0, 0.0, 0.0]).collect();
        run(&mut b, &tangents);
        b
    }

    pub fn edge(e: &Edge) -> Vec<u8> {
        edge_with(e, &[])
    }

    /// The table of `edges`: two ends for each, the first at `a`, linked to
    /// each other, and each pair `passes` names linked as a passage at the
    /// junction they share.
    fn table(edges: &[Edge], passes: &[(u32, u32)]) -> Vec<u8> {
        let junction = |end: usize| {
            let e = &edges[end / 2];
            if end & 1 == 0 { e.a } else { e.b }
        };
        let mut links: Vec<Vec<usize>> = (0..edges.len() * 2).map(|i| vec![i ^ 1]).collect();
        for &(a, b) in passes {
            let (a, b) = (a as usize * 2, b as usize * 2);
            let pair = [a, a + 1]
                .into_iter()
                .flat_map(|p| [(p, b), (p, b + 1)])
                .find(|&(p, q)| junction(p) == junction(q))
                .expect("a passage joins edges with a junction in common");
            links[pair.0].push(pair.1);
            links[pair.1].push(pair.0);
        }
        let mut out = (links.len() as u32).to_le_bytes().to_vec();
        for (i, l) in links.iter().enumerate() {
            let words = [1, junction(i), (i / 2) as u32, l.len() as u32];
            out.extend(words.iter().flat_map(|w| w.to_le_bytes()));
            for &to in l {
                out.extend_from_slice(&(to as u32).to_le_bytes());
                out.extend_from_slice(&1u32.to_le_bytes());
            }
        }
        out
    }

    /// A file of `classes`, then `edges` as `bytes` lays each out, with
    /// `passes` in the table, filler, then `connectors` of a pair of edges
    /// and their points.
    pub fn file_of(
        classes: &[&str],
        edges: &[Edge],
        bytes: &[Vec<u8>],
        passes: &[(u32, u32)],
        connectors: &[(u32, u32, Vec<[f64; 3]>)],
    ) -> Vec<u8> {
        let mut b = container::build("landscape4::lRoadNetwork", 5, 0);
        b.extend_from_slice(&(classes.len() as u32).to_le_bytes());
        for c in classes {
            b.extend_from_slice(&(c.len() as u32).to_le_bytes());
            b.extend_from_slice(c.as_bytes());
        }
        b.extend_from_slice(&table(edges, passes));
        b.extend_from_slice(&(edges.len() as u32).to_le_bytes());
        for e in bytes {
            b.extend_from_slice(e);
        }
        b.extend_from_slice(&[0; 64]);
        for (x, y, points) in connectors {
            b.extend_from_slice(&3u32.to_le_bytes());
            b.extend_from_slice(&x.to_le_bytes());
            b.extend_from_slice(&y.to_le_bytes());
            run(&mut b, points);
        }
        b.extend_from_slice(&[0; 64]);
        b
    }

    /// A file of `classes` and `edges`, each with its unit tangents after
    /// it, joined by no passage.
    pub fn file(classes: &[&str], edges: &[Edge]) -> Vec<u8> {
        let bytes: Vec<Vec<u8>> = edges.iter().map(edge).collect();
        file_of(classes, edges, &bytes, &[], &[])
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::build::{self, file};
    use super::*;
    use crate::scratch::Scratch;

    const REACH: Reach = Reach {
        min_x: -100_000.0,
        max_x: 100_000.0,
        min_y: -100.0,
        max_y: 3000.0,
        min_z: -100_000.0,
        max_z: 100_000.0,
    };

    fn edge(kind: u8, class: u8, a: u32, b: u32, x: f64) -> build::Edge {
        build::Edge {
            kind,
            class,
            a,
            b,
            points: vec![
                [x, 101.5, 2000.0],
                [x + 5.0, 102.25, 2003.0],
                [x + 9.0, 103.0, 2004.5],
            ],
        }
    }

    #[test]
    fn reads_a_built_edge() {
        let b = file(&["road", "rw"], &[edge(KIND_ROAD, 0, 10411, 10412, 1000.0)]);
        let net = parse(&b, "X.rn4", &REACH).unwrap();
        assert_eq!(net.classes, ["road", "rw"]);
        assert_eq!(
            net.edges,
            [Edge {
                id: 0,
                kind: KIND_ROAD,
                class: 0,
                a: Some(10411),
                b: Some(10412),
                points: vec![
                    [1000.0, 101.5, 2000.0],
                    [1005.0, 102.25, 2003.0],
                    [1009.0, 103.0, 2004.5]
                ],
            }]
        );
        assert_eq!(net.class(&net.edges[0]), "road");
    }

    #[test]
    fn numbers_every_edge_in_file_order_and_skips_connectors() {
        let edges = [
            edge(KIND_ROAD, 0, 1, 2, 1000.0),
            edge(KIND_AIRFIELD, 2, 3, 4, 2000.0),
            edge(KIND_RAIL, 1, 2, 5, 3000.0),
        ];
        let net = parse(&file(&["road", "rw", "vpp"], &edges), "X.rn4", &REACH).unwrap();
        let got: Vec<_> = net
            .edges
            .iter()
            .map(|e| (e.id, e.kind, e.class, e.a, e.b))
            .collect();
        assert_eq!(
            got,
            [
                (0, 2, 0, Some(1), Some(2)),
                (1, 0, 2, Some(3), Some(4)),
                (2, 1, 1, Some(2), Some(5))
            ]
        );
    }

    #[test]
    fn an_id_spread_across_the_map_and_the_none_value_are_no_junction() {
        let edges = [
            // 7 joins two ends 4.5 m apart; 9 joins ends 30 km apart.
            edge(KIND_ROAD, 0, 9, 7, 1000.0),
            edge(KIND_RAIL, 1, 7, 9, 1009.0),
            edge(KIND_ROAD, 0, 9, NO_NODE, 31_000.0),
            // An airfield line naming 9 is neither measured nor cleared.
            edge(KIND_AIRFIELD, 2, 9, 9, 60_000.0),
        ];
        let net = parse(&file(&["road", "rw", "vpp"], &edges), "X.rn4", &REACH).unwrap();
        let ends: Vec<_> = net.edges.iter().map(|e| (e.a, e.b)).collect();
        assert_eq!(
            ends,
            [
                (None, Some(7)),
                (Some(7), None),
                (None, None),
                (Some(9), Some(9))
            ]
        );
        assert_eq!(net.catch_all, [9]);
    }

    #[test]
    fn an_edge_at_x_zero_and_one_with_properties_are_edges_and_numbers_count_them() {
        let e = [
            edge(KIND_ROAD, 0, 1, 2, 1000.0),
            edge(KIND_ROAD, 0, 2, 3, 0.0),
            edge(KIND_ROAD, 0, 3, 4, 2000.0),
        ];
        let bytes = [
            build::edge(&e[0]),
            build::edge(&e[1]),
            build::edge_with(&e[2], &[("oneway", "true"), ("inversetraffic", "true")]),
        ];
        let b = build::file_of(&["road"], &e, &bytes, &[], &[]);
        let net = parse(&b, "X.rn4", &REACH).unwrap();
        let got: Vec<_> = net.edges.iter().map(|e| (e.id, e.a)).collect();
        assert_eq!(got, [(0, Some(1)), (1, Some(2)), (2, Some(3))]);
        assert_eq!(net.edges[2].points[0][0], 2000.0);
    }

    #[test]
    fn reads_the_table_and_connectors_and_passes_over_points_out_of_reach() {
        let e = [
            edge(KIND_ROAD, 0, 1, 2, 1000.0),
            edge(KIND_ROAD, 0, 2, 3, 1009.0),
        ];
        let bytes: Vec<Vec<u8>> = e.iter().map(build::edge).collect();
        let on = vec![[1009.0, 103.0, 2004.5], [1012.0, 103.0, 2004.5]];
        let wild = vec![[1e8, 103.0, 2004.5], [1012.0, 103.0, 2004.5]];
        let b = build::file_of(
            &["road"],
            &e,
            &bytes,
            &[(0, 1)],
            &[(0, 1, on), (1, 0, wild)],
        );
        let net = parse(&b, "X.rn4", &REACH).unwrap();
        let [c] = &net.connectors[..] else {
            panic!("{:?}", net.connectors)
        };
        assert_eq!((c.a, c.b, c.length), (0, 1, 3.0));
        assert_eq!(c.ends, [[1009.0, 2004.5], [1012.0, 2004.5]]);
        assert_eq!(net.table.ends(2).unwrap(), [[0, 1], [2, 3]]);
        assert_eq!((net.table.junction[1], net.table.edge[2]), (2, 1));
        let links: Vec<_> = net.table.links(1).map(|l| l.0).collect();
        assert_eq!(links, [0, 2]);
    }

    #[test]
    fn refuses_a_table_that_gives_an_edge_no_two_ends() {
        let mut b = container::build("landscape4::lRoadNetwork", 5, 0);
        b.extend_from_slice(&1u32.to_le_bytes());
        b.extend_from_slice(&4u32.to_le_bytes());
        b.extend_from_slice(b"road");
        b.extend_from_slice(&1u32.to_le_bytes());
        for w in [1u32, 7, 0, 0] {
            b.extend_from_slice(&w.to_le_bytes());
        }
        b.extend_from_slice(&1u32.to_le_bytes());
        let e = parse(&b, "X.rn4", &REACH).unwrap_err().to_string();
        assert_eq!(e, "edge 0 has under two table ends");
    }

    #[test]
    fn refuses_a_class_past_the_table_and_an_unknown_kind() {
        let b = file(&["road"], &[edge(KIND_ROAD, 1, 1, 2, 1000.0)]);
        let e = parse(&b, "X.rn4", &REACH).unwrap_err().to_string();
        assert_eq!(e, "edge 0 of 1 was not found");

        let b = file(&["road"], &[edge(3, 0, 1, 2, 1000.0)]);
        let e = parse(&b, "X.rn4", &REACH).unwrap_err().to_string();
        assert_eq!(e, "edge 0 of 1 was not found");
    }

    #[test]
    fn refuses_another_class_of_file() {
        let mut b = container::build("t5::rn5file", 0, 0);
        b.extend_from_slice(&[0; 32]);
        let e = parse(&b, "X.rn5", &REACH).unwrap_err().to_string();
        assert!(e.contains("read by the Rn5 decoder"), "{e}");

        let b = container::build("landscape4::lRoadNetwork", 6, 0);
        let e = parse(&b, "X.rn4", &REACH).unwrap_err().to_string();
        assert!(e.contains("version 6 has no decoder"), "{e}");
    }

    #[test]
    fn finds_edges_on_both_sides_of_a_window_and_across_its_end() {
        let e = [
            edge(KIND_ROAD, 0, 1, 2, 1000.0),
            edge(KIND_ROAD, 0, 2, 3, 2000.0),
            edge(KIND_RAIL, 0, 3, 4, 3000.0),
        ];
        // The second edge's points straddle the end of the first window.
        let mut bytes: Vec<Vec<u8>> = e.iter().map(build::edge).collect();
        let lead = file(&["road"], &e[..0]).len() + 4 * 8 * 6;
        bytes[0].resize(WINDOW - lead - 30, 0);
        let long = bytes[1].len() + WINDOW;
        bytes[1].resize(long, 0);
        let b = build::file_of(&["road"], &e, &bytes, &[], &[]);
        let mut got = Vec::new();
        scan(
            &b[..],
            "X.rn4",
            &REACH,
            |e| got.push((e.id, e.a, e.points[0][0])),
            |_| {},
        )
        .unwrap();
        assert_eq!(
            got,
            [
                (0, Some(1), 1000.0),
                (1, Some(2), 2000.0),
                (2, Some(3), 3000.0)
            ]
        );
    }

    #[test]
    fn refuses_a_file_with_no_road_or_rail_edge() {
        let b = file(&["vpp"], &[edge(KIND_AIRFIELD, 0, 1, 2, 1000.0)]);
        let e = parse(&b, "X.rn4", &REACH).unwrap_err().to_string();
        assert_eq!(e, "no road or rail edge was found");
    }

    #[test]
    fn refuses_a_class_table_that_is_not_one() {
        let mut b = container::build("landscape4::lRoadNetwork", 5, 0);
        b.extend_from_slice(&100_000u32.to_le_bytes());
        let e = parse(&b, "X.rn4", &REACH).unwrap_err().to_string();
        assert_eq!(e, "a class table of 100000 entries");

        let mut b = container::build("landscape4::lRoadNetwork", 5, 0);
        b.extend_from_slice(&2u32.to_le_bytes());
        b.extend_from_slice(&4u32.to_le_bytes());
        b.extend_from_slice(b"road");
        let e = parse(&b, "X.rn4", &REACH).unwrap_err().to_string();
        assert_eq!(e, "the class table runs past the end of the file");
    }

    #[test]
    fn writes_roads_and_rail_apart_with_alt_last_and_no_airfield_lines() {
        let edges = [
            edge(KIND_ROAD, 0, 10411, 10412, 1000.0),
            edge(KIND_AIRFIELD, 2, 3, 4, 2000.0),
            edge(KIND_RAIL, 1, 10412, 5, 3000.0),
            edge(KIND_ROAD, 0, 10412, 6, 4000.0),
        ];
        let net = parse(&file(&["road", "rw", "vpp"], &edges), "X.rn4", &REACH).unwrap();
        let s = Scratch::new();
        fs::create_dir_all(&s.0).unwrap();
        assert_eq!(write_tables(&net, &s.0).unwrap(), (2, 1));
        let rows = |name: &str| -> Vec<Value> {
            fs::read_to_string(s.0.join(name))
                .unwrap()
                .lines()
                .map(|l| serde_json::from_str(l).unwrap())
                .collect()
        };
        let roads = rows(ROADS_FILE);
        assert_eq!(
            roads[0],
            json!({
                "id": 0, "class": "road", "a": 10411, "b": 10412,
                "points": [[1000.0, 2000.0, 101.5], [1005.0, 2003.0, 102.25], [1009.0, 2004.5, 103.0]],
            })
        );
        assert_eq!(roads[1]["id"], 3);
        let rail = rows(RAILROADS_FILE);
        assert_eq!(
            (rail.len(), &rail[0]["id"], &rail[0]["class"]),
            (1, &json!(2), &json!("rw"))
        );
        let left: Vec<_> = fs::read_dir(&s.0)
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(left.len(), 2, "{left:?}");
    }
}
