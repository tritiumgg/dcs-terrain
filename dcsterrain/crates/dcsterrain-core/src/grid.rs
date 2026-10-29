//! Tile addressing over the cell grid, and windows of cells read across tiles
//! from a `Source`.
//!
//! Rows run north with DCS x and columns east with DCS z. Cell `(row, col)`
//! covers `[origin + index × cell_size, origin + (index + 1) × cell_size)` on
//! each axis and is sampled at its centre.

use serde::Deserialize;

use crate::source::{Layer, Refusal, Source};

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
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
    /// Tiles along x and along z. The last in each direction may reach past
    /// the grid, its cells there `nodata`.
    pub fn tiles(&self) -> (u32, u32) {
        (
            self.height.div_ceil(self.tile_size),
            self.width.div_ceil(self.tile_size),
        )
    }

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
        src: &(impl Source + ?Sized),
        layer: Layer,
        row0: i64,
        col0: i64,
        rows: u32,
        cols: u32,
    ) -> Result<Window, Refusal> {
        let g = src.grid();
        let mut data = vec![layer.nodata(); rows as usize * cols as usize];
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
        // Each tile the window touches is read once, all of them together.
        let mut want: Vec<(u32, u32)> = Vec::new();
        for tx in tiles(r_lo, r_hi).take_while(|_| on_grid) {
            for tz in tiles(c_lo, c_hi) {
                want.push((tx as u32, tz as u32));
            }
        }
        src.tiles(layer, &want, &mut |i, t| {
            let (tx, tz) = (i64::from(want[i].0), i64::from(want[i].1));
            if t.len() as i64 != ts * ts {
                return Err(Refusal(format!(
                    "{} tile ({tx}, {tz}) holds {} cells, not {}",
                    layer.name(),
                    t.len(),
                    ts * ts
                )));
            }
            let (r0, r1) = (r_lo.max(tx * ts), r_hi.min((tx + 1) * ts));
            let (c0, c1) = (c_lo.max(tz * ts), c_hi.min((tz + 1) * ts));
            for r in r0..r1 {
                for c in c0..c1 {
                    let from = (r - tx * ts) * ts + (c - tz * ts);
                    let to = (r - row0) * i64::from(cols) + (c - col0);
                    data[to as usize] = t[from as usize];
                }
            }
            Ok(())
        })?;
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

    /// As `sample`, from this window, which must hold the cells around
    /// `(x, z)`: `None` off the grid, where a cell that carries weight lies
    /// outside the window, or where it is `nodata`.
    pub fn value_at(&self, g: &Grid, x: f64, z: f64) -> Option<f64> {
        let (row, col) = g.cell_at(x, z)?;
        let l = self.layer;
        let present = |v: i32| (v != l.nodata()).then_some(f64::from(v));
        if l != Layer::Height {
            return present(self.get(row.into(), col.into())?);
        }
        let c = f64::from(g.cell_size);
        let axis = |v: f64, origin: f64, n: u32| {
            let f = ((v - origin) / c - 0.5).clamp(0.0, f64::from(n - 1));
            let i = f.floor();
            (i as i64, f - i)
        };
        let (r0, tr) = axis(x, g.origin_x, g.height);
        let (c0, tc) = axis(z, g.origin_z, g.width);
        let mut sum = 0.0;
        for (dr, dc, weight) in [
            (0, 0, (1.0 - tr) * (1.0 - tc)),
            (0, 1, (1.0 - tr) * tc),
            (1, 0, tr * (1.0 - tc)),
            (1, 1, tr * tc),
        ] {
            if weight == 0.0 {
                continue;
            }
            sum += weight * present(self.get(r0 + dr, c0 + dc)?)?;
        }
        Some(sum)
    }
}

