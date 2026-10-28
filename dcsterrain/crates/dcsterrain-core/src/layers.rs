//! The layers a query derives from the ground and the scenery, computed
//! over a rectangle of cells from a window padded by each layer's reach,
//! and never stored.
//!
//! Each reads every tile of its window once, and the scenery and roads of
//! the window's box once, so a layer over a region costs one pass however
//! many cells it holds.

use crate::classes::Class;
use crate::decode::rn4::{KIND_RAIL, KIND_ROAD};
use crate::decode::scn5::Pool;
use crate::grid::{Grid, Window};
use crate::source::{Cover, Layer, Refusal, Source};

/// How far a distance searches when the asker names no maximum.
pub const DISTANCE_CAP_M: f64 = 5000.0;
/// Under this slope a cell has no aspect.
pub const FLAT_DEG: f64 = 0.5;
/// A cell is built up when this many buildings stand within
/// `BUILTUP_RADIUS_M` of it.
pub const BUILTUP_MIN: u32 = 3;
pub const BUILTUP_RADIUS_M: f64 = 100.0;
/// The longest radius or cap a layer takes: its window grows with the square.
pub const MAX_REACH_M: f64 = 50_000.0;

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
    /// The cell's height less the mean height of the cells whose centers
    /// lie within `radius` meters of its own.
    Tpi { radius: f64 },
    /// Meters to the nearest cell of the target, center to center, and
    /// `cap` where none lies within `cap`.
    Distance { to: Target, cap: f64 },
    /// Objects of the class in the cells within `radius`.
    Count { class: Class, radius: f64 },
    /// Instances of the trees or lines pool per hectare of the cells within
    /// `radius`, every one of them counted in the area, on the grid or not.
    Density { pool: Pool, radius: f64 },
    /// The share of the covered cells within `radius` that are of `cover`.
    CoverFraction { cover: Cover, radius: f64 },
}

/// What a distance is measured to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// Any cell a road edge passes through.
    Road,
    /// Any cell a railway edge passes through.
    Rail,
    /// Any lake, sea or river cell.
    Water,
    /// Any built-up cell: `BUILTUP_MIN` buildings within `BUILTUP_RADIUS_M`.
    Builtup,
    Cover(Cover),
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

    /// The box the cells cover, in DCS meters.
    fn bounds(self, g: &Grid) -> [f64; 4] {
        let c = f64::from(g.cell_size);
        [
            g.origin_x + self.row0 as f64 * c,
            g.origin_z + self.col0 as f64 * c,
            g.origin_x + (self.row0 + i64::from(self.rows)) as f64 * c,
            g.origin_z + (self.col0 + i64::from(self.cols)) as f64 * c,
        ]
    }

    /// The index of the cell holding `(x, z)`, or `None` outside.
    fn index(self, g: &Grid, x: f64, z: f64) -> Option<usize> {
        let c = f64::from(g.cell_size);
        let r = ((x - g.origin_x) / c).floor() - self.row0 as f64;
        let k = ((z - g.origin_z) / c).floor() - self.col0 as f64;
        let inside = |v: f64, n: u32| v >= 0.0 && v < f64::from(n);
        (inside(r, self.rows) && inside(k, self.cols))
            .then(|| r as usize * self.cols as usize + k as usize)
    }
}

