//! The horizon: for each horizon cell, `SPAN` grid cells a side, the
//! highest elevation angle to the ground along each of `BINS` bearings, seen
//! from `OBSERVER_M` above the ground at the cell's center, out to `RANGE_M`.
//!
//! The horizon is laid out in tiles over the grid's own: a horizon tile
//! holds the horizon cells of one grid tile. Computing one costs a march
//! through the ground for `RANGE_M` around it, so a tile is computed once,
//! from a window of heights shared by every tile of a batch, and an install
//! keeps it in its cache.
//!
//! Each bearing is one ray along the bin's center line, sampled every half
//! grid cell, each sample bilinear between the four cell centers around it.
//! A sample with any of the four outside the mesh is passed over. The march
//! skips any square of cells whose highest corner cannot raise the angle
//! found so far, which leaves the answer exactly as a march of every sample
//! would give it.

use rayon::prelude::*;

use crate::grid::Grid;
use crate::source::{Layer, Refusal, Source};

/// Bearings, the first due north and each next one clockwise.
pub const BINS: usize = 32;
/// Grid cells along each side of a horizon cell: 200 m at 50 m.
pub const SPAN: u32 = 4;
/// How far the ground is looked at.
pub const RANGE_M: f64 = 40_000.0;
/// The observer's eye above the ground.
pub const OBSERVER_M: f64 = 2.0;
/// An angle's code is its degrees above `FLOOR_DEG` in quarters, rounded:
/// 0 is -10° or lower, 254 is 53.5° or higher.
pub const FLOOR_DEG: f64 = -10.0;
pub const STEP_DEG: f64 = 0.25;
/// The code of every bin of a horizon cell whose center is outside the mesh.
pub const UNDEFINED: u8 = 255;
/// The most horizon tiles one call computes: about 16 000 horizon cells,
/// a 25 km square, and seconds on one core. A larger set is refused before
/// anything is computed, naming its size.
pub const TILES_MAX: usize = 64;

/// Horizon tiles a batch spans along each axis. One batch reads one window,
/// its tiles and `RANGE_M` around them: about 2 100 cells square, 9 MiB of
/// heights and 12 MiB of the pyramid that skips the march over them.
const BATCH: u32 = 8;
/// The most windows one call reads.
const WINDOWS_MAX: usize = 4;
/// Pyramid levels above the cells: the coarsest square is 256 cells.
const LEVELS: usize = 8;
const NODATA: i16 = i16::MIN;

/// The degrees a code stands for, `None` for `UNDEFINED`.
pub fn degrees(code: u8) -> Option<f64> {
    (code != UNDEFINED).then(|| FLOOR_DEG + f64::from(code) * STEP_DEG)
}

/// The code of an angle given as its tangent.
fn encode(tan: f64) -> u8 {
    let deg = tan.atan().to_degrees();
    ((deg - FLOOR_DEG) / STEP_DEG)
        .round()
        .clamp(0.0, f64::from(UNDEFINED - 1)) as u8
}

/// Horizon cells along each side of a horizon tile.
pub fn tile_cells(g: &Grid) -> u32 {
    g.tile_size / SPAN
}

/// The bytes of one horizon tile: its horizon cells row-major, columns
/// fastest, each `BINS` codes.
pub fn tile_len(g: &Grid) -> usize {
    (tile_cells(g) as usize).pow(2) * BINS
}

/// Computes the horizon tiles `tiles`, in order, each `tile_len` bytes.
pub fn compute(
    src: &(impl Source + ?Sized),
    tiles: &[(u32, u32)],
) -> Result<Vec<Vec<u8>>, Refusal> {
    let g = src.grid();
    if !g.tile_size.is_multiple_of(SPAN) {
        return Err(Refusal(format!(
            "a tile of {} cells holds no whole number of horizon cells of {SPAN}",
            g.tile_size
        )));
    }
    if tiles.len() > TILES_MAX {
        return Err(Refusal(format!(
            "this needs {} horizon tiles computed, over the {TILES_MAX} one query may compute; \
             shrink the region, and the tiles of a smaller one are kept for the next",
            tiles.len()
        )));
    }
    let mut out = vec![Vec::new(); tiles.len()];
    let Some(tx0) = tiles.iter().map(|t| t.0).min() else {
        return Ok(out);
    };
    let tz0 = tiles.iter().map(|t| t.1).min().unwrap_or(0);
    // Batches are counted from the corner of the set, so a set that fits in
    // one batch is read as one window wherever it lies.
    let mut batches: Vec<((u32, u32), Vec<usize>)> = Vec::new();
    for (i, &(tx, tz)) in tiles.iter().enumerate() {
        let b = ((tx - tx0) / BATCH, (tz - tz0) / BATCH);
        match batches.iter_mut().find(|(k, _)| *k == b) {
            Some((_, v)) => v.push(i),
            None => batches.push((b, vec![i])),
        }
    }
    // Each batch reads and lays out its own window, which costs as much as
    // the march over a small batch, so tiles far apart are bounded too.
    if batches.len() > WINDOWS_MAX {
        return Err(Refusal(format!(
            "these horizon tiles lie {} windows of {BATCH} tiles apart, over the {WINDOWS_MAX} \
             one query may read; ask for tiles closer together",
            batches.len()
        )));
    }
    for (_, idx) in batches {
        let mine: Vec<(u32, u32)> = idx.iter().map(|&i| tiles[i]).collect();
        let made = batch(src, &g, &mine)?;
        for (i, t) in idx.into_iter().zip(made) {
            out[i] = t;
        }
    }
    Ok(out)
}

