//! An install terrain's scenery, read in place: `scene.nodes` from the
//! cache, built from the scene file the first time scenery is asked for,
//! then each box through the pools' own trees, read a chunk at a time.
//!
//! Nothing maps the scene file: on Windows a map costs about 2 MiB of
//! commit per GiB, 15.5 MiB on GermanyColdWar, against the 4 MiB of chunks
//! kept here, and every query stays within a few milliseconds either way.

use std::fs::File;
use std::io::{self, BufReader};
use std::path::Path;
use std::sync::{Arc, Mutex};

use super::{Lru, refusal};
use crate::cache::{Entry, TerrainCache};
use crate::decode::install::Map;
use crate::decode::scn5::{Index, Instance, Pool, Scene};
use crate::decode::surface5::ReadAt;
use crate::extract::FileEntry;
use crate::source::Refusal;

/// Bytes read from the scene file at a time.
const CHUNK: u64 = 16 << 10;
/// Chunks kept between reads: 4 MiB.
const CHUNKS: usize = 256;

/// What a box query needs of the scene file's head.
pub struct SceneNodes;

pub static SCENE_NODES: SceneNodes = SceneNodes;

impl Entry for SceneNodes {
    fn name(&self) -> &str {
        "scene.nodes"
    }

    fn version(&self) -> u32 {
        1
    }

    fn roles(&self) -> &[&str] {
        &["scene"]
    }

    fn build(&self, root: &Path, map: &Map, files: &[FileEntry]) -> Result<Vec<u8>, Refusal> {
        let f = files
            .iter()
            .find(|f| f.role == "scene")
            .ok_or_else(|| Refusal(format!("{} has no scene file", map.theatre)))?;
        let file = File::open(root.join(&f.path)).map_err(|e| refusal(&f.path, e))?;
        let mut r = BufReader::new(file);
        let scene = Scene::open(&mut r, &f.path).map_err(|e| refusal(&f.path, e))?;
        let index = scene.index(&mut r).map_err(|e| refusal(&f.path, e))?;
        Ok(index.encode())
    }
}

/// A file read in chunks, the last few kept: a query's reads cluster in a
/// pool's tree and boxes, and many land in one chunk. Its length is checked
/// once, when opened; a file rewritten at the same length after that reads
/// as the old one's offsets would, which the cache key catches at the next
/// open.
struct Chunks {
    file: File,
    len: u64,
    kept: Mutex<Lru<u64, Arc<Vec<u8>>>>,
}

impl Chunks {
    fn chunk(&self, k: u64) -> io::Result<Arc<Vec<u8>>> {
        if let Some(c) = self.kept.lock().unwrap().get(&k) {
            return Ok(c);
        }
        // Read outside the lock, so other threads go on answering from the
        // chunks already held.
        let start = k * CHUNK;
        let mut b = vec![0u8; CHUNK.min(self.len.saturating_sub(start)) as usize];
        self.file.read_exact_at(&mut b, start)?;
        let c = Arc::new(b);
        self.kept.lock().unwrap().put(k, c.clone(), 1);
        Ok(c)
    }
}

impl ReadAt for Chunks {
    fn read_exact_at(&self, buf: &mut [u8], offset: u64) -> io::Result<()> {
        if offset
            .checked_add(buf.len() as u64)
            .is_none_or(|e| e > self.len)
        {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        let mut done = 0;
        while done < buf.len() {
            let at = offset + done as u64;
            let c = self.chunk(at / CHUNK)?;
            let from = (at % CHUNK) as usize;
            let n = (c.len() - from).min(buf.len() - done);
            buf[done..done + n].copy_from_slice(&c[from..from + n]);
            done += n;
        }
        Ok(())
    }

    fn size(&self) -> io::Result<u64> {
        Ok(self.len)
    }
}

/// The scenery of one terrain: its index, and the scene file it reads.
pub struct Scenery {
    index: Index,
    path: String,
    file: Chunks,
}

impl Scenery {
    pub(super) fn open(root: &Path, cache: &TerrainCache) -> Result<Scenery, Refusal> {
        let opened = cache.get(&SCENE_NODES)?;
        let unreadable = |e: &dyn std::fmt::Display| {
            Refusal(format!(
                "{} cannot be read ({e}); `dcsterrain cache clear --terrain {}` rebuilds it",
                opened.path.display(),
                cache.map().theatre
            ))
        };
        let bytes = opened.payload().map_err(|e| unreadable(&e))?;
        let index = Index::decode(&bytes).map_err(|e| unreadable(&e))?;
        let Some(f) = cache.map().files.iter().find(|f| f.role == "scene") else {
            return Err(Refusal(format!(
                "{} has no scene file",
                cache.map().theatre
            )));
        };
        let path = f.path.clone();
        let file = File::open(root.join(&path)).map_err(|e| refusal(&path, e))?;
        // The file can change between the key's check and this open; the
        // offsets are then another file's.
        let len = file.metadata().map_err(|e| refusal(&path, e))?.len();
        if len != index.len {
            return Err(Refusal(format!(
                "{path} changed while it was being opened ({len} bytes, where its index was \
                 read from {}); open it again",
                index.len
            )));
        }
        Ok(Scenery {
            index,
            path,
            file: Chunks {
                file,
                len,
                kept: Mutex::new(Lru::new(CHUNKS)),
            },
        })
    }

    /// The model table, the nodes, the group and the pools.
    pub fn index(&self) -> &Index {
        &self.index
    }

    /// Calls `f` with every instance of `pool` whose box in that pool meets
    /// `b`, min x, min z, max x, max z in DCS meters; for objects, the group
    /// records whose footprint meets it too.
    pub fn each_in(
        &self,
        pool: Pool,
        b: [f64; 4],
        f: impl FnMut(&Instance),
    ) -> Result<(), Refusal> {
        self.index
            .each_in(&self.file, pool, b, f)
            .map_err(|e| refusal(&self.path, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratch::Scratch;

    #[test]
    fn a_read_across_chunks_and_up_to_the_end_returns_the_files_bytes() {
        let s = Scratch::new();
        let path = s.0.join("f.bin");
        // Two and a half chunks, each byte its offset's low bits.
        let bytes: Vec<u8> = (0..(5 * CHUNK / 2)).map(|i| (i % 251) as u8).collect();
        std::fs::create_dir_all(&s.0).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        let c = Chunks {
            file: File::open(&path).unwrap(),
            len: bytes.len() as u64,
            kept: Mutex::new(Lru::new(2)),
        };
        for (at, n) in [
            (0, 10),
            (CHUNK - 3, 8),
            (CHUNK / 2, 2 * CHUNK),
            (bytes.len() as u64 - 5, 5),
        ] {
            let mut buf = vec![0u8; n as usize];
            c.read_exact_at(&mut buf, at).unwrap();
            assert_eq!(buf, bytes[at as usize..(at + n) as usize], "{at}+{n}");
        }
        let mut buf = [0u8; 6];
        let e = c
            .read_exact_at(&mut buf, bytes.len() as u64 - 5)
            .unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::UnexpectedEof);
        assert_eq!(c.kept.lock().unwrap().items.len(), 2);
    }
}
