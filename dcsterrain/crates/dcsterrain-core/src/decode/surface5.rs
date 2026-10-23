//! `landscape5::Surface5File`, version 5: the ground as a triangle mesh cut
//! into blocks.
//!
//! The file is a table of contents followed by data arrays. The table runs
//! from the end of the header to the data base the container header states,
//! and every array offset is relative to that base. Its framing between the
//! parts that matter is not understood, so the parts are found by pattern
//! rather than walked:
//!
//! ```text
//! after the version   u32 4, u32 0, u32 0, u32 N, u32 7    N finest blocks
//! a block             f32[16] matrix, row-major: identity rotation, then
//!                     (Tx, Ty, Tz, 1); f32[6] box min x,y,z, max x,y,z
//!                     in block coordinates; then framing and fields
//! a field             u32 1, u32 len, len bytes of name, u16 format,
//!                     u16 stride, u32 flag, u64 size, u48 offset
//! ```
//!
//! A field belongs to the nearest matrix before it, and a block is a matrix
//! with a `Pbase` field. The finest mesh is the blocks whose first field is
//! `TRITYPE`; they come first, N of them. The rest are render copies and
//! coarser levels, which nothing here reads.

use std::fmt;

mod mesh;
pub use mesh::{Block, BlockInfo, Error, Mesh, Subset, Surface5, open, reader};
mod sample;
pub use sample::{Hit, LAKE, RIVER, ROAD_STRIP, SEA, cover, water_class};
mod raster;
pub use raster::{
    RUNWAY_MATERIAL, SURFACE_LAND, SURFACE_ROAD, SURFACE_RUNWAY, SURFACE_WATER, Tile, grid_for,
    rasterize, write_tiles,
};
pub(crate) use raster::{entry as tile_entry, put as put_tile};

/// The words that follow the version. N, the fourth, varies by map; the
/// others are the same on every map seen.
const LAYOUT_WORDS: [Option<u32>; 5] = [Some(4), Some(0), Some(0), None, Some(7)];

const ONE: [u8; 4] = 1.0f32.to_le_bytes();
const ZERO: [u8; 4] = [0; 4];
/// The first twelve floats of every block matrix: an identity rotation.
const IDENTITY: [[u8; 4]; 12] = [
    ONE, ZERO, ZERO, ZERO, ZERO, ONE, ZERO, ZERO, ZERO, ZERO, ONE, ZERO,
];
/// After the identity: the translation and w, then the box.
const MATRIX_TAIL: usize = 4 * 4 + 6 * 4;

/// A field name is 2 to 64 bytes. One real field, `N`, has a single-letter
/// name this skips; nothing reads it.
const NAME_LEN: std::ops::RangeInclusive<usize> = 2..=64;
/// After the name: format, stride, flag, size and a 6-byte offset.
const FIELD_TAIL: usize = 2 + 2 + 4 + 8 + 6;
/// Larger strides or sizes are not fields but bytes that happen to match.
const MAX_STRIDE: u16 = 64;
const MAX_SIZE: u64 = 1 << 40;

/// What makes a surface5 file unreadable, in words that name the part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Malformed(pub String);

impl fmt::Display for Malformed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One data array a block names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    /// Where the field's leading `u32 1` sits in the file.
    pub at: usize,
    pub name: String,
    pub format: u16,
    pub stride: u16,
    pub size: u64,
    /// From the data base.
    pub offset: u64,
}

impl Field {
    /// Where the name ends, which is where a subset's material follows.
    pub fn name_end(&self) -> usize {
        self.at + 8 + self.name.len()
    }
}

/// A block as the table of contents describes it.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockToc {
    /// Where the matrix sits in the file.
    pub at: usize,
    /// The translation from block to world coordinates.
    pub t: [f32; 3],
    /// Min x, y, z, then max x, y, z, in block coordinates.
    pub bbox: [f32; 6],
    /// In file order.
    pub fields: Vec<Field>,
}

impl BlockToc {
    /// The first field of this name: a later one belongs to a subset.
    pub fn field(&self, name: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.name == name)
    }

    pub fn is_finest(&self) -> bool {
        self.fields.first().is_some_and(|f| f.name == "TRITYPE")
    }
}

