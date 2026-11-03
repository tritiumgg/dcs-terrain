//! `viewshed`: the cells around an observer at which a target is seen.
//!
//! Radials leave the observer one per mask cell on the outer ring and keep
//! the steepest angle to the ground so far at every half cell: over the
//! mesh's own triangles out to a kilometer, over the raster beyond. A cell
//! takes the radial nearest its center in bearing, and is seen when its
//! target stands at or above that radial's steepest angle short of it, so
//! one sweep judges every cell. `visible` judges over the triangles all the
//! way.

use std::f64::consts::{FRAC_PI_2, PI, TAU};
use std::sync::atomic::{AtomicU8, Ordering};

use rayon::prelude::*;
use serde_json::json;

use super::geojson::{Features, props};
use super::sightline::horizon;
use super::{Terrain, outline, refused};
use crate::call::{Call, cancelled, cut_short};
use crate::grid::{Grid, Window};
use crate::source::{Layer, Refusal, Source};
use crate::threads::try_map;
use crate::types::{Issue, Observer, Polygon, ViewshedRequest, ViewshedResponse};

/// The most cells the mask, or the window of heights under it, may hold:
/// a radius of about 50 km at 50 m, under a second of sweep.
pub(super) const CELLS_MAX: u64 = 4 << 20;

/// A mask of `cells`, or a window of heights of `cells` under it, over the
/// most one may hold. Only a coarser mask makes a smaller mask; the window
/// is of terrain cells whatever the mask's.
pub(super) fn too_large(what: &str, cells: u64, hint: &str, path: &str) -> Vec<Issue> {
    vec![Issue::new(
        "too_large",
        &format!("this sweep's {what} holds {cells} cells, over the {CELLS_MAX} one may; {hint}"),
        path,
    )]
}

/// Standard base64 with padding.
pub(super) fn base64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = u32::from(c[0]) << 16
            | u32::from(*c.get(1).unwrap_or(&0)) << 8
            | u32::from(*c.get(2).unwrap_or(&0));
        for k in 0..4 {
            if k <= c.len() {
                s.push(char::from(A[(n >> (18 - 6 * k) & 63) as usize]));
            } else {
                s.push('=');
            }
        }
    }
    s
}

pub fn viewshed(terrain: &Terrain, q: ViewshedRequest) -> Result<ViewshedResponse, Vec<Issue>> {
    let target = (!q.target_alt_agl.is_finite()).then(|| {
        Issue::new(
            "invalid_request",
            "the target height must be finite",
            "target_alt_agl",
        )
    });
    let s = prepare(
        terrain,
        q.observer,
        ["observer", "radius"],
        q.radius,
        q.resolution,
        target.into_iter().collect(),
    )?;
    let (bits, seen, total) = s.run(q.target_alt_agl);
    Call::current().check()?;
    let (side, res, (origin_x, origin_z), o) = (s.side, s.res, s.origin, q.observer);

    let polygons = (q.as_polygons || q.geojson).then(|| {
        let lit = |r: i64, c: i64| {
            let i = r * side + c;
            (0..side).contains(&r)
                && (0..side).contains(&c)
                && bits[(i / 8) as usize] & (0x80 >> (i % 8)) != 0
        };
        let corner = |r: f64, c: f64| [origin_x + r * res, origin_z + c * res];
        outline::polygons(&lit, side, side, &corner, res / 2.0)
    });
    let rings = q.as_polygons.then(|| {
        polygons
            .iter()
            .flatten()
            .flatten()
            .map(|r| Polygon { points: r.clone() })
            .collect()
    });
    let geojson = match &polygons {
        Some(p) if q.geojson => {
            let mut f = Features::new(terrain)?;
            f.polygons(
                p,
                props([
                    ("observer", json!(o)),
                    ("radius", json!(q.radius)),
                    ("target_alt_agl", json!(q.target_alt_agl)),
                ]),
            );
            f.point([o.x, o.z], props([("alt_agl", json!(o.alt_agl))]));
            Some(f.finish())
        }
        _ => None,
    };
    Ok(ViewshedResponse {
        provenance: terrain.provenance(),
        origin_x,
        origin_z,
        cell: res,
        width: side as u32,
        height: side as u32,
        bits: base64(&bits),
        visible_fraction: if total == 0 {
            0.0
        } else {
            seen as f64 / total as f64
        },
        rings,
        geojson,
    })
}

/// How many radials a sweep out to `radius` at cells of `res` takes: one
/// per cell on its outer ring.
pub(super) fn radials(radius: f64, res: f64) -> u64 {
    ((TAU * radius / res).ceil() as u64).max(8)
}

/// The issues with a sweep's radius, found at `path` in the request, and
/// with its resolution, which may be no finer than the cell size.
pub(super) fn check_reach(
    src: &dyn Source,
    radius: f64,
    resolution: Option<f64>,
    path: &str,
) -> Vec<Issue> {
    let mut issues = Vec::new();
    if !(radius.is_finite() && radius > 0.0) {
        issues.push(Issue::new(
            "invalid_request",
            "the radius must be positive",
            path,
        ));
    }
    issues.extend(check_resolution(src, resolution));
    issues
}

/// The issue, at `path`, with a sweep out to a valid `radius` at cells of
/// `res` over terrain cells of `cell`: its mask, or the window of heights
/// under it, holding more cells than one may.
pub(super) fn check_size(radius: f64, res: f64, cell: f64, path: &str) -> Vec<Issue> {
    let side = 2.0 * (radius / res).ceil() + 1.0;
    if side * side > CELLS_MAX as f64 {
        return too_large(
            "mask",
            (side * side) as u64,
            "shrink the radius or coarsen the resolution",
            path,
        );
    }
    let span = (2.0 * (radius + 2.0 * cell) / cell).floor() + 2.0;
    if span * span > CELLS_MAX as f64 {
        return too_large(
            "window of heights",
            (span * span) as u64,
            "shrink the radius",
            path,
        );
    }
    Vec::new()
}

