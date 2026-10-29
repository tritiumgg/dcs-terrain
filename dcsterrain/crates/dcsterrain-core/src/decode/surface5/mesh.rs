//! The finest mesh: each block's vertices, triangles and triangle types, and
//! the material subsets asked for.
//!
//! A block's first `TRI` is its whole triangle list. Each later `TRI` is a
//! render subset of one material, indexing the `baseIndex` list before it,
//! which in turn indexes `Pbase`. Only the subsets of the materials a caller
//! names are read: the rest are as large as the mesh itself.
//!
//! A whole map's finest mesh runs to 177 M triangles, more than a machine
//! should hold at once, so opening a file reads only its table of contents,
//! and a block's arrays are read when it is asked for, each at its offset,
//! so threads can share one open file.

use std::fmt;
use std::fs::File;
use std::io;
use std::path::Path;

use super::super::container::{self, MAX_HEAD};
use super::super::{Decoder, dispatch};
use super::{BlockToc, Field, Malformed, Toc, scan};
use crate::extract::Bounds;

/// The bytes after a subset's `TRI` name in which its shader and material
/// are found.
const MATERIAL_WINDOW: usize = 160;
/// How far a vertex may lie outside its block's box. None does, on any map
/// seen.
const BOX_SLACK_M: f32 = 0.01;

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Malformed(Malformed),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "cannot be read: {e}"),
            Error::Malformed(m) => m.fmt(f),
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<Malformed> for Error {
    fn from(m: Malformed) -> Self {
        Error::Malformed(m)
    }
}

fn malformed(s: String) -> Error {
    Error::Malformed(Malformed(s))
}

/// Reads bytes at an offset without a cursor, so threads can share one
/// handle to a file.
pub trait ReadAt {
    fn read_exact_at(&self, buf: &mut [u8], offset: u64) -> io::Result<()>;
    fn size(&self) -> io::Result<u64>;
}

impl ReadAt for File {
    #[cfg(unix)]
    fn read_exact_at(&self, buf: &mut [u8], offset: u64) -> io::Result<()> {
        std::os::unix::fs::FileExt::read_exact_at(self, buf, offset)
    }

    #[cfg(windows)]
    fn read_exact_at(&self, mut buf: &mut [u8], mut offset: u64) -> io::Result<()> {
        use std::os::windows::fs::FileExt;
        while !buf.is_empty() {
            match self.seek_read(buf, offset) {
                Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
                Ok(n) => {
                    buf = &mut buf[n..];
                    offset += n as u64;
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    fn size(&self) -> io::Result<u64> {
        Ok(self.metadata()?.len())
    }
}

impl ReadAt for [u8] {
    fn read_exact_at(&self, buf: &mut [u8], offset: u64) -> io::Result<()> {
        let at = usize::try_from(offset).unwrap_or(usize::MAX);
        let src = at
            .checked_add(buf.len())
            .and_then(|end| self.get(at..end))
            .ok_or(io::ErrorKind::UnexpectedEof)?;
        buf.copy_from_slice(src);
        Ok(())
    }

    fn size(&self) -> io::Result<u64> {
        Ok(self.len() as u64)
    }
}

/// Where one array lies, from the data base.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Array {
    offset: u64,
    size: u64,
    stride: u16,
}

impl From<&Field> for Array {
    fn from(f: &Field) -> Self {
        Array {
            offset: f.offset,
            size: f.size,
            stride: f.stride,
        }
    }
}

/// A subset as the table of contents names it. Shader and material are
/// indices into the file's name list, which every block shares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SubsetToc {
    shader: u16,
    material: u16,
    base: Array,
    tri: Array,
}

/// A finest-mesh block as the table of contents describes it.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockInfo {
    pub t: [f32; 3],
    pub bbox: [f32; 6],
    tritype: Array,
    tri: Array,
    pbase: Array,
    depth: Option<Array>,
    subsets: Vec<SubsetToc>,
}

impl BlockInfo {
    /// The box the file states, in world x and z: `[min_x, min_z, max_x,
    /// max_z]`. Every vertex lies inside it, on every map seen.
    pub fn world_box(&self) -> [f64; 4] {
        world_box(self.t, self.bbox)
    }

