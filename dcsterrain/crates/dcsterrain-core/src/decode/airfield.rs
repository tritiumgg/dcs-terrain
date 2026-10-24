//! `landscape4::lRoadNetwork`, version 5, as an airfield's
//! `AirfieldsTaxiways/<name>.rn4`: the taxi graph's nodes, among them the
//! named stands and runway ends.
//!
//! The class table and the edges are framed as in the map-wide file. The
//! nodes follow the edges, one record per crossroad in index order:
//!
//! ```text
//! [u32 3, u32 a, u32 b, u32 n, n × f64 (x, y, z)]   connector runs, optional
//! [u32 len, len bytes]                              name, named nodes only
//! f64[16]   matrix, row-major: row 0 the local X axis, row 3 (x, y, z, 1)
//! u32 count, count × (u32 node, f32 distance)
//! ```
//!
//! The framing before a matrix varies, so nodes are found by their matrix,
//! and a node's index is the ordinal of its matrix in the file: that is the
//! index the game gives a stand.

use std::fs;
use std::path::Path;

use super::Decoder;
use super::rn4::{self, Error, f64_at, u32_at};

/// The bytes of a node's matrix.
pub const MATRIX_BYTES: usize = 128;
/// A rotation entry, scaled by at most rounding.
const UNIT: f64 = 1.0001;
/// Where a translation may lie. No map's origin is on a runway, and no
/// airfield is below the sea or above the highest mountains by much.
const MIN_ABS_X_M: f64 = 1.0;
const MAX_ABS_X_M: f64 = 1e6;
const MAX_ABS_Z_M: f64 = 2e6;
const MIN_Y_M: f64 = -500.0;
const MAX_Y_M: f64 = 6000.0;
/// Names are short designators and stand numbers.
const MAX_NAME_LEN: usize = 39;
/// More links than an airfield has nodes is bytes that are not a list.
const MAX_LINKS: u32 = 100_000;

/// The node matrix at `i`, if one is there: a rotation whose translation
/// lies within the map, with the affine column `0, 0, 0, 1`.
pub fn matrix_at(b: &[u8], i: usize) -> Option<[f64; 16]> {
    if i + MATRIX_BYTES > b.len() {
        return None;
    }
    // The cheapest refusal first: almost every offset fails it.
    if f64_at(b, i + 120) != 1.0 {
        return None;
    }
    let mut m = [0.0; 16];
    for (k, v) in m.iter_mut().enumerate() {
        *v = f64_at(b, i + 8 * k);
    }
    let rotation = m[..12].iter().enumerate().all(|(k, v)| {
        if k % 4 == 3 {
            *v == 0.0
        } else {
            v.abs() <= UNIT
        }
    });
    let [x, y, z] = [m[12], m[13], m[14]];
    (rotation
        && (MIN_ABS_X_M..MAX_ABS_X_M).contains(&x.abs())
        && z.abs() < MAX_ABS_Z_M
        && y > MIN_Y_M
        && y < MAX_Y_M)
        .then_some(m)
}

/// The name that ends exactly at `i`: a length, then that many printable
/// bytes. The shortest that fits wins.
fn name_before(b: &[u8], i: usize) -> Option<String> {
    (1..=MAX_NAME_LEN.min(i.saturating_sub(4))).find_map(|len| {
        let bytes = &b[i - len..i];
        (u32_at(b, i - len - 4) == Some(len as u32) && bytes.iter().all(|c| c.is_ascii_graphic()))
            .then(|| String::from_utf8(bytes.to_vec()).expect("checked ASCII"))
    })
}

/// One crossroad of the taxi graph.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    /// The ordinal of its matrix in the file.
    pub index: u32,
    /// The name as the file spells it, `k#` prefix and all.
    pub name: Option<String>,
    pub matrix: [f64; 16],
    /// The nodes it lists, each with a distance.
    pub links: Vec<(u32, f32)>,
}