/// A layer's value at `(x, z)`, or `None` off the grid or where it is
/// `nodata`.
///
/// `height` is bilinear between the four cell centres around the point, and
/// `None` when any of them that carries weight is `nodata`, so a slope is
/// never drawn down to the nodata value. Within half a cell of the grid's
/// edge the edge cell stands for the one beyond it. Every other layer is a
/// code, which has no value between two cells, so it reads the cell holding
/// the point.
pub fn sample(
    src: &(impl Source + ?Sized),
    layer: Layer,
    x: f64,
    z: f64,
) -> Result<Option<f64>, Refusal> {
    let g = src.grid();
    let Some((row, col)) = g.cell_at(x, z) else {
        return Ok(None);
    };
    let present = |v: i32| (v != layer.nodata()).then_some(f64::from(v));
    if layer != Layer::Height {
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

/// How far `p` lies outside the box `[min_x, min_z, max_x, max_z]`, 0
/// within it.
pub fn box_distance([x0, z0, x1, z1]: [f64; 4], p: [f64; 2]) -> f64 {
    let dx = (x0 - p[0]).max(p[0] - x1).max(0.0);
    let dz = (z0 - p[1]).max(p[1] - z1).max(0.0);
    dx.hypot(dz)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::{Cover, Ground, Identity, TILE_SIZE};
    use crate::synth::{self, Synth};

    #[test]
    fn measures_how_far_a_point_lies_outside_a_box() {
        let b = [0.0, 0.0, 10.0, 20.0];
        assert_eq!(box_distance(b, [5.0, 5.0]), 0.0);
        assert_eq!(box_distance(b, [10.0, 20.0]), 0.0);
        assert_eq!(box_distance(b, [-3.0, 5.0]), 3.0);
        assert_eq!(box_distance(b, [5.0, 24.0]), 4.0);
        assert_eq!(box_distance(b, [13.0, 24.0]), 5.0);
    }

    fn synth() -> Synth {
        Synth::new(
            synth::DEFAULT_SEED,
            synth::DEFAULT_SIZE_KM,
            synth::CELL_SIZE_M,
        )
        .unwrap()
    }

    fn cells(w: &Window) -> impl Iterator<Item = (i64, i64)> + '_ {
        (0..i64::from(w.rows))
            .flat_map(move |r| (0..i64::from(w.cols)).map(move |c| (w.row0 + r, w.col0 + c)))
    }

    /// Every layer's window at `(row0, col0)`, `n` cells a side, against the
    /// closed form cell by cell.
    fn assert_window(s: &Synth, row0: i64, col0: i64, n: u32) {
        for layer in Layer::ALL {
            let w = Window::read(s, layer, row0, col0, n, n).unwrap();
            assert_eq!(w.data.len(), (n * n) as usize);
            for (r, c) in cells(&w) {
                assert_eq!(
                    w.get(r, c),
                    Some(s.cell(layer, r, c)),
                    "{} window at ({row0}, {col0}), cell ({r}, {c})",
                    layer.name()
                );
            }
        }
    }

    #[test]
    fn addresses_cells_and_tiles() {
        let g = synth().grid();
        let (ox, oz, c) = (g.origin_x, g.origin_z, f64::from(g.cell_size));
        let (h, w) = (f64::from(g.height), f64::from(g.width));
        assert_eq!(g.tile_size, TILE_SIZE);
        assert_eq!(g.tiles(), (22, 22));
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
        assert_eq!(g.tile_of(63, 63), (0, 0, 64 * 64 - 1));
        assert_eq!(g.tile_of(64, 63), (1, 0, 63));
        assert_eq!(g.tile_of(63, 64), (0, 1, 63 * 64));
        assert_eq!(g.tile_of(300, 1300), (4, 20, 44 * 64 + 20));
    }

    #[test]
    fn a_window_reads_across_every_tile_edge_and_corner() {
        let s = synth();
        // The grid is 1400 cells a side in 22 tiles; the inner tile
        // boundaries are at multiples of 64 up to 1344.
        for k in 1..=21i64 {
            // Along one boundary, away from the other.
            assert_window(&s, k * 64 - 2, 700, 4);
            assert_window(&s, 700, k * 64 - 2, 4);
        }
        for k in [1, 2, 10, 20, 21i64] {
            for m in [1, 2, 10, 20, 21i64] {
                assert_window(&s, k * 64 - 2, m * 64 - 2, 4);
            }
        }
    }

    #[test]
    fn a_window_reads_past_every_grid_edge_and_corner_as_nodata() {
        let s = synth();
        let n = i64::from(s.grid().height);
        assert_eq!(n, 1400);
        // Four edges, four corners: the window is half off the grid. The
        // north and east ones reach rows and columns 1400 to 1407, which the
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
            (1406, 1406),
        ] {
            assert_window(&s, r, c, 4);
        }
        for (r, c) in [(-10, -10), (-10, 700), (2000, 2000), (700, 1400)] {
            for layer in Layer::ALL {
                let w = Window::read(&s, layer, r, c, 5, 5).unwrap();
                assert!(w.data.iter().all(|&v| v == layer.nodata()), "({r}, {c})");
            }
        }
        let empty = Window::read(&s, Layer::Height, 10, 10, 0, 3).unwrap();
        assert!(empty.data.is_empty());
        assert_eq!(empty.get(10, 10), None);
        // A window so far out its end would overflow reads nothing.
        let far = Window::read(&s, Layer::Height, i64::MAX - 1, 0, 4, 4).unwrap();
        assert!(far.data.iter().all(|&v| v == -32768));
    }

    #[test]
    fn a_window_over_the_whole_grid_matches_the_closed_form() {
        let s = synth();
        let n = s.grid().height + 2;
        for layer in Layer::ALL {
            let w = Window::read(&s, layer, -1, -1, n, n).unwrap();
            for (r, c) in cells(&w) {
                assert_eq!(
                    w.get(r, c),
                    Some(s.cell(layer, r, c)),
                    "{} ({r}, {c})",
                    layer.name()
                );
            }
        }
    }

    #[test]
    fn samples_the_synthetic_terrain() {
        let s = synth();
        let g = s.grid();
        let h = |r: i64, c: i64| f64::from(s.cell(Layer::Height, r, c));
        let at = |layer: Layer, r: f64, c: f64| {
            sample(&s, layer, g.origin_x + r * 50.0, g.origin_z + c * 50.0).unwrap()
        };
        // Across the tile corner at (704, 704), a quarter and three quarters
        // of the way between centres.
        let want = 0.75 * 0.25 * h(703, 703)
            + 0.75 * 0.75 * h(703, 704)
            + 0.25 * 0.25 * h(704, 703)
            + 0.25 * 0.75 * h(704, 704);
        let got = at(Layer::Height, 703.75, 704.25).unwrap();
        assert!((got - want).abs() < 1e-9, "{got} against {want}");
        assert_eq!(at(Layer::Height, 300.5, 1300.5), Some(h(300, 1300)));
        // The plane rises north and not east, so rows and columns swapped
        // would read another height.
        assert_ne!(h(1000, 1300), h(1300, 1000));
        assert_eq!(at(Layer::Height, 1000.5, 1300.5), Some(h(1000, 1300)));

        // The sea, at sea level.
        assert_eq!(at(Layer::Height, 300.0, 300.0), Some(0.0));
        assert_eq!(at(Layer::Surface, 300.0, 300.0), Some(3.0));
        assert_eq!(at(Layer::Water, 300.0, 300.0), Some(2.0));
        let water = f64::from(Cover::Water.code());
        assert_eq!(at(Layer::Cover, 300.0, 300.0), Some(water));

        // Row 39 is the last outside the mesh: nodata carries weight
        // between it and row 40, and none at row 40's centre.
        assert_eq!(s.cell(Layer::Height, 39, 700), -32768);
        assert_ne!(s.cell(Layer::Height, 40, 700), -32768);
        assert_eq!(at(Layer::Height, 40.0, 700.5), None);
        assert_eq!(at(Layer::Height, 40.5, 700.5), Some(h(40, 700)));
        assert_eq!(at(Layer::Water, 39.5, 700.5), None);
        // Within half a cell of the grid's edge the edge cell, outside the
        // mesh, stands for the one beyond it.
        assert_eq!(at(Layer::Height, 0.1, 700.5), None);

        // A code reads the cell holding the point.
        let forest = f64::from(Cover::Forest.code());
        assert_eq!(at(Layer::Cover, 810.9, 1110.1), Some(forest));
    }

    /// A grid in tiles of 2, height `10 × row + col`, all land, for the
    /// interpolation's arithmetic to come out in round numbers on a grid that
    /// is not square and that ends in land.
    struct Small {
        identity: Identity,
        grid: Grid,
    }

    impl Source for Small {
        fn identity(&self) -> &Identity {
            &self.identity
        }

        fn grid(&self) -> Grid {
            self.grid
        }

        fn tile(&self, layer: Layer, tx: u32, tz: u32) -> Result<Vec<i32>, Refusal> {
            let at = |i: u32| (tx * 2 + i / 2, tz * 2 + i % 2);
            let off = |(r, c): (u32, u32)| r >= self.grid.height || c >= self.grid.width;
            Ok((0..4)
                .map(|i| match layer {
                    _ if off(at(i)) => layer.nodata(),
                    Layer::Height => (at(i).0 * 10 + at(i).1) as i32,
                    Layer::Cover => Cover::Plain.code(),
                    Layer::Surface => 1,
                    Layer::Water => 0,
                })
                .collect())
        }

        fn point(&self, _: f64, _: f64) -> Result<Option<Ground>, Refusal> {
            Ok(None)
        }
    }

    fn small(height: u32, width: u32) -> Small {
        Small {
            identity: Identity {
                terrain: "small".into(),
                dcs_build: "0".into(),
                fingerprint: "0".into(),
            },
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
        let h = Layer::Height;
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
        // One cell has no neighbour to weigh, and stands for the whole cell.
        let one = small(1, 1);
        for (n, e) in [(0.0, 0.0), (0.5, 0.5), (0.99, 0.2)] {
            let v = sample(&one, h, -1000.0 + n * 50.0, 2000.0 + e * 50.0).unwrap();
            assert_eq!(v, Some(0.0), "({n}, {e})");
        }
    }

    #[test]
    fn refuses_a_tile_of_the_wrong_size() {
        struct Short(Small);
        impl Source for Short {
            fn identity(&self) -> &Identity {
                self.0.identity()
            }
            fn grid(&self) -> Grid {
                self.0.grid()
            }
            fn tile(&self, _: Layer, _: u32, _: u32) -> Result<Vec<i32>, Refusal> {
                Ok(vec![0; 3])
            }
            fn point(&self, x: f64, z: f64) -> Result<Option<Ground>, Refusal> {
                self.0.point(x, z)
            }
        }
        let e = Window::read(&Short(small(2, 2)), Layer::Water, 0, 0, 2, 2).unwrap_err();
        assert!(e.0.contains("holds 3 cells, not 4"), "{e}");
    }
}