    /// The bytes reading the block with every subset takes: what a caller
    /// holding many prepared blocks at once can bound itself by.
    pub fn read_len(&self) -> u64 {
        let arrays = [
            Some(self.tritype),
            Some(self.tri),
            Some(self.pbase),
            self.depth,
        ];
        let subsets = self.subsets.iter().map(|s| s.base.size + s.tri.size);
        arrays.iter().flatten().map(|a| a.size).sum::<u64>() + subsets.sum::<u64>()
    }
}

fn world_box(t: [f32; 3], bbox: [f32; 6]) -> [f64; 4] {
    let (t, b) = (t.map(f64::from), bbox.map(f64::from));
    [t[0] + b[0], t[2] + b[2], t[0] + b[3], t[2] + b[5]]
}

fn union(boxes: impl Iterator<Item = [f64; 4]>) -> Bounds {
    let mut b = Bounds {
        min_x: f64::INFINITY,
        min_z: f64::INFINITY,
        max_x: f64::NEG_INFINITY,
        max_z: f64::NEG_INFINITY,
    };
    for [x0, z0, x1, z1] in boxes {
        b.min_x = b.min_x.min(x0);
        b.min_z = b.min_z.min(z0);
        b.max_x = b.max_x.max(x1);
        b.max_z = b.max_z.max(z1);
    }
    b
}

/// The triangles of one material subset, as indices into the block's
/// vertices.
#[derive(Debug, Clone, PartialEq)]
pub struct Subset {
    pub shader: String,
    pub material: String,
    pub tri: Vec<[u32; 3]>,
}

/// One block of the finest mesh, read.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    /// The block's place in the file, which is its precedence where
    /// triangles overlap.
    pub index: usize,
    pub t: [f32; 3],
    pub bbox: [f32; 6],
    /// Block coordinates; `y` is the ground, or the bed under water.
    pub p: Vec<[f32; 3]>,
    /// Water depth above `p.y`, zero where the file has no depth field.
    pub depth: Vec<f32>,
    pub tri: Vec<[u32; 3]>,
    pub tritype: Vec<u32>,
    pub subsets: Vec<Subset>,
}

impl Block {
    /// A vertex in world coordinates: x north, z east, and the height of the
    /// water surface over water and of the ground elsewhere.
    pub fn world(&self, i: u32) -> [f64; 3] {
        let p = self.p[i as usize];
        let t = self.t.map(f64::from);
        [
            t[0] + f64::from(p[0]),
            t[1] + f64::from(p[1]) + f64::from(self.depth[i as usize]),
            t[2] + f64::from(p[2]),
        ]
    }

    pub fn world_box(&self) -> [f64; 4] {
        world_box(self.t, self.bbox)
    }
}

/// Blocks read from one file, in file order.
#[derive(Debug, Clone, PartialEq)]
pub struct Mesh {
    pub blocks: Vec<Block>,
}

impl Mesh {
    /// The union of the blocks' boxes.
    pub fn bounds(&self) -> Bounds {
        union(self.blocks.iter().map(Block::world_box))
    }
}

/// The shader and material named after a subset's `TRI`: the first
/// length-prefixed string ending in `<digit>.<digit>`, and the one after it.
fn material_after(head: &[u8], name_end: usize) -> Option<(String, String)> {
    let w = &head[name_end.min(head.len())..(name_end + MATERIAL_WINDOW).min(head.len())];
    let printable = |c: u8| (0x20..=0x7e).contains(&c);
    let mut strings = Vec::new();
    let mut j = 0;
    while j + 4 < w.len() {
        let len = w[j] as usize;
        if (1..=64).contains(&len) && w[j + 1..j + 4] == [0, 0, 0] && printable(w[j + 4]) {
            let run = w[j + 4..].iter().take_while(|&&c| printable(c)).count();
            // The string must end where its length says; a longer printable
            // run is not this string.
            if run == len {
                strings.push(String::from_utf8(w[j + 4..j + 4 + run].to_vec()).unwrap());
            }
            j += 4 + run;
        } else {
            j += 1;
        }
    }
    let is_shader = |s: &str| {
        let b = s.as_bytes();
        b.len() >= 3
            && b[b.len() - 2] == b'.'
            && b[b.len() - 1].is_ascii_digit()
            && b[b.len() - 3].is_ascii_digit()
    };
    let k = strings.iter().position(|s| is_shader(s))?;
    Some((strings[k].clone(), strings.get(k + 1)?.clone()))
}

