//! A small extract that passes `check`, built cell by cell for the reader's
//! tests.
//!
//! The grid is 300 rows by 600 columns, so 2 by 3 tiles, the last row of
//! tiles padded past the grid. Each tile is one case of what an extract
//! writes:
//!
//! | tile   | what it is                                                   |
//! |--------|--------------------------------------------------------------|
//! | (0, 0) | land, its first row outside the mesh, a trees tile written    |
//! | (0, 1) | all sea: only its water tile is written                      |
//! | (0, 2) | land                                                         |
//! | (1, 0) | wholly outside the mesh: nothing is written                  |
//! | (1, 1) | land, padded past the grid                                   |
//! | (1, 2) | land, padded past the grid                                   |
//!
//! No `lines` tile is written, and no `trees` tile but (0, 0)'s.

use std::fs;
use std::path::Path;

use serde_json::{Value, json};

use super::{LAYERS, MANIFEST, TILE_SIZE, layer};

pub const ROWS: u32 = 300;
pub const COLS: u32 = 600;
/// Trees in each cell of tile (0, 0) whose row and column are both even.
pub const TREES: i32 = 3;

/// A cell's value in a layer, by the table above.
pub fn cell(name: &str, row: u32, col: u32) -> i32 {
    let l = layer(name).unwrap();
    let ts = TILE_SIZE;
    let (tx, tz) = (row / ts, col / ts);
    let outside = row >= ROWS || col >= COLS || (tx, tz) == (1, 0) || (row == 0 && tz == 0);
    if outside {
        return l.nodata;
    }
    let sea = (tx, tz) == (0, 1);
    match name {
        "height" if sea => 0,
        "height" => (row + col) as i32 % 1000,
        "water" if sea => 2,
        "water" => 0,
        "surface" if sea => 3,
        "surface" => 1,
        "trees" if (tx, tz) == (0, 0) && row.is_multiple_of(2) && col.is_multiple_of(2) => TREES,
        _ => 0,
    }
}

/// Whether the extract writes a layer's tile.
fn written(name: &str, tx: u32, tz: u32) -> bool {
    match (tx, tz) {
        (1, 0) => false,
        (0, 1) => name == "water",
        (0, 0) => name != "lines",
        _ => !matches!(name, "trees" | "lines"),
    }
}

/// Writes the extract into `dir`.
pub fn write(dir: &Path) {
    let ts = TILE_SIZE;
    let mut tiles = Vec::new();
    for l in LAYERS {
        for tx in 0..ROWS.div_ceil(ts) {
            for tz in 0..COLS.div_ceil(ts) {
                if !written(l.name, tx, tz) {
                    continue;
                }
                let values: Vec<i32> = (0..ts * ts)
                    .map(|i| cell(l.name, tx * ts + i / ts, tz * ts + i % ts))
                    .collect();
                let path = format!("tiles/{}/{tx}_{tz}.bin", l.name);
                put(dir, &path, &encode(l.dtype.bytes(), &values));
                let present = || values.iter().filter(|&&v| v != l.nodata);
                tiles.push(json!({
                    "layer": l.name, "tx": tx, "tz": tz, "path": path,
                    "min": present().min(), "max": present().max(),
                }));
            }
        }
    }
    let tables = super::tables::write_rows(dir);
    let layers: serde_json::Map<String, Value> = LAYERS
        .iter()
        .map(|l| {
            let entry = json!({
                "dtype": l.dtype.name(), "nodata": l.nodata, "unit": "", "source": "fixture",
            });
            (l.name.to_string(), entry)
        })
        .collect();
    let manifest = json!({
        "format_version": super::FORMAT_VERSION,
        "theatre": "Fixture",
        "dcs_build": "0.0.0.0",
        "dcs_build_timestamp": "00000000-000000",
        "files": [{
            "role": "surface", "path": "Mods/terrains/Fixture/Surface/F.surface5",
            "class": "landscape5::Surface5File", "version": 5, "size": 1, "payload_size": null,
        }],
        "extracted_at": "2000-01-01T00:00:00Z",
        "authored_bounds_m": {"min_x": 0, "min_z": 0, "max_x": 15000, "max_z": 30000},
        "authored_bounds_source": "fixture",
        "grid": {
            "cell_size": 50, "origin_x": 0, "origin_z": 0,
            "height": ROWS, "width": COLS, "tile_size": ts,
        },
        "omit_sea_tiles": true,
        "layers": layers,
        "tiles": tiles,
        "tables": tables,
        "timing_ms": {},
        "notes": [],
    });
    put(dir, MANIFEST, manifest.to_string().as_bytes());
}

/// Little-endian samples of `bytes` each; a value is the caller's to keep in
/// range.
fn encode(bytes: u64, values: &[i32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(values.len() * bytes as usize);
    for &v in values {
        out.extend_from_slice(&v.to_le_bytes()[..bytes as usize]);
    }
    out
}

pub fn put(dir: &Path, rel: &str, bytes: &[u8]) {
    let p = dir.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, bytes).unwrap();
}
