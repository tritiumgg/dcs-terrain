//! The layers a query derives from the ground and the scenery, computed
//! over a rectangle of cells from a window padded by each layer's reach,
//! and never stored.
//!
//! Each reads every tile of its window once, so a layer over a region costs
//! one pass however many cells it holds.

use crate::grid::{Grid, Window};
use crate::source::{Cover, Layer, Refusal, Source};

/// Under this slope a cell has no aspect.
pub const FLAT_DEG: f64 = 0.5;

pub const TPI_300: Derived = Derived::Tpi { radius: 300.0 };
pub const TPI_2000: Derived = Derived::Tpi { radius: 2000.0 };

/// One derived layer, with what it is measured within.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Derived {
    /// 1 where the height is known and the cell is not sea, else 0.
    Valid,
    /// Degrees, from Horn's 3 × 3 gradient.
    Slope,
    /// The downslope azimuth in degrees from north, clockwise; undefined
    /// under `FLAT_DEG`.
    Aspect,
    /// Meters between the highest and lowest cell of the 3 × 3.
    Roughness,
    /// The cell's height less the mean height of the cells whose centres
    /// lie within `radius` meters of its own.
    Tpi { radius: f64 },
    /// The share of the covered cells within `radius` that are of `cover`.
    CoverFraction { cover: Cover, radius: f64 },
}

/// A rectangle of grid cells; it may reach past the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cells {
    pub row0: i64,
    pub col0: i64,
    pub rows: u32,
    pub cols: u32,
}

impl Cells {
    /// The cells holding the box `[min_x, min_z, max_x, max_z]` in DCS
    /// meters.
    pub fn covering(g: &Grid, b: [f64; 4]) -> Cells {
        let c = f64::from(g.cell_size);
        let row0 = ((b[0] - g.origin_x) / c).floor() as i64;
        let col0 = ((b[1] - g.origin_z) / c).floor() as i64;
        let row1 = ((b[2] - g.origin_x) / c).floor() as i64;
        let col1 = ((b[3] - g.origin_z) / c).floor() as i64;
        Cells {
            row0,
            col0,
            rows: (row1 - row0 + 1).max(0) as u32,
            cols: (col1 - col0 + 1).max(0) as u32,
        }
    }

    fn pad(self, n: u32) -> Cells {
        Cells {
            row0: self.row0 - i64::from(n),
            col0: self.col0 - i64::from(n),
            rows: self.rows + 2 * n,
            cols: self.cols + 2 * n,
        }
    }

    fn len(self) -> usize {
        self.rows as usize * self.cols as usize
    }

    fn read(self, src: &(impl Source + ?Sized), layer: Layer) -> Result<Window, Refusal> {
        Window::read(src, layer, self.row0, self.col0, self.rows, self.cols)
    }
}

/// A derived layer over a rectangle of cells, row-major with columns
/// fastest; NaN where it is undefined.
#[derive(Debug, Clone, PartialEq)]
pub struct Raster {
    pub cells: Cells,
    pub values: Vec<f32>,
}

impl Raster {
    /// The value at a grid row and column, or `None` outside the raster.
    pub fn get(&self, row: i64, col: i64) -> Option<f32> {
        let (r, c) = (row - self.cells.row0, col - self.cells.col0);
        let inside = (0..i64::from(self.cells.rows)).contains(&r)
            && (0..i64::from(self.cells.cols)).contains(&c);
        inside.then(|| self.values[(r * i64::from(self.cells.cols) + c) as usize])
    }
}

/// Computes `d` over the cells `at`.
pub fn derive(src: &(impl Source + ?Sized), d: Derived, at: Cells) -> Result<Raster, Refusal> {
    let g = src.grid();
    let reach = match d {
        Derived::Tpi { radius } | Derived::CoverFraction { radius, .. } => radius,
        _ => 0.0,
    };
    if !(reach.is_finite() && reach >= 0.0) {
        return Err(Refusal(format!(
            "a radius must be a finite number of meters, not {reach}"
        )));
    }
    let raster = |values| Raster { cells: at, values };
    Ok(match d {
        Derived::Valid => raster(valid(src, at)?),
        Derived::Slope | Derived::Aspect | Derived::Roughness => raster(horn(src, d, at)?),
        Derived::Tpi { radius } => raster(tpi(src, &Disk::new(radius, &g), at)?),
        Derived::CoverFraction { cover, radius } => {
            let disk = Disk::new(radius, &g);
            let w = at.pad(disk.reach).read(src, Layer::Cover)?;
            let is: Vec<f64> = w
                .data
                .iter()
                .map(|&v| f64::from(v == cover.code()))
                .collect();
            let covered: Vec<bool> = w.data.iter().map(|&v| v != Layer::Cover.nodata()).collect();
            let (sum, count) = disk.sums(&is, Some(&covered), at.pad(disk.reach), disk.reach);
            let share = sum.iter().zip(&count).map(|(&s, &n)| match n {
                0 => f32::NAN,
                n => (s / f64::from(n)) as f32,
            });
            raster(share.collect())
        }
    })
}