impl Node {
    /// `[x, y, z]`, `y` up.
    pub fn position(&self) -> [f64; 3] {
        [self.matrix[12], self.matrix[13], self.matrix[14]]
    }

    /// The heading of the local X axis, radians from north towards east.
    pub fn heading(&self) -> f64 {
        self.matrix[2].atan2(self.matrix[0])
    }
}

/// Every node from `from` on, in file order.
fn nodes(b: &[u8], from: usize) -> Result<Vec<Node>, Error> {
    let mut out = Vec::new();
    let mut i = from;
    while i + MATRIX_BYTES <= b.len() {
        let Some(matrix) = matrix_at(b, i) else {
            i += 1;
            continue;
        };
        let index = out.len() as u32;
        let at = i + MATRIX_BYTES;
        let count = u32_at(b, at)
            .filter(|&n| n <= MAX_LINKS && at + 4 + 8 * n as usize <= b.len())
            .ok_or_else(|| {
                Error::Malformed(format!(
                    "node {index} at byte {i} has no link list after its matrix"
                ))
            })?;
        let links = (0..count as usize)
            .map(|k| {
                let p = at + 4 + 8 * k;
                let d = f32::from_le_bytes(b[p + 4..p + 8].try_into().unwrap());
                (u32_at(b, p).unwrap(), d)
            })
            .collect();
        out.push(Node {
            index,
            name: name_before(b, i),
            matrix,
            links,
        });
        i = at;
    }
    Ok(out)
}

/// A name without its `k#` prefix: `1#27` is runway end 27.
pub fn strip(name: &str) -> &str {
    match name.split_once('#') {
        Some((k, rest)) if k.bytes().all(|c| c.is_ascii_digit()) => rest,
        _ => name,
    }
}

/// A runway designator's number and side, `L`, `R`, `C` or none.
fn designator(name: &str) -> Option<(i64, Option<char>)> {
    let s = strip(name);
    let digits = s.bytes().take_while(u8::is_ascii_digit).count();
    let (num, side) = s.split_at(digits);
    let side = match side {
        "" => None,
        "L" => Some('L'),
        "R" => Some('R'),
        "C" => Some('C'),
        _ => return None,
    };
    // Leading zeros are any number; the significant digits one or two.
    let n = num.trim_start_matches('0');
    (digits > 0 && n.len() <= 2).then(|| (n.parse().unwrap_or(0), side))
}

/// Whether two ends are the two ends of one runway by name: numbers 18
/// apart on a compass of 36, left facing right.
fn reciprocal(a: &str, b: &str) -> bool {
    let (Some((na, sa)), Some((nb, sb))) = (designator(a), designator(b)) else {
        return false;
    };
    let other = match sa {
        Some('L') => Some('R'),
        Some('R') => Some('L'),
        s => s,
    };
    (na + 17).rem_euclid(36) + 1 == (nb - 1).rem_euclid(36) + 1 && sb == other
}

/// A runway as its two paired ends, by node index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Runway {
    pub edge1: u32,
    pub edge2: u32,
}

/// An airfield's taxi graph, as far as it is read.
#[derive(Debug, Clone, PartialEq)]
pub struct Airfield {
    pub classes: Vec<String>,
    pub nodes: Vec<Node>,
    pub runways: Vec<Runway>,
    /// The named nodes that are not runway ends, by index.
    pub stands: Vec<u32>,
}

impl Airfield {
    pub fn node(&self, index: u32) -> &Node {
        &self.nodes[index as usize]
    }

    /// A node's name without its prefix; empty for an unnamed node.
    pub fn name(&self, index: u32) -> &str {
        self.node(index).name.as_deref().map_or("", strip)
    }

    /// `edge1`, a hyphen and `edge2`.
    pub fn runway_name(&self, r: &Runway) -> String {
        format!("{}-{}", self.name(r.edge1), self.name(r.edge2))
    }

