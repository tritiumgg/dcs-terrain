//! `t5::rn5file`, version 0, an airfield's `AirfieldsTaxiways/<name>.rn5`:
//! the class of every stand, which names its record in `assets.assets`.
//!
//! ```text
//! after the version   u32 count, count × (u32 len, len bytes)   road classes
//!                     u32 count, count × (u32 len, len bytes)   stand classes
//! a node              u32 ordinal, u32 class, u32 k, k × u32,
//!                     f64[16] matrix, f64[16] its inverse
//! ```
//!
//! The nodes are the `.rn4`'s, laid flat at `y` 0, though not always in the
//! same order: a node's first word is its ordinal in this file. As in the
//! `.rn4`, the framing between nodes is not understood, so each is found by
//! its matrix, its class read back from the words before it, and the stand
//! it classes found by position.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use super::Decoder;
use super::airfield::{self, Airfield, MATRIX_BYTES};
use super::rn4::{self, Error, u32_at};

/// How close a flat node lies to its `.rn4` node, in meters on each axis.
const MATCH_M: f64 = 1e-3;
/// A node matrix here may carry a scale, which its inverse undoes: on
/// GermanyColdWar entries reach 1.09. No scale seen is near this.
const SCALED: f64 = 100.0;
/// The longest list seen before a matrix is shorter than this.
const MAX_LIST: usize = 5;

/// An airfield's stand classes.
#[derive(Debug, Clone, PartialEq)]
pub struct StandClasses {
    pub classes: Vec<String>,
    /// The class of each `.rn4` node the file gives one, by node index.
    pub by_node: BTreeMap<u32, u32>,
}

impl StandClasses {
    /// The class name of the `.rn4` node `index`, if the file gives one.
    pub fn of(&self, index: u32) -> Option<&str> {
        self.by_node
            .get(&index)
            .map(|&c| self.classes[c as usize].as_str())
    }
}

/// The class of the node whose matrix is at `o`, the file's `ordinal`th:
/// `ordinal, class, k` then `k` list entries end at the matrix. The longest
/// list is tried first, since a short one can read a list entry as its
/// count.
fn class_before(b: &[u8], o: usize, ordinal: u32, classes: usize) -> Option<u32> {
    (0..=MAX_LIST).rev().find_map(|k| {
        let at = |back: usize| o.checked_sub(back + 4 * k).and_then(|p| u32_at(b, p));
        (at(4) == Some(k as u32) && at(12) == Some(ordinal))
            .then(|| at(8))
            .flatten()
            .filter(|&c| (c as usize) < classes)
    })
}

/// Decodes an airfield's `.rn5` against its decoded `.rn4`. `name` is how
/// an error names it.
pub fn parse(b: &[u8], name: &str, rn4: &Airfield) -> Result<StandClasses, Error> {
    let h = rn4::header(b, name, Decoder::Rn5)?;
    let (_, at) = airfield::class_table(b, h.body())?;
    let (classes, mut i) = airfield::class_table(b, at)?;
    let mut by_node = BTreeMap::new();
    let mut ordinal = 0;
    while i + MATRIX_BYTES <= b.len() {
        let Some(m) = airfield::matrix_at(b, i, SCALED) else {
            i += 1;
            continue;
        };
        let (x, y, z) = (m[12], m[13], m[14]);
        let node = rn4.nodes.iter().find(|n| {
            let [nx, _, nz] = n.position();
            y == 0.0 && (nx - x).abs() < MATCH_M && (nz - z).abs() < MATCH_M
        });
        if let Some(n) = node
            && let Some(c) = class_before(b, i, ordinal, classes.len())
        {
            by_node.insert(n.index, c);
        }
        ordinal += 1;
        // The inverse is the same node's, and would pass for a matrix.
        i += 2 * MATRIX_BYTES;
    }
    Ok(StandClasses { classes, by_node })
}

/// Reads and decodes the `.rn5` at `path` against its decoded `.rn4`.
pub fn read(path: &Path, rn4: &Airfield) -> Result<StandClasses, Error> {
    parse(&fs::read(path)?, &path.to_string_lossy(), rn4)
}

#[cfg(test)]
pub mod build {
    //! `.rn5` files as the decoder reads them, for tests.

    use super::super::airfield::build::{matrix, put_matrix};
    use super::super::container;

    fn names(b: &mut Vec<u8>, list: &[&str]) {
        b.extend_from_slice(&(list.len() as u32).to_le_bytes());
        for s in list {
            b.extend_from_slice(&(s.len() as u32).to_le_bytes());
            b.extend_from_slice(s.as_bytes());
        }
    }

    /// An `.rn5` of two stand classes, `stand_shelter` and `stand_end_24m`,
    /// and a node for each of `nodes`, as `(ordinal, class, list, x, z)`.
    pub fn file(nodes: &[(u32, u32, &[u32], f64, f64)]) -> Vec<u8> {
        scaled(nodes, 1.0)
    }