/// The table of contents: every block, finest first.
#[derive(Debug, Clone, PartialEq)]
pub struct Toc {
    pub blocks: Vec<BlockToc>,
    /// How many of `blocks` are the finest mesh.
    pub finest: usize,
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn f32_at(b: &[u8], at: usize) -> f32 {
    f32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn u64_at(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}

fn u48_at(b: &[u8], at: usize) -> u64 {
    let mut w = [0u8; 8];
    w[..6].copy_from_slice(&b[at..at + 6]);
    u64::from_le_bytes(w)
}

/// A matrix at `i`: its translation and box, when the identity starts here
/// and w is 1.
fn matrix_at(toc: &[u8], i: usize) -> Option<([f32; 3], [f32; 6])> {
    let tail = i + IDENTITY.len() * 4;
    if tail + MATRIX_TAIL > toc.len() {
        return None;
    }
    if !IDENTITY
        .iter()
        .enumerate()
        .all(|(k, w)| toc[i + k * 4..i + k * 4 + 4] == *w)
    {
        return None;
    }
    if f32_at(toc, tail + 12) != 1.0 {
        return None;
    }
    let t = [0, 1, 2].map(|k| f32_at(toc, tail + k * 4));
    let bbox = [0, 1, 2, 3, 4, 5].map(|k| f32_at(toc, tail + 16 + k * 4));
    Some((t, bbox))
}

/// A field at `i`, when `u32 1`, a length, a name of exactly that length and
/// a plausible tail start here.
fn field_at(toc: &[u8], i: usize) -> Option<Field> {
    if toc.len() < i + 8 || u32_at(toc, i) != 1 {
        return None;
    }
    let len = u32_at(toc, i + 4) as usize;
    if !NAME_LEN.contains(&len) {
        return None;
    }
    let name = toc.get(i + 8..i + 8 + len)?;
    let word = |c: &u8| c.is_ascii_alphanumeric() || *c == b'_';
    if !name[0].is_ascii_alphabetic() || !name.iter().all(word) {
        return None;
    }
    // The length delimits the name; the format code that follows may be any
    // byte, a letter included.
    let q = i + 8 + len;
    if toc.len() < q + FIELD_TAIL {
        return None;
    }
    let stride = u16_at(toc, q + 2);
    let size = u64_at(toc, q + 8);
    if stride > MAX_STRIDE || size >= MAX_SIZE {
        return None;
    }
    Some(Field {
        at: i,
        name: String::from_utf8(name.to_vec()).expect("checked ASCII"),
        format: u16_at(toc, q),
        stride,
        size,
        offset: u48_at(toc, q + 16),
    })
}

/// Scans the table of contents, `head` being the file from offset 0 to the
/// data base, and `body` the offset where the class's version ends.
pub fn scan(head: &[u8], body: usize) -> Result<Toc, Malformed> {
    let words = head
        .get(body..body + 4 * LAYOUT_WORDS.len())
        .ok_or_else(|| Malformed("the table of contents ends before its layout words".into()))?;
    let words: Vec<u32> = (0..LAYOUT_WORDS.len())
        .map(|k| u32_at(words, k * 4))
        .collect();
    for (k, (&got, want)) in words.iter().zip(LAYOUT_WORDS).enumerate() {
        if want.is_some_and(|w| w != got) {
            return Err(Malformed(format!(
                "layout word {k} is {got}, where every file seen has {}",
                want.unwrap()
            )));
        }
    }
    let declared = words[3] as usize;

    let mut matrices: Vec<BlockToc> = Vec::new();
    let mut i = body;
    while i < head.len() {
        if let Some((t, bbox)) = matrix_at(head, i) {
            matrices.push(BlockToc {
                at: i,
                t,
                bbox,
                fields: Vec::new(),
            });
            i += IDENTITY.len() * 4 + MATRIX_TAIL;
        } else if let Some(f) = field_at(head, i) {
            i = f.name_end();
            // A field before the first matrix belongs to no block.
            if let Some(m) = matrices.last_mut() {
                m.fields.push(f);
            }
        } else {
            i += 1;
        }
    }

    let blocks: Vec<BlockToc> = matrices
        .into_iter()
        .filter(|m| m.field("Pbase").is_some())
        .collect();
    let finest = blocks.iter().take_while(|b| b.is_finest()).count();
    if let Some(late) = blocks[finest..].iter().position(BlockToc::is_finest) {
        return Err(Malformed(format!(
            "block {} is of the finest mesh, but follows block {finest}, which is not",
            finest + late
        )));
    }
    if finest != declared {
        return Err(Malformed(format!(
            "the header declares {declared} finest blocks, and the table of contents holds {finest}"
        )));
    }
    Ok(Toc { blocks, finest })
}

/// Builds surface5 files for tests: the parts a decoder finds, with framing
/// words between them that it must step over.
#[cfg(test)]
pub mod build {
    use super::super::container;

    /// A field and the bytes of its array.
    pub struct Array {
        pub name: &'static str,
        pub format: u16,
        pub stride: u16,
        pub data: Vec<u8>,
        /// A shader and material written after the field, as a subset's
        /// `TRI` carries them.
        pub material: Option<(&'static str, &'static str)>,
    }

    pub fn u32s(name: &'static str, v: &[u32]) -> Array {
        Array {
            name,
            format: 4,
            stride: 4,
            data: v.iter().flat_map(|x| x.to_le_bytes()).collect(),
            material: None,
        }
    }

    pub fn f32s(name: &'static str, v: &[f32]) -> Array {
        Array {
            name,
            format: 0,
            stride: 4,
            data: v.iter().flat_map(|x| x.to_le_bytes()).collect(),
            material: None,
        }
    }

    pub fn tri(v: &[[u32; 3]], material: Option<(&'static str, &'static str)>) -> Array {
        Array {
            name: "TRI",
            format: 6,
            stride: 12,
            data: v.iter().flatten().flat_map(|x| x.to_le_bytes()).collect(),
            material,
        }
    }

    pub fn vec3(name: &'static str, v: &[[f32; 3]]) -> Array {
        Array {
            name,
            format: 2,
            stride: 12,
            data: v.iter().flatten().flat_map(|x| x.to_le_bytes()).collect(),
            material: None,
        }
    }

    pub struct Block {
        pub t: [f32; 3],
        pub bbox: [f32; 6],
        pub arrays: Vec<Array>,
    }

    fn str_(out: &mut Vec<u8>, s: &str) {
        out.extend_from_slice(&(s.len() as u32).to_le_bytes());
        out.extend_from_slice(s.as_bytes());
    }

    /// A file holding `blocks`, with `finest` declared in the header.
    pub fn file(blocks: &[Block], finest: u32) -> Vec<u8> {
        let mut toc = Vec::new();
        for w in [4, 0, 0, finest, 7] {
            toc.extend_from_slice(&u32::to_le_bytes(w));
        }
        let mut data = Vec::new();
        for b in blocks {
            // Framing the decoder does not read.
            toc.extend_from_slice(&[0xEE; 12]);
            for k in 0..3 {
                for j in 0..4 {
                    toc.extend_from_slice(&f32::to_le_bytes(if j == k { 1.0 } else { 0.0 }));
                }
            }
            for v in b.t {
                toc.extend_from_slice(&v.to_le_bytes());
            }
            toc.extend_from_slice(&1.0f32.to_le_bytes());
            for v in b.bbox {
                toc.extend_from_slice(&v.to_le_bytes());
            }
            toc.extend_from_slice(&[0x07, 0, 0, 0, 0x2A, 0, 0, 0]);
            for a in &b.arrays {
                toc.extend_from_slice(&1u32.to_le_bytes());
                str_(&mut toc, a.name);
                toc.extend_from_slice(&a.format.to_le_bytes());
                toc.extend_from_slice(&a.stride.to_le_bytes());
                toc.extend_from_slice(&u32::from(a.name.starts_with("TRI")).to_le_bytes());
                toc.extend_from_slice(&(a.data.len() as u64).to_le_bytes());
                toc.extend_from_slice(&(data.len() as u64).to_le_bytes()[..6]);
                if let Some((shader, material)) = a.material {
                    toc.extend_from_slice(&[0, 0, 0, 0, 2, 0, 0, 0]);
                    str_(&mut toc, shader);
                    str_(&mut toc, material);
                    toc.extend_from_slice(&0u32.to_le_bytes());
                    toc.extend_from_slice(&(-1i32).to_le_bytes());
                }
                toc.extend_from_slice(&[0x05, 0, 0, 0]);
                data.extend_from_slice(&a.data);
            }
        }
        let class = "landscape5::Surface5File";
        let head = container::build(class, 5, 0).len();
        let base = (head + toc.len()) as u64;
        let mut out = container::build(class, 5, base);
        out.extend_from_slice(&toc);
        out.extend_from_slice(&data);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::build::{Block, f32s, file, tri, u32s, vec3};
    use super::*;
    use crate::decode::container;

    fn finest(t: [f32; 3]) -> Block {
        Block {
            t,
            bbox: [0.0, 0.0, 0.0, 100.0, 50.0, 100.0],
            arrays: vec![
                u32s("TRITYPE", &[0, 8]),
                tri(&[[0, 1, 2], [0, 2, 3]], Some(("Surface5.0", "Land"))),
                vec3("Pbase", &[[0.0; 3]; 4]),
                f32s("depth", &[0.0; 4]),
            ],
        }
    }

    fn coarse() -> Block {
        Block {
            t: [0.0; 3],
            bbox: [0.0, 0.0, 0.0, 200.0, 50.0, 200.0],
            arrays: vec![tri(&[[0, 1, 2]], None), vec3("Pbase", &[[0.0; 3]; 3])],
        }
    }

    fn scan_file(f: &[u8]) -> Result<Toc, Malformed> {
        let h = container::parse(f).unwrap();
        scan(&f[..h.data_base as usize], h.body())
    }

    #[test]
    fn finds_each_block_its_fields_and_the_finest_mesh() {
        let f = file(
            &[
                finest([10.0, 0.0, 20.0]),
                finest([110.0, 0.0, 20.0]),
                coarse(),
            ],
            2,
        );
        let toc = scan_file(&f).unwrap();
        assert_eq!(toc.blocks.len(), 3);
        assert_eq!(toc.finest, 2);
        let b = &toc.blocks[1];
        assert_eq!(b.t, [110.0, 0.0, 20.0]);
        assert_eq!(b.bbox[3], 100.0);
        let names: Vec<&str> = b.fields.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["TRITYPE", "TRI", "Pbase", "depth"]);
        let tri = b.field("TRI").unwrap();
        assert_eq!((tri.stride, tri.size), (12, 24));
        assert!(!toc.blocks[2].is_finest());
    }

    #[test]
    fn a_field_offset_is_from_the_data_base() {
        let f = file(&[finest([0.0; 3])], 1);
        let toc = scan_file(&f).unwrap();
        let base = container::parse(&f).unwrap().data_base as usize;
        let tt = toc.blocks[0].field("TRITYPE").unwrap();
        let at = base + tt.offset as usize;
        assert_eq!(u32_at(&f, at + 4), 8);
        let pb = toc.blocks[0].field("Pbase").unwrap();
        assert_eq!(pb.offset, 8 + 24);
    }

    #[test]
    fn skips_a_one_letter_name_and_reads_a_letter_format() {
        let mut b = finest([0.0; 3]);
        b.arrays.insert(2, vec3("N", &[[0.0; 3]; 4]));
        let toc = scan_file(&file(&[b], 1)).unwrap();
        assert!(toc.blocks[0].field("N").is_none());

        let mut head = vec![0u8; 8];
        head.extend_from_slice(&1u32.to_le_bytes());
        head.extend_from_slice(&3u32.to_le_bytes());
        head.extend_from_slice(b"TRI");
        head.push(b'X');
        head.extend_from_slice(&[0; FIELD_TAIL - 1]);
        let f = field_at(&head, 8).unwrap();
        assert_eq!((f.name.as_str(), f.format), ("TRI", u16::from(b'X')));
    }

    #[test]
    fn refuses_a_finest_count_the_header_does_not_declare() {
        let f = file(&[finest([0.0; 3]), coarse()], 2);
        assert_eq!(
            scan_file(&f).unwrap_err().0,
            "the header declares 2 finest blocks, and the table of contents holds 1"
        );
    }

    #[test]
    fn refuses_a_finest_block_after_a_coarser_one() {
        let f = file(&[coarse(), finest([0.0; 3])], 1);
        let e = scan_file(&f).unwrap_err().0;
        assert!(e.starts_with("block 1 is of the finest mesh"), "{e}");
    }

    #[test]
    fn refuses_layout_words_no_file_has() {
        let mut f = file(&[finest([0.0; 3])], 1);
        let body = container::parse(&f).unwrap().body();
        f[body + 16] = 8;
        assert_eq!(
            scan_file(&f).unwrap_err().0,
            "layout word 4 is 8, where every file seen has 7"
        );
    }
}