fn valid(src: &(impl Source + ?Sized), at: Cells) -> Result<Vec<f32>, Refusal> {
    const SEA: i32 = 2;
    let h = at.read(src, Layer::Height)?;
    let w = at.read(src, Layer::Water)?;
    Ok(h.data
        .iter()
        .zip(&w.data)
        .map(|(&h, &w)| f32::from(u8::from(h != Layer::Height.nodata() && w != SEA)))
        .collect())
}

/// Slope, aspect or roughness from each cell's 3 × 3, undefined where any
/// of the nine has no height.
fn horn(src: &(impl Source + ?Sized), d: Derived, at: Cells) -> Result<Vec<f32>, Refusal> {
    let w = at.pad(1).read(src, Layer::Height)?;
    let cs = f64::from(src.grid().cell_size);
    let wc = w.cols as usize;
    let mut out = Vec::with_capacity(at.len());
    for r in 1..=at.rows as usize {
        for c in 1..=at.cols as usize {
            let mut z = [0.0f64; 9];
            let mut known = true;
            for (k, v) in z.iter_mut().enumerate() {
                let h = w.data[(r + k / 3 - 1) * wc + c + k % 3 - 1];
                known &= h != Layer::Height.nodata();
                *v = f64::from(h);
            }
            if !known {
                out.push(f32::NAN);
                continue;
            }
            // z[0..3] is the row to the south, z[6..9] the row to the north.
            let north = ((z[6] + 2.0 * z[7] + z[8]) - (z[0] + 2.0 * z[1] + z[2])) / (8.0 * cs);
            let east = ((z[2] + 2.0 * z[5] + z[8]) - (z[0] + 2.0 * z[3] + z[6])) / (8.0 * cs);
            let slope = north.hypot(east).atan().to_degrees();
            let v = match d {
                Derived::Slope => slope,
                Derived::Aspect if slope < FLAT_DEG => f64::NAN,
                Derived::Aspect => (-east).atan2(-north).to_degrees().rem_euclid(360.0),
                _ => {
                    let (lo, hi) = z
                        .iter()
                        .fold((f64::MAX, f64::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v)));
                    hi - lo
                }
            };
            out.push(v as f32);
        }
    }
    Ok(out)
}

fn tpi(src: &(impl Source + ?Sized), disk: &Disk, at: Cells) -> Result<Vec<f32>, Refusal> {
    let win = at.pad(disk.reach);
    let w = win.read(src, Layer::Height)?;
    let known: Vec<bool> = w
        .data
        .iter()
        .map(|&h| h != Layer::Height.nodata())
        .collect();
    let h: Vec<f64> = w
        .data
        .iter()
        .zip(&known)
        .map(|(&h, &k)| if k { f64::from(h) } else { 0.0 })
        .collect();
    let (sum, count) = disk.sums(&h, Some(&known), win, disk.reach);
    let wc = win.cols as usize;
    let p = disk.reach as usize;
    let mut out = Vec::with_capacity(at.len());
    for r in 0..at.rows as usize {
        for c in 0..at.cols as usize {
            let i = (r + p) * wc + c + p;
            let o = r * at.cols as usize + c;
            out.push(if known[i] {
                (h[i] - sum[o] / f64::from(count[o])) as f32
            } else {
                f32::NAN
            });
        }
    }
    Ok(out)
}

/// The cells whose centres lie within a radius of a cell's centre, as one
/// run of columns on each row.
struct Disk {
    /// Cells from the centre to the farthest row.
    reach: u32,
    /// Per row offset from `-reach` to `reach`, the columns either side.
    half: Vec<i64>,
}

