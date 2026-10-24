//! Tile addressing over the cell grid, and tiles read back whole whether or
//! not they were written (ADR 0039).
//!
//! Rows run north with DCS x and columns east with DCS z. Cell `(row, col)`
//! covers `[origin + index × cell_size, origin + (index + 1) × cell_size)` on
//! each axis and is sampled at its centre.

use std::collections::HashMap;

use crate::extract::{Extract, Failure, Grid, Layer, Samples, TileEntry, layer};

const WATER_SEA: i32 = 2;
const HEIGHT_SEA: i32 = 0;
const SURFACE_WATER: i32 = 3;

impl Grid {
    /// The cell holding `(x, z)`, or `None` off the grid.
    pub fn cell_at(&self, x: f64, z: f64) -> Option<(u32, u32)> {
        let c = f64::from(self.cell_size);
        let row = ((x - self.origin_x) / c).floor();
        let col = ((z - self.origin_z) / c).floor();
        // A NaN fails both comparisons and lands here too.
        let inside = |v: f64, n: u32| v >= 0.0 && v < f64::from(n);
        (inside(row, self.height) && inside(col, self.width)).then_some((row as u32, col as u32))
    }

    /// DCS `x` and `z` of a cell's centre.
    pub fn centre(&self, row: u32, col: u32) -> (f64, f64) {
        let c = f64::from(self.cell_size);
        (
            self.origin_x + (f64::from(row) + 0.5) * c,
            self.origin_z + (f64::from(col) + 0.5) * c,
        )
    }

    /// The tile holding a cell, and the cell's sample index within it.
    pub fn tile_of(&self, row: u32, col: u32) -> (u32, u32, usize) {
        let ts = self.tile_size;
        let i = (row % ts) as usize * ts as usize + (col % ts) as usize;
        (row / ts, col / ts, i)
    }
}

/// Where tiles come from: the tiles that were written, and the geometry they
/// sit on. A tile that was not written is `None`, and `tile` rebuilds it.
pub trait Tiles {
    fn grid(&self) -> &Grid;
    fn stored(&self, layer: Layer, tx: u32, tz: u32) -> Result<Option<Samples>, Failure>;
}

/// An extract's tiles, found by the manifest's listing.
pub struct ExtractTiles<'a> {
    extract: &'a Extract,
    listed: HashMap<(&'a str, u32, u32), &'a TileEntry>,
}

impl<'a> ExtractTiles<'a> {
    pub fn new(extract: &'a Extract) -> Self {
        let listed = extract
            .manifest
            .tiles
            .iter()
            .map(|t| ((t.layer.as_str(), t.tx, t.tz), t))
            .collect();
        ExtractTiles { extract, listed }
    }
}

impl Tiles for ExtractTiles<'_> {
    fn grid(&self) -> &Grid {
        &self.extract.manifest.grid
    }

    fn stored(&self, layer: Layer, tx: u32, tz: u32) -> Result<Option<Samples>, Failure> {
        match self.listed.get(&(layer.name, tx, tz)) {
            Some(t) => self.extract.read_tile(t).map(Some),
            None => Ok(None),
        }
    }
}

/// One tile of a layer, every sample of it, rebuilt from the `water` tile at
/// the same place when it was not written: no `water` tile means every cell
/// is outside the mesh; an all-sea one stands for its `height` and
/// `surface`; and a count tile is left out when every covered cell is 0.
pub fn tile(src: &impl Tiles, layer: Layer, tx: u32, tz: u32) -> Result<Samples, Failure> {
    if let Some(s) = src.stored(layer, tx, tz)? {
        return Ok(s);
    }
    let n = src.grid().tile_size as usize;
    let n = n * n;
    let fill = |v: i32| Samples::filled(layer.dtype, v, n);
    let water = layer_named("water");
    let w = match layer.name {
        "water" => None,
        _ => src.stored(water, tx, tz)?,
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
        "height" | "surface" if w.iter().all(|v| v == WATER_SEA) => Ok(fill(match layer.name {
            "height" => HEIGHT_SEA,
            _ => SURFACE_WATER,
        })),
        _ => Err(Failure::new(
            format!("tiles/{}/{tx}_{tz}.bin", layer.name),
            "not written, though its water tile is and is not all sea",
        )),
    }
}