    /// Radians from north towards east, `edge1` to `edge2`, true.
    pub fn course(&self, r: &Runway) -> f64 {
        let ([x1, _, z1], [x2, _, z2]) =
            (self.node(r.edge1).position(), self.node(r.edge2).position());
        (z2 - z1).atan2(x2 - x1)
    }
}

fn has_hash(nodes: &[Node], i: u32) -> bool {
    nodes[i as usize]
        .name
        .as_deref()
        .is_some_and(|n| n.contains('#'))
}

/// The named nodes that are runway ends. A stand lists every end with its
/// distance and an end lists every stand, so the named nodes two-colour,
/// and the ends are the side that holds a `#` name, or failing that the
/// smaller. Where no named node lists another, the ends are the `#` names
/// and every name reciprocal to one of them.
fn runway_ends(nodes: &[Node]) -> Vec<u32> {
    let named: Vec<u32> = nodes
        .iter()
        .filter(|n| n.name.is_some())
        .map(|n| n.index)
        .collect();
    let is_named = |i: u32| nodes.get(i as usize).is_some_and(|n| n.name.is_some());
    let mut adj: Vec<Vec<u32>> = vec![Vec::new(); nodes.len()];
    for &i in &named {
        for &(j, _) in nodes[i as usize].links.iter().filter(|(j, _)| is_named(*j)) {
            adj[i as usize].push(j);
            adj[j as usize].push(i);
        }
    }
    if named.iter().all(|&i| adj[i as usize].is_empty()) {
        let hashed: Vec<u32> = named
            .iter()
            .copied()
            .filter(|&i| has_hash(nodes, i))
            .collect();
        let name = |i: u32| nodes[i as usize].name.as_deref().unwrap();
        return named
            .iter()
            .copied()
            .filter(|&i| has_hash(nodes, i) || hashed.iter().any(|&h| reciprocal(name(h), name(i))))
            .collect();
    }
    // Breadth first from each uncoloured node in index order, which starts
    // on side 0.
    let mut colour: Vec<Option<u8>> = vec![None; nodes.len()];
    for &start in &named {
        if colour[start as usize].is_some() {
            continue;
        }
        colour[start as usize] = Some(0);
        let mut queue = vec![start];
        while let Some(i) = queue.pop() {
            let c = colour[i as usize].unwrap();
            for &j in &adj[i as usize] {
                if colour[j as usize].is_none() {
                    colour[j as usize] = Some(1 - c);
                    queue.push(j);
                }
            }
        }
    }
    let side = |c: u8| -> Vec<u32> {
        named
            .iter()
            .copied()
            .filter(|&i| colour[i as usize] == Some(c))
            .collect()
    };
    let (zero, one) = (side(0), side(1));
    let hash = |s: &[u32]| s.iter().any(|&i| has_hash(nodes, i));
    match (hash(&zero), hash(&one)) {
        (true, false) => zero,
        (false, true) => one,
        _ if one.len() < zero.len() => one,
        _ => zero,
    }
}

/// Pairs runway ends, taking the pair reciprocal by name before one that
/// is not and the longer before the shorter, until none is left to pair.
/// `edge1` is the end whose name has a `#`, or failing that the lower index.
fn pair(nodes: &[Node], ends: &[u32]) -> Vec<Runway> {
    let name = |i: u32| nodes[i as usize].name.as_deref().unwrap_or("");
    let length = |a: u32, b: u32| {
        let ([x1, _, z1], [x2, _, z2]) =
            (nodes[a as usize].position(), nodes[b as usize].position());
        (x2 - x1).hypot(z2 - z1)
    };
    let mut free = ends.to_vec();
    let mut out = Vec::new();
    while free.len() >= 2 {
        let mut best: Option<(bool, f64, usize, usize)> = None;
        for i in 0..free.len() {
            for j in i + 1..free.len() {
                let key = (
                    reciprocal(name(free[i]), name(free[j])),
                    length(free[i], free[j]),
                );
                if best.is_none_or(|(r, l, ..)| key > (r, l)) {
                    best = Some((key.0, key.1, i, j));
                }
            }
        }
        let (_, _, i, j) = best.unwrap();
        let (a, b) = (free[i], free[j]);
        free.remove(j);
        free.remove(i);
        let (edge1, edge2) = match (has_hash(nodes, a), has_hash(nodes, b)) {
            (false, true) => (b, a),
            (true, false) => (a, b),
            _ => (a.min(b), a.max(b)),
        };
        out.push(Runway { edge1, edge2 });
    }
    out
}

