//! The header every map file opens with: a container version, a header size,
//! a data base, then the class name and the class's own format version.
//!
//! ```text
//! 0x00  u32   container version, 2
//! 0x04  u32   header size, 0x30
//! 0x08  u64   data base: where `.surface5` and `.scn5` offsets start
//! 0x10  16 bytes, zero in every file seen
//! 0x20  u32 len, len bytes of ASCII   class name
//! then  u32   format version
//! ```

use std::fmt;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

pub const CONTAINER_VERSION: u32 = 2;
pub const HEADER_SIZE: u32 = 0x30;
const CLASS_AT: usize = 0x20;
/// Longer than any class name seen, short enough that a stray length in a
/// file of another kind is refused rather than read.
pub const MAX_CLASS_LEN: usize = 128;
/// The most bytes a header can take.
pub const MAX_HEAD: usize = CLASS_AT + 4 + MAX_CLASS_LEN + 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub data_base: u64,
    pub class: String,
    pub version: u32,
}

/// Why a file's head is not a container header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotContainer {
    Short(usize),
    ContainerVersion(u32),
    HeaderSize(u32),
    ClassLen(u32),
    ClassName,
}

impl fmt::Display for NotContainer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NotContainer::Short(n) => write!(f, "{n} bytes, too short for a container header"),
            NotContainer::ContainerVersion(v) => {
                write!(
                    f,
                    "container version {v}, where every map file has {CONTAINER_VERSION}"
                )
            }
            NotContainer::HeaderSize(s) => {
                write!(
                    f,
                    "header size {s:#x}, where every map file has {HEADER_SIZE:#x}"
                )
            }
            NotContainer::ClassLen(n) => write!(f, "a class name of {n} bytes"),
            NotContainer::ClassName => write!(f, "a class name that is not printable ASCII"),
        }
    }
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// Parses a header from the head of a file.
pub fn parse(head: &[u8]) -> Result<Header, NotContainer> {
    let short = || NotContainer::Short(head.len());
    let container = u32_at(head, 0).ok_or_else(short)?;
    if container != CONTAINER_VERSION {
        return Err(NotContainer::ContainerVersion(container));
    }
    let size = u32_at(head, 4).ok_or_else(short)?;
    if size != HEADER_SIZE {
        return Err(NotContainer::HeaderSize(size));
    }
    let data_base = u64::from_le_bytes(head.get(8..16).ok_or_else(short)?.try_into().unwrap());
    let len = u32_at(head, CLASS_AT).ok_or_else(short)?;
    if len == 0 || len as usize > MAX_CLASS_LEN {
        return Err(NotContainer::ClassLen(len));
    }
    let start = CLASS_AT + 4;
    let name = head.get(start..start + len as usize).ok_or_else(short)?;
    if !name.iter().all(|c| c.is_ascii_graphic()) {
        return Err(NotContainer::ClassName);
    }
    let version = u32_at(head, start + len as usize).ok_or_else(short)?;
    Ok(Header {
        data_base,
        class: String::from_utf8(name.to_vec()).expect("checked ASCII"),
        version,
    })
}

/// Reads the head of the file at `path` and parses it. The outer error is
/// the file system's; the inner says the file is not a container.
pub fn read(path: &Path) -> io::Result<Result<Header, NotContainer>> {
    let mut head = Vec::with_capacity(MAX_HEAD);
    File::open(path)?
        .take(MAX_HEAD as u64)
        .read_to_end(&mut head)?;
    Ok(parse(&head))
}

/// A header as the files write it, for tests that build one.
#[cfg(test)]
pub fn build(class: &str, version: u32, data_base: u64) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&CONTAINER_VERSION.to_le_bytes());
    b.extend_from_slice(&HEADER_SIZE.to_le_bytes());
    b.extend_from_slice(&data_base.to_le_bytes());
    b.extend_from_slice(&[0; 16]);
    b.extend_from_slice(&(class.len() as u32).to_le_bytes());
    b.extend_from_slice(class.as_bytes());
    b.extend_from_slice(&version.to_le_bytes());
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_built_header() {
        let mut b = build("landscape5::Scene5File", 8, 11_888_248);
        b.extend_from_slice(&[0xAB; 64]);
        let h = parse(&b).unwrap();
        assert_eq!(h.class, "landscape5::Scene5File");
        assert_eq!(h.version, 8);
        assert_eq!(h.data_base, 11_888_248);
    }

    #[test]
    fn a_header_cut_anywhere_is_short() {
        let b = build("t5::rn5file", 0, 0);
        for n in 0..b.len() {
            assert!(
                matches!(parse(&b[..n]), Err(NotContainer::Short(m)) if m == n),
                "cut at {n}"
            );
        }
    }

    #[test]
    fn refuses_what_is_not_a_container() {
        let mut b = build("landscape4::lRoadNetwork", 5, 0);
        b[0] = 3;
        assert_eq!(parse(&b), Err(NotContainer::ContainerVersion(3)));

        let mut b = build("landscape4::lRoadNetwork", 5, 0);
        b[4] = 0x40;
        assert_eq!(parse(&b), Err(NotContainer::HeaderSize(0x40)));

        let mut b = build("x", 5, 0);
        b[CLASS_AT..CLASS_AT + 4].copy_from_slice(&1000u32.to_le_bytes());
        assert_eq!(parse(&b), Err(NotContainer::ClassLen(1000)));

        let b = build("bad name", 5, 0);
        assert_eq!(parse(&b), Err(NotContainer::ClassName));
    }

    #[test]
    fn lua_source_is_not_a_container() {
        let src = b"beacons = {\n\t{ display_name = _('Anapa'); };\n}\n";
        assert!(parse(src).is_err());
    }
}
