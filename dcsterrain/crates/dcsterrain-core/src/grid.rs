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

/// A rectangle of one layer's cells, read across tiles into one buffer.
///
/// It may reach past the grid on any side; a cell there reads as the
/// layer's `nodata`, as a cell outside the mesh does.
#[derive(Debug, Clone)]
pub struct Window {
    pub layer: Layer,
    /// The grid row and column of the first sample.
    pub row0: i64,
    pub col0: i64,
    pub rows: u32,
    pub cols: u32,
    /// Row-major with columns fastest, as a tile is.
    pub data: Vec<i32>,
}

impl Window {
    pub fn read(
        src: &impl Tiles,
        layer: Layer,
        row0: i64,
        col0: i64,
        rows: u32,
        cols: u32,
    ) -> Result<Window, Failure> {
        let g = *src.grid();
        let mut data = vec![layer.nodata; rows as usize * cols as usize];
        // The part of the window on the grid, as half-open grid indices.
        let clip = |start: i64, len: u32, n: u32| {
            (
                start.max(0),
                start.saturating_add(i64::from(len)).min(i64::from(n)),
            )
        };
        let (r_lo, r_hi) = clip(row0, rows, g.height);
        let (c_lo, c_hi) = clip(col0, cols, g.width);
        let ts = i64::from(g.tile_size);
        let on_grid = r_lo < r_hi && c_lo < c_hi;
        let tiles = |lo: i64, hi: i64| lo / ts..(hi - 1) / ts + 1;
        // Each tile the window touches is read once.
        for tx in tiles(r_lo, r_hi).take_while(|_| on_grid) {
            for tz in tiles(c_lo, c_hi) {
                let t = tile(src, layer, tx as u32, tz as u32)?;
                let (r0, r1) = (r_lo.max(tx * ts), r_hi.min((tx + 1) * ts));
                let (c0, c1) = (c_lo.max(tz * ts), c_hi.min((tz + 1) * ts));
                for r in r0..r1 {
                    for c in c0..c1 {
                        let from = (r - tx * ts) * ts + (c - tz * ts);
                        let to = (r - row0) * i64::from(cols) + (c - col0);
                        data[to as usize] = t.get(from as usize);
                    }
                }
            }
        }
        Ok(Window {
            layer,
            row0,
            col0,
            rows,
            cols,
            data,
        })
    }

    /// The sample at a grid row and column, or `None` outside the window.
    pub fn get(&self, row: i64, col: i64) -> Option<i32> {
        let (r, c) = (row - self.row0, col - self.col0);
        let inside =
            (0..i64::from(self.rows)).contains(&r) && (0..i64::from(self.cols)).contains(&c);
        inside.then(|| self.data[(r * i64::from(self.cols) + c) as usize])
    }
}