/// The `baseIndex` a subset's `TRI` at field `i` indexes: the nearest one
/// before it with no other `TRI` between. Newer maps put two dozen vertex
/// attribute arrays between the two, so the distance is no guide.
fn base_index_for(fields: &[Field], i: usize) -> Option<&Field> {
    for j in (1..i).rev() {
        match fields[j].name.as_str() {
            "baseIndex" => return Some(&fields[j]),
            "TRI" => return None,
            _ => {}
        }
    }
    None
}

fn intern(names: &mut Vec<String>, s: String) -> Result<u16, Error> {
    let k = match names.iter().position(|n| *n == s) {
        Some(k) => k,
        None => {
            names.push(s);
            names.len() - 1
        }
    };
    u16::try_from(k).map_err(|_| malformed("the file names more than 65 536 materials".into()))
}

fn block_info(
    head: &[u8],
    index: usize,
    toc: &BlockToc,
    names: &mut Vec<String>,
) -> Result<BlockInfo, Error> {
    let need = |name: &str| {
        toc.field(name)
            .map(Array::from)
            .ok_or_else(|| malformed(format!("block {index} has no {name} field")))
    };
    let mut subsets = Vec::new();
    let first = toc.fields.iter().position(|f| f.name == "TRI");
    for (i, f) in toc
        .fields
        .iter()
        .enumerate()
        .skip(first.map_or(0, |k| k + 1))
    {
        if f.name != "TRI" {
            continue;
        }
        let Some((shader, material)) = material_after(head, f.name_end()) else {
            continue;
        };
        // A subset with no baseIndex is a decal, such as runway markings,
        // over a vertex list of its own that is not the ground's.
        let Some(base) = base_index_for(&toc.fields, i) else {
            continue;
        };
        subsets.push(SubsetToc {
            shader: intern(names, shader)?,
            material: intern(names, material)?,
            base: base.into(),
            tri: f.into(),
        });
    }
    Ok(BlockInfo {
        t: toc.t,
        bbox: toc.bbox,
        tritype: need("TRITYPE")?,
        tri: need("TRI")?,
        pbase: need("Pbase")?,
        depth: toc.field("depth").map(Array::from),
        subsets,
    })
}

/// An opened surface5 file: the finest blocks as its table of contents
/// describes them.
#[derive(Debug, Clone, PartialEq)]
pub struct Surface5 {
    pub blocks: Vec<BlockInfo>,
    names: Vec<String>,
    data_base: u64,
    len: u64,
}