/// The issue with a resolution, which may be no finer than the cell size.
pub(super) fn check_resolution(src: &dyn Source, resolution: Option<f64>) -> Option<Issue> {
    let cell = f64::from(src.grid().cell_size);
    let res = resolution.unwrap_or(cell);
    (!(res.is_finite() && res >= cell)).then(|| {
        Issue::new(
            "invalid_request",
            &format!("the resolution must be at least the cell size, {cell} m"),
            "resolution",
        )
    })
}

/// The altitude of the eye of the site `o`, found at `path` in the request,
/// over the mesh triangle's own height under it. `issues` are the others
/// the request has, returned with any of the site's, the site's first.
pub(super) fn site_eye(
    terrain: &Terrain,
    o: Observer,
    path: &str,
    mut issues: Vec<Issue>,
) -> Result<f64, Vec<Issue>> {
    if ![o.x, o.z, o.alt_agl].iter().all(|v| v.is_finite()) {
        issues.insert(
            0,
            Issue::new("invalid_point", "every coordinate must be finite", path),
        );
    }
    if !issues.is_empty() {
        return Err(issues);
    }
    match terrain.source().point(o.x, o.z).map_err(refused)? {
        Some(g) => Ok(g.height + o.alt_agl),
        None => Err(vec![Issue::new(
            "off_terrain",
            "no mesh covers this point, so it has no ground to stand on",
            path,
        )]),
    }
}

/// Checks a sweep's site and radius, found at `at` in the request, and
/// its resolution, and reads the heights it stands on. `more` are the
/// issues the caller found with the rest of its request, returned with
/// these. Each radial crosses the mesh's triangles out to `MESH_REACH_M`.
pub(super) fn prepare(
    terrain: &Terrain,
    o: Observer,
    at: [&str; 2],
    radius: f64,
    resolution: Option<f64>,
    more: Vec<Issue>,
) -> Result<Sweep, Vec<Issue>> {
    prepare_reaching(terrain, o, at, radius, resolution, more, MESH_REACH_M)
}

/// How far out from the site a sweep's radials cross the mesh's own
/// triangles: past it a meter of error in the ground moves a line
/// at most a twentieth of a meter for each kilometer beyond (ADR 0006).
pub(super) const MESH_REACH_M: f64 = 1000.0;

/// `prepare`, the radials crossing the mesh out to `mesh_reach`: 0 for the
/// raster alone, infinity for the mesh all the way.
pub(super) fn prepare_reaching(
    terrain: &Terrain,
    o: Observer,
    at: [&str; 2],
    radius: f64,
    resolution: Option<f64>,
    more: Vec<Issue>,
    mesh_reach: f64,
) -> Result<Sweep, Vec<Issue>> {
    let src = terrain.source();
    let g = src.grid();
    let cell = f64::from(g.cell_size);
    let res = resolution.unwrap_or(cell);
    let mut issues = check_reach(src, radius, resolution, at[1]);
    issues.extend(more);
    // The site first: on the mesh, its position is bounded, and so is
    // every index below.
    let eye = site_eye(terrain, o, at[0], issues)?;

    // Counted before any index is made, so no radius overflows one.
    let size = check_size(radius, res, cell, at[1]);
    if !size.is_empty() {
        return Err(size);
    }

    // The mask, aligned to multiples of its cell from the grid's origin, so
    // at the cell size its cells are the grid's.
    let n = (radius / res).ceil();
    let side = 2.0 * n + 1.0;
    let (n, side) = (n as i64, side as i64);
    let row0 = ((o.x - g.origin_x) / res).floor() as i64 - n;
    let col0 = ((o.z - g.origin_z) / res).floor() as i64 - n;
    let (origin_x, origin_z) = (
        g.origin_x + row0 as f64 * res,
        g.origin_z + col0 as f64 * res,
    );

    // The heights out to the radius and a cell beyond, for the bilinear.
    let reach = radius + 2.0 * cell;
    let index = |v: f64, origin: f64| ((v - origin) / cell).floor() as i64;
    let (r_lo, r_hi) = (
        index(o.x - reach, g.origin_x),
        index(o.x + reach, g.origin_x),
    );
    let (c_lo, c_hi) = (
        index(o.z - reach, g.origin_z),
        index(o.z + reach, g.origin_z),
    );
    let (rows, cols) = ((r_hi - r_lo + 1) as u32, (c_hi - c_lo + 1) as u32);
    // The read and the trace below stop part way for a caller who gave up,
    // and the sweep then fails as cancelled.
    let call = Call::current();
    let r = |e: Refusal| {
        if call.stopped() {
            cancelled()
        } else {
            refused(e)
        }
    };
    let w = Window::read(src, Layer::Height, r_lo, c_lo, rows, cols).map_err(r)?;
    let radials = radials(radius, res);
    let bearing = TAU / radials as f64;
    let step = cell / 2.0;
    // Near the site the raster's heights, rounded to the meter, would tilt
    // every line beyond by their error times the ratio of the ranges, so
    // there each radial crosses the mesh's own triangles (ADR 0006).
    let steps = (mesh_reach.min(radius) / step).floor() as u64;
    let near = if steps == 0 {
        vec![Vec::new(); radials as usize]
    } else {
        let length = steps as f64 * step;
        let segments: Vec<[[f64; 2]; 2]> = (0..radials)
            .map(|k| {
                let (sin, cos) = (k as f64 * bearing).sin_cos();
                [[o.x, o.z], [o.x + length * cos, o.z + length * sin]]
            })
            .collect();
        // A radial at a time: over a dense mesh the pieces of every radial
        // together run to tens of MiB, and only its horizon is kept.
        try_map(&segments, true, |&[a, b]| {
            if call.stopped() {
                return Err(cut_short());
            }
            Ok::<_, Refusal>(horizon(&src.along(a, b)?, eye, length, step))
        })
        .map_err(r)?
    };
    Ok(Sweep {
        w,
        g,
        o: (o.x, o.z),
        eye,
        radius,
        res,
        step,
        n,
        side,
        origin: (origin_x, origin_z),
        radials,
        bearing,
        parallel: side * side >= PARALLEL_CELLS,
        near,
    })
}

/// Masks of at least this many cells are swept over every core; a smaller
/// one is done before a pool of threads would have started.
const PARALLEL_CELLS: i64 = 1 << 16;

/// The low bits of a cell's key: the last step short of it, then its index.
const LAST_AND_INDEX: u64 = (1 << 44) - 1;

