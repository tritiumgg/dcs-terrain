//! Reads an extract directory: its manifest and its tiles.
//!
//! The manifest is written last, so a directory without one is an
//! interrupted run and is refused rather than read in part. `check` validates
//! the rest.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::grid::Grid;

mod check;
#[cfg(test)]
mod fixture;
mod tables;

pub use check::{Summary, check};

/// The only version this build reads.
pub const FORMAT_VERSION: u32 = 2;
pub const CELL_SIZE_M: u32 = 50;
pub const TILE_SIZE: u32 = 256;
pub const MANIFEST: &str = "manifest.json";

const WATER_SEA: i32 = 2;
const HEIGHT_SEA: i32 = 0;
const SURFACE_WATER: i32 = 3;

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

    /// `n` samples of `dtype`, sample `i` being `f(i)`. A value outside the
    /// dtype's range is the caller's error, and wraps.
    pub fn from_fn(dtype: Dtype, n: usize, f: impl Fn(usize) -> i32) -> Samples {
        match dtype {
            Dtype::U8 => Samples::U8((0..n).map(|i| f(i) as u8).collect()),
            Dtype::I16 => Samples::I16((0..n).map(|i| f(i) as i16).collect()),
            Dtype::U16 => Samples::U16((0..n).map(|i| f(i) as u16).collect()),
        }
    }

    pub fn filled(dtype: Dtype, v: i32, n: usize) -> Samples {
        Samples::from_fn(dtype, n, |_| v)
    }

    pub(crate) fn decode(dtype: Dtype, bytes: &[u8]) -> Samples {
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

    /// One tile of a layer, every sample of it, rebuilt from the `water` tile
    /// at the same place when it was not written: no `water` tile means every
    /// cell is outside the mesh; an all-sea one stands for its `height` and
    /// `surface`; and a count tile is left out when every covered cell is 0.
    pub fn tile(&self, layer: Layer, tx: u32, tz: u32) -> Result<Samples, Failure> {
        let stored = |name: &str| {
            let listed = self.manifest.tiles.iter();
            match listed
                .into_iter()
                .find(|t| t.layer == name && (t.tx, t.tz) == (tx, tz))
            {
                Some(t) => self.read_tile(t).map(Some),
                None => Ok(None),
            }
        };
        if let Some(s) = stored(layer.name)? {
            return Ok(s);
        }
        let n = self.manifest.grid.tile_size as usize;
        let n = n * n;
        let fill = |v: i32| Samples::filled(layer.dtype, v, n);
        let water = LAYERS[1];
        let w = match layer.name {
            "water" => None,
            _ => stored(water.name)?,
        };
        let Some(w) = w else {
            return Ok(fill(layer.nodata));
        };
        match layer.name {
            "trees" | "lines" => Ok(Samples::from_fn(layer.dtype, n, |i| {
                if w.get(i) == water.nodata {
                    layer.nodata
                } else {
                    0
                }
            })),
            "height" | "surface" if w.iter().all(|v| v == WATER_SEA) => {
                Ok(fill(match layer.name {
                    "height" => HEIGHT_SEA,
                    _ => SURFACE_WATER,
                }))
            }
            _ => Err(Failure::new(
                format!("tiles/{}/{tx}_{tz}.bin", layer.name),
                "not written, though its water tile is and is not all sea",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratch::Scratch;

    fn fixture_extract() -> Scratch {
        let s = Scratch::new();
        fixture::write(&s.0);
        s
    }

    fn edit_manifest(dir: &Path, f: impl FnOnce(&mut Value)) {
        let path = dir.join(MANIFEST);
        let mut m: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        f(&mut m);
        fs::write(&path, m.to_string()).unwrap();
    }

    fn l(name: &str) -> Layer {
        layer(name).unwrap()
    }

    #[test]
    fn the_format_matches_the_synthetic_constants() {
        use crate::synth;
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
    fn opens_the_fixture_and_reads_a_tile() {
        let s = fixture_extract();
        let x = Extract::open(&s.0).unwrap();
        assert_eq!(x.manifest.theatre, "Fixture");
        assert_eq!(x.manifest.grid.tiles(), (2, 3));
        let entry = x
            .manifest
            .tiles
            .iter()
            .find(|e| e.layer == "height" && e.tx == 1 && e.tz == 2)
            .unwrap();
        let samples = x.read_tile(entry).unwrap();
        assert!(matches!(samples, Samples::I16(_)));
        let (row, col) = (270u32, 530u32);
        let i = ((row - 256) * 256 + (col - 512)) as usize;
        assert_eq!(samples.get(i), fixture::cell("height", row, col));
        assert_eq!(x.tile(l("height"), 1, 2).unwrap(), samples);
    }

    #[test]
    fn an_all_sea_tile_reads_as_sea_level_water() {
        let s = fixture_extract();
        let x = Extract::open(&s.0).unwrap();
        // (0, 1) is all sea, so only its water tile was written.
        let water = x.tile(l("water"), 0, 1).unwrap();
        assert!(water.iter().all(|v| v == 2));
        let height = x.tile(l("height"), 0, 1).unwrap();
        assert!(height.iter().all(|v| v == 0));
        assert_eq!(height.len(), 256 * 256);
        assert!(x.tile(l("surface"), 0, 1).unwrap().iter().all(|v| v == 3));
    }

    #[test]
    fn an_absent_count_tile_is_zero_on_the_mesh_and_nodata_off_it() {
        let s = fixture_extract();
        let x = Extract::open(&s.0).unwrap();
        // The first row of tile (0, 0) is outside the mesh, and no lines
        // tile is written.
        let t = x.tile(l("lines"), 0, 0).unwrap();
        assert!(matches!(t, Samples::U16(_)));
        for (i, v) in t.iter().enumerate() {
            let want = if i < 256 { 65535 } else { 0 };
            assert_eq!(v, want, "sample {i}");
        }
        // A written count tile reads as written.
        let trees = x.tile(l("trees"), 0, 0).unwrap();
        assert_eq!(trees.get(2 * 256 + 4), fixture::TREES);
        assert_eq!(trees.get(2 * 256 + 5), 0);
    }

    #[test]
    fn a_tile_with_no_water_tile_is_nodata_in_every_layer() {
        let s = fixture_extract();
        let x = Extract::open(&s.0).unwrap();
        for layer in LAYERS {
            let t = x.tile(layer, 1, 0).unwrap();
            assert_eq!(t.len(), 256 * 256);
            assert!(t.iter().all(|v| v == layer.nodata), "{}", layer.name);
        }
    }

    #[test]
    fn refuses_an_absent_height_whose_water_is_not_all_sea() {
        let s = fixture_extract();
        edit_manifest(&s.0, |m| {
            m["tiles"]
                .as_array_mut()
                .unwrap()
                .retain(|t| !(t["layer"] == "height" && t["tx"] == 0 && t["tz"] == 0));
        });
        let x = Extract::open(&s.0).unwrap();
        let f = x.tile(l("height"), 0, 0).unwrap_err();
        assert_eq!(f.path, "tiles/height/0_0.bin");
        assert!(f.message.contains("not all sea"), "{f}");
    }

    #[test]
    fn each_absent_rule_stands_on_the_water_tile_alone() {
        let s = fixture_extract();
        // Sea beside a cell off the mesh is not all sea: the all-sea water
        // tile given one cell outside the mesh.
        let p = "tiles/water/0_1.bin";
        let mut w = vec![2u8; 256 * 256];
        w[256 * 256 - 1] = 255;
        fixture::put(&s.0, p, &w);
        let x = Extract::open(&s.0).unwrap();
        assert!(x.tile(l("height"), 0, 1).is_err());
        let lines = x.tile(l("lines"), 0, 1).unwrap();
        assert_eq!(lines.get(0), 0);
        assert_eq!(lines.get(256 * 256 - 1), 65535);
        // A water tile with no covered cell, read anyway, stands for no sea
        // and no count.
        fixture::put(&s.0, p, &vec![255u8; 256 * 256]);
        assert!(x.tile(l("surface"), 0, 1).is_err());
        let trees = x.tile(l("trees"), 0, 1).unwrap();
        assert_eq!(trees, Samples::U16(vec![65535; 256 * 256]));
    }

    #[test]
    fn refuses_a_directory_without_a_manifest() {
        let s = fixture_extract();
        fs::remove_file(s.0.join(MANIFEST)).unwrap();
        let f = Extract::open(&s.0).unwrap_err();
        assert_eq!(f.path, MANIFEST);
        assert!(f.message.contains("did not finish"), "{f}");
    }

    #[test]
    fn refuses_a_version_it_does_not_read_before_its_keys() {
        let s = fixture_extract();
        fs::write(s.0.join(MANIFEST), r#"{"format_version": 3}"#).unwrap();
        let f = Extract::open(&s.0).unwrap_err();
        assert!(f.message.contains("format_version 3"), "{f}");
    }

    #[test]
    fn names_every_manifest_key_it_lacks() {
        let s = fixture_extract();
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
        let s = fixture_extract();
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
        let s = fixture_extract();
        let x = Extract::open(&s.0).unwrap();
        let entry = &x.manifest.tiles[0];
        fs::write(s.0.join(&entry.path), [0u8; 10]).unwrap();
        let f = x.read_tile(entry).unwrap_err();
        assert_eq!(f.path, entry.path);
        assert!(f.message.contains("10 bytes"), "{f}");
    }
}