impl Surface5 {
    /// Reads the header and table of contents of a file whose header names
    /// this decoder. `name` is how an error names the file.
    pub fn open<R: ReadAt + ?Sized>(r: &R, name: &str) -> Result<Surface5, Error> {
        let len = r.size()?;
        let mut first = vec![0u8; len.min(MAX_HEAD as u64) as usize];
        r.read_exact_at(&mut first, 0)?;
        let h = container::parse(&first).map_err(|e| malformed(format!("not a container: {e}")))?;
        match dispatch(name, &h) {
            Ok(Decoder::Surface5) => {}
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
        let mut head = vec![0u8; h.data_base as usize];
        r.read_exact_at(&mut head, 0)?;
        let Toc { blocks, finest } = scan(&head, h.body())?;
        let mut names = Vec::new();
        let blocks = blocks[..finest]
            .iter()
            .enumerate()
            .map(|(i, b)| block_info(&head, i, b, &mut names))
            .collect::<Result<_, _>>()?;
        Ok(Surface5 {
            blocks,
            names,
            data_base: h.data_base,
            len,
        })
    }

    /// The table of contents with only the subsets of `materials`, and only
    /// the names they use. A large terrain has two hundred subsets for each
    /// one of a material that is read.
    pub fn keep(&self, materials: &[&str]) -> Surface5 {
        let mut names = Vec::new();
        let mut rename = |k: u16| {
            let s = &self.names[usize::from(k)];
            match names.iter().position(|n| n == s) {
                Some(i) => i as u16,
                None => {
                    names.push(s.clone());
                    (names.len() - 1) as u16
                }
            }
        };
        let blocks = self
            .blocks
            .iter()
            .map(|b| BlockInfo {
                subsets: b
                    .subsets
                    .iter()
                    .filter(|s| materials.contains(&self.names[usize::from(s.material)].as_str()))
                    .map(|s| SubsetToc {
                        shader: rename(s.shader),
                        material: rename(s.material),
                        ..*s
                    })
                    .collect(),
                ..b.clone()
            })
            .collect();
        Surface5 {
            blocks,
            names,
            ..*self
        }
    }

    /// The table of contents as bytes, for a cache: finding it in the file
    /// takes a pattern scan, and reading it back takes none.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let array = |out: &mut Vec<u8>, a: &Array| {
            out.extend_from_slice(&a.offset.to_le_bytes());
            out.extend_from_slice(&a.size.to_le_bytes());
            out.extend_from_slice(&a.stride.to_le_bytes());
        };
        out.extend_from_slice(&self.data_base.to_le_bytes());
        out.extend_from_slice(&self.len.to_le_bytes());
        out.extend_from_slice(&(self.names.len() as u32).to_le_bytes());
        for n in &self.names {
            out.extend_from_slice(&(n.len() as u32).to_le_bytes());
            out.extend_from_slice(n.as_bytes());
        }
        out.extend_from_slice(&(self.blocks.len() as u32).to_le_bytes());
        for b in &self.blocks {
            for v in b.t.iter().chain(&b.bbox) {
                out.extend_from_slice(&v.to_le_bytes());
            }
            for a in [&b.tritype, &b.tri, &b.pbase] {
                array(&mut out, a);
            }
            out.push(u8::from(b.depth.is_some()));
            if let Some(a) = &b.depth {
                array(&mut out, a);
            }
            out.extend_from_slice(&(b.subsets.len() as u32).to_le_bytes());
            for s in &b.subsets {
                out.extend_from_slice(&s.shader.to_le_bytes());
                out.extend_from_slice(&s.material.to_le_bytes());
                array(&mut out, &s.base);
                array(&mut out, &s.tri);
            }
        }
        out
    }

    /// Reads back what `encode` wrote.
    pub fn decode(bytes: &[u8]) -> Result<Surface5, Malformed> {
        let mut r = Bytes { b: bytes, at: 0 };
        let data_base = r.u64()?;
        let len = r.u64()?;
        let names = (0..r.u32()?)
            .map(|_| {
                let n = r.u32()? as usize;
                String::from_utf8(r.take(n)?.to_vec())
                    .map_err(|_| Malformed("a material name is not UTF-8".into()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let name = |k: u16| {
            if usize::from(k) < names.len() {
                Ok(k)
            } else {
                Err(Malformed(format!(
                    "a subset names material {k}, of {}",
                    names.len()
                )))
            }
        };
        // A block takes at least 95 bytes, so a corrupt count reserves no
        // more than the bytes left could hold.
        let n = r.u32()?;
        let mut blocks = Vec::with_capacity((n as usize).min((bytes.len() - r.at) / 95));
        for _ in 0..n {
            let t = [r.f32()?, r.f32()?, r.f32()?];
            let mut bbox = [0f32; 6];
            for v in &mut bbox {
                *v = r.f32()?;
            }
            let (tritype, tri, pbase) = (r.array()?, r.array()?, r.array()?);
            let depth = match r.take(1)?[0] {
                0 => None,
                _ => Some(r.array()?),
            };
            // Likewise a subset takes 40.
            let k = r.u32()?;
            let mut subsets = Vec::with_capacity((k as usize).min((bytes.len() - r.at) / 40));
            for _ in 0..k {
                subsets.push(SubsetToc {
                    shader: name(r.u16()?)?,
                    material: name(r.u16()?)?,
                    base: r.array()?,
                    tri: r.array()?,
                });
            }
            blocks.push(BlockInfo {
                t,
                bbox,
                tritype,
                tri,
                pbase,
                depth,
                subsets,
            });
        }
        if r.at != bytes.len() {
            return Err(Malformed(format!(
                "{} bytes follow the table of contents",
                bytes.len() - r.at
            )));
        }
        // What opening the file would have refused.
        if blocks.is_empty() {
            return Err(Malformed("the table of contents holds no blocks".into()));
        }
        let finite = |b: &BlockInfo| b.t.iter().chain(&b.bbox).all(|v| v.is_finite());
        if let Some(i) = blocks.iter().position(|b| !finite(b)) {
            return Err(Malformed(format!(
                "block {i}'s place or box is not a finite number"
            )));
        }
        if data_base > len {
            return Err(Malformed(format!(
                "the data base {data_base} lies past the end of the file, {len} bytes"
            )));
        }
        Ok(Surface5 {
            blocks,
            names,
            data_base,
            len,
        })
    }

    /// The length of the file the table of contents was read from.
    pub fn file_len(&self) -> u64 {
        self.len
    }

    /// The union of the finest blocks' boxes: the map's extent.
    pub fn bounds(&self) -> Bounds {
        union(self.blocks.iter().map(BlockInfo::world_box))
    }

    /// An array of `T`, read straight into place, refused unless its
    /// stride is `T`'s size and it lies inside the file. `T` is made of
    /// little-endian 32-bit words, as the file is.
    fn array<R: ReadAt + ?Sized, T: bytemuck::Pod>(
        &self,
        r: &R,
        what: impl Fn() -> String,
        a: Array,
    ) -> Result<Vec<T>, Error> {
        let stride = std::mem::size_of::<T>() as u16;
        if a.stride != stride {
            return Err(malformed(format!(
                "{} has a stride of {}, where {stride} is read",
                what(),
                a.stride
            )));
        }
        if !a.size.is_multiple_of(u64::from(stride)) {
            return Err(malformed(format!(
                "{} holds {} bytes, not a whole number of elements",
                what(),
                a.size
            )));
        }
        let end = self
            .data_base
            .checked_add(a.offset)
            .and_then(|s| s.checked_add(a.size));
        let start = self.data_base.wrapping_add(a.offset);
        if end.is_none_or(|end| end > self.len) {
            return Err(malformed(format!(
                "{} runs past the end of the file",
                what()
            )));
        }
        let mut v = vec![T::zeroed(); (a.size / u64::from(stride)) as usize];
        r.read_exact_at(bytemuck::cast_slice_mut(&mut v), start)?;
        if cfg!(target_endian = "big") {
            for w in bytemuck::cast_slice_mut::<T, u32>(&mut v) {
                *w = u32::from_le(*w);
            }
        }
        Ok(v)
    }

    /// Reads finest block `index`, with the subsets of `materials`.
    pub fn read_block<R: ReadAt + ?Sized>(
        &self,
        r: &R,
        index: usize,
        materials: &[&str],
    ) -> Result<Block, Error> {
        let info = &self.blocks[index];
        let field = |name: &'static str| move || format!("block {index}'s {name} field");
        let tritype = self.array::<_, u32>(r, field("TRITYPE"), info.tritype)?;
        let tri = self.array::<_, [u32; 3]>(r, field("TRI"), info.tri)?;
        let p = self.array::<_, [f32; 3]>(r, field("Pbase"), info.pbase)?;
        let depth = match info.depth {
            Some(a) => self.array::<_, f32>(r, field("depth"), a)?,
            None => vec![0.0; p.len()],
        };
        if tritype.len() != tri.len() {
            return Err(malformed(format!(
                "block {index} has {} triangle types for {} triangles",
                tritype.len(),
                tri.len()
            )));
        }
        if depth.len() != p.len() {
            return Err(malformed(format!(
                "block {index} has {} depths for {} vertices",
                depth.len(),
                p.len()
            )));
        }
        check_indices(&format!("block {index}'s triangle list"), &tri, p.len())?;
        // A NaN passes every comparison with the box below, so it is
        // refused here.
        let bad = p.iter().flatten().chain(&depth).any(|v| !v.is_finite());
        if bad {
            return Err(malformed(format!(
                "block {index} holds a vertex or depth that is not a finite number"
            )));
        }
        // A reader skips a block by its box, so a vertex outside it would be
        // ground nothing reads.
        let b = info.bbox;
        let outside = p.iter().position(|v| {
            let off = (b[0] - v[0])
                .max(v[0] - b[3])
                .max(b[2] - v[2])
                .max(v[2] - b[5]);
            off > BOX_SLACK_M
        });
        if let Some(i) = outside {
            return Err(malformed(format!(
                "block {index}'s vertex {i} lies outside the block's box"
            )));
        }

        let mut subsets = Vec::new();
        for s in &info.subsets {
            let material = &self.names[s.material as usize];
            if !materials.contains(&material.as_str()) {
                continue;
            }
            let what = || format!("block {index}'s {material} subset");
            let base = self.array::<_, u32>(r, what, s.base)?;
            let local = self.array::<_, [u32; 3]>(r, what, s.tri)?;
            check_indices(&what(), &local, base.len())?;
            let tri: Vec<[u32; 3]> = local.iter().map(|t| t.map(|k| base[k as usize])).collect();
            check_indices(&what(), &tri, p.len())?;
            subsets.push(Subset {
                shader: self.names[s.shader as usize].clone(),
                material: material.clone(),
                tri,
            });
        }
        Ok(Block {
            index,
            t: info.t,
            bbox: info.bbox,
            p,
            depth,
            tri,
            tritype,
            subsets,
        })
    }

    /// Reads every finest block: for a test, or a map small enough to hold.
    pub fn read_all<R: ReadAt + ?Sized>(&self, r: &R, materials: &[&str]) -> Result<Mesh, Error> {
        let blocks = (0..self.blocks.len())
            .map(|i| self.read_block(r, i, materials))
            .collect::<Result<_, _>>()?;
        Ok(Mesh { blocks })
    }
}

/// A cursor over an encoded table of contents.
struct Bytes<'a> {
    b: &'a [u8],
    at: usize,
}

impl Bytes<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], Malformed> {
        let s = self
            .at
            .checked_add(n)
            .and_then(|end| self.b.get(self.at..end))
            .ok_or_else(|| Malformed("the table of contents ends early".into()))?;
        self.at += n;
        Ok(s)
    }

    fn u16(&mut self) -> Result<u16, Malformed> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn u32(&mut self) -> Result<u32, Malformed> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn u64(&mut self) -> Result<u64, Malformed> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    fn f32(&mut self) -> Result<f32, Malformed> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn array(&mut self) -> Result<Array, Malformed> {
        Ok(Array {
            offset: self.u64()?,
            size: self.u64()?,
            stride: self.u16()?,
        })
    }
}