/// The tiles of one batch, from one window.
fn batch(
    src: &(impl Source + ?Sized),
    g: &Grid,
    tiles: &[(u32, u32)],
) -> Result<Vec<Vec<u8>>, Refusal> {
    let ts = i64::from(g.tile_size);
    let range = RANGE_M / f64::from(g.cell_size);
    // One more cell, for the bilinear square of the farthest sample.
    let pad = range.ceil() as i64 + 1;
    let lo = |k: u32| i64::from(k) * ts - pad;
    let hi = |k: u32| (i64::from(k) + 1) * ts + pad;
    // Off the grid every cell is outside the mesh, so the window stops at it.
    let r0 = lo(tiles.iter().map(|t| t.0).min().unwrap_or(0)).max(0);
    let c0 = lo(tiles.iter().map(|t| t.1).min().unwrap_or(0)).max(0);
    let r1 = hi(tiles.iter().map(|t| t.0).max().unwrap_or(0)).min(i64::from(g.height));
    let c1 = hi(tiles.iter().map(|t| t.1).max().unwrap_or(0)).min(i64::from(g.width));
    let v = Heights::read(src, g, r0, c0, (r1 - r0).max(0), (c1 - c0).max(0))?;
    let p = Pyramid::new(&v);
    let dirs: Vec<(f64, f64)> = (0..BINS)
        .map(|b| {
            let az = b as f64 * std::f64::consts::TAU / BINS as f64;
            (az.cos(), az.sin())
        })
        .collect();
    let n = tile_cells(g);
    let span = i64::from(SPAN);
    // A horizon cell's center, in cells from the window's first cell center.
    let center = (span as f64 - 1.0) / 2.0;
    // Every horizon cell marches on its own, so they go to any core in any
    // order and each tile's bytes are the same.
    Ok(tiles
        .par_iter()
        .map(|&(tx, tz)| {
            let mut t = vec![UNDEFINED; tile_len(g)];
            t.par_chunks_mut(BINS).enumerate().for_each(|(k, bins)| {
                let (hr, hc) = (k as u32 / n, k as u32 % n);
                let row = i64::from(tx) * ts + i64::from(hr) * span;
                let col = i64::from(tz) * ts + i64::from(hc) * span;
                let or = (row - r0) as f64 + center;
                let oc = (col - c0) as f64 + center;
                march(&v, &p, or, oc, range, &dirs, bins);
            });
            t
        })
        .collect())
}

/// Heights over a window clipped to the grid, `NODATA` outside the mesh.
struct Heights {
    rows: usize,
    cols: usize,
    h: Vec<i16>,
    /// The highest height in the window.
    top: f64,
    cell: f64,
}

