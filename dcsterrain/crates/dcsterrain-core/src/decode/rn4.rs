//! `landscape4::lRoadNetwork`, version 5, the map-wide `roads/<Map>.rn4`:
//! the road and rail network as edges between node ids, each a polyline of
//! f64 vertices.
//!
//! The file opens with the class table, then a node table, the edges and a
//! spatial index, packed with no alignment. Only the class table's framing
//! is understood, so the edges are found by scanning for their point runs:
//!
//! ```text
//! after the version   u32 count, count × (u32 len, len bytes)   classes
//! an edge, q being the offset of its point count:
//!   q-14  u8   kind: 2 road, 1 rail, 0 airfield line
//!   q-13  u8   class, an index into the class table
//!   q-12  u32  node a
//!   q-8   u32  node b
//!   q-4   u32  0
//!   q     u32  n, then n × f64 (x, y, z)
//! ```
//!
//! A run is a point count followed by that many triples that all lie within
//! reach of the ground. The zero before the count is what tells an edge from
//! the short connector runs of the node table, which carry no class. Tangents
//! follow the points as unit vectors, so no tangent array passes for a run.

use std::collections::HashMap;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
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
/// How far past the mesh's extent a vertex may lie, sideways and in height.
/// Wide, because the box only has to refuse bytes that are not coordinates.
const REACH_XZ_M: f64 = 20_000.0;
const REACH_Y_M: f64 = 500.0;
/// No vertex sits on the x = 0 line of any map, while zeroed bytes read as
/// a triple of zeros there.
const MIN_ABS_X_M: f64 = 1.0;
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

    fn holds(&self, [x, y, z]: [f64; 3]) -> bool {
        x.abs() > MIN_ABS_X_M
            && (self.min_x..=self.max_x).contains(&x)
            && (self.min_z..=self.max_z).contains(&z)
            && y > self.min_y
            && y < self.max_y
    }
}