fn check_indices(what: &str, tri: &[[u32; 3]], n: usize) -> Result<(), Error> {
    match tri.iter().flatten().find(|&&v| v as usize >= n) {
        Some(v) => Err(malformed(format!("{what} names vertex {v}, of {n}"))),
        None => Ok(()),
    }
}

/// Opens the surface5 file at `path`.
pub fn open(path: &Path) -> Result<Surface5, Error> {
    Surface5::open(&File::open(path)?, &path.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::super::build::{self, f32s, file, tri, u32s, vec3};
    use super::*;

    fn two_triangles() -> build::Block {
        build::Block {
            t: [1000.0, 5.0, 2000.0],
            bbox: [0.0, 0.0, 0.0, 100.0, 30.0, 100.0],
            arrays: vec![
                u32s("TRITYPE", &[0, 2]),
                tri(&[[0, 1, 2], [1, 3, 2]], Some(("Surface5.0", "Land"))),
                u32s("baseIndex", &[3, 1, 2]),
                tri(&[[0, 1, 2]], Some(("Lake5.1", "Lake"))),
                u32s("baseIndex", &[0, 1, 2]),
                u32s("materialParamsIndex", &[0]),
                tri(&[[0, 2, 1]], Some(("Runway5.0", "Runway"))),
                vec3(
                    "Pbase",
                    &[
                        [0.0, 10.0, 0.0],
                        [100.0, 10.0, 0.0],
                        [0.0, 20.0, 100.0],
                        [100.0, 0.0, 100.0],
                    ],
                ),
                f32s("depth", &[0.0, 0.0, 0.0, 4.0]),
            ],
        }
    }

    fn read_bytes(f: Vec<u8>, name: &str, m: &[&str]) -> Result<Mesh, Error> {
        let r = &f[..];
        Surface5::open(r, name)?.read_all(r, m)
    }

    fn read_built(blocks: &[build::Block], n: u32, m: &[&str]) -> Result<Mesh, Error> {
        read_bytes(file(blocks, n), "t.surface5", m)
    }

    #[test]
    fn reads_vertices_triangles_and_types() {
        let mesh = read_built(&[two_triangles()], 1, &[]).unwrap();
        let b = &mesh.blocks[0];
        assert_eq!(b.tri, [[0, 1, 2], [1, 3, 2]]);
        assert_eq!(b.tritype, [0, 2]);
        assert_eq!(b.world(0), [1000.0, 15.0, 2000.0]);
        // Over water the height is the surface: bed plus depth.
        assert_eq!(b.world(3), [1100.0, 9.0, 2100.0]);
        assert!(b.subsets.is_empty());
    }

    #[test]
    fn reads_one_block_on_demand() {
        let mut east = two_triangles();
        east.t = [1000.0, 0.0, 2100.0];
        let f = file(&[two_triangles(), east], 2);
        let r = &f[..];
        let s = Surface5::open(r, "t").unwrap();
        assert_eq!(s.blocks.len(), 2);
        let b = s.read_block(r, 1, &[]).unwrap();
        assert_eq!(b.index, 1);
        assert_eq!(b.world(0), [1000.0, 10.0, 2100.0]);
    }

    #[test]
    fn the_table_of_contents_reads_back_from_its_bytes() {
        let mut east = two_triangles();
        east.t = [1000.0, 0.0, 2100.0];
        east.arrays.retain(|a| a.name != "depth");
        let f = file(&[two_triangles(), east], 2);
        let s = Surface5::open(&f[..], "t").unwrap();
        let back = Surface5::decode(&s.encode()).unwrap();
        assert_eq!(back, s);
        let m = ["Lake", "Runway"];
        assert_eq!(
            back.read_all(&f[..], &m).unwrap(),
            s.read_all(&f[..], &m).unwrap()
        );
    }

    #[test]
    fn a_kept_table_of_contents_holds_and_reads_only_its_materials() {
        let f = file(&[two_triangles(), two_triangles()], 2);
        let s = Surface5::open(&f[..], "t").unwrap();
        let kept = s.keep(&["Runway"]);
        assert_eq!(kept.names, ["Runway5.0", "Runway"]);
        assert!(kept.blocks.iter().all(|b| b.subsets.len() == 1));
        assert!(kept.encode().len() < s.encode().len());
        let back = Surface5::decode(&kept.encode()).unwrap();
        assert_eq!(back, kept);
        let m = ["Runway"];
        assert_eq!(
            back.read_all(&f[..], &m).unwrap(),
            s.read_all(&f[..], &m).unwrap()
        );
        // A material it dropped reads as absent.
        let lake = back.read_all(&f[..], &["Lake"]).unwrap();
        assert!(lake.blocks.iter().all(|b| b.subsets.is_empty()));
    }

    #[test]
    fn refuses_a_table_of_contents_no_file_would_give() {
        let f = file(&[two_triangles()], 1);
        let s = Surface5::open(&f[..], "t").unwrap();
        let mut none = s.clone();
        none.blocks.clear();
        let e = Surface5::decode(&none.encode()).unwrap_err();
        assert_eq!(e.0, "the table of contents holds no blocks");
        let mut nan = s.clone();
        nan.blocks[0].bbox[3] = f32::NAN;
        let e = Surface5::decode(&nan.encode()).unwrap_err();
        assert_eq!(e.0, "block 0's place or box is not a finite number");
        // An array whose offset overflows is refused, not wrapped.
        let mut far = s.clone();
        far.blocks[0].pbase.offset = u64::MAX - 2;
        let far = Surface5::decode(&far.encode()).unwrap();
        let e = far.read_block(&f[..], 0, &[]).unwrap_err().to_string();
        assert_eq!(e, "block 0's Pbase field runs past the end of the file");
    }

    #[test]
    fn refuses_a_vertex_that_is_not_a_number() {
        let mut b = two_triangles();
        b.arrays[7] = vec3(
            "Pbase",
            &[[0.0, 10.0, 0.0], [f32::NAN; 3], [0.0; 3], [0.0; 3]],
        );
        let e = read_built(&[b], 1, &[]).unwrap_err().to_string();
        assert_eq!(
            e,
            "block 0 holds a vertex or depth that is not a finite number"
        );
    }

    #[test]
    fn refuses_a_table_of_contents_cut_short_or_with_bytes_after() {
        let f = file(&[two_triangles()], 1);
        let bytes = Surface5::open(&f[..], "t").unwrap().encode();
        let e = Surface5::decode(&bytes[..bytes.len() - 1]).unwrap_err();
        assert_eq!(e.0, "the table of contents ends early");
        let mut long = bytes.clone();
        long.push(0);
        let e = Surface5::decode(&long).unwrap_err();
        assert_eq!(e.0, "1 bytes follow the table of contents");
    }

    #[test]
    fn reads_only_the_subsets_asked_for_through_their_base_index() {
        let mesh = read_built(&[two_triangles()], 1, &["Runway"]).unwrap();
        let s = &mesh.blocks[0].subsets;
        assert_eq!(s.len(), 1);
        assert_eq!(
            (s[0].shader.as_str(), s[0].material.as_str()),
            ("Runway5.0", "Runway")
        );
        assert_eq!(s[0].tri, [[0, 2, 1]]);

        let mesh = read_built(&[two_triangles()], 1, &["Lake", "Runway"]).unwrap();
        assert_eq!(mesh.blocks[0].subsets[0].tri, [[3, 1, 2]]);
    }

    #[test]
    fn a_base_index_pairs_across_any_number_of_attribute_arrays() {
        let mut b = two_triangles();
        for _ in 0..24 {
            b.arrays.insert(5, f32s("mainTexCoord", &[0.0, 0.0]));
        }
        let mesh = read_built(&[b], 1, &["Runway"]).unwrap();
        assert_eq!(mesh.blocks[0].subsets[0].tri, [[0, 2, 1]]);
    }

    #[test]
    fn skips_a_subset_with_no_base_index() {
        let mut b = two_triangles();
        b.arrays.remove(4);
        let mesh = read_built(&[b], 1, &["Runway"]).unwrap();
        assert!(mesh.blocks[0].subsets.is_empty());
    }

    #[test]
    fn a_missing_depth_is_zero() {
        let mut b = two_triangles();
        b.arrays.retain(|a| a.name != "depth");
        let mesh = read_built(&[b], 1, &[]).unwrap();
        assert_eq!(mesh.blocks[0].world(3)[1], 5.0);
    }

    #[test]
    fn the_bounds_are_the_union_of_the_boxes() {
        let mut east = two_triangles();
        east.t = [1000.0, 0.0, 2100.0];
        let f = file(&[two_triangles(), east], 2);
        let b = Surface5::open(&f[..], "t").unwrap().bounds();
        assert_eq!(
            (b.min_x, b.min_z, b.max_x, b.max_z),
            (1000.0, 2000.0, 1100.0, 2200.0)
        );
    }

    #[test]
    fn refuses_a_triangle_naming_a_vertex_it_lacks() {
        let mut b = two_triangles();
        b.arrays[1] = tri(&[[0, 1, 2], [1, 4, 2]], None);
        let e = read_built(&[b], 1, &[]).unwrap_err().to_string();
        assert_eq!(e, "block 0's triangle list names vertex 4, of 4");

        let mut b = two_triangles();
        b.arrays[4] = u32s("baseIndex", &[0, 1, 9]);
        let e = read_built(&[b], 1, &["Runway"]).unwrap_err().to_string();
        assert_eq!(e, "block 0's Runway subset names vertex 9, of 4");
    }

    #[test]
    fn refuses_a_vertex_outside_its_box() {
        let mut b = two_triangles();
        b.bbox[5] = 99.0;
        let e = read_built(&[b], 1, &[]).unwrap_err().to_string();
        assert_eq!(e, "block 0's vertex 2 lies outside the block's box");
    }

    #[test]
    fn refuses_a_type_count_that_is_not_the_triangle_count() {
        let mut b = two_triangles();
        b.arrays[0] = u32s("TRITYPE", &[0]);
        let e = read_built(&[b], 1, &[]).unwrap_err().to_string();
        assert_eq!(e, "block 0 has 1 triangle types for 2 triangles");
    }

    #[test]
    fn refuses_an_array_past_the_end_of_the_file() {
        let mut f = file(&[two_triangles()], 1);
        f.truncate(f.len() - 4);
        let e = read_bytes(f, "t", &[]).unwrap_err().to_string();
        assert_eq!(e, "block 0's depth field runs past the end of the file");
    }

    #[test]
    fn refuses_a_version_no_decoder_reads() {
        let mut f = file(&[two_triangles()], 1);
        let at = container::parse(&f).unwrap().body() - 4;
        f[at] = 6;
        let e = read_bytes(f, "x.surface5", &[]).unwrap_err().to_string();
        assert!(
            e.starts_with("x.surface5: landscape5::Surface5File version 6"),
            "{e}"
        );
    }

    #[test]
    fn finds_the_material_after_a_tri_name() {
        let mut w = vec![9u8; 6];
        for s in ["xx", "Blend5.1", "Forest", "Town"] {
            w.extend_from_slice(&(s.len() as u32).to_le_bytes());
            w.extend_from_slice(s.as_bytes());
        }
        assert_eq!(
            material_after(&w, 0),
            Some(("Blend5.1".to_string(), "Forest".to_string()))
        );
    }
}