/// Decodes an airfield's `.rn4`. `name` is how an error names it.
pub fn parse(b: &[u8], name: &str) -> Result<Airfield, Error> {
    let h = rn4::header(b, name, Decoder::RoadNetwork)?;
    let (classes, at) = rn4::classes(b, h.body())?;
    let nodes = nodes(b, at)?;
    let ends = runway_ends(&nodes);
    let runways = pair(&nodes, &ends);
    let stands = nodes
        .iter()
        .filter(|n| n.name.is_some() && !ends.contains(&n.index))
        .map(|n| n.index)
        .collect();
    Ok(Airfield {
        classes,
        nodes,
        runways,
        stands,
    })
}

/// Reads and decodes the airfield `.rn4` at `path`.
pub fn read(path: &Path) -> Result<Airfield, Error> {
    parse(&fs::read(path)?, &path.to_string_lossy())
}

#[cfg(test)]
pub mod build {
    //! Airfield files as the decoder reads them, for tests.

    use super::super::container;

    pub struct Node {
        pub name: Option<&'static str>,
        /// `[x, y, z]`.
        pub at: [f64; 3],
        pub heading: f64,
        pub links: Vec<(u32, f32)>,
    }

    pub fn matrix(at: [f64; 3], heading: f64) -> [f64; 16] {
        let (s, c) = heading.sin_cos();
        [
            c, 0.0, s, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            -s, 0.0, c, 0.0, //
            at[0], at[1], at[2], 1.0,
        ]
    }

    pub fn put_matrix(b: &mut Vec<u8>, m: &[f64; 16]) {
        for v in m {
            b.extend_from_slice(&v.to_le_bytes());
        }
    }

    /// A file of `classes`, one edge of points out of the matrix test, then
    /// each node: a connector run before every other one, its name, matrix
    /// and links.
    pub fn file(classes: &[&str], nodes: &[Node]) -> Vec<u8> {
        let mut b = container::build("landscape4::lRoadNetwork", 5, 0);
        b.extend_from_slice(&(classes.len() as u32).to_le_bytes());
        for c in classes {
            b.extend_from_slice(&(c.len() as u32).to_le_bytes());
            b.extend_from_slice(c.as_bytes());
        }
        b.extend_from_slice(&[0, 0, 7, 0, 7, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0]);
        b.extend_from_slice(&2u32.to_le_bytes());
        for v in [-5000.0f64, 12.0, 240_000.0, -5010.0, 12.0, 240_003.0] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        for (k, n) in nodes.iter().enumerate() {
            if k % 2 == 1 {
                b.extend_from_slice(&[3, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0]);
                for v in [n.at, n.at].iter().flatten() {
                    b.extend_from_slice(&v.to_le_bytes());
                }
            }
            if let Some(name) = n.name {
                b.extend_from_slice(&(name.len() as u32).to_le_bytes());
                b.extend_from_slice(name.as_bytes());
            }
            put_matrix(&mut b, &matrix(n.at, n.heading));
            b.extend_from_slice(&(n.links.len() as u32).to_le_bytes());
            for (node, d) in &n.links {
                b.extend_from_slice(&node.to_le_bytes());
                b.extend_from_slice(&d.to_le_bytes());
            }
        }
        b.extend_from_slice(&[0; 32]);
        b
    }
}

#[cfg(test)]
mod tests {
    use super::build::{self, file};
    use super::*;
    use crate::decode::container;