/// A derived layer over a rectangle of cells, row-major with columns
/// fastest; NaN where it is undefined.
#[derive(Debug, Clone, PartialEq)]
pub struct Raster {
    pub cells: Cells,
    pub values: Vec<f32>,
    /// Whether any distance read as its cap.
    pub capped: bool,
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
        Derived::Tpi { radius }
        | Derived::Count { radius, .. }
        | Derived::Density { radius, .. }
        | Derived::CoverFraction { radius, .. } => radius,
        Derived::Distance { cap, .. } => cap,
        _ => 0.0,
    };
    if !(0.0..=MAX_REACH_M).contains(&reach) {
        return Err(Refusal(format!(
            "a radius or cap must be between 0 and {MAX_REACH_M} m, not {reach}"
        )));
    }
    if let Derived::Density {
        pool: Pool::Objects,
        ..
    } = d
    {
        return Err(Refusal(
            "a density is of the trees or lines pool; objects are counted by class".to_string(),
        ));
    }
    let raster = |values| Raster {
        cells: at,
        values,
        capped: false,
    };
    Ok(match d {
        Derived::Valid => raster(valid(src, at)?),
        Derived::Slope | Derived::Aspect | Derived::Roughness => raster(horn(src, d, at)?),
        Derived::Tpi { radius } => raster(tpi(src, &Disk::new(radius, &g), at)?),
        Derived::Count { class, radius } => {
            let disk = Disk::new(radius, &g);
            let n = structures(src, class, at.pad(disk.reach))?;
            let (sum, _) = disk.sums(&n, None, at.pad(disk.reach), disk.reach);
            raster(sum.into_iter().map(|s| s as f32).collect())
        }
        Derived::Density { pool, radius } => {
            let disk = Disk::new(radius, &g);
            let n = instances(src, pool, at.pad(disk.reach))?;
            let (sum, _) = disk.sums(&n, None, at.pad(disk.reach), disk.reach);
            let c = f64::from(g.cell_size);
            let hectares = disk.cells() as f64 * c * c / 10_000.0;
            raster(sum.into_iter().map(|s| (s / hectares) as f32).collect())
        }
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
        Derived::Distance { to, cap } => distance(src, to, cap, at)?,
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
                // Adding zero turns a -0 into 0.
                Derived::Aspect => (-east).atan2(-north).to_degrees().rem_euclid(360.0) + 0.0,
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

/// The cells whose centers lie within a radius of a cell's center, as one
/// run of columns on each row.
struct Disk {
    /// Cells from the center to the farthest row.
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

    fn cells(&self) -> i64 {
        self.half.iter().map(|h| 2 * h + 1).sum()
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

/// Objects of `class` in each cell of `win`.
fn structures(src: &(impl Source + ?Sized), class: Class, win: Cells) -> Result<Vec<f64>, Refusal> {
    let g = src.grid();
    let mut n = vec![0.0; win.len()];
    src.structures_in(win.bounds(&g), &mut |x, z, c| {
        if let (true, Some(i)) = (c == class, win.index(&g, x, z)) {
            n[i] += 1.0;
        }
    })?;
    Ok(n)
}

/// Instances of `pool` in each cell of `win`.
fn instances(src: &(impl Source + ?Sized), pool: Pool, win: Cells) -> Result<Vec<f64>, Refusal> {
    let g = src.grid();
    let mut n = vec![0.0; win.len()];
    src.instances_in(pool, win.bounds(&g), &mut |x, z| {
        if let Some(i) = win.index(&g, x, z) {
            n[i] += 1.0;
        }
    })?;
    Ok(n)
}

fn distance(
    src: &(impl Source + ?Sized),
    to: Target,
    cap: f64,
    at: Cells,
) -> Result<Raster, Refusal> {
    let g = src.grid();
    let cs = f64::from(g.cell_size);
    let pad = (cap / cs).ceil() as u32;
    let win = at.pad(pad);
    let seeds: Vec<bool> = match to {
        Target::Water => {
            let w = win.read(src, Layer::Water)?;
            w.data.iter().map(|&v| (1..=3).contains(&v)).collect()
        }
        Target::Cover(c) => {
            let w = win.read(src, Layer::Cover)?;
            w.data.iter().map(|&v| v == c.code()).collect()
        }
        Target::Road | Target::Rail => {
            let kind = if to == Target::Road {
                KIND_ROAD
            } else {
                KIND_RAIL
            };
            crossed(src, kind, win)?
        }
        Target::Builtup => {
            let disk = Disk::new(BUILTUP_RADIUS_M, &g);
            let n = structures(src, Class::Building, win.pad(disk.reach))?;
            let (sum, _) = disk.sums(&n, None, win.pad(disk.reach), disk.reach);
            sum.iter().map(|&s| s >= f64::from(BUILTUP_MIN)).collect()
        }
    };
    let d2 = edt(&seeds, win.rows as usize, win.cols as usize);
    let (wc, p) = (win.cols as usize, pad as usize);
    let mut capped = false;
    let mut values = Vec::with_capacity(at.len());
    for r in 0..at.rows as usize {
        for c in 0..at.cols as usize {
            let d = d2[(r + p) * wc + c + p].sqrt() * cs;
            capped |= d > cap;
            values.push(d.min(cap) as f32);
        }
    }
    Ok(Raster {
        cells: at,
        values,
        capped,
    })
}

/// The cells of `win` that an edge of `kind` passes through.
fn crossed(src: &(impl Source + ?Sized), kind: u8, win: Cells) -> Result<Vec<bool>, Refusal> {
    let g = src.grid();
    let [x0, z0, x1, z1] = win.bounds(&g);
    let mut seeds = vec![false; win.len()];
    for e in src.road_graph()?.edges.iter().filter(|e| e.kind == kind) {
        let meets = e
            .points
            .iter()
            .fold([f64::MAX, f64::MAX, f64::MIN, f64::MIN], |b, p| {
                let (x, z) = (f64::from(p[0]), f64::from(p[1]));
                [b[0].min(x), b[1].min(z), b[2].max(x), b[3].max(z)]
            });
        if meets[0] > x1 || meets[2] < x0 || meets[1] > z1 || meets[3] < z0 {
            continue;
        }
        // In cells from the window's corner.
        let c = f64::from(g.cell_size);
        let at = |p: &[f32; 2]| {
            [
                (f64::from(p[0]) - g.origin_x) / c - win.row0 as f64,
                (f64::from(p[1]) - g.origin_z) / c - win.col0 as f64,
            ]
        };
        let mut mark = |r: i64, k: i64| {
            if (0..i64::from(win.rows)).contains(&r) && (0..i64::from(win.cols)).contains(&k) {
                seeds[r as usize * win.cols as usize + k as usize] = true;
            }
        };
        if let [p] = e.points.as_slice() {
            let [r, k] = at(p);
            mark(r.floor() as i64, k.floor() as i64);
        }
        for s in e.points.windows(2) {
            walk(at(&s[0]), at(&s[1]), &mut mark);
        }
    }
    Ok(seeds)
}

/// Calls `f` with every cell the segment from `a` to `b` passes through,
/// both in cells, in order: Amanatides and Woo's walk, stepping across
/// whichever cell boundary the segment meets first. A segment through a
/// corner touches the two cells beside it only at a point, and takes
/// neither.
fn walk(a: [f64; 2], b: [f64; 2], f: &mut impl FnMut(i64, i64)) {
    let cell = |v: f64| v.floor() as i64;
    let (mut i, mut j) = (cell(a[0]), cell(a[1]));
    let (end_i, end_j) = (cell(b[0]), cell(b[1]));
    // Per axis: the step, the segment's fraction to the first boundary,
    // and its fraction between boundaries.
    let axis = |from: f64, d: f64, at: i64| {
        if d > 0.0 {
            (1, (at as f64 + 1.0 - from) / d, 1.0 / d)
        } else if d < 0.0 {
            (-1, (from - at as f64) / -d, -1.0 / d)
        } else {
            (0, f64::INFINITY, f64::INFINITY)
        }
    };
    let (si, mut ti, di) = axis(a[0], b[0] - a[0], i);
    let (sj, mut tj, dj) = axis(a[1], b[1] - a[1], j);
    f(i, j);
    // The walk ends in `b`'s cell; the count only stops rounding from
    // carrying it past.
    let steps = (end_i - i).abs() + (end_j - j).abs();
    for _ in 0..steps {
        if (i, j) == (end_i, end_j) {
            break;
        }
        if ti < tj {
            i += si;
            ti += di;
        } else if tj < ti {
            j += sj;
            tj += dj;
        } else {
            i += si;
            j += sj;
            ti += di;
            tj += dj;
        }
        f(i, j);
    }
}

/// Squared distance in cells from each cell to the nearest seed, by
/// Felzenszwalb and Huttenlocher's two passes of lower parabola envelopes;
/// infinite where there is no seed.
fn edt(seeds: &[bool], rows: usize, cols: usize) -> Vec<f64> {
    let mut f: Vec<f64> = seeds
        .iter()
        .map(|&s| if s { 0.0 } else { f64::INFINITY })
        .collect();
    let n = rows.max(cols);
    let (mut line, mut out) = (vec![0.0; n], vec![0.0; n]);
    let (mut v, mut z) = (vec![0usize; n], vec![0.0; n + 1]);
    for c in 0..cols {
        for r in 0..rows {
            line[r] = f[r * cols + c];
        }
        edt_1d(&line[..rows], &mut out[..rows], &mut v, &mut z);
        for r in 0..rows {
            f[r * cols + c] = out[r];
        }
    }
    for r in 0..rows {
        line[..cols].copy_from_slice(&f[r * cols..(r + 1) * cols]);
        edt_1d(&line[..cols], &mut out[..cols], &mut v, &mut z);
        f[r * cols..(r + 1) * cols].copy_from_slice(&out[..cols]);
    }
    f
}

fn edt_1d(f: &[f64], d: &mut [f64], v: &mut [usize], z: &mut [f64]) {
    // Only finite samples carry parabolas; with none, nothing is near.
    let mut k: Option<usize> = None;
    for q in 0..f.len() {
        if f[q].is_infinite() {
            continue;
        }
        let qf = q as f64;
        loop {
            let Some(j) = k else {
                k = Some(0);
                v[0] = q;
                z[0] = f64::NEG_INFINITY;
                z[1] = f64::INFINITY;
                break;
            };
            let p = v[j] as f64;
            let s = ((f[q] + qf * qf) - (f[v[j]] + p * p)) / (2.0 * qf - 2.0 * p);
            if s <= z[j] {
                k = j.checked_sub(1);
                continue;
            }
            v[j + 1] = q;
            z[j + 1] = s;
            z[j + 2] = f64::INFINITY;
            k = Some(j + 1);
            break;
        }
    }
    let Some(_) = k else {
        d.fill(f64::INFINITY);
        return;
    };
    let mut j = 0;
    for (q, out) in d.iter_mut().enumerate() {
        let qf = q as f64;
        while z[j + 1] < qf {
            j += 1;
        }
        let p = v[j] as f64;
        *out = (qf - p) * (qf - p) + f[v[j]];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roads::{Edge, Graph};
    use crate::source::{Ground, Identity};
    use crate::synth::*;

    fn synth() -> Synth {
        Synth::new(DEFAULT_SEED, DEFAULT_SIZE_KM, CELL_SIZE_M).unwrap()
    }

    const C: f64 = CELL_SIZE_M as f64;

    /// The grid row or column of the cell holding an offset.
    fn at(v: f64) -> i64 {
        (v / C).floor() as i64
    }

    /// `n` cells a side, the cell holding `(north, east)` at its center.
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

    /// The offsets of the cells whose centers lie within `radius` of a
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
            // On the plane alone the disk's mean is its center's height.
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

    fn dist(to: Target) -> Derived {
        Derived::Distance {
            to,
            cap: DISTANCE_CAP_M,
        }
    }

    #[test]
    fn distances_to_roads_rail_and_cover_count_whole_cells() {
        let s = synth();
        // The roads and rail run along rows of cell centers.
        let cases = [
            (Target::Road, ROAD_EW_NORTH_M, 1.0, 10025.0),
            (
                Target::Rail,
                ROAD_EW_NORTH_M - RAIL_SOUTH_OFFSET_M,
                -1.0,
                10025.0,
            ),
            (
                Target::Cover(Cover::Forest),
                FOREST_SOUTH_M + 25.0,
                -1.0,
                57025.0,
            ),
        ];
        for (to, from, way, east) in cases {
            for k in 0..30 {
                let v = one(&s, dist(to), from + way * f64::from(k) * C, east);
                assert_eq!(v, (f64::from(k) * C) as f32, "{to:?} {k}");
            }
        }
        // The forest's hole is 1 km square; its center is 500 m from trees.
        let hole = one(&s, dist(Target::Cover(Cover::Forest)), 42025.0, 57025.0);
        assert_eq!(hole, 500.0);
    }

    #[test]
    fn distance_to_the_lake_is_its_radius_off_within_a_cell_and_stops_at_the_cap() {
        let s = synth();
        let cells = Cells {
            rows: 1,
            ..around(LAKE_NORTH_M, LAKE_EAST_M + LAKE_RADIUS_M + 1000.0, 41)
        };
        let got = derive(&s, dist(Target::Water), cells).unwrap();
        assert!(!got.capped);
        for (r, c) in each(cells) {
            let (n, e) = s.form().centre(r as u32, c as u32);
            let want = ((n - LAKE_NORTH_M).hypot(e - LAKE_EAST_M) - LAKE_RADIUS_M).max(0.0);
            close(got.get(r, c).unwrap(), want, C, "to the lake");
        }
        let cap = Derived::Distance {
            to: Target::Water,
            cap: 1000.0,
        };
        let got = derive(&s, cap, around(LAKE_NORTH_M, LAKE_EAST_M + 5000.0, 3)).unwrap();
        assert!(got.capped);
        assert!(got.values.iter().all(|&v| v == 1000.0));
    }

    /// The synthetic ground with buildings of a test's own.
    struct Placed {
        synth: Synth,
        buildings: Vec<(f64, f64)>,
        roads: Graph,
    }

    impl Source for Placed {
        fn identity(&self) -> &Identity {
            self.synth.identity()
        }
        fn grid(&self) -> Grid {
            self.synth.grid()
        }
        fn tile(&self, layer: Layer, tx: u32, tz: u32) -> Result<Vec<i32>, Refusal> {
            self.synth.tile(layer, tx, tz)
        }
        fn point(&self, x: f64, z: f64) -> Result<Option<Ground>, Refusal> {
            self.synth.point(x, z)
        }
        fn road_graph(&self) -> Result<&Graph, Refusal> {
            Ok(&self.roads)
        }
        fn structures_in(
            &self,
            _: [f64; 4],
            f: &mut dyn FnMut(f64, f64, Class),
        ) -> Result<(), Refusal> {
            for &(x, z) in &self.buildings {
                f(x, z, Class::Building);
            }
            Ok(())
        }
    }

    /// A tight cluster of five, a looser one of three, and two alone.
    fn placed() -> Placed {
        let synth = synth();
        let offsets = [
            (30010.0, 60010.0),
            (30030.0, 60040.0),
            (30060.0, 60020.0),
            (30080.0, 60090.0),
            (29990.0, 59980.0),
            (31000.0, 61000.0),
            (31060.0, 61000.0),
            (31000.0, 61070.0),
            (32000.0, 60000.0),
            (29000.0, 62000.0),
        ];
        let buildings = offsets
            .iter()
            .map(|&(n, e)| synth.form().abs(n, e))
            .collect();
        let roads = Graph {
            classes: Vec::new(),
            nodes: Vec::new(),
            edges: Vec::new(),
        };
        Placed {
            synth,
            buildings,
            roads,
        }
    }

    /// The buildings in each cell, by grid row and column.
    fn per_cell(p: &Placed) -> std::collections::HashMap<(i64, i64), u32> {
        let g = p.grid();
        let mut m = std::collections::HashMap::new();
        for &(x, z) in &p.buildings {
            let k = (
                ((x - g.origin_x) / C).floor() as i64,
                ((z - g.origin_z) / C).floor() as i64,
            );
            *m.entry(k).or_insert(0) += 1;
        }
        m
    }

    #[test]
    fn counts_match_brute_force() {
        let p = placed();
        let cells = around(30500.0, 60500.0, 50);
        let m = per_cell(&p);
        for radius in [50.0, 150.0, 400.0] {
            let d = Derived::Count {
                class: Class::Building,
                radius,
            };
            let got = derive(&p, d, cells).unwrap();
            for (r, c) in each(cells) {
                let want: u32 = disk(radius)
                    .into_iter()
                    .map(|(dr, dc)| m.get(&(r + dr, c + dc)).copied().unwrap_or(0))
                    .sum();
                assert_eq!(
                    got.get(r, c).unwrap(),
                    want as f32,
                    "{radius} at ({r}, {c})"
                );
            }
        }
    }

    #[test]
    fn distance_to_builtup_matches_brute_force() {
        let p = placed();
        let m = per_cell(&p);
        let builtup = |r: i64, c: i64| {
            disk(BUILTUP_RADIUS_M)
                .into_iter()
                .map(|(dr, dc)| m.get(&(r + dr, c + dc)).copied().unwrap_or(0))
                .sum::<u32>()
                >= BUILTUP_MIN
        };
        let cells = around(30500.0, 60500.0, 60);
        let seeds: Vec<(i64, i64)> = each(cells.pad(100))
            .filter(|&(r, c)| builtup(r, c))
            .collect();
        // Both clusters, and neither building alone.
        assert!(seeds.contains(&(at(30030.0), at(60030.0))));
        assert!(seeds.contains(&(at(31030.0), at(61030.0))));
        assert!(!seeds.contains(&(at(32000.0), at(60000.0))));
        assert!(!seeds.contains(&(at(29000.0), at(62000.0))));
        // Over both clusters, and over ground 2.5 km north of them, whose
        // nearest seeds lie only in the window's pad.
        for cells in [cells, around(33500.0, 60500.0, 20)] {
            let got = derive(&p, dist(Target::Builtup), cells).unwrap();
            assert!(!got.capped);
            for (r, c) in each(cells) {
                let want = seeds
                    .iter()
                    .map(|&(sr, sc)| (((sr - r).pow(2) + (sc - c).pow(2)) as f64).sqrt() * C)
                    .fold(f64::MAX, f64::min);
                close(got.get(r, c).unwrap(), want, 1e-3, "to built up");
            }
        }
    }

    #[test]
    fn the_transform_matches_brute_force() {
        let (rows, cols) = (37, 53);
        let mut state = 7u64;
        let seeds: Vec<bool> = (0..rows * cols)
            .map(|_| {
                state = state
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                state >> 59 == 0
            })
            .collect();
        let got = edt(&seeds, rows, cols);
        for r in 0..rows {
            for c in 0..cols {
                let want = (0..rows * cols)
                    .filter(|&i| seeds[i])
                    .map(|i| ((i / cols).abs_diff(r).pow(2) + (i % cols).abs_diff(c).pow(2)) as f64)
                    .fold(f64::INFINITY, f64::min);
                assert_eq!(got[r * cols + c], want, "({r}, {c})");
            }
        }
        assert!(edt(&[false; 6], 2, 3).iter().all(|d| d.is_infinite()));
    }

    #[test]
    fn counts_and_densities_are_the_generators() {
        let s = synth();
        let count = |radius| Derived::Count {
            class: Class::Industrial,
            radius,
        };
        let (n, e) = (SCENERY_INDUSTRIAL_NORTH_M, SCENERY_INDUSTRIAL_EAST_M);
        // The pair stand two cells apart.
        assert_eq!(one(&s, count(150.0), n, e), 2.0);
        assert_eq!(one(&s, count(50.0), n, e), 1.0);
        let town = Derived::Count {
            class: Class::Building,
            radius: 2000.0,
        };
        let all = one(&s, town, SCENERY_TOWN_NORTH_M, SCENERY_TOWN_EAST_M);
        assert_eq!(all, SCENERY_TOWN_COUNT as f32);
        let trees = Derived::Density {
            pool: Pool::Trees,
            radius: 150.0,
        };
        // Three trees a cell, a quarter hectare each.
        let per_ha = f64::from(TREES_PER_CELL) / (C * C / 10_000.0);
        close(
            one(&s, trees, 40500.0, 55500.0),
            per_ha,
            1e-4,
            "in the forest",
        );
        close(one(&s, trees, 19000.0, 62000.0), 0.0, 0.0, "on the plane");
        let poles = Derived::Density {
            pool: Pool::Lines,
            radius: 100.0,
        };
        // One pole in the thirteen cells within 100 m.
        let one_in = 1.0 / (13.0 * C * C / 10_000.0);
        close(
            one(&s, poles, LINE_SOUTH_M + 1000.0, LINE_EAST_M),
            one_in,
            1e-4,
            "poles",
        );
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

    /// The cells the segment crosses for a positive length, each square
    /// tested by clipping the segment to it.
    fn crossed_cells(a: [f64; 2], b: [f64; 2]) -> std::collections::BTreeSet<(i64, i64)> {
        let d = [b[0] - a[0], b[1] - a[1]];
        let lo = |k: usize| a[k].min(b[k]).floor() as i64;
        let hi = |k: usize| a[k].max(b[k]).floor() as i64;
        let mut out = std::collections::BTreeSet::new();
        for i in lo(0)..=hi(0) {
            for j in lo(1)..=hi(1) {
                let (mut t0, mut t1) = (0.0f64, 1.0f64);
                for (k, c) in [(0, i), (1, j)] {
                    let (s, e) = (c as f64, c as f64 + 1.0);
                    if d[k] == 0.0 {
                        if !(s..e).contains(&a[k]) {
                            t1 = -1.0;
                        }
                        continue;
                    }
                    let (u, v) = ((s - a[k]) / d[k], (e - a[k]) / d[k]);
                    t0 = t0.max(u.min(v));
                    t1 = t1.min(u.max(v));
                }
                // A single point counts only where the segment is one.
                if t1 > t0 + 1e-12 || (d == [0.0, 0.0] && t1 >= t0) {
                    out.insert((i, j));
                }
            }
        }
        out
    }

    #[test]
    fn the_walk_takes_every_cell_a_segment_passes_through() {
        let mut state = 11u64;
        let mut unit = || {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (state >> 11) as f64 / (1u64 << 53) as f64
        };
        for _ in 0..500 {
            let mut p = || [unit() * 40.0 - 20.0, unit() * 40.0 - 20.0];
            let (a, b) = (p(), p());
            let mut got = std::collections::BTreeSet::new();
            walk(a, b, &mut |i, j| {
                got.insert((i, j));
            });
            assert_eq!(got, crossed_cells(a, b), "{a:?} to {b:?}");
        }
        // Along a row, and from a cell into itself.
        let mut got = Vec::new();
        walk([2.5, 0.5], [2.5, 3.5], &mut |i, j| got.push((i, j)));
        walk([7.2, 7.2], [7.9, 7.1], &mut |i, j| got.push((i, j)));
        assert_eq!(got, [(2, 0), (2, 1), (2, 2), (2, 3), (7, 7)]);
    }

    #[test]
    fn a_slanting_road_marks_every_cell_it_passes_through() {
        let mut p = placed();
        let t = *p.synth.form();
        let (a, b) = (t.abs(20_000.3, 30_000.7), t.abs(21_234.5, 31_777.1));
        let points = vec![[a.0 as f32, a.1 as f32], [b.0 as f32, b.1 as f32]];
        p.roads.edges.push(Edge {
            kind: KIND_ROAD,
            class: 0,
            a: 0,
            b: 1,
            length: 0.0,
            crosses_water: false,
            points: points.clone(),
        });
        let g = p.grid();
        let units = |q: [f32; 2]| {
            [
                (f64::from(q[0]) - g.origin_x) / C,
                (f64::from(q[1]) - g.origin_z) / C,
            ]
        };
        let want = crossed_cells(units(points[0]), units(points[1]));
        // Starting inside the road's span, so a window edge cuts it.
        let cells = around(20_800.0, 31_000.0, 30);
        let d = Derived::Distance {
            to: Target::Road,
            cap: C,
        };
        let got = derive(&p, d, cells).unwrap();
        let mut on = 0;
        for (r, c) in each(cells) {
            let v = got.get(r, c).unwrap();
            assert_eq!(v == 0.0, want.contains(&(r, c)), "({r}, {c}) reads {v}");
            on += usize::from(v == 0.0);
        }
        assert!(on > 20);
    }

    #[test]
    fn cells_past_the_grid_have_no_ground() {
        let s = synth();
        let n = i64::from(s.form().cells());
        for cells in [
            Cells {
                row0: n - 3,
                col0: -3,
                rows: 6,
                cols: 6,
            },
            Cells {
                row0: -10,
                col0: n + 5,
                rows: 2,
                cols: 3,
            },
        ] {
            let valid = derive(&s, Derived::Valid, cells).unwrap();
            assert!(valid.values.iter().all(|&v| v == 0.0));
            for d in [
                Derived::Slope,
                TPI_300,
                Derived::CoverFraction {
                    cover: Cover::Plain,
                    radius: 100.0,
                },
            ] {
                let r = derive(&s, d, cells).unwrap();
                assert!(r.values.iter().all(|v| v.is_nan()), "{d:?}");
            }
            let r = derive(&s, dist(Target::Water), cells).unwrap();
            assert!(r.capped && r.values.iter().all(|&v| v == DISTANCE_CAP_M as f32));
        }
    }

    #[test]
    fn refuses_a_radius_that_is_not_a_length() {
        let s = synth();
        let cells = around(19000.0, 62000.0, 2);
        for radius in [f64::NAN, -1.0, f64::INFINITY, MAX_REACH_M + 1.0] {
            assert!(derive(&s, Derived::Tpi { radius }, cells).is_err());
        }
        let objects = Derived::Density {
            pool: Pool::Objects,
            radius: 100.0,
        };
        assert!(derive(&s, objects, cells).is_err());
    }

    #[test]
    fn covering_a_box_takes_every_cell_it_touches() {
        let s = synth();
        let (x, z) = s.form().abs(1000.0, 2000.0);
        let c = Cells::covering(&s.grid(), [x, z, x + 120.0, z + 10.0]);
        assert_eq!((c.row0, c.col0, c.rows, c.cols), (20, 40, 3, 1));
    }
}