/// A layer's value at `(x, z)`, or `None` off the grid or where it is
/// `nodata`.
///
/// `height` is bilinear between the four cell centres around the point, and
/// `None` when any of them that carries weight is `nodata`, so a slope is
/// never drawn down to the nodata value. Within half a cell of the grid's
/// edge the edge cell stands for the one beyond it. Every other layer is a
/// code or a count, which has no value between two cells, so it reads the
/// cell holding the point.
pub fn sample(src: &impl Tiles, layer: Layer, x: f64, z: f64) -> Result<Option<f64>, Failure> {
    let g = *src.grid();
    let Some((row, col)) = g.cell_at(x, z) else {
        return Ok(None);
    };
    let present = |v: i32| (v != layer.nodata).then_some(f64::from(v));
    if layer.name != "height" {
        let w = Window::read(src, layer, row.into(), col.into(), 1, 1)?;
        return Ok(present(w.data[0]));
    }
    let c = f64::from(g.cell_size);
    // In cells from the centre of the first cell, held inside the centres.
    let axis = |v: f64, origin: f64, n: u32| {
        let f = ((v - origin) / c - 0.5).clamp(0.0, f64::from(n - 1));
        let i = f.floor();
        (i as i64, f - i)
    };
    let (r0, tr) = axis(x, g.origin_x, g.height);
    let (c0, tc) = axis(z, g.origin_z, g.width);
    let w = Window::read(src, layer, r0, c0, 2, 2)?;
    let mut sum = 0.0;
    for (i, weight) in [
        (1.0 - tr) * (1.0 - tc),
        (1.0 - tr) * tc,
        tr * (1.0 - tc),
        tr * tc,
    ]
    .into_iter()
    .enumerate()
    {
        if weight == 0.0 {
            continue;
        }
        let Some(v) = present(w.data[i]) else {
            return Ok(None);
        };
        sum += weight * v;
    }
    Ok(Some(sum))
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
        // A window over its corner reads it as nodata and its neighbours as
        // written.
        let th = theatre();
        for layer in crate::extract::LAYERS {
            let w = Window::read(&src, layer, 510, 510, 4, 4).unwrap();
            for (r, c) in cells(&w) {
                let inside = (512..768).contains(&r) && (512..768).contains(&c);
                let want = if inside {
                    layer.nodata
                } else {
                    truth(&th, layer, r, c)
                };
                assert_eq!(w.get(r, c), Some(want), "{} ({r}, {c})", layer.name);
            }
        }
    }

    /// What the closed form says a cell of a layer holds, off the grid
    /// included.
    fn truth(t: &synth::Theatre, layer: Layer, row: i64, col: i64) -> i32 {
        if row < 0 || col < 0 {
            return layer.nodata;
        }
        let c = t.cell(row as u32, col as u32);
        match layer.name {
            "height" => i32::from(c.height),
            "water" => i32::from(c.water),
            "surface" => i32::from(c.surface),
            "trees" => i32::from(c.trees),
            _ => i32::from(c.lines),
        }
    }

    fn cells(w: &Window) -> impl Iterator<Item = (i64, i64)> + '_ {
        (0..i64::from(w.rows))
            .flat_map(move |r| (0..i64::from(w.cols)).map(move |c| (w.row0 + r, w.col0 + c)))
    }

    fn assert_window(src: &impl Tiles, t: &synth::Theatre, row0: i64, col0: i64, n: u32) {
        for layer in crate::extract::LAYERS {
            let w = Window::read(src, layer, row0, col0, n, n).unwrap();
            assert_eq!(w.data.len(), (n * n) as usize);
            for (r, c) in cells(&w) {
                assert_eq!(
                    w.get(r, c),
                    Some(truth(t, layer, r, c)),
                    "{} window at ({row0}, {col0}), cell ({r}, {c})",
                    layer.name
                );
            }
        }
    }

    #[test]
    fn a_window_reads_across_every_tile_edge_and_corner() {
        let (_s, x) = synth_extract();
        let src = ExtractTiles::new(&x);
        let t = theatre();
        // The grid is 1400 cells a side in 6 tiles; the inner tile
        // boundaries are at multiples of 256 up to 1280.
        for k in 1..=5i64 {
            // Along one boundary, away from the other.
            assert_window(&src, &t, k * 256 - 2, 100, 4);
            assert_window(&src, &t, 100, k * 256 - 2, 4);
            for m in 1..=5i64 {
                assert_window(&src, &t, k * 256 - 2, m * 256 - 2, 4);
            }
        }
    }

    #[test]
    fn a_window_reads_past_every_grid_edge_and_corner_as_nodata() {
        let (_s, x) = synth_extract();
        let src = ExtractTiles::new(&x);
        let t = theatre();
        let n = i64::from(x.manifest.grid.height);
        assert_eq!(n, i64::from(t.cells()));
        assert_eq!(x.manifest.grid.width, x.manifest.grid.height);
        // Four edges, four corners: the window is half off the grid. The
        // north and east ones reach rows and columns 1400 to 1535, which the
        // last tile pads past the grid and the window never reads, and the
        // last one past the tiles too.
        for (r, c) in [
            (-2, 700),
            (n - 2, 700),
            (700, -2),
            (700, n - 2),
            (-2, -2),
            (-2, n - 2),
            (n - 2, -2),
            (n - 2, n - 2),
            (1534, 1534),
        ] {
            assert_window(&src, &t, r, c, 4);
        }
        for (r, c) in [(-10, -10), (-10, 700), (2000, 2000), (700, 1400)] {
            for layer in crate::extract::LAYERS {
                let w = Window::read(&src, layer, r, c, 5, 5).unwrap();
                assert!(w.data.iter().all(|&v| v == layer.nodata), "({r}, {c})");
            }
        }
        let empty = Window::read(&src, l("height"), 10, 10, 0, 3).unwrap();
        assert!(empty.data.is_empty());
        assert_eq!(empty.get(10, 10), None);
    }

    #[test]
    fn a_window_over_the_whole_grid_matches_the_theatre() {
        let (_s, x) = synth_extract();
        let src = ExtractTiles::new(&x);
        let t = theatre();
        let n = x.manifest.grid.height + 2;
        let windows: Vec<Window> = crate::extract::LAYERS
            .iter()
            .map(|&layer| Window::read(&src, layer, -1, -1, n, n).unwrap())
            .collect();
        for (r, c) in cells(&windows[0]) {
            let cell = if r < 0 || c < 0 {
                None
            } else {
                Some(t.cell(r as u32, c as u32))
            };
            let want = [
                cell.map_or(-32768, |c| i32::from(c.height)),
                cell.map_or(255, |c| i32::from(c.water)),
                cell.map_or(0, |c| i32::from(c.surface)),
                cell.map_or(65535, |c| i32::from(c.trees)),
                cell.map_or(65535, |c| i32::from(c.lines)),
            ];
            for (w, want) in windows.iter().zip(want) {
                assert_eq!(w.get(r, c), Some(want), "{} ({r}, {c})", w.layer.name);
            }
        }
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

    /// A grid in tiles of 2, height `10 × row + col`, all land.
    struct Small {
        grid: Grid,
    }

    impl Tiles for Small {
        fn grid(&self) -> &Grid {
            &self.grid
        }

        fn stored(&self, layer: Layer, tx: u32, tz: u32) -> Result<Option<Samples>, Failure> {
            let at = |i: usize| (tx * 2 + i as u32 / 2, tz * 2 + i as u32 % 2);
            let off = |(r, c): (u32, u32)| r >= self.grid.height || c >= self.grid.width;
            Ok(Some(Samples::from_fn(layer.dtype, 4, |i| {
                match layer.name {
                    _ if off(at(i)) => layer.nodata,
                    "height" => (at(i).0 * 10 + at(i).1) as i32,
                    "surface" => 1,
                    _ => 0,
                }
            })))
        }
    }

    /// Rows run with x and columns with z, so a grid that is not square
    /// catches the two swapped.
    fn small(height: u32, width: u32) -> Small {
        Small {
            grid: Grid {
                cell_size: 50,
                origin_x: -1000.0,
                origin_z: 2000.0,
                height,
                width,
                tile_size: 2,
            },
        }
    }

    #[test]
    fn height_is_bilinear_between_cell_centres() {
        let s = small(3, 5);
        let h = l("height");
        let at = |n: f64, e: f64| sample(&s, h, -1000.0 + n * 50.0, 2000.0 + e * 50.0).unwrap();
        // At a centre, the cell; between four, across the tile corner at
        // (2, 2), the weighted mean.
        assert_eq!(at(0.5, 0.5), Some(0.0));
        assert_eq!(at(2.5, 1.5), Some(21.0));
        assert_eq!(at(1.5, 4.5), Some(14.0));
        assert_eq!(at(1.0, 1.0), Some(5.5));
        assert_eq!(at(2.0, 2.0), Some(16.5));
        let v = at(1.75, 2.25).unwrap();
        assert!((v - (12.5 + 1.75)).abs() < 1e-9, "{v}");
        // Within half a cell of the edge the edge cell stands beyond it.
        assert_eq!(at(0.1, 1.5), Some(1.0));
        assert_eq!(at(0.01, 0.01), Some(0.0));
        assert_eq!(at(2.99, 4.99), Some(24.0));
        assert_eq!(at(2.9, 1.0), Some(20.5));
        assert_eq!(at(1.0, 4.9), Some(9.0));
        // Off the grid, the far edges open.
        assert_eq!(at(-0.01, 1.0), None);
        assert_eq!(at(1.0, -0.01), None);
        assert_eq!(at(3.0, 1.0), None);
        assert_eq!(at(1.0, 5.0), None);
        assert_eq!(sample(&s, h, f64::NAN, 2000.0).unwrap(), None);
        assert_eq!(sample(&s, h, f64::INFINITY, 2000.0).unwrap(), None);
        // Every other layer is the cell holding the point.
        let surface = sample(&s, l("surface"), -1000.0 + 75.0, 2000.0 + 75.0).unwrap();
        assert_eq!(surface, Some(1.0));
        // One cell has no neighbour to weigh, and stands for the whole cell.
        let one = small(1, 1);
        for (n, e) in [(0.0, 0.0), (0.5, 0.5), (0.99, 0.2)] {
            let v = sample(&one, h, -1000.0 + n * 50.0, 2000.0 + e * 50.0).unwrap();
            assert_eq!(v, Some(0.0), "({n}, {e})");
        }
    }

    #[test]
    fn a_window_on_a_grid_that_is_not_square() {
        let s = small(3, 5);
        let w = Window::read(&s, l("height"), -1, -1, 5, 7).unwrap();
        for r in -1..4i64 {
            for c in -1..6i64 {
                let on = (0..3).contains(&r) && (0..5).contains(&c);
                let want = if on { (r * 10 + c) as i32 } else { -32768 };
                assert_eq!(w.get(r, c), Some(want), "({r}, {c})");
            }
        }
        // A window so far out its end would overflow reads nothing.
        let far = Window::read(&s, l("height"), i64::MAX - 1, 0, 4, 4).unwrap();
        assert!(far.data.iter().all(|&v| v == -32768));
    }

    #[test]
    fn samples_the_synthetic_extract() {
        let (_s, x) = synth_extract();
        let src = ExtractTiles::new(&x);
        let t = theatre();
        let g = x.manifest.grid;
        let h = |r: u32, c: u32| f64::from(t.cell(r, c).height);
        let at = |layer: &str, r: f64, c: f64| {
            let (x0, z0) = (g.origin_x, g.origin_z);
            sample(&src, l(layer), x0 + r * 50.0, z0 + c * 50.0).unwrap()
        };
        // Across the tile corner at (256, 256), a quarter and three quarters
        // of the way between centres.
        let want = 0.75 * 0.25 * h(255, 255)
            + 0.75 * 0.75 * h(255, 256)
            + 0.25 * 0.25 * h(256, 255)
            + 0.25 * 0.75 * h(256, 256);
        let got = at("height", 255.75, 256.25).unwrap();
        assert!((got - want).abs() < 1e-9, "{got} against {want}");
        assert_eq!(at("height", 300.5, 1300.5), Some(h(300, 1300)));

        // The all-sea tile (1, 1), rebuilt.
        assert_eq!(at("height", 300.0, 300.0), Some(0.0));
        assert_eq!(at("surface", 300.0, 300.0), Some(3.0));
        assert_eq!(at("water", 300.0, 300.0), Some(2.0));

        // Row 39 is the last outside the mesh: nodata carries weight
        // between it and row 40, and none at row 40's centre.
        assert_eq!(t.cell(39, 700).height, synth::HEIGHT_NODATA);
        assert_ne!(t.cell(40, 700).height, synth::HEIGHT_NODATA);
        assert_eq!(at("height", 40.0, 700.5), None);
        assert_eq!(at("height", 40.5, 700.5), Some(h(40, 700)));
        assert_eq!(at("water", 39.5, 700.5), None);

        // A count, nearest: inside the forest, and an absent tile's 0.
        let trees = f64::from(t.cell(810, 1110).trees);
        assert!(trees > 0.0);
        assert_eq!(at("trees", 810.9, 1110.1), Some(trees));
        assert_eq!(at("trees", 100.5, 100.5), Some(0.0));
    }

    /// One 2 by 2 tile at (0, 0) of whichever layers are given.
    struct Sparse(Vec<(&'static str, Samples)>, Grid);

    impl Tiles for Sparse {
        fn grid(&self) -> &Grid {
            &self.1
        }

        fn stored(&self, layer: Layer, tx: u32, tz: u32) -> Result<Option<Samples>, Failure> {
            let here = (tx, tz) == (0, 0);
            let found = self.0.iter().find(|(n, _)| here && *n == layer.name);
            Ok(found.map(|(_, s)| s.clone()))
        }
    }

    #[test]
    fn each_absent_rule_stands_on_the_water_tile_alone() {
        let grid = small(2, 2).grid;
        let water = |v: [u8; 4]| ("water", Samples::U8(v.to_vec()));
        // A water tile with no covered cell is never written, but read one
        // stands for no sea and no count.
        let none = Sparse(vec![water([255; 4])], grid);
        assert!(tile(&none, l("height"), 0, 0).is_err());
        assert!(tile(&none, l("surface"), 0, 0).is_err());
        let trees = tile(&none, l("trees"), 0, 0).unwrap();
        assert_eq!(trees, Samples::U16(vec![65535; 4]));
        // Sea beside a cell off the mesh is not all sea.
        let edge = Sparse(vec![water([2, 2, 2, 255])], grid);
        assert!(tile(&edge, l("height"), 0, 0).is_err());
        let lines = tile(&edge, l("lines"), 0, 0).unwrap();
        assert_eq!(lines, Samples::U16(vec![0, 0, 0, 65535]));
        // A written tile is read as written, whatever the water tile says.
        let height = ("height", Samples::I16(vec![7; 4]));
        let alone = Sparse(vec![height], grid);
        assert_eq!(
            tile(&alone, l("height"), 0, 0).unwrap(),
            Samples::I16(vec![7; 4])
        );
        assert_eq!(
            tile(&alone, l("water"), 0, 0).unwrap(),
            Samples::U8(vec![255; 4])
        );
    }
}
