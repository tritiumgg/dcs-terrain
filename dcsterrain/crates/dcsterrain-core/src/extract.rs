//! Reads an extract directory: its manifest and its tiles (ADR 0039).
//!
//! The manifest is written last, so a directory without one is an
//! interrupted run and is refused rather than read in part. `check` validates
//! the rest without packing.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

mod check;
mod tables;

pub use check::{Summary, check};

/// The only version this build reads.
pub const FORMAT_VERSION: u32 = 2;
pub const CELL_SIZE_M: u32 = 50;
pub const TILE_SIZE: u32 = 256;
pub const MANIFEST: &str = "manifest.json";

/// How a layer's samples are stored: little-endian, no header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dtype {
    U8,
    I16,
    U16,
}

impl Dtype {
    pub fn name(self) -> &'static str {
        match self {
            Dtype::U8 => "u8",
            Dtype::I16 => "i16",
            Dtype::U16 => "u16",
        }
    }

    pub fn bytes(self) -> u64 {
        match self {
            Dtype::U8 => 1,
            Dtype::I16 | Dtype::U16 => 2,
        }
    }
}

/// A tile layer as the format fixes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layer {
    pub name: &'static str,
    pub dtype: Dtype,
    pub nodata: i32,
}

pub const LAYERS: [Layer; 5] = [
    Layer {
        name: "height",
        dtype: Dtype::I16,
        nodata: -32768,
    },
    Layer {
        name: "water",
        dtype: Dtype::U8,
        nodata: 255,
    },
    Layer {
        name: "surface",
        dtype: Dtype::U8,
        nodata: 0,
    },
    Layer {
        name: "trees",
        dtype: Dtype::U16,
        nodata: 65535,
    },
    Layer {
        name: "lines",
        dtype: Dtype::U16,
        nodata: 65535,
    },
];

pub fn layer(name: &str) -> Option<Layer> {
    LAYERS.iter().copied().find(|l| l.name == name)
}

/// Every key the manifest carries; a null value is allowed, an absent key is
/// not.
const MANIFEST_KEYS: [&str; 17] = [
    "format_version",
    "extractor_version",
    "theatre",
    "dcs_build",
    "dcs_build_timestamp",
    "files",
    "extracted_at",
    "authored_bounds_m",
    "authored_bounds_source",
    "grid",
    "omit_sea_tiles",
    "layers",
    "tiles",
    "tables",
    "hook",
    "timing_ms",
    "notes",
];

const TILE_KEYS: [&str; 6] = ["layer", "tx", "tz", "path", "min", "max"];
const FILE_KEYS: [&str; 6] = ["role", "path", "class", "version", "size", "payload_size"];

#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub format_version: u32,
    pub extractor_version: String,
    pub theatre: String,
    pub dcs_build: String,
    pub dcs_build_timestamp: String,
    pub files: Vec<FileEntry>,
    pub extracted_at: String,
    pub authored_bounds_m: Bounds,
    pub authored_bounds_source: String,
    pub grid: Grid,
    pub omit_sea_tiles: bool,
    pub layers: BTreeMap<String, LayerEntry>,
    pub tiles: Vec<TileEntry>,
    pub tables: BTreeMap<String, String>,
    pub hook: Option<Value>,
    pub timing_ms: BTreeMap<String, Value>,
    pub notes: Vec<String>,
}