    /// As `file`, every matrix scaled by `s` and its inverse by `1 / s`.
    pub fn scaled(nodes: &[(u32, u32, &[u32], f64, f64)], s: f64) -> Vec<u8> {
        let scale = |mut m: [f64; 16], s: f64| {
            m[..12].iter_mut().for_each(|v| *v *= s);
            m
        };
        let mut b = container::build("t5::rn5file", 0, 0);
        names(&mut b, &["vpp", "taxiway"]);
        names(&mut b, &["stand_shelter", "stand_end_24m"]);
        for &(node, class, list, x, z) in nodes {
            for v in [node, class, list.len() as u32].iter().chain(list) {
                b.extend_from_slice(&v.to_le_bytes());
            }
            put_matrix(&mut b, &scale(matrix([x, 0.0, z], 0.3), s));
            // The inverse, whose translation lies on no node.
            put_matrix(&mut b, &scale(matrix([-x, 0.0, -z], -0.3), 1.0 / s));
        }
        b.extend_from_slice(&[0; 16]);
        b
    }
}

#[cfg(test)]
mod tests {
    use super::super::airfield::build as afb;
    use super::super::container;
    use super::build::file;
    use super::*;

    fn rn4() -> Airfield {
        let node = |name, x, z| afb::Node {
            name: Some(name),
            at: [x, 18.25, z],
            heading: 0.3,
            links: vec![],
        };
        let b = afb::file(
            &["vpp"],
            &[
                node("1#22", -4427.9, 243_999.5),
                node("40", -4829.5, 244_622.0),
                node("90", -5155.0, 244_536.25),
            ],
        );
        airfield::parse(&b, "Anapa.rn4").unwrap()
    }

    #[test]
    fn reads_each_stand_class_by_its_rn4_node() {
        let b = file(&[
            (0, 1, &[7, 8], -4829.5, 244_622.0),
            (1, 0, &[], -5155.0, 244_536.25),
        ]);
        let c = parse(&b, "Anapa.rn5", &rn4()).unwrap();
        assert_eq!(c.classes, ["stand_shelter", "stand_end_24m"]);
        assert_eq!(c.of(1), Some("stand_end_24m"));
        assert_eq!(c.of(2), Some("stand_shelter"));
        assert_eq!(c.of(0), None);
    }

    #[test]
    fn a_node_is_numbered_in_this_file_and_found_by_position() {
        // The .rn4's nodes 2 and 1, in that order.
        let b = file(&[
            (0, 0, &[], -5155.0, 244_536.25),
            (1, 1, &[3], -4829.5, 244_622.0),
        ]);
        let c = parse(&b, "Anapa.rn5", &rn4()).unwrap();
        assert_eq!(
            (c.of(1), c.of(2)),
            (Some("stand_end_24m"), Some("stand_shelter"))
        );

        // A scaled matrix is a node's; its inverse is not another.
        let b = super::build::scaled(
            &[
                (0, 0, &[], -5155.0, 244_536.25),
                (1, 1, &[3], -4829.5, 244_622.0),
            ],
            1.09,
        );
        let c = parse(&b, "Buchel.rn5", &rn4()).unwrap();
        assert_eq!(
            (c.of(1), c.of(2)),
            (Some("stand_end_24m"), Some("stand_shelter"))
        );

        // A first word that is not the node's ordinal is not its framing.
        let b = file(&[(1, 1, &[], -4829.5, 244_622.0)]);
        assert!(parse(&b, "Anapa.rn5", &rn4()).unwrap().by_node.is_empty());
    }

    #[test]
    fn a_list_of_one_zero_is_not_read_as_an_empty_list() {
        // Node 0 of class 0 listing [0]: read as an empty list, the list's
        // count 1 would pass as the class.
        let b = file(&[(0, 0, &[0], -4427.9, 243_999.5)]);
        let c = parse(&b, "Anapa.rn5", &rn4()).unwrap();
        assert_eq!(c.of(0), Some("stand_shelter"));
    }

    #[test]
    fn a_node_off_every_rn4_node_or_with_a_class_past_the_table_is_unread() {
        let b = file(&[
            (0, 1, &[], -4829.0, 244_622.0),
            (1, 2, &[], -5155.0, 244_536.25),
        ]);
        let c = parse(&b, "Anapa.rn5", &rn4()).unwrap();
        assert!(c.by_node.is_empty(), "{:?}", c.by_node);
    }

    #[test]
    fn refuses_another_class_of_file() {
        let b = container::build("landscape4::lRoadNetwork", 5, 0);
        let e = parse(&b, "X.rn4", &rn4()).unwrap_err().to_string();
        assert!(e.contains("read by the RoadNetwork decoder"), "{e}");
    }
}