/// One sweep's mask, site and heights: what `viewshed` and the radar floor
/// share.
pub(super) struct Sweep {
    w: Window,
    g: Grid,
    pub o: (f64, f64),
    /// The site's altitude.
    pub eye: f64,
    pub radius: f64,
    pub res: f64,
    /// The distance between two samples of a radial.
    step: f64,
    /// The mask's cells from its edge to the observer's, and along a side.
    pub n: i64,
    pub side: i64,
    pub origin: (f64, f64),
    radials: u64,
    /// The bearing between two radials.
    bearing: f64,
    parallel: bool,
    /// Each radial's steepest slope over the mesh's own triangles out to
    /// each of its first steps, as `sightline::horizon` finds it; the
    /// raster beyond.
    near: Vec<Vec<f64>>,
}

/// Fills `out` with the raster's slope from the eye at each step of a radial
/// running `cos` north and `sin` east, from step `first`, a chunk at a time
/// so that the arithmetic compiles to wide registers where the machine has
/// them. Additions, multiplications and divisions round alike at any width
/// and nothing here fuses a multiply with an add, so each slope is what
/// `Sweep::slope` finds.
fn slopes(s: &Sweep, first: u64, cos: f64, sin: f64, out: &mut [f64]) {
    #[cfg(target_arch = "x86_64")]
    if std::arch::is_x86_feature_detected!("avx2") {
        // SAFETY: the machine has just been seen to have AVX2.
        unsafe { slopes_avx2(s, first, cos, sin, out) };
        return;
    }
    slopes_any(s, first, cos, sin, out);
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
fn slopes_avx2(s: &Sweep, first: u64, cos: f64, sin: f64, out: &mut [f64]) {
    slopes_any(s, first, cos, sin, out);
}

/// Steps of a radial, or cells of one, taken together.
const CHUNK: usize = 32;

/// The raster's bilinear height at the first `m` of `x` and `z`, each lane
/// `good` only where four cells with ground lie around it in the window, as
/// `Window::value_at` would then find; another lane is left to it.
#[inline(always)]
fn heights(
    s: &Sweep,
    (x, z): (&[f64; CHUNK], &[f64; CHUNK]),
    m: usize,
    h: &mut [f64; CHUNK],
    good: &mut [bool; CHUNK],
) {
    let g = &s.g;
    let c = f64::from(g.cell_size);
    let (rows, cols) = (f64::from(g.height), f64::from(g.width));
    let (r0, c0) = (s.w.row0 as i32, s.w.col0 as i32);
    let (wr, wc) = (s.w.rows as i32, s.w.cols as i32);
    let (mut at, mut tr, mut tc) = ([0i32; CHUNK], [0f64; CHUNK], [0f64; CHUNK]);
    for j in 0..m {
        let (qx, qz) = ((x[j] - g.origin_x) / c, (z[j] - g.origin_z) / c);
        let (fr, fc) = (
            (qx - 0.5).clamp(0.0, rows - 1.0),
            (qz - 0.5).clamp(0.0, cols - 1.0),
        );
        let (ir, ic) = (fr as i32, fc as i32);
        tr[j] = fr - f64::from(ir);
        tc[j] = fc - f64::from(ic);
        let (r, k) = (ir - r0, ic - c0);
        let inside = (qx >= 0.0) & (qx < rows) & (qz >= 0.0) & (qz < cols);
        let held = (r >= 0) & (k >= 0) & (r + 1 < wr) & (k + 1 < wc);
        good[j] = inside & held;
        // A lane off the window reads cell 0, and is not good.
        at[j] = if good[j] { r * wc + k } else { 0 };
    }
    let data = &s.w.data[..];
    assert!(data.len() > (wr * wc) as usize - 1 && wc > 0);
    let nodata = Layer::Height.nodata();
    let stride = wc as usize;
    let mut v = [[0i32; CHUNK]; 4];
    for (o, v) in [0, 1, stride, stride + 1].into_iter().zip(&mut v) {
        for j in 0..m {
            // `at` is 0, or lies a row and a column inside the window.
            *v.get_mut(j).expect("under a chunk") = data[(at[j] as usize + o).min(data.len() - 1)];
        }
    }
    for j in 0..m {
        good[j] &=
            (v[0][j] != nodata) & (v[1][j] != nodata) & (v[2][j] != nodata) & (v[3][j] != nodata);
        h[j] = (1.0 - tr[j]) * (1.0 - tc[j]) * f64::from(v[0][j])
            + (1.0 - tr[j]) * tc[j] * f64::from(v[1][j])
            + tr[j] * (1.0 - tc[j]) * f64::from(v[2][j])
            + tr[j] * tc[j] * f64::from(v[3][j]);
    }
}

#[inline(always)]
fn slopes_any(s: &Sweep, first: u64, cos: f64, sin: f64, out: &mut [f64]) {
    let (o, step, eye) = (s.o, s.step, s.eye);
    let (mut d, mut x, mut z) = ([0f64; CHUNK], [0f64; CHUNK], [0f64; CHUNK]);
    let (mut h, mut good) = ([0f64; CHUNK], [true; CHUNK]);
    for (n, out) in out.chunks_mut(CHUNK).enumerate() {
        let from = first + (n * CHUNK) as u64;
        let m = out.len();
        for j in 0..m {
            d[j] = (from + j as u64) as f64 * step;
            x[j] = o.0 + d[j] * cos;
            z[j] = o.1 + d[j] * sin;
        }
        heights(s, (&x, &z), m, &mut h, &mut good);
        for j in 0..m {
            out[j] = (h[j] - eye) / d[j];
        }
        for j in 0..m {
            if !good[j] {
                // Near an edge of the grid or of the ground.
                out[j] = s
                    .height(x[j], z[j])
                    .map_or(f64::NEG_INFINITY, |h| (h - eye) / d[j]);
            }
        }
    }
}

/// What a cell of the mask asks of the sweep.
#[cfg(test)]
enum Ask {
    /// Out of the radius or off the ground: never set, never counted.
    Out,
    /// Too close for a step between: nothing stands in the way.
    Near,
    /// Its radial, the last step short of it and its index, in that order
    /// from the high bits, so the keys of a radial sort into sweep order.
    Swept(u64),
}

impl Sweep {
    fn height(&self, x: f64, z: f64) -> Option<f64> {
        self.w.value_at(&self.g, x, z)
    }

    /// The slope from the eye to radial `k`'s ground at its step `done`,
    /// the radial running `cos` north and `sin` east: within the mesh's
    /// reach the steepest over the triangles out to there, beyond it the
    /// raster at that step; minus infinity where there is no ground. The
    /// mesh's slopes allow half of `visible`'s 0.1 mm, the raster's none:
    /// the raster rounds its heights to the meter, so the hair is nothing
    /// there.
    fn slope(&self, k: u64, done: u64, cos: f64, sin: f64) -> f64 {
        if let Some(&s) = self.near[k as usize].get(done as usize - 1) {
            return s;
        }
        let d = done as f64 * self.step;
        self.height(self.o.0 + d * cos, self.o.1 + d * sin)
            .map_or(f64::NEG_INFINITY, |h| (h - self.eye) / d)
    }

    /// The steepest slope short of each step of radial `k`, out to step
    /// `top`: entry 0 is before the first.
    fn steepest(&self, k: u64, top: u64, cos: f64, sin: f64) -> Vec<f64> {
        let mut out = vec![f64::NEG_INFINITY; top as usize + 1];
        let near = (self.near[k as usize].len() as u64).min(top);
        for done in 1..=near {
            out[done as usize] = self.slope(k, done, cos, sin);
        }
        slopes(self, near + 1, cos, sin, &mut out[near as usize + 1..]);
        let mut most = f64::NEG_INFINITY;
        for s in &mut out {
            most = most.max(*s);
            *s = most;
        }
        out
    }

    pub fn centre(&self, r: i64, c: i64) -> (f64, f64) {
        (
            self.origin.0 + (r as f64 + 0.5) * self.res,
            self.origin.1 + (c as f64 + 0.5) * self.res,
        )
    }

    /// One cell as the sweep sees it, by the C runtime's `hypot` and `atan2`:
    /// what `ask_row` must give.
    #[cfg(test)]
    fn ask(&self, r: i64, c: i64) -> Ask {
        let (x, z) = self.centre(r, c);
        let (dx, dz) = (x - self.o.0, z - self.o.1);
        let d = dx.hypot(dz);
        // The cell holding the observer counts however small the radius.
        let own = r == self.n && c == self.n;
        if (d > self.radius && !own) || self.height(x, z).is_none() {
            return Ask::Out;
        }
        let i = r * self.side + c;
        let last = ((d / self.step).ceil() as u64).max(1) - 1;
        if last == 0 {
            return Ask::Near;
        }
        let k = (dz.atan2(dx).rem_euclid(TAU) / self.bearing).round() as u64 % self.radials;
        Ask::Swept((k << 44) | (last << 24) | i as u64)
    }

    /// The keys of row `r` into `keys`, calling `near` with each counted cell
    /// too close for a step between it and the site; returns how many it
    /// called. Square roots and a rough arctangent, in wide registers where
    /// the machine has them, stand for the C runtime's `hypot` and `atan2`
    /// until a value lies near where the last bits could tell.
    #[inline(always)]
    fn row_any<N: Fn(i64, f64)>(&self, r: i64, near: &N, keys: &mut Vec<u64>) -> u64 {
        const WIDTH: usize = 16;
        let g = &self.g;
        let cell = f64::from(g.cell_size);
        let (x, _) = self.centre(r, 0);
        let dx = x - self.o.0;
        let first = (((x - g.origin_x) / cell - 0.5).floor()) as i64;
        let wr = first - self.w.row0;
        let row_ok = first >= 0
            && first + 1 < i64::from(g.height)
            && wr >= 0
            && wr + 1 < i64::from(self.w.rows);
        let wc = i64::from(self.w.cols);
        let nodata = Layer::Height.nodata();
        let (mut z, mut dz, mut s) = ([0f64; WIDTH], [0f64; WIDTH], [0f64; WIDTH]);
        let (mut steps, mut up, mut at, mut round) =
            ([0f64; WIDTH], [0f64; WIDTH], [0f64; WIDTH], [0f64; WIDTH]);
        let (mut ic, mut close) = ([0i32; WIDTH], [false; WIDTH]);
        let mut near_count = 0;
        let mut c = 0;
        while c < self.side {
            let m = (self.side - c).min(WIDTH as i64) as usize;
            for j in 0..m {
                z[j] = self.origin.1 + ((c + j as i64) as f64 + 0.5) * self.res;
                dz[j] = z[j] - self.o.1;
                s[j] = (dx * dx + dz[j] * dz[j]).sqrt();
                steps[j] = s[j] / self.step;
                up[j] = steps[j].ceil();
                close[j] = ((steps[j] - (steps[j] + 0.5).floor()).abs() < 1e-7)
                    | ((s[j] - self.radius).abs() < 1e-6);
                let (ax, az) = (dx.abs(), dz[j].abs());
                let t = ax.min(az) / ax.max(az);
                let t2 = t * t;
                let p = -0.011_721_20;
                let p = p * t2 + 0.052_653_32;
                let p = p * t2 - 0.116_432_87;
                let p = p * t2 + 0.193_543_46;
                let p = p * t2 - 0.332_623_47;
                let a = t * (p * t2 + 0.999_977_26);
                let a = if az > ax { FRAC_PI_2 - a } else { a };
                let a = if dx < 0.0 { PI - a } else { a };
                let a = if dz[j] < 0.0 { TAU - a } else { a };
                at[j] = a / self.bearing;
                round[j] = (at[j] + 0.5).floor();
                ic[j] = ((z[j] - g.origin_z) / cell - 0.5).floor() as i32;
            }
            for j in 0..m {
                let cc = c + j as i64;
                let wide = (close[j]).then(|| dx.hypot(dz[j]));
                let d = wide.unwrap_or(s[j]);
                // The cell holding the observer counts however small the
                // radius.
                let own = r == self.n && cc == self.n;
                if d > self.radius && !own {
                    continue;
                }
                // Four cells with ground give the bilinear ground, whatever
                // its weights; anything else takes the long way.
                let w = i64::from(ic[j]) - self.w.col0;
                let ground = row_ok
                    && ic[j] >= 0
                    && i64::from(ic[j]) + 1 < i64::from(g.width)
                    && w >= 0
                    && w + 1 < wc
                    && {
                        let at = (wr * wc + w) as usize;
                        let d = &self.w.data;
                        let e = at + wc as usize;
                        d[at] != nodata
                            && d[at + 1] != nodata
                            && d[e] != nodata
                            && d[e + 1] != nodata
                    };
                if !ground && self.height(x, z[j]).is_none() {
                    continue;
                }
                let i = r * self.side + cc;
                let last = if wide.is_some() {
                    (d / self.step).ceil()
                } else {
                    up[j]
                };
                let last = (last as u64).max(1) - 1;
                if last == 0 {
                    near(i, self.height(x, z[j]).expect("kept for its ground"));
                    near_count += 1;
                    continue;
                }
                // A bearing near halfway between radials goes to the exact.
                let k = if (at[j] - round[j]).abs() < 0.45 {
                    round[j] as u64 % self.radials
                } else {
                    (dz[j].atan2(dx).rem_euclid(TAU) / self.bearing).round() as u64 % self.radials
                };
                keys.push((k << 44) | (last << 24) | i as u64);
            }
            c += m as i64;
        }
        near_count
    }

    fn row<N: Fn(i64, f64)>(&self, r: i64, near: &N, keys: &mut Vec<u64>) -> u64 {
        #[cfg(target_arch = "x86_64")]
        if std::arch::is_x86_feature_detected!("avx2") {
            // SAFETY: the machine has just been seen to have AVX2.
            return unsafe { self.row_avx2(r, near, keys) };
        }
        self.row_any(r, near, keys)
    }

    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx2")]
    fn row_avx2<N: Fn(i64, f64)>(&self, r: i64, near: &N, keys: &mut Vec<u64>) -> u64 {
        self.row_any(r, near, keys)
    }

    /// `f` over `items`, over every core when the mask is large enough. Once
    /// the caller has given up, the rest are skipped and their answers
    /// left empty: the call fails as cancelled before any is used.
    fn map<T: Send, U: Send + Default>(
        &self,
        items: Vec<T>,
        f: impl Fn(T) -> U + Sync + Send,
    ) -> Vec<U> {
        let call = Call::current();
        let f = |t| if call.stopped() { U::default() } else { f(t) };
        if self.parallel {
            items.into_par_iter().map(f).collect()
        } else {
            items.into_iter().map(f).collect()
        }
    }

    /// Sweeps one radial over its cells, keyed without the radial, calling
    /// `far` with each cell's index, range and ground, and the steepest
    /// slope from the site to the ground short of it; returns how many
    /// cells `far` said yes to.
    fn radial<F>(&self, k: u64, cells: &mut [u64], far: &F, exact: bool) -> u64
    where
        F: Fn(i64, f64, f64, f64) -> bool,
    {
        #[cfg(target_arch = "x86_64")]
        if std::arch::is_x86_feature_detected!("avx2") {
            // SAFETY: the machine has just been seen to have AVX2.
            return unsafe { self.radial_avx2(k, cells, far, exact) };
        }
        self.radial_any(k, cells, far, exact)
    }

    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx2")]
    fn radial_avx2<F>(&self, k: u64, cells: &mut [u64], far: &F, exact: bool) -> u64
    where
        F: Fn(i64, f64, f64, f64) -> bool,
    {
        self.radial_any(k, cells, far, exact)
    }

    #[inline(always)]
    fn radial_any<F>(&self, k: u64, cells: &mut [u64], far: &F, exact: bool) -> u64
    where
        F: Fn(i64, f64, f64, f64) -> bool,
    {
        cells.sort_unstable();
        let (sin, cos) = (k as f64 * self.bearing).sin_cos();
        let o = self.o;
        let top = cells.last().map_or(0, |key| key >> 24);
        let steepest = self.steepest(k, top, cos, sin);
        let mut yes = 0u64;
        let (mut x, mut z, mut d) = ([0f64; CHUNK], [0f64; CHUNK], [0f64; CHUNK]);
        let (mut h, mut good) = ([0f64; CHUNK], [true; CHUNK]);
        for chunk in cells.chunks(CHUNK) {
            let m = chunk.len();
            for (j, &key) in chunk.iter().enumerate() {
                let i = (key & 0xFFFFFF) as i64;
                (x[j], z[j]) = self.centre(i / self.side, i % self.side);
            }
            heights(self, (&x, &z), m, &mut h, &mut good);
            for j in 0..m {
                let (dx, dz) = (x[j] - o.0, z[j] - o.1);
                d[j] = (dx * dx + dz * dz).sqrt();
            }
            for (j, &key) in chunk.iter().enumerate() {
                let (last, i) = (key >> 24, (key & 0xFFFFFF) as i64);
                let d = if exact {
                    (x[j] - o.0).hypot(z[j] - o.1)
                } else {
                    d[j]
                };
                let ground = if good[j] {
                    h[j]
                } else {
                    self.height(x[j], z[j]).expect("kept for its ground")
                };
                yes += u64::from(far(i, d, ground, steepest[last as usize]));
            }
        }
        yes
    }

    /// Calls `near` with each counted cell too close for a step between it
    /// and the site, with its index and ground, and `far` as `radial` does
    /// with every other; returns how many cells are near or `far` said yes
    /// to, and how many are counted.
    ///
    /// Each radial is swept on its own, so the radials go to any core in any
    /// order and the answer is the same, as long as `near` and `far` write
    /// only the cell they are given: each cell is given once.
    pub fn sweep<N, F>(&self, near: &N, far: &F) -> (u64, u64)
    where
        N: Fn(i64, f64) + Sync,
        F: Fn(i64, f64, f64, f64) -> bool + Sync,
    {
        self.sweeping(near, far, true)
    }

    /// `sweep`, `far` given a range to the last bits only if `exact`: a
    /// square root otherwise, in place of the C runtime's `hypot`.
    fn sweeping<N, F>(&self, near: &N, far: &F, exact: bool) -> (u64, u64)
    where
        N: Fn(i64, f64) + Sync,
        F: Fn(i64, f64, f64, f64) -> bool + Sync,
    {
        let side = self.side;
        // Each row's swept keys, and how many of its cells are near.
        let rows = self.map((0..side).collect(), |r| {
            let mut keys = Vec::with_capacity(side as usize);
            let near_count = self.row(r, near, &mut keys);
            (keys, near_count)
        });
        // A caller who gave up has no use for the counts, and the rows it
        // skipped are empty: neither pass over the keys below is worth it.
        if Call::current().stopped() {
            return (0, 0);
        }
        // The keys bucketed by radial, in place of a sort of them all.
        let mut start = vec![0usize; self.radials as usize + 1];
        for (keys, _) in &rows {
            for &key in keys {
                start[(key >> 44) as usize + 1] += 1;
            }
        }
        for k in 0..self.radials as usize {
            start[k + 1] += start[k];
        }
        let mut flat = vec![0u64; start[self.radials as usize]];
        let mut at = start.clone();
        for (keys, _) in &rows {
            for &key in keys {
                let k = (key >> 44) as usize;
                flat[at[k]] = key & LAST_AND_INDEX;
                at[k] += 1;
            }
        }
        let mut radials = Vec::new();
        let mut rest = &mut flat[..];
        for k in 0..self.radials as usize {
            let (mine, more) = rest.split_at_mut(start[k + 1] - start[k]);
            if !mine.is_empty() {
                radials.push((k as u64, mine));
            }
            rest = more;
        }
        let swept = self.map(radials, |(k, cells)| self.radial(k, cells, far, exact));
        let near_count: u64 = rows.iter().map(|r| r.1).sum();
        (
            near_count + swept.iter().sum::<u64>(),
            near_count + start[self.radials as usize] as u64,
        )
    }

    /// The mask's bits for a target `target` above the ground, the cells
    /// set and the cells counted.
    fn run(&self, target: f64) -> (Vec<u8>, u64, u64) {
        let side = self.side;
        let bits: Vec<AtomicU8> = (0..((side * side) as usize).div_ceil(8))
            .map(|_| AtomicU8::new(0))
            .collect();
        let eye = self.eye;
        let far = |i: i64, d: f64, ground: f64, steepest: f64| {
            let slope = (ground + target - eye) / d;
            // Only a slope within rounding of the steepest needs the range
            // to the last bit.
            let seen = if (slope - steepest).abs() > 1e-12 * slope.abs() {
                slope >= steepest
            } else {
                let (x, z) = self.centre(i / side, i % side);
                (ground + target - eye) / (x - self.o.0).hypot(z - self.o.1) >= steepest
            };
            if seen {
                set(&bits, i);
            }
            seen
        };
        let (seen, total) = self.sweeping(&|i, _| set(&bits, i), &far, false);
        (
            bits.into_iter().map(AtomicU8::into_inner).collect(),
            seen,
            total,
        )
    }

    /// The sweep as one thread does it, every cell's key in one sort: what
    /// `run` must give, bit for bit.
    #[cfg(test)]
    fn run_serial(&self, target: f64) -> (Vec<u8>, u64, u64) {
        let side = self.side;
        let mut bits = vec![0u8; ((side * side) as usize).div_ceil(8)];
        let mut set = |i: i64| bits[(i / 8) as usize] |= 0x80 >> (i % 8);
        let (mut cells, mut seen) = (Vec::new(), 0u64);
        for r in 0..side {
            for c in 0..side {
                match self.ask(r, c) {
                    Ask::Out => {}
                    Ask::Near => {
                        set(r * side + c);
                        seen += 1;
                    }
                    Ask::Swept(key) => cells.push(key),
                }
            }
        }
        cells.sort_unstable();
        let total = cells.len() as u64 + seen;
        let (o, eye) = (self.o, self.eye);
        let mut at = 0;
        while at < cells.len() {
            let k = cells[at] >> 44;
            let (sin, cos) = (k as f64 * self.bearing).sin_cos();
            let (mut steepest, mut done) = (f64::NEG_INFINITY, 0u64);
            while at < cells.len() && cells[at] >> 44 == k {
                let (last, i) = ((cells[at] >> 24) & 0xFFFFF, (cells[at] & 0xFFFFFF) as i64);
                while done < last {
                    done += 1;
                    steepest = steepest.max(self.slope(k, done, cos, sin));
                }
                let (x, z) = self.centre(i / side, i % side);
                let d = (x - o.0).hypot(z - o.1);
                let target = self.height(x, z).expect("kept for its ground") + target;
                if (target - eye) / d >= steepest {
                    set(i);
                    seen += 1;
                }
                at += 1;
            }
        }
        (bits, seen, total)
    }
}

fn set(bits: &[AtomicU8], i: i64) {
    bits[(i / 8) as usize].fetch_or(0x80 >> (i % 8), Ordering::Relaxed);
}

#[cfg(test)]
pub(super) mod tests {
    use super::super::geojson::signed_area;
    use super::super::geojson::tests::assert_valid;
    use super::super::sightline::{Heights, walk};
    use super::super::tests::synth;
    use super::super::{Terrain, answer};
    use crate::synth::{HILL_A_EAST_M, HILL_A_NORTH_M, ORIGIN_X_M, ORIGIN_Z_M};
    use serde_json::{Value, json};

    /// North of the big hill, on its flank, so the hill shades the south.
    fn observer() -> (f64, f64) {
        (
            HILL_A_NORTH_M + 2500.0 + f64::from(ORIGIN_X_M),
            HILL_A_EAST_M + f64::from(ORIGIN_Z_M),
        )
    }

    fn ask(t: &Terrain, radius: f64, extra: Value) -> Value {
        let (x, z) = observer();
        let mut q = json!({
            "observer": {"x": x, "z": z, "alt_agl": 2},
            "radius": radius,
            "target_alt_agl": 2,
        });
        q.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        answer(t, "viewshed", &q).unwrap()
    }

    pub fn unbase64(s: &str) -> Vec<u8> {
        let a = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = Vec::new();
        for c in s.as_bytes().chunks(4) {
            let v: Vec<u32> = c
                .iter()
                .filter(|&&b| b != b'=')
                .map(|&b| a.find(char::from(b)).unwrap() as u32)
                .collect();
            let n = v.iter().fold(0, |n, &d| n << 6 | d) << (6 * (4 - v.len()));
            for k in 0..v.len() - 1 {
                out.push((n >> (16 - 8 * k)) as u8);
            }
        }
        out
    }

    /// The mask a response carries.
    struct Mask {
        side: i64,
        cell: f64,
        origin: (f64, f64),
        bits: Vec<u8>,
    }

    impl Mask {
        fn of(v: &Value) -> Mask {
            assert_eq!(v["width"], v["height"]);
            Mask {
                side: v["width"].as_i64().unwrap(),
                cell: v["cell"].as_f64().unwrap(),
                origin: (
                    v["origin_x"].as_f64().unwrap(),
                    v["origin_z"].as_f64().unwrap(),
                ),
                bits: unbase64(v["bits"].as_str().unwrap()),
            }
        }

        fn get(&self, r: i64, c: i64) -> bool {
            let i = r * self.side + c;
            (0..self.side).contains(&r)
                && (0..self.side).contains(&c)
                && self.bits[(i / 8) as usize] & (0x80 >> (i % 8)) != 0
        }

        fn centre(&self, r: i64, c: i64) -> (f64, f64) {
            (
                self.origin.0 + (r as f64 + 0.5) * self.cell,
                self.origin.1 + (c as f64 + 0.5) * self.cell,
            )
        }
    }

    #[test]
    fn the_visible_share_is_what_a_sightline_to_every_cell_finds() {
        let t = synth();
        let radius = 4000.0;
        let v = ask(&t, radius, json!({}));
        let m = Mask::of(&v);
        let src = t.source();
        let (ox, oz) = observer();
        let eye = src.point(ox, oz).unwrap().unwrap().height + 2.0;
        let mut h = Heights::new(src);
        let (mut total, mut seen, mut differ) = (0, 0, 0);
        for r in 0..m.side {
            for c in 0..m.side {
                let (x, z) = m.centre(r, c);
                if (x - ox).hypot(z - oz) > radius {
                    assert!(!m.get(r, c));
                    continue;
                }
                let g = h.at(x, z).unwrap().unwrap();
                let s = walk(&mut h, [ox, oz, eye], [x, z, g + 2.0], 25.0).unwrap();
                let brute = s.blocking.is_none();
                total += 1;
                seen += usize::from(brute);
                differ += usize::from(brute != m.get(r, c));
            }
        }
        let brute = seen as f64 / total as f64;
        let got = v["visible_fraction"].as_f64().unwrap();
        // The hill hides a real share, and the sweep finds the same share.
        assert!((0.2..0.9).contains(&brute), "{brute}");
        assert!((got - brute).abs() < 0.005, "{got} against {brute}");
        assert!((differ as f64) < 0.01 * total as f64, "{differ} of {total}");
    }

    /// Each ring of a `MultiPolygon` back in DCS x and z, open, with
    /// whether it is a hole.
    pub fn rings_back(t: &Terrain, poly: &Value) -> Vec<(bool, Vec<[f64; 2]>)> {
        let tm = t.source().tables().unwrap().crs.tmerc();
        let mut rings = Vec::new();
        for p in poly["coordinates"].as_array().unwrap() {
            for (i, ring) in p.as_array().unwrap().iter().enumerate() {
                let mut pts: Vec<[f64; 2]> = ring
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|q| {
                        let (x, z) = tm.to_xz(q[1].as_f64().unwrap(), q[0].as_f64().unwrap());
                        [x, z]
                    })
                    .collect();
                pts.pop();
                rings.push((i > 0, pts));
            }
        }
        rings
    }

    pub fn crosses(r: &[[f64; 2]], x: f64, z: f64) -> bool {
        let mut odd = false;
        for i in 0..r.len() {
            let (a, b) = (r[i], r[(i + 1) % r.len()]);
            if (a[1] > z) != (b[1] > z) && x < a[0] + (z - a[1]) / (b[1] - a[1]) * (b[0] - a[0]) {
                odd = !odd;
            }
        }
        odd
    }

    #[test]
    fn the_geojson_encloses_the_visible_cells() {
        let t = synth();
        let v = ask(&t, 3000.0, json!({"geojson": true, "as_polygons": true}));
        let g = &v["geojson"];
        assert_valid(g);
        let m = Mask::of(&v);
        let f = &g["features"][0];
        assert_eq!(f["geometry"]["type"], "MultiPolygon");
        assert_eq!(f["properties"]["radius"], 3000.0);
        assert_eq!(g["features"][1]["geometry"]["type"], "Point");
        let rings = rings_back(&t, &f["geometry"]);
        assert_eq!(v["rings"].as_array().unwrap().len(), rings.len());
        let area: f64 = rings
            .iter()
            .map(|(hole, r)| {
                let a = signed_area(r).abs() / 2.0;
                if *hole { -a } else { a }
            })
            .sum();
        let inside = |x, z| rings.iter().filter(|(_, r)| crosses(r, x, z)).count() % 2 == 1;
        let (mut cells, mut edges) = (0.0, 0.0);
        for r in -1..=m.side {
            for c in -1..=m.side {
                let lit = m.get(r, c);
                cells += f64::from(u8::from(lit));
                for (dr, dc) in [(1, 0), (0, 1)] {
                    edges += f64::from(u8::from(lit != m.get(r + dr, c + dc)));
                }
                // Away from the outline, a cell's center is inside it
                // exactly when the cell is set.
                let near = (-1..=1)
                    .flat_map(|dr| (-1..=1).map(move |dc| (dr, dc)))
                    .any(|(dr, dc)| m.get(r + dr, c + dc) != lit);
                if !near {
                    let (x, z) = m.centre(r, c);
                    assert_eq!(inside(x, z), lit, "cell ({r}, {c})");
                }
            }
        }
        // The area projected back is the set cells' to within one cell
        // along every edge of the outline.
        let cell_area = m.cell * m.cell;
        assert!(cells > 100.0 && edges > 0.0);
        assert!(
            (area - cells * cell_area).abs() <= edges * cell_area,
            "{area} against {} with {edges} edges",
            cells * cell_area
        );
    }

    #[test]
    fn a_plane_is_seen_whole_and_a_coarser_mask_agrees() {
        let t = synth();
        let (x, z) = (
            f64::from(ORIGIN_X_M) + 30000.0,
            f64::from(ORIGIN_Z_M) + 60000.0,
        );
        let q = |res: f64| {
            json!({
                "observer": {"x": x, "z": z, "alt_agl": 2},
                "radius": 2000, "target_alt_agl": 2, "resolution": res,
            })
        };
        assert_eq!(
            answer(&t, "viewshed", &q(50.0)).unwrap()["visible_fraction"],
            1.0
        );
        let coarse = answer(&t, "viewshed", &q(200.0)).unwrap();
        assert_eq!(coarse["cell"], 200.0);
        assert_eq!(coarse["visible_fraction"], 1.0);
        let share = |v: Value| v["visible_fraction"].as_f64().unwrap();
        let fine = share(ask(&t, 4000.0, json!({})));
        let coarse = share(ask(&t, 4000.0, json!({"resolution": 100})));
        assert!((fine - coarse).abs() < 0.02, "{fine} against {coarse}");
    }

    #[test]
    fn a_sweep_is_not_prepared_for_a_caller_who_gave_up() {
        use crate::call::{Ticket, within};
        use crate::types::Observer;

        let t = synth();
        let (x, z) = observer();
        let o = Observer {
            x,
            z,
            alt_agl: 10.0,
        };
        let ticket = Ticket::new(Box::new(|_, _| {}));
        ticket.cancel();
        let at = ["observer", "radius"];
        let r = within(ticket, || super::prepare(&t, o, at, 2000.0, None, vec![]));
        let Err(issues) = r else {
            panic!("a sweep was prepared");
        };
        assert_eq!(issues[0].code, "cancelled");
    }

    #[test]
    fn refuses_a_viewshed_too_large_and_a_bad_request() {
        let t = synth();
        let observer = |x: f64| json!({"x": x, "z": 0, "alt_agl": 2});
        let e = answer(
            &t,
            "viewshed",
            &json!({"observer": observer(0.0), "radius": 200000, "target_alt_agl": 0}),
        )
        .unwrap_err();
        assert_eq!(
            (e[0].code.as_str(), e[0].path.as_str()),
            ("too_large", "radius")
        );
        let e = answer(
            &t,
            "viewshed",
            &json!({"observer": observer(0.0), "radius": -1, "target_alt_agl": 0,
                    "resolution": 10}),
        )
        .unwrap_err();
        let paths: Vec<_> = e.iter().map(|i| i.path.as_str()).collect();
        assert_eq!(paths, ["radius", "resolution"]);
        let e = answer(
            &t,
            "viewshed",
            &json!({"observer": observer(1e8), "radius": 1000, "target_alt_agl": 0}),
        )
        .unwrap_err();
        assert_eq!(e[0].code, "off_terrain");
    }

    #[test]
    fn an_observer_on_a_cell_center_or_with_a_tiny_radius_sees_its_own_cell() {
        let t = synth();
        let (x, z) = (
            f64::from(ORIGIN_X_M) + 20025.0,
            f64::from(ORIGIN_Z_M) + 30025.0,
        );
        let q = |radius: f64, res: f64| {
            json!({"observer": {"x": x, "z": z, "alt_agl": 2}, "radius": radius,
                   "target_alt_agl": 2, "resolution": res})
        };
        let v = answer(&t, "viewshed", &q(1000.0, 50.0)).unwrap();
        assert_eq!(v["visible_fraction"], 1.0);
        let v = answer(&t, "viewshed", &q(10.0, 100.0)).unwrap();
        assert_eq!(v["visible_fraction"], 1.0);
        assert_eq!(v["width"], 3);
        let e = answer(&t, "viewshed", &q(1e300, 1e300)).unwrap_err();
        assert_eq!(e[0].code, "too_large");
    }

    #[test]
    fn the_sweep_over_every_core_is_the_one_thread_sweep_bit_for_bit() {
        let t = synth();
        let (x, z) = observer();
        let (east, north) = (f64::from(ORIGIN_Z_M), f64::from(ORIGIN_X_M));
        // On the hill's flank, on its top, on a plane, near the terrain's
        // corner so the radius runs off the mesh, coarse and fine, low and
        // high targets.
        let mut hidden = 0;
        for (x, z, radius, res, target) in [
            (x, z, 7000.0, 50.0, 2.0),
            (x, z, 3000.0, 50.0, 0.0),
            (
                HILL_A_NORTH_M + north,
                HILL_A_EAST_M + east,
                9000.0,
                50.0,
                30.0,
            ),
            (north + 30000.0, east + 60000.0, 8000.0, 100.0, 2.0),
            (north + 2500.0, east + 3000.0, 9000.0, 50.0, 10.0),
            (north + 20000.0, east + 50000.0, 7000.0, 50.0, 2.0),
            (x + 13.0, z - 7.0, 6000.0, 75.0, 0.5),
        ] {
            let q: super::ViewshedRequest = serde_json::from_value(json!({
                "observer": {"x": x, "z": z, "alt_agl": 2},
                "radius": radius,
                "target_alt_agl": target,
                "resolution": res,
            }))
            .unwrap();
            let mut s = super::prepare(
                &t,
                q.observer,
                ["observer", "radius"],
                q.radius,
                q.resolution,
                vec![],
            )
            .unwrap();
            let want = s.run_serial(target);
            assert!(want.1 > 0, "{x} {z}");
            hidden += usize::from(want.1 < want.2);
            for parallel in [false, true] {
                s.parallel = parallel;
                assert!(s.run(target) == want, "{x} {z} {radius} {res} {parallel}");
            }
        }
        // Most see some of their ground hidden.
        assert!(hidden >= 4, "{hidden}");
    }

    #[test]
    fn base64_is_the_standard_alphabet_padded() {
        assert_eq!(super::base64(b""), "");
        assert_eq!(super::base64(b"f"), "Zg==");
        assert_eq!(super::base64(b"fo"), "Zm8=");
        assert_eq!(super::base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(unbase64("Zm9vYg=="), b"foob");
    }
}