/// One install file the decoders read.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct FileEntry {
    pub role: String,
    pub path: String,
    pub class: Option<String>,
    pub version: Option<u32>,
    pub size: u64,
    pub payload_size: Option<u64>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Bounds {
    pub min_x: f64,
    pub min_z: f64,
    pub max_x: f64,
    pub max_z: f64,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Grid {
    pub cell_size: u32,
    pub origin_x: f64,
    pub origin_z: f64,
    /// Rows, running north.
    pub height: u32,
    /// Columns, running east.
    pub width: u32,
    pub tile_size: u32,
}

impl Grid {
    /// Tiles along x and along z; the last in each direction is a full tile.
    pub fn tiles(&self) -> (u32, u32) {
        (
            self.height.div_ceil(self.tile_size),
            self.width.div_ceil(self.tile_size),
        )
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct LayerEntry {
    pub dtype: String,
    pub nodata: i64,
    pub unit: String,
    pub source: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TileEntry {
    pub layer: String,
    pub tx: u32,
    pub tz: u32,
    pub path: String,
    /// Over the samples that are not nodata; null when every one is.
    pub min: Option<i64>,
    pub max: Option<i64>,
}

impl TileEntry {
    /// The one path a tile may have, so a manifest cannot point outside the
    /// directory.
    pub fn expected_path(&self) -> String {
        format!("tiles/{}/{}_{}.bin", self.layer, self.tx, self.tz)
    }
}

/// What is wrong, and the file it is wrong in, relative to the extract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub path: String,
    pub message: String,
}

impl Failure {
    pub fn new(path: impl Into<String>, message: impl Into<String>) -> Self {
        Failure {
            path: path.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

/// One tile's samples, row-major with columns fastest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Samples {
    U8(Vec<u8>),
    I16(Vec<i16>),
    U16(Vec<u16>),
}

impl Samples {
    pub fn len(&self) -> usize {
        match self {
            Samples::U8(v) => v.len(),
            Samples::I16(v) => v.len(),
            Samples::U16(v) => v.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get(&self, i: usize) -> i32 {
        match self {
            Samples::U8(v) => i32::from(v[i]),
            Samples::I16(v) => i32::from(v[i]),
            Samples::U16(v) => i32::from(v[i]),
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = i32> + '_ {
        (0..self.len()).map(|i| self.get(i))
    }

    fn decode(dtype: Dtype, bytes: &[u8]) -> Samples {
        let pairs = || bytes.as_chunks::<2>().0.iter().copied();
        match dtype {
            Dtype::U8 => Samples::U8(bytes.to_vec()),
            Dtype::I16 => Samples::I16(pairs().map(i16::from_le_bytes).collect()),
            Dtype::U16 => Samples::U16(pairs().map(u16::from_le_bytes).collect()),
        }
    }
}

/// An extract whose manifest parsed at the version this build reads.
#[derive(Debug, Clone)]
pub struct Extract {
    pub dir: PathBuf,
    pub manifest: Manifest,
}

impl Extract {
    /// Reads the manifest. Nothing else is checked: `check` does that.
    pub fn open(dir: &Path) -> Result<Extract, Failure> {
        let path = dir.join(MANIFEST);
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let why = if dir.is_dir() {
                    "missing: the run that wrote this directory did not finish; run it again"
                } else {
                    "missing: no extract directory here"
                };
                return Err(Failure::new(MANIFEST, why));
            }
            Err(e) => return Err(Failure::new(MANIFEST, format!("cannot read: {e}"))),
        };
        let doc: Value = serde_json::from_str(&text)
            .map_err(|e| Failure::new(MANIFEST, format!("not JSON: {e}")))?;
        let Some(obj) = doc.as_object() else {
            return Err(Failure::new(MANIFEST, "not a JSON object"));
        };
        // The version comes first: a manifest of another version is refused
        // as that, not as a list of keys it lacks.
        match obj.get("format_version").and_then(Value::as_u64) {
            Some(v) if v == u64::from(FORMAT_VERSION) => {}
            Some(v) => {
                return Err(Failure::new(
                    MANIFEST,
                    format!("format_version {v} is not one this build reads ({FORMAT_VERSION})"),
                ));
            }
            None => return Err(Failure::new(MANIFEST, "no numeric format_version")),
        }
        let missing: Vec<&str> = MANIFEST_KEYS
            .iter()
            .copied()
            .filter(|k| !obj.contains_key(*k))
            .collect();
        if !missing.is_empty() {
            return Err(Failure::new(
                MANIFEST,
                format!("lacks {}", missing.join(", ")),
            ));
        }
        // An `Option` field reads an absent key as null, so the nullable keys
        // of each entry are required here, before serde can excuse them.
        for (list, keys) in [("tiles", &TILE_KEYS[..]), ("files", &FILE_KEYS[..])] {
            let entries = obj[list].as_array().map(Vec::as_slice).unwrap_or_default();
            for (i, e) in entries.iter().enumerate() {
                let lacks: Vec<&str> = keys
                    .iter()
                    .copied()
                    .filter(|k| e.get(*k).is_none())
                    .collect();
                if !lacks.is_empty() {
                    return Err(Failure::new(
                        MANIFEST,
                        format!("{list}[{i}] lacks {}", lacks.join(", ")),
                    ));
                }
            }
        }
        let manifest: Manifest =
            serde_json::from_value(doc).map_err(|e| Failure::new(MANIFEST, e.to_string()))?;
        Ok(Extract {
            dir: dir.to_path_buf(),
            manifest,
        })
    }

    /// Reads one listed tile, refusing a file of any size but a full tile.
    pub fn read_tile(&self, tile: &TileEntry) -> Result<Samples, Failure> {
        let Some(layer) = layer(&tile.layer) else {
            return Err(Failure::new(
                &tile.path,
                format!("unknown layer {}", tile.layer),
            ));
        };
        let ts = u64::from(self.manifest.grid.tile_size);
        let want = ts * ts * layer.dtype.bytes();
        let bytes = fs::read(self.dir.join(&tile.path)).map_err(|e| {
            let why = if e.kind() == io::ErrorKind::NotFound {
                "listed in the manifest but not on disk".to_string()
            } else {
                format!("cannot read: {e}")
            };
            Failure::new(&tile.path, why)
        })?;
        if bytes.len() as u64 != want {
            return Err(Failure::new(
                &tile.path,
                format!(
                    "{} bytes, where a {} tile is {want}",
                    bytes.len(),
                    layer.dtype.name()
                ),
            ));
        }
        Ok(Samples::decode(layer.dtype, &bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratch::Scratch;
    use crate::synth;

    fn synth_extract() -> Scratch {
        let s = Scratch::new();
        let t =
            synth::Theatre::new(synth::DEFAULT_SEED, synth::DEFAULT_SIZE_KM, CELL_SIZE_M).unwrap();
        synth::write(&t, &s.0).unwrap();
        s
    }

    #[test]
    fn the_format_matches_what_synth_writes() {
        assert_eq!(FORMAT_VERSION, synth::FORMAT_VERSION);
        assert_eq!(TILE_SIZE, synth::TILE_SIZE);
        assert_eq!(CELL_SIZE_M, synth::CELL_SIZE_M);
        let nodata = [
            i32::from(synth::HEIGHT_NODATA),
            i32::from(synth::WATER_NODATA),
            i32::from(synth::SURFACE_NODATA),
            i32::from(synth::COUNT_NODATA),
            i32::from(synth::COUNT_NODATA),
        ];
        assert_eq!(LAYERS.map(|l| l.nodata), nodata);
    }

    #[test]
    fn opens_the_synthetic_extract_and_reads_a_tile() {
        let s = synth_extract();
        let x = Extract::open(&s.0).unwrap();
        assert_eq!(x.manifest.theatre, synth::THEATRE);
        assert_eq!(x.manifest.grid.tiles(), (6, 6));

        let t =
            synth::Theatre::new(synth::DEFAULT_SEED, synth::DEFAULT_SIZE_KM, CELL_SIZE_M).unwrap();
        let entry = x
            .manifest
            .tiles
            .iter()
            .find(|e| e.layer == "height" && e.tx == 1 && e.tz == 5)
            .unwrap();
        let samples = x.read_tile(entry).unwrap();
        assert!(matches!(samples, Samples::I16(_)));
        let (row, col) = (300u32, 1300u32);
        let i = ((row - 256) * 256 + (col - 1280)) as usize;
        assert_eq!(samples.get(i), i32::from(t.cell(row, col).height));
    }

    #[test]
    fn refuses_a_directory_without_a_manifest() {
        let s = synth_extract();
        fs::remove_file(s.0.join(MANIFEST)).unwrap();
        let f = Extract::open(&s.0).unwrap_err();
        assert_eq!(f.path, MANIFEST);
        assert!(f.message.contains("did not finish"), "{f}");
    }

    #[test]
    fn refuses_a_version_it_does_not_read_before_its_keys() {
        let s = synth_extract();
        fs::write(s.0.join(MANIFEST), r#"{"format_version": 3}"#).unwrap();
        let f = Extract::open(&s.0).unwrap_err();
        assert!(f.message.contains("format_version 3"), "{f}");
    }

    #[test]
    fn names_every_manifest_key_it_lacks() {
        let s = synth_extract();
        fs::write(
            s.0.join(MANIFEST),
            r#"{"format_version": 2, "theatre": "x"}"#,
        )
        .unwrap();
        let f = Extract::open(&s.0).unwrap_err();
        assert!(
            f.message.contains("grid") && f.message.contains("hook"),
            "{f}"
        );
        assert!(!f.message.contains("theatre"), "{f}");
    }

    #[test]
    fn a_nullable_entry_key_must_still_be_present() {
        let s = synth_extract();
        let path = s.0.join(MANIFEST);
        let mut m: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        m["tiles"][2].as_object_mut().unwrap().remove("max");
        m["files"][0]
            .as_object_mut()
            .unwrap()
            .remove("payload_size");
        fs::write(&path, m.to_string()).unwrap();
        let f = Extract::open(&s.0).unwrap_err();
        assert!(f.message.contains("tiles[2] lacks max"), "{f}");

        m["tiles"][2]["max"] = Value::Null;
        fs::write(&path, m.to_string()).unwrap();
        let f = Extract::open(&s.0).unwrap_err();
        assert!(f.message.contains("files[0] lacks payload_size"), "{f}");
    }

    #[test]
    fn refuses_a_tile_of_the_wrong_size() {
        let s = synth_extract();
        let x = Extract::open(&s.0).unwrap();
        let entry = &x.manifest.tiles[0];
        fs::write(s.0.join(&entry.path), [0u8; 10]).unwrap();
        let f = x.read_tile(entry).unwrap_err();
        assert_eq!(f.path, entry.path);
        assert!(f.message.contains("10 bytes"), "{f}");
    }
}