    fn node(name: Option<&'static str>, x: f64, links: Vec<(u32, f32)>) -> build::Node {
        build::Node {
            name,
            at: [x, 18.5, 244_600.0],
            heading: 0.5,
            links,
        }
    }

    #[test]
    fn reads_every_node_in_file_order_with_its_name_and_links() {
        let b = file(
            &["vpp", "markingforbigair"],
            &[
                node(None, -4800.0, vec![(1, 12.5)]),
                node(Some("40"), -4829.5, vec![(2, 1500.0), (3, 900.0)]),
                node(Some("1#22"), -4427.9, vec![]),
                node(Some("04"), -6396.9, vec![(1, 900.0)]),
            ],
        );
        let a = parse(&b, "Anapa.rn4").unwrap();
        assert_eq!(a.classes, ["vpp", "markingforbigair"]);
        let got: Vec<_> = a
            .nodes
            .iter()
            .map(|n| (n.index, n.name.as_deref(), n.position()[0], n.links.clone()))
            .collect();
        assert_eq!(
            got,
            [
                (0, None, -4800.0, vec![(1, 12.5)]),
                (1, Some("40"), -4829.5, vec![(2, 1500.0), (3, 900.0)]),
                (2, Some("1#22"), -4427.9, vec![]),
                (3, Some("04"), -6396.9, vec![(1, 900.0)]),
            ]
        );
        assert_eq!(a.nodes[1].position(), [-4829.5, 18.5, 244_600.0]);
        assert!((a.nodes[1].heading() - 0.5).abs() < 1e-12);
    }

    #[test]
    fn a_matrix_out_of_the_map_or_not_a_rotation_is_not_a_node() {
        let at = [-4800.0, 18.5, 244_600.0];
        let mut b = vec![0u8; 3];
        build::put_matrix(&mut b, &build::matrix(at, 1.0));
        assert!(matrix_at(&b, 3).is_some());
        for (k, v) in [
            (0, 1.5),
            (3, 0.5),
            (12, 0.0),
            (13, 7000.0),
            (14, 3e6),
            (15, 0.5),
        ] {
            let mut m = build::matrix(at, 1.0);
            m[k] = v;
            let mut b = Vec::new();
            build::put_matrix(&mut b, &m);
            assert_eq!(matrix_at(&b, 0), None, "entry {k} = {v}");
        }
    }

    #[test]
    fn refuses_a_link_list_past_the_end() {
        let mut b = file(&["vpp"], &[]);
        b.truncate(b.len() - 32);
        build::put_matrix(&mut b, &build::matrix([-4800.0, 18.5, 244_600.0], 0.0));
        b.extend_from_slice(&5u32.to_le_bytes());
        let e = parse(&b, "X.rn4").unwrap_err().to_string();
        assert!(e.starts_with("node 0 at byte"), "{e}");
        assert!(e.ends_with("has no link list after its matrix"), "{e}");
    }

    #[test]
    fn designators_are_reciprocal_18_apart_with_left_facing_right() {
        assert_eq!(strip("1#27"), "27");
        assert_eq!(strip("Stand01"), "Stand01");
        for (a, b) in [
            ("09", "27"),
            ("27", "09"),
            ("1#27", "009"),
            ("13R", "31L"),
            ("05R", "23L"),
            ("18", "36"),
            ("36", "18"),
            ("01C", "19C"),
        ] {
            assert!(reciprocal(a, b), "{a} {b}");
        }
        for (a, b) in [
            ("09", "28"),
            ("13R", "31R"),
            ("09", "27L"),
            ("Stand01", "Stand02"),
            ("123", "33"),
            ("09X", "27X"),
        ] {
            assert!(!reciprocal(a, b), "{a} {b}");
        }
    }

    fn at(name: &'static str, x: f64, z: f64, links: &[u32]) -> build::Node {
        build::Node {
            name: Some(name),
            at: [x, 20.0, z],
            heading: 0.0,
            links: links.iter().map(|&j| (j, 100.0)).collect(),
        }
    }