impl Heights {
    /// Reads each tile the window touches once, straight into 16 bits: the
    /// window is the larger part of the memory a batch takes.
    fn read(
        src: &(impl Source + ?Sized),
        g: &Grid,
        r0: i64,
        c0: i64,
        rows: i64,
        cols: i64,
    ) -> Result<Heights, Refusal> {
        let (rows, cols) = (rows as usize, cols as usize);
        let mut h = vec![NODATA; rows * cols];
        let ts = i64::from(g.tile_size);
        let nodata = Layer::Height.nodata();
        if rows > 0 && cols > 0 {
            let (r1, c1) = (r0 + rows as i64, c0 + cols as i64);
            let mut want: Vec<(u32, u32)> = Vec::new();
            for tx in r0 / ts..=(r1 - 1) / ts {
                for tz in c0 / ts..=(c1 - 1) / ts {
                    want.push((tx as u32, tz as u32));
                }
            }
            src.tiles(Layer::Height, &want, &mut |i, t| {
                let (tx, tz) = (i64::from(want[i].0), i64::from(want[i].1));
                if t.len() as i64 != ts * ts {
                    return Err(Refusal(format!(
                        "height tile ({tx}, {tz}) holds {} cells, not {}",
                        t.len(),
                        ts * ts
                    )));
                }
                for r in r0.max(tx * ts)..r1.min((tx + 1) * ts) {
                    for c in c0.max(tz * ts)..c1.min((tz + 1) * ts) {
                        let v = t[((r - tx * ts) * ts + c - tz * ts) as usize];
                        h[(r - r0) as usize * cols + (c - c0) as usize] = if v == nodata {
                            NODATA
                        } else {
                            v.clamp(-i32::from(i16::MAX), i32::from(i16::MAX)) as i16
                        };
                    }
                }
                Ok(())
            })?;
        }
        let top = h.iter().filter(|&&v| v != NODATA).max().copied();
        Ok(Heights {
            rows,
            cols,
            h,
            top: top.map_or(f64::NEG_INFINITY, f64::from),
            cell: f64::from(g.cell_size),
        })
    }

    /// The bilinear height at `(r, c)`, in cells where cell `(i, j)`'s
    /// center is `(i, j)`; `None` off the window or where any of the four
    /// cells around it is outside the mesh.
    fn at(&self, r: f64, c: f64) -> Option<f64> {
        if !(r >= 0.0 && c >= 0.0) {
            return None;
        }
        let (i, j) = (r as usize, c as usize);
        if i + 1 >= self.rows || j + 1 >= self.cols {
            return None;
        }
        let k = i * self.cols + j;
        let q = [
            self.h[k],
            self.h[k + 1],
            self.h[k + self.cols],
            self.h[k + self.cols + 1],
        ];
        if q.contains(&NODATA) {
            return None;
        }
        let [a, b, c2, d] = q.map(f64::from);
        let (fr, fc) = (r - i as f64, c - j as f64);
        Some(a + (b - a) * fc + (c2 - a) * fr + (a - b - c2 + d) * fr * fc)
    }
}

/// Maxima over squares of the window: level 0 holds, for each cell, the
/// highest of the four a sample in its square reads, and each next level
/// the highest of four squares of the last, so a square's value bounds every
/// sample inside it.
struct Pyramid {
    levels: Vec<(usize, Vec<i16>)>,
}

impl Pyramid {
    fn new(v: &Heights) -> Pyramid {
        let (rows, cols) = (v.rows, v.cols);
        let mut l0 = vec![NODATA; rows * cols];
        l0.par_chunks_mut(cols.max(1))
            .enumerate()
            .for_each(|(r, row)| {
                let r1 = (r + 1).min(rows - 1);
                for (c, m) in row.iter_mut().enumerate() {
                    let c1 = (c + 1).min(cols - 1);
                    *m = v.h[r * cols + c]
                        .max(v.h[r * cols + c1])
                        .max(v.h[r1 * cols + c])
                        .max(v.h[r1 * cols + c1]);
                }
            });
        let mut levels = vec![(cols, l0)];
        let mut pr = rows;
        for _ in 0..LEVELS {
            let (pc, prev) = levels.last().expect("level 0 is there");
            let pc = *pc;
            let (nr, nc) = (pr.div_ceil(2), pc.div_ceil(2));
            let mut next = vec![NODATA; nr * nc];
            for (i, row) in next.chunks_exact_mut(nc.max(1)).enumerate() {
                for (j, m) in row.iter_mut().enumerate() {
                    for (a, b) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
                        let (r, c) = (2 * i + a, 2 * j + b);
                        if r < pr && c < pc {
                            *m = (*m).max(prev[r * pc + c]);
                        }
                    }
                }
            }
            levels.push((nc, next));
            pr = nr;
        }
        Pyramid { levels }
    }
}