fn layer_named(name: &str) -> Layer {
    layer(name).expect("a layer the format has")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::{CELL_SIZE_M, MANIFEST, check};
    use crate::scratch::Scratch;
    use crate::synth;
    use serde_json::Value;
    use std::fs;

    fn theatre() -> synth::Theatre {
        synth::Theatre::new(synth::DEFAULT_SEED, synth::DEFAULT_SIZE_KM, CELL_SIZE_M).unwrap()
    }

    fn synth_extract() -> (Scratch, Extract) {
        let s = Scratch::new();
        synth::write(&theatre(), &s.0).unwrap();
        let x = Extract::open(&s.0).unwrap();
        (s, x)
    }

    fn l(name: &str) -> Layer {
        layer_named(name)
    }

    #[test]
    fn addresses_cells_and_tiles() {
        let (_s, x) = synth_extract();
        let g = &x.manifest.grid;
        let (ox, oz, c) = (g.origin_x, g.origin_z, f64::from(g.cell_size));
        let (h, w) = (f64::from(g.height), f64::from(g.width));
        assert_eq!(g.cell_at(ox, oz), Some((0, 0)));
        assert_eq!(g.cell_at(ox + 1.0, oz + c), Some((0, 1)));
        assert_eq!(
            g.cell_at(ox + h * c - 0.01, oz + w * c - 0.01),
            Some((g.height - 1, g.width - 1))
        );
        // The far edges are open.
        assert_eq!(g.cell_at(ox + h * c, oz), None);
        assert_eq!(g.cell_at(ox, oz + w * c), None);
        assert_eq!(g.cell_at(ox - 0.01, oz), None);
        assert_eq!(g.cell_at(ox, oz - 0.01), None);
        assert_eq!(g.cell_at(f64::NAN, oz), None);
        let (cx, cz) = g.centre(300, 1300);
        assert_eq!((cx - ox, cz - oz), (300.5 * c, 1300.5 * c));
        assert_eq!(g.cell_at(cx, cz), Some((300, 1300)));

        assert_eq!(g.tile_of(0, 0), (0, 0, 0));
        assert_eq!(g.tile_of(255, 255), (0, 0, 256 * 256 - 1));
        assert_eq!(g.tile_of(256, 255), (1, 0, 255));
        assert_eq!(g.tile_of(255, 256), (0, 1, 255 * 256));
        assert_eq!(g.tile_of(300, 1300), (1, 5, 44 * 256 + 20));
    }

    #[test]
    fn a_written_tile_reads_as_written() {
        let (_s, x) = synth_extract();
        let src = ExtractTiles::new(&x);
        let entry = x
            .manifest
            .tiles
            .iter()
            .find(|e| e.layer == "height" && e.tx == 1 && e.tz == 5);
        let s = tile(&src, l("height"), 1, 5).unwrap();
        assert_eq!(s, x.read_tile(entry.unwrap()).unwrap());
    }

    #[test]
    fn an_all_sea_tile_reads_as_sea_level_water() {
        let (_s, x) = synth_extract();
        let src = ExtractTiles::new(&x);
        // (1, 1) is all sea, so only its water tile was written.
        assert!(src.stored(l("height"), 1, 1).unwrap().is_none());
        assert!(src.stored(l("surface"), 1, 1).unwrap().is_none());
        assert!(tile(&src, l("water"), 1, 1).unwrap().iter().all(|v| v == 2));
        assert!(
            tile(&src, l("height"), 1, 1)
                .unwrap()
                .iter()
                .all(|v| v == 0)
        );
        assert!(
            tile(&src, l("surface"), 1, 1)
                .unwrap()
                .iter()
                .all(|v| v == 3)
        );
        assert_eq!(tile(&src, l("height"), 1, 1).unwrap().len(), 256 * 256);
    }

    #[test]
    fn an_absent_count_tile_is_zero_on_the_mesh_and_nodata_off_it() {
        let (_s, x) = synth_extract();
        let src = ExtractTiles::new(&x);
        let t = theatre();
        // (0, 0) has neither trees nor lines, and holds margin outside the mesh.
        for name in ["trees", "lines"] {
            assert!(src.stored(l(name), 0, 0).unwrap().is_none(), "{name}");
            let s = tile(&src, l(name), 0, 0).unwrap();
            assert!(matches!(s, Samples::U16(_)));
            let (mut zero, mut nodata) = (0, 0);
            for i in 0..256 * 256 {
                let (row, col) = (i as u32 / 256, i as u32 % 256);
                let want = if t.cell(row, col).water == synth::WATER_NODATA {
                    nodata += 1;
                    65535
                } else {
                    zero += 1;
                    0
                };
                assert_eq!(s.get(i), want, "{name} ({row}, {col})");
            }
            assert!(
                zero > 0 && nodata > 0,
                "{name}: {zero} zero, {nodata} nodata"
            );
        }
    }

    /// The synthetic extract has no tile wholly outside the mesh, so one is
    /// made by taking a tile out of its manifest and off the disk.
    #[test]
    fn a_tile_with_no_water_tile_is_nodata_in_every_layer() {
        let (s, _) = synth_extract();
        let path = s.0.join(MANIFEST);
        let mut m: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let tiles = m["tiles"].as_array_mut().unwrap();
        tiles.retain(|t| {
            let here = t["tx"] == 2 && t["tz"] == 2;
            if here {
                fs::remove_file(s.0.join(t["path"].as_str().unwrap())).unwrap();
            }
            !here
        });
        fs::write(&path, m.to_string()).unwrap();
        check(&s.0).unwrap();

        let x = Extract::open(&s.0).unwrap();
        let src = ExtractTiles::new(&x);
        for layer in crate::extract::LAYERS {
            let t = tile(&src, layer, 2, 2).unwrap();
            assert_eq!(t.len(), 256 * 256);
            assert!(t.iter().all(|v| v == layer.nodata), "{}", layer.name);
        }
        // Its neighbour is untouched.
        assert!(src.stored(l("height"), 2, 3).unwrap().is_some());
    }

    #[test]
    fn refuses_an_absent_height_whose_water_is_not_all_sea() {
        let (s, _) = synth_extract();
        let path = s.0.join(MANIFEST);
        let mut m: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        m["tiles"]
            .as_array_mut()
            .unwrap()
            .retain(|t| !(t["layer"] == "height" && t["tx"] == 0 && t["tz"] == 0));
        fs::write(&path, m.to_string()).unwrap();
        let x = Extract::open(&s.0).unwrap();
        let f = tile(&ExtractTiles::new(&x), l("height"), 0, 0).unwrap_err();
        assert_eq!(f.path, "tiles/height/0_0.bin");
        assert!(f.message.contains("not all sea"), "{f}");
    }
}