    fn names(a: &Airfield, v: &[u32]) -> Vec<String> {
        v.iter().map(|&i| a.name(i).to_string()).collect()
    }

    #[test]
    fn the_ends_are_the_side_with_a_hash_even_one_that_lists_nothing() {
        // Stand 04 shares its name with the runway end; 1#22 lists nothing.
        let b = file(
            &["vpp"],
            &[
                at("04", -6396.9, 242_258.1, &[2, 3, 4]),
                at("1#22", -4427.9, 243_999.5, &[]),
                at("40", -4829.5, 244_622.0, &[0]),
                at("04", -5551.3, 242_507.4, &[0]),
                at("90", -5155.0, 244_536.2, &[0]),
                node(None, -5000.0, vec![(0, 10.0)]),
            ],
        );
        let a = parse(&b, "Anapa.rn4").unwrap();
        assert_eq!(a.stands, [2, 3, 4]);
        assert_eq!(a.runways, [Runway { edge1: 1, edge2: 0 }]);
        assert_eq!(a.runway_name(&a.runways[0]), "22-04");
        let course = (242_258.1f64 - 243_999.5).atan2(-6396.9 - -4427.9);
        assert_eq!(a.course(&a.runways[0]), course);
    }

    #[test]
    fn with_no_hash_the_ends_are_the_smaller_side() {
        let b = file(
            &["vpp"],
            &[
                at("01", 1000.0, 1000.0, &[]),
                at("09", 5000.0, 0.0, &[0, 3, 4]),
                at("27", 5000.0, 3000.0, &[0, 3, 4]),
                at("02", 1000.0, 1100.0, &[]),
                at("03", 1000.0, 1200.0, &[]),
            ],
        );
        let a = parse(&b, "X.rn4").unwrap();
        assert_eq!(names(&a, &a.stands), ["01", "02", "03"]);
        assert_eq!(a.runways, [Runway { edge1: 1, edge2: 2 }]);
    }

    #[test]
    fn with_no_lists_the_ends_are_the_hash_names_and_their_reciprocals() {
        let b = file(
            &["vpp"],
            &[
                at("1#06", 1000.0, 1000.0, &[]),
                at("14", 1200.0, 1500.0, &[]),
                at("24", 2000.0, 2500.0, &[]),
                at("15", 1250.0, 1500.0, &[]),
            ],
        );
        let a = parse(&b, "X.rn4").unwrap();
        assert_eq!(names(&a, &a.stands), ["14", "15"]);
        assert_eq!(a.runway_name(&a.runways[0]), "06-24");
    }

    #[test]
    fn pairs_by_name_first_then_by_length() {
        // Two parallel runways whose cross pairings are the longer, and two
        // ends named for no common runway, paired by length.
        let b = file(
            &["vpp"],
            &[
                at("09L", 1000.0, 0.0, &[]),
                at("09R", 1300.0, 0.0, &[]),
                at("1#27R", 1000.0, 3000.0, &[]),
                at("27L", 1300.0, 3000.0, &[]),
                at("1#27", 9000.0, 3000.0, &[]),
                at("9C", 9000.0, 0.0, &[]),
                at("01", 1100.0, 1500.0, &[0, 1, 2, 3, 4, 5]),
            ],
        );
        let a = parse(&b, "X.rn4").unwrap();
        let got: Vec<String> = a.runways.iter().map(|r| a.runway_name(r)).collect();
        assert_eq!(got, ["27R-09L", "09R-27L", "27-9C"]);
        assert_eq!(names(&a, &a.stands), ["01"]);
    }

    #[test]
    fn refuses_another_class_of_file() {
        let mut b = container::build("t5::rn5file", 0, 0);
        b.extend_from_slice(&[0; 32]);
        let e = parse(&b, "X.rn5").unwrap_err().to_string();
        assert!(e.contains("read by the Rn5 decoder"), "{e}");
    }
}