/// One edge as the file holds it. `points` are `[x, y, z]`, `y` up.
#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    /// The ordinal among the edges of the scan, every kind counted.
    pub id: u32,
    pub kind: u8,
    pub class: u8,
    /// The junction at the first vertex, and at the last. The edge stops short
    /// of it; none where the file names no junction.
    pub a: Option<u32>,
    pub b: Option<u32>,
    pub points: Vec<[f64; 3]>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Network {
    pub classes: Vec<String>,
    pub edges: Vec<Edge>,
    /// The ids whose ends spread across the map, written as no junction.
    pub catch_all: Vec<u32>,
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

/// The points of the run whose count is at `q`, if one is there.
fn run(b: &[u8], q: usize, reach: &Reach) -> Option<Vec<[f64; 3]>> {
    let n = u32_at(b, q)?;
    if !(MIN_POINTS..=MAX_POINTS).contains(&n) {
        return None;
    }
    let start = q + 4;
    if start + 24 * n as usize > b.len() {
        return None;
    }
    let point = |k: usize| {
        let at = start + 24 * k;
        [f64_at(b, at), f64_at(b, at + 8), f64_at(b, at + 16)]
    };
    // Nearly every offset fails on its first point, so nothing is allocated
    // until the whole run has passed.
    if !(0..n as usize).all(|k| reach.holds(point(k))) {
        return None;
    }
    Some((0..n as usize).map(point).collect())
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
    let h = header(b, name, Decoder::RoadNetwork)?;
    let (classes, mut i) = classes(b, h.body())?;
    let mut edges = Vec::new();
    // Greedy from the left: a candidate that starts inside an accepted run
    // is that run's own bytes.
    while i + 4 <= b.len() {
        let Some(points) = run(b, i, reach) else {
            i += 1;
            continue;
        };
        let q = i;
        i = q + 4 + 24 * points.len();
        if q < 14 || u32_at(b, q - 4) != Some(0) {
            continue;
        }
        let (kind, class) = (b[q - 14], b[q - 13]);
        let id = edges.len() as u32;
        if kind > KIND_ROAD {
            return Err(malformed(format!(
                "edge {id} at byte {q} has kind {kind}, where 0, 1 and 2 are read"
            )));
        }
        if usize::from(class) >= classes.len() {
            return Err(malformed(format!(
                "edge {id} at byte {q} has class {class}, past the {} the table holds",
                classes.len()
            )));
        }
        edges.push(Edge {
            id,
            kind,
            class,
            a: node(u32_at(b, q - 12).unwrap()),
            b: node(u32_at(b, q - 8).unwrap()),
            points,
        });
    }
    // An empty mesh, or a file of another layout, leaves every run out of
    // reach: no map has no roads, so that is an error, not an empty table.
    if !edges.iter().any(|e| e.kind != KIND_AIRFIELD) {
        return Err(malformed("no road or rail edge was found".into()));
    }
    let catch_all = unjoin_catch_alls(&mut edges);
    Ok(Network {
        classes,
        edges,
        catch_all,
    })
}

fn node(id: u32) -> Option<u32> {
    (id != NO_NODE).then_some(id)
}

/// Clears every end naming an id whose road and rail ends do not all lie
/// within reach of their mean, and returns those ids. Each map has
/// one id whose ends are spread across it, and joined through one node they
/// would weld its dead ends together.
fn unjoin_catch_alls(edges: &mut [Edge]) -> Vec<u32> {
    let mut ends: HashMap<u32, Vec<[f64; 2]>> = HashMap::new();
    for e in edges.iter().filter(|e| e.kind != KIND_AIRFIELD) {
        let (first, last) = (e.points[0], e.points[e.points.len() - 1]);
        for (id, p) in [(e.a, first), (e.b, last)] {
            if let Some(id) = id {
                ends.entry(id).or_default().push([p[0], p[2]]);
            }
        }
    }
    let mut wide: Vec<u32> = ends
        .into_iter()
        .filter(|(_, l)| spread(l) > JUNCTION_REACH_M)
        .map(|(id, _)| id)
        .collect();
    wide.sort_unstable();
    for e in edges.iter_mut().filter(|e| e.kind != KIND_AIRFIELD) {
        for end in [&mut e.a, &mut e.b] {
            if end.is_some_and(|id| wide.binary_search(&id).is_ok()) {
                *end = None;
            }
        }
    }
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

/// Reads and decodes the road network at `path`.
pub fn read(path: &Path, reach: &Reach) -> Result<Network, Error> {
    parse(&fs::read(path)?, &path.to_string_lossy(), reach)
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

#[cfg(test)]
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

    /// A file of `classes`, a node-table connector, then each edge with its
    /// unit tangents after it, then filler.
    pub fn file(classes: &[&str], edges: &[Edge]) -> Vec<u8> {
        let mut b = container::build("landscape4::lRoadNetwork", 5, 0);
        b.extend_from_slice(&(classes.len() as u32).to_le_bytes());
        for c in classes {
            b.extend_from_slice(&(c.len() as u32).to_le_bytes());
            b.extend_from_slice(c.as_bytes());
        }
        // A connector run: plausible points with no zero before the count.
        b.extend_from_slice(&[3, 0, 0, 0, 7, 0, 0, 0, 8, 0, 0, 0]);
        run(&mut b, &[[5000.0, 10.0, 5000.0], [5010.0, 10.0, 5000.0]]);
        for e in edges {
            // Bytes before the kind that the decoder does not read.
            b.extend_from_slice(&[0xEE; 5]);
            b.push(e.kind);
            b.push(e.class);
            b.extend_from_slice(&e.a.to_le_bytes());
            b.extend_from_slice(&e.b.to_le_bytes());
            b.extend_from_slice(&0u32.to_le_bytes());
            run(&mut b, &e.points);
            let tangents: Vec<[f64; 3]> = e.points.iter().map(|_| [1.0, 0.0, 0.0]).collect();
            run(&mut b, &tangents);
        }
        b.extend_from_slice(&[0; 64]);
        b
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
    fn a_run_out_of_reach_is_not_an_edge() {
        let mut far = edge(KIND_ROAD, 0, 1, 2, 1000.0);
        far.points[1][1] = 5000.0;
        let net = parse(
            &file(&["road"], &[far, edge(KIND_ROAD, 0, 2, 3, 4000.0)]),
            "X.rn4",
            &REACH,
        )
        .unwrap();
        assert_eq!(net.edges.len(), 1);
        assert_eq!((net.edges[0].id, net.edges[0].a), (0, Some(2)));
    }

    #[test]
    fn refuses_a_class_past_the_table_and_an_unknown_kind() {
        let b = file(&["road"], &[edge(KIND_ROAD, 1, 1, 2, 1000.0)]);
        let e = parse(&b, "X.rn4", &REACH).unwrap_err().to_string();
        assert!(e.contains("has class 1, past the 1 the table holds"), "{e}");

        let b = file(&["road"], &[edge(3, 0, 1, 2, 1000.0)]);
        let e = parse(&b, "X.rn4", &REACH).unwrap_err().to_string();
        assert!(e.contains("has kind 3"), "{e}");
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