impl Disk {
    fn new(radius: f64, g: &Grid) -> Disk {
        let c = f64::from(g.cell_size);
        let reach = (radius / c).floor() as i64;
        // Whole cells, so the test is exact in integers.
        let r2 = radius * radius;
        let half = (-reach..=reach)
            .map(|dr| {
                let mut h = reach;
                while h > 0 && ((dr * dr + h * h) as f64) * c * c > r2 {
                    h -= 1;
                }
                h
            })
            .collect();
        Disk {
            reach: reach as u32,
            half,
        }
    }

    /// For each cell of the window `win` lying `pad` cells inside it, the sum
    /// of `v` over the disk around it and, given `known`, how many of those
    /// cells are known; `v` is laid out over `win`. Row prefix sums, so a
    /// cell costs one subtraction per row of the disk.
    fn sums(
        &self,
        v: &[f64],
        known: Option<&[bool]>,
        win: Cells,
        pad: u32,
    ) -> (Vec<f64>, Vec<u32>) {
        let (wr, wc) = (win.rows as usize, win.cols as usize);
        let prefix = |f: &dyn Fn(usize) -> f64| {
            let mut p = vec![0.0; wr * (wc + 1)];
            for r in 0..wr {
                for c in 0..wc {
                    p[r * (wc + 1) + c + 1] = p[r * (wc + 1) + c] + f(r * wc + c);
                }
            }
            p
        };
        let pv = prefix(&|i| v[i]);
        let pk = known.map(|k| prefix(&|i| f64::from(u8::from(k[i]))));
        let (rows, cols) = (wr - 2 * pad as usize, wc - 2 * pad as usize);
        let mut sum = Vec::with_capacity(rows * cols);
        let mut count = Vec::with_capacity(rows * cols);
        let reach = self.reach as i64;
        for r in 0..rows {
            for c in 0..cols {
                let (cr, cc) = ((r + pad as usize) as i64, (c + pad as usize) as i64);
                let (mut s, mut n) = (0.0, 0.0);
                for (k, &h) in self.half.iter().enumerate() {
                    let row = (cr + k as i64 - reach) as usize * (wc + 1);
                    let (lo, hi) = (row + (cc - h) as usize, row + (cc + h + 1) as usize);
                    s += pv[hi] - pv[lo];
                    if let Some(pk) = &pk {
                        n += pk[hi] - pk[lo];
                    }
                }
                sum.push(s);
                count.push(n as u32);
            }
        }
        (sum, count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth::*;

    fn synth() -> Synth {
        Synth::new(DEFAULT_SEED, DEFAULT_SIZE_KM, CELL_SIZE_M).unwrap()
    }

    const C: f64 = CELL_SIZE_M as f64;

    /// The grid row or column of the cell holding an offset.
    fn at(v: f64) -> i64 {
        (v / C).floor() as i64
    }

    /// `n` cells a side, the cell holding `(north, east)` at its centre.
    fn around(north: f64, east: f64, n: u32) -> Cells {
        let h = i64::from(n / 2);
        Cells {
            row0: at(north) - h,
            col0: at(east) - h,
            rows: n,
            cols: n,
        }
    }

    fn each(c: Cells) -> impl Iterator<Item = (i64, i64)> {
        (0..i64::from(c.rows))
            .flat_map(move |r| (0..i64::from(c.cols)).map(move |k| (c.row0 + r, c.col0 + k)))
    }

    /// The offsets of the cells whose centres lie within `radius` of a
    /// cell's, tested one by one.
    fn disk(radius: f64) -> Vec<(i64, i64)> {
        let reach = (radius / C) as i64 + 1;
        (-reach..=reach)
            .flat_map(|dr| (-reach..=reach).map(move |dc| (dr, dc)))
            .filter(|&(dr, dc)| ((dr * dr + dc * dc) as f64) * C * C <= radius * radius)
            .collect()
    }

    fn one(s: &(impl Source + ?Sized), d: Derived, north: f64, east: f64) -> f32 {
        derive(s, d, around(north, east, 1)).unwrap().values[0]
    }

    fn close(got: f32, want: f64, tol: f64, what: &str) {
        assert!(
            (f64::from(got) - want).abs() <= tol,
            "{what}: {got}, not {want} within {tol}"
        );
    }

    #[test]
    fn the_plane_has_its_exact_slope_aspect_and_roughness() {
        let s = synth();
        let cells = around(19000.0, 62000.0, 20);
        let slope = derive(&s, Derived::Slope, cells).unwrap();
        let aspect = derive(&s, Derived::Aspect, cells).unwrap();
        let rough = derive(&s, Derived::Roughness, cells).unwrap();
        for i in 0..cells.len() {
            close(
                slope.values[i],
                PLANE_SLOPE.atan().to_degrees(),
                1e-4,
                "slope",
            );
            // The plane rises north, so it falls south.
            close(aspect.values[i], 180.0, 1e-3, "aspect");
            // Two cells north is one meter higher, rounded or not.
            assert_eq!(rough.values[i], 1.0);
        }
    }

    #[test]
    fn slope_and_aspect_on_the_hill_follow_its_gradient() {
        let s = synth();
        let t = s.form();
        let cells = around(HILL_A_NORTH_M, HILL_A_EAST_M, 80);
        let slope = derive(&s, Derived::Slope, cells).unwrap();
        let aspect = derive(&s, Derived::Aspect, cells).unwrap();
        let mut steep = 0;
        for (i, (row, col)) in each(cells).enumerate() {
            let (n, e) = t.centre(row as u32, col as u32);
            let d = 1e-3;
            let gn = (t.ground(n + d, e) - t.ground(n - d, e)) / (2.0 * d);
            let ge = (t.ground(n, e + d) - t.ground(n, e - d)) / (2.0 * d);
            let want = gn.hypot(ge).atan().to_degrees();
            // A rounded meter in the sums is 0.01 of gradient, about 0.6°.
            close(slope.values[i], want, 0.6, "slope");
            if want > 5.0 {
                steep += 1;
                let az = (-ge).atan2(-gn).to_degrees().rem_euclid(360.0);
                let off = (f64::from(aspect.values[i]) - az + 540.0).rem_euclid(360.0) - 180.0;
                // The same rounded meter, turned across the gradient.
                let tol = (0.012 / gn.hypot(ge)).atan().to_degrees() + 0.5;
                assert!(off.abs() < tol, "aspect {} for {az}", aspect.values[i]);
            }
        }
        assert!(steep > 1000);
        // The summit is flat, so it faces nowhere.
        assert!(one(&s, Derived::Aspect, HILL_A_NORTH_M, HILL_A_EAST_M).is_nan());
    }

    #[test]
    fn roughness_is_the_spread_of_each_three_by_three_and_none_at_the_edge() {
        let s = synth();
        // The mesh begins two kilometers in, at row 40.
        let rough = derive(
            &s,
            Derived::Roughness,
            around(MESH_MARGIN_M, HILL_A_EAST_M, 6),
        )
        .unwrap();
        for (r, c) in each(rough.cells) {
            let v = rough.get(r, c).unwrap();
            assert_eq!(v.is_nan(), r <= 40, "row {r}");
        }
        let cells = around(HILL_A_NORTH_M, HILL_A_EAST_M, 40);
        let hill = derive(&s, Derived::Roughness, cells).unwrap();
        for (r, c) in each(cells) {
            let z: Vec<i32> = (-1..=1)
                .flat_map(|dr| (-1..=1).map(move |dc| (dr, dc)))
                .map(|(dr, dc)| s.cell(Layer::Height, r + dr, c + dc))
                .collect();
            let want = z.iter().max().unwrap() - z.iter().min().unwrap();
            assert_eq!(hill.get(r, c).unwrap(), want as f32);
        }
    }

    #[test]
    fn tpi_window_sums_match_brute_force() {
        let s = synth();
        for (d, radius) in [(TPI_300, 300.0), (TPI_2000, 2000.0)] {
            // Over the hill and across the edge of the mesh, whose cells
            // carry no height into a mean.
            for cells in [
                around(HILL_A_NORTH_M, HILL_A_EAST_M, 24),
                around(MESH_MARGIN_M + 100.0, HILL_A_EAST_M, 24),
            ] {
                let got = derive(&s, d, cells).unwrap();
                for (r, c) in each(cells) {
                    let h = s.cell(Layer::Height, r, c);
                    let v = got.get(r, c).unwrap();
                    if h == Layer::Height.nodata() {
                        assert!(v.is_nan());
                        continue;
                    }
                    let known: Vec<f64> = disk(radius)
                        .into_iter()
                        .map(|(dr, dc)| s.cell(Layer::Height, r + dr, c + dc))
                        .filter(|&h| h != Layer::Height.nodata())
                        .map(f64::from)
                        .collect();
                    let mean = known.iter().sum::<f64>() / known.len() as f64;
                    close(v, f64::from(h) - mean, 1e-3, "tpi");
                }
            }
        }
    }

    #[test]
    fn tpi_is_the_closed_form_relief() {
        let s = synth();
        let t = s.form();
        for (d, radius) in [(TPI_300, 300.0), (TPI_2000, 2000.0)] {
            // On the plane alone the disk's mean is its centre's height.
            close(one(&s, d, 19000.0, 62000.0), 0.0, 0.5, "tpi on the plane");
            let (row, col) = (at(HILL_A_NORTH_M), at(HILL_A_EAST_M));
            let (n, e) = t.centre(row as u32, col as u32);
            let ring: Vec<f64> = disk(radius)
                .into_iter()
                .map(|(dr, dc)| t.ground(n + dr as f64 * C, e + dc as f64 * C))
                .collect();
            let want = t.ground(n, e) - ring.iter().sum::<f64>() / ring.len() as f64;
            close(one(&s, d, n, e), want, 0.5, "tpi at the summit");
        }
        assert!(one(&s, TPI_2000, HILL_A_NORTH_M, HILL_A_EAST_M) > 150.0);
    }

    #[test]
    fn valid_is_ground_that_is_not_sea() {
        let s = synth();
        for (n, e, want) in [
            (19000.0, 62000.0, 1.0),
            (LAKE_NORTH_M, LAKE_EAST_M, 1.0),
            (30000.0, RIVER_EAST_M + 10.0, 1.0),
            (5000.0, 5000.0, 0.0),
            (1000.0, 30000.0, 0.0),
        ] {
            assert_eq!(one(&s, Derived::Valid, n, e), want, "({n}, {e})");
        }
    }

    #[test]
    fn cover_fractions_match_brute_force() {
        let s = synth();
        let forest = |radius| Derived::CoverFraction {
            cover: Cover::Forest,
            radius,
        };
        close(one(&s, forest(300.0), 40500.0, 55500.0), 1.0, 0.0, "inside");
        close(
            one(&s, forest(300.0), 19000.0, 62000.0),
            0.0,
            0.0,
            "outside",
        );
        assert!(one(&s, forest(300.0), 500.0, 500.0).is_nan());
        let edge = around(FOREST_SOUTH_M, FOREST_WEST_M, 30);
        let margin = around(MESH_MARGIN_M, 60000.0, 30);
        for (cells, cover) in [(edge, Cover::Forest), (margin, Cover::Plain)] {
            let d = Derived::CoverFraction {
                cover,
                radius: 300.0,
            };
            let got = derive(&s, d, cells).unwrap();
            for (r, c) in each(cells) {
                let near: Vec<i32> = disk(300.0)
                    .into_iter()
                    .map(|(dr, dc)| s.cell(Layer::Cover, r + dr, c + dc))
                    .filter(|&v| v != Layer::Cover.nodata())
                    .collect();
                let v = got.get(r, c).unwrap();
                if near.is_empty() {
                    assert!(v.is_nan());
                    continue;
                }
                let n = near.iter().filter(|&&v| v == cover.code()).count();
                close(v, n as f64 / near.len() as f64, 1e-6, "share");
            }
        }
    }

    #[test]
    fn refuses_a_radius_that_is_not_a_length() {
        let s = synth();
        let cells = around(19000.0, 62000.0, 2);
        for radius in [f64::NAN, -1.0, f64::INFINITY] {
            assert!(derive(&s, Derived::Tpi { radius }, cells).is_err());
        }
    }

    #[test]
    fn covering_a_box_takes_every_cell_it_touches() {
        let s = synth();
        let (x, z) = s.form().abs(1000.0, 2000.0);
        let c = Cells::covering(&s.grid(), [x, z, x + 120.0, z + 10.0]);
        assert_eq!((c.row0, c.col0, c.rows, c.cols), (20, 40, 3, 1));
    }
}