/// The bins of one horizon cell centered at `(or, oc)` in window cells.
fn march(
    v: &Heights,
    p: &Pyramid,
    or: f64,
    oc: f64,
    range: f64,
    dirs: &[(f64, f64)],
    out: &mut [u8],
) {
    let Some(ground) = v.at(or, oc) else {
        out.fill(UNDEFINED);
        return;
    };
    let eye = ground + OBSERVER_M;
    // How far the highest ground in the window rises above the eye.
    let lift = v.top - eye;
    let floor = FLOOR_DEG.to_radians().tan();
    for (bin, &(dr, dc)) in out.iter_mut().zip(dirs) {
        let mut best = floor;
        let mut t = 0.5;
        // Where the ray leaves the square that last failed the test at each
        // level; until then that square is not tested again.
        let mut failed = [0f64; LEVELS + 1];
        'ray: while t <= range {
            // Past this, nothing in the window can raise the angle: at a
            // positive angle the nearest sample is the hardest to beat, at a
            // negative one the farthest.
            let bound = if best >= 0.0 { best * t } else { best * range } * v.cell;
            if lift <= bound {
                break;
            }
            let (r, c) = (or + t * dr, oc + t * dc);
            // Out of the window is out for good: the window is convex.
            if !(r >= 0.0 && c >= 0.0) || r as usize + 1 >= v.rows || c as usize + 1 >= v.cols {
                break;
            }
            // Squares near the eye are too close to skip much.
            if t >= 8.0 {
                let mut l = ((t / 4.0) as u64).ilog2().min(LEVELS as u32) as usize;
                while l >= 1 {
                    if t < failed[l] {
                        l -= 1;
                        continue;
                    }
                    let (nc, m) = &p.levels[l];
                    let (bi, bj) = ((r as usize) >> l, (c as usize) >> l);
                    let top = f64::from(m[bi * nc + bj]);
                    let size = (1usize << l) as f64;
                    let (x0, x1) = (bi as f64 * size, (bi + 1) as f64 * size);
                    let (z0, z1) = (bj as f64 * size, (bj + 1) as f64 * size);
                    // The nearest and the farthest the square lies from the eye.
                    let near = (x0 - or)
                        .max(or - x1)
                        .max(0.0)
                        .hypot((z0 - oc).max(oc - z1).max(0.0));
                    let far = (or - x0)
                        .abs()
                        .max((or - x1).abs())
                        .hypot((oc - z0).abs().max((oc - z1).abs()))
                        .min(range);
                    let limit = if best >= 0.0 { best * near } else { best * far } * v.cell;
                    let leave_r = if dr > 1e-12 {
                        (x1 - or) / dr
                    } else if dr < -1e-12 {
                        (x0 - or) / dr
                    } else {
                        f64::INFINITY
                    };
                    let leave_c = if dc > 1e-12 {
                        (z1 - oc) / dc
                    } else if dc < -1e-12 {
                        (z0 - oc) / dc
                    } else {
                        f64::INFINITY
                    };
                    let leave = leave_r.min(leave_c);
                    if near > 0.0 && top - eye <= limit {
                        // On to the first sample past the square.
                        t = ((leave / 0.5).ceil() * 0.5).max(t + 0.5);
                        continue 'ray;
                    }
                    failed[l] = leave;
                    l -= 1;
                }
            }
            if let Some(h) = v.at(r, c) {
                best = best.max((h - eye) / (t * v.cell));
            }
            t += 0.5;
        }
        *bin = encode(best);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::{Ground, Identity};
    use crate::synth::Synth;
    use crate::synth::{CELL_SIZE_M, DEFAULT_SEED, DEFAULT_SIZE_KM, HILL_A_EAST_M, HILL_A_NORTH_M};

    /// Rough ground: short ridges on long swells, a spike every so often,
    /// and a patch outside the mesh, so a march both skips squares and
    /// passes samples over.
    struct Rough {
        identity: Identity,
        grid: Grid,
    }

    impl Rough {
        fn new(height: u32, width: u32, tile_size: u32) -> Rough {
            Rough {
                identity: Identity {
                    terrain: "rough".into(),
                    dcs_build: "0".into(),
                    fingerprint: "0".into(),
                },
                grid: Grid {
                    cell_size: 50,
                    origin_x: 0.0,
                    origin_z: 0.0,
                    height,
                    width,
                    tile_size,
                },
            }
        }

        fn height(&self, r: i64, c: i64) -> Option<i32> {
            let g = self.grid;
            if r >= i64::from(g.height) || c >= i64::from(g.width) || r < 0 || c < 0 {
                return None;
            }
            if (40..46).contains(&r) && (20..23).contains(&c) {
                return None;
            }
            let (x, z) = (r as f64, c as f64);
            let ridge = 120.0 * (x / 9.0).sin() * (z / 13.0).cos() + 2.0 * x;
            let swell = 900.0 * (x / 310.0).sin() * (z / 270.0).cos();
            let spike = if (r * 7 + c * 13) % 97 == 0 {
                300.0
            } else {
                0.0
            };
            Some((ridge + swell + spike).round() as i32)
        }
    }

    impl Source for Rough {
        fn identity(&self) -> &Identity {
            &self.identity
        }

        fn grid(&self) -> Grid {
            self.grid
        }

        fn tile(&self, layer: Layer, tx: u32, tz: u32) -> Result<Vec<i32>, Refusal> {
            let ts = i64::from(self.grid.tile_size);
            Ok((0..ts * ts)
                .map(|i| {
                    let (r, c) = (i64::from(tx) * ts + i / ts, i64::from(tz) * ts + i % ts);
                    match layer {
                        Layer::Height => self.height(r, c).unwrap_or(layer.nodata()),
                        _ => layer.nodata(),
                    }
                })
                .collect())
        }

        fn point(&self, _: f64, _: f64) -> Result<Option<Ground>, Refusal> {
            Ok(None)
        }
    }

    /// Every sample of every ray out to the range, as the module describes
    /// the march, with nothing skipped and nothing shared with it.
    fn brute(s: &Rough, row: i64, col: i64) -> [u8; BINS] {
        let at = |r: f64, c: f64| -> Option<f64> {
            if r < 0.0 || c < 0.0 {
                return None;
            }
            let (i, j) = (r.floor() as i64, c.floor() as i64);
            let (fr, fc) = (r - i as f64, c - j as f64);
            let h = |a: i64, b: i64| s.height(i + a, j + b).map(f64::from);
            let (a, b, c2, d) = (h(0, 0)?, h(0, 1)?, h(1, 0)?, h(1, 1)?);
            Some(
                a * (1.0 - fr) * (1.0 - fc)
                    + b * (1.0 - fr) * fc
                    + c2 * fr * (1.0 - fc)
                    + d * fr * fc,
            )
        };
        let (or, oc) = (row as f64 + 1.5, col as f64 + 1.5);
        let mut out = [UNDEFINED; BINS];
        let Some(eye) = at(or, oc).map(|h| h + OBSERVER_M) else {
            return out;
        };
        let cell = f64::from(s.grid.cell_size);
        // Past the grid's diagonal nothing is left to sample.
        let diagonal = f64::from(s.grid.height).hypot(f64::from(s.grid.width));
        let last = (2.0 * (RANGE_M / cell).min(diagonal + 2.0)) as i64;
        for (b, o) in out.iter_mut().enumerate() {
            let az = (b as f64 * 360.0 / BINS as f64).to_radians();
            let mut best = FLOOR_DEG.to_radians().tan();
            for k in 1..=last {
                let t = k as f64 * 0.5;
                if let Some(h) = at(or + t * az.cos(), oc + t * az.sin()) {
                    best = best.max((h - eye) / (t * cell));
                }
            }
            *o = encode(best);
        }
        out
    }

    /// Each horizon cell of `tiles` whose index in its tile `pick` takes,
    /// against the brute march; returns how many were undefined, above level
    /// and below it.
    fn agree(s: &Rough, tiles: &[(u32, u32)], pick: impl Fn(usize) -> bool) -> [usize; 3] {
        let ts = i64::from(s.grid.tile_size);
        let n = tile_cells(&s.grid) as i64;
        let mut seen = [0usize; 3];
        for (&(tx, tz), t) in tiles.iter().zip(compute(s, tiles).unwrap()) {
            assert_eq!(t.len(), tile_len(&s.grid));
            for (k, bins) in t.as_chunks::<BINS>().0.iter().enumerate() {
                if !pick(k) {
                    continue;
                }
                let row = i64::from(tx) * ts + k as i64 / n * 4;
                let col = i64::from(tz) * ts + k as i64 % n * 4;
                assert_eq!(*bins, brute(s, row, col), "horizon cell at ({row}, {col})");
                seen[match bins[0] {
                    UNDEFINED => 0,
                    40.. => 1,
                    _ => 2,
                }] += 1;
            }
        }
        seen
    }

    #[test]
    fn the_march_agrees_with_every_sample_of_every_ray() {
        let s = Rough::new(90, 76, 8);
        let (ntx, ntz) = s.grid.tiles();
        let all: Vec<(u32, u32)> = (0..ntx)
            .flat_map(|x| (0..ntz).map(move |z| (x, z)))
            .collect();
        // Two scattered halves, each over several batches.
        let (even, odd): (Vec<_>, Vec<_>) = all.iter().partition(|(x, z)| (x + z) % 2 == 0);
        let mut seen = [0usize; 3];
        for set in [even, odd] {
            for (a, b) in seen.iter_mut().zip(agree(&s, &set, |_| true)) {
                *a += b;
            }
        }
        // Cells outside the mesh, and both a horizon above and below level.
        assert!(seen.iter().all(|&n| n > 0), "{seen:?}");
    }

    #[test]
    fn the_march_agrees_out_to_the_range_on_a_large_grid() {
        // Wider than the range, so the far squares of the pyramid are
        // skipped, rays end at the range, and windows start inside the grid.
        let s = Rough::new(1900, 1750, 64);
        // A corner, an inner tile, the partial last one and one off the
        // grid, in four windows.
        let tiles = [(0, 0), (14, 13), (29, 27), (15, 0), (31, 26)];
        let seen = agree(&s, &tiles, |k| k % 7 == 3);
        assert!(seen[0] > 0 && seen[1] > 20, "{seen:?}");
    }

    #[test]
    fn refuses_more_tiles_or_windows_than_one_query_may_compute() {
        let s = Rough::new(90, 76, 8);
        let many = vec![(0, 0); TILES_MAX + 1];
        let e = compute(&s, &many).unwrap_err();
        assert!(e.0.contains(&format!("{}", TILES_MAX + 1)), "{e}");
        let apart: Vec<(u32, u32)> = (0..=WINDOWS_MAX as u32).map(|k| (k * BATCH, 0)).collect();
        let e = compute(&s, &apart).unwrap_err();
        assert!(e.0.contains(&format!("{} windows", WINDOWS_MAX + 1)), "{e}");
        assert!(compute(&s, &[]).unwrap().is_empty());
    }

    #[test]
    fn the_hill_stands_at_the_angle_its_closed_form_gives() {
        let s = Synth::new(DEFAULT_SEED, DEFAULT_SIZE_KM, CELL_SIZE_M).unwrap();
        let form = s.form();
        let g = s.grid();
        let cs = f64::from(g.cell_size);
        // Horizon cells 10 km west, 6 km south and 4 km north-east of the
        // big hill's top, so the hill rises over a level, a falling and a
        // rising foreground.
        for (dn, de) in [(0.0, -10_000.0), (-6_000.0, 0.0), (2_800.0, 2_800.0)] {
            let span = f64::from(SPAN) * cs;
            let hr = ((HILL_A_NORTH_M + dn) / span).floor() as u32;
            let hc = ((HILL_A_EAST_M + de) / span).floor() as u32;
            let n = tile_cells(&g);
            let (tx, tz) = (hr / n, hc / n);
            let t = &compute(&s, &[(tx, tz)]).unwrap()[0];
            let k = ((hr % n) * n + hc % n) as usize;
            let bins = &t[k * BINS..(k + 1) * BINS];
            // The center of the horizon cell, a corner shared by four cells.
            let (north, east) = ((f64::from(hr) + 0.5) * span, (f64::from(hc) + 0.5) * span);
            let eye = form.height(north, east) + OBSERVER_M;
            for (b, &code) in bins.iter().enumerate() {
                let az = (b as f64 * 360.0 / BINS as f64).to_radians();
                let mut best = FLOOR_DEG.to_radians().tan();
                for d in 1..=RANGE_M as i64 {
                    let d = d as f64;
                    let (pn, pe) = (north + d * az.cos(), east + d * az.sin());
                    if !form.covered(pn, pe) {
                        break;
                    }
                    best = best.max((form.height(pn, pe) - eye) / d);
                }
                let want = best.atan().to_degrees().max(FLOOR_DEG);
                let got = degrees(code).unwrap();
                assert!(
                    (got - want).abs() <= 0.25,
                    "bin {b} from ({north}, {east}): {got}° against {want}°"
                );
            }
        }
    }

    #[test]
    fn codes_quarter_degrees_from_minus_ten() {
        assert_eq!(encode(0.0), 40);
        assert_eq!(degrees(40), Some(0.0));
        assert_eq!(encode(1.0), 220);
        assert_eq!(degrees(220), Some(45.0));
        assert_eq!(encode(-1.0), 0);
        assert_eq!(encode(100.0), 254);
        assert_eq!(degrees(UNDEFINED), None);
        // Half a step either way rounds to the nearest.
        assert_eq!(encode(0.124f64.to_radians().tan()), 40);
        assert_eq!(encode(0.126f64.to_radians().tan()), 41);
    }
}
