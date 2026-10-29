//! Opt-in: `radar_floor`, `coverage`, `approach_spectrum` and
//! `unmask_profile` on every terrain of an install, from a few sites each:
//! three airfields and the highest of a few random points. For each site
//! the GeoJSON bands must nest, the raster floor at sampled cells is
//! compared with brute-force `visible` calls and with the exact point
//! floor, and every call is timed. Set `DCSTERRAIN_INSTALL` to the DCS
//! install root, and `DCSTERRAIN_TERRAINS` to a comma-separated list of ids
//! to take only those; without the install this passes without reading
//! anything, which is what CI does. It also times the radar floor with the
//! raster alone, the mesh near the site, and the mesh all the way, in
//! milliseconds and, on Windows, CPU cycles. Run it with `--release` and
//! `--nocapture` for the figures, and `RAYON_NUM_THREADS` to cap the
//! cores. The install is only read; the cache is a scratch directory.

use std::path::PathBuf;
use std::time::Instant;

use serde_json::{Value, json};

use dcsterrain_core::cache::Cache;
use dcsterrain_core::query::{Terrain, answer, floor_msl_reaching};
use dcsterrain_core::types::Observer;

fn install_root() -> Option<PathBuf> {
    match std::env::var_os("DCSTERRAIN_INSTALL") {
        Some(p) => Some(PathBuf::from(p)),
        None => {
            eprintln!("DCSTERRAIN_INSTALL is not set; skipped");
            None
        }
    }
}

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A generator of numbers in 0..1, the same each run.
struct Unit(u64);

impl Unit {
    fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

const RADIUS: f64 = 20_000.0;
const BANDS: [f64; 4] = [30.0, 100.0, 300.0, 1000.0];

/// The raster alone, the mesh out to the reach a sweep takes, and the mesh
/// all the way.
const REACHES: [f64; 3] = [0.0, 1000.0, f64::INFINITY];

/// The CPU cycles this process has spent, over all its threads; 0 where
/// the platform does not count them.
fn cycles() -> u64 {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        use windows_sys::Win32::System::WindowsProgramming::QueryProcessCycleTime;
        let mut c = 0u64;
        // SAFETY: the pseudo-handle of this process, and a u64 to write to.
        unsafe { QueryProcessCycleTime(GetCurrentProcess(), &mut c) };
        c
    }
    #[cfg(not(windows))]
    0
}

/// `op` with `q`, and the milliseconds it took.
fn call(t: &Terrain, op: &str, q: &Value) -> (Value, f64) {
    let s = Instant::now();
    let v = answer(t, op, q).unwrap_or_else(|e| panic!("{op} {q}: {e:?}"));
    (v, s.elapsed().as_secs_f64() * 1e3)
}

fn unbase64(s: &str) -> Vec<u8> {
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

fn floats(s: &str) -> Vec<f32> {
    unbase64(s)
        .chunks(4)
        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
        .collect()
}

/// Each ring of a `MultiPolygon` as `[lon, lat]`, open.
fn rings(g: &Value) -> Vec<Vec<[f64; 2]>> {
    let mut out = Vec::new();
    for p in g["coordinates"].as_array().unwrap() {
        for r in p.as_array().unwrap() {
            let mut pts: Vec<[f64; 2]> = r
                .as_array()
                .unwrap()
                .iter()
                .map(|q| [q[0].as_f64().unwrap(), q[1].as_f64().unwrap()])
                .collect();
            pts.pop();
            out.push(pts);
        }
    }
    out
}

/// Whether `p` lies inside the rings, even-odd, or within `tol` degrees of
/// an edge of theirs.
fn inside_or_on(rings: &[Vec<[f64; 2]>], p: [f64; 2], tol: f64) -> bool {
    let mut odd = false;
    for r in rings {
        for i in 0..r.len() {
            let (a, b) = (r[i], r[(i + 1) % r.len()]);
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let u =
                (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
            if (p[0] - a[0] - u * dx).hypot(p[1] - a[1] - u * dy) <= tol {
                return true;
            }
            if (a[1] > p[1]) != (b[1] > p[1])
                && p[0] < a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0])
            {
                odd = !odd;
            }
        }
    }
    odd
}

fn percentile(v: &[f64], p: f64) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    let mut v = v.to_vec();
    v.sort_unstable_by(f64::total_cmp);
    v[((v.len() - 1) as f64 * p).round() as usize]
}

/// Three airfields spread over the terrain, and the highest ground of 64
/// random points on it.
fn sites(t: &Terrain, u: &mut Unit) -> Vec<(String, f64, f64)> {
    let g = t.source().grid();
    let c = f64::from(g.cell_size);
    let (h, w) = (f64::from(g.height) * c, f64::from(g.width) * c);
    let mut out: Vec<(String, f64, f64)> = Vec::new();
    for (fx, fz) in [(0.5, 0.5), (0.25, 0.3), (0.7, 0.75)] {
        let (x, z) = (g.origin_x + fx * h, g.origin_z + fz * w);
        let (v, _) = call(t, "nearest", &json!({"kind": "airdrome", "x": x, "z": z}));
        if let Some(a) = v["results"].as_array().and_then(|r| r.first()) {
            let (ax, az) = (a["x"].as_f64().unwrap(), a["z"].as_f64().unwrap());
            if !out.iter().any(|s| s.1 == ax && s.2 == az) {
                out.push((format!("airdrome {} ({ax:.0}, {az:.0})", a["name"]), ax, az));
            }
        }
    }
    let pts: Vec<[f64; 2]> = (0..64)
        .map(|_| [g.origin_x + u.next() * h, g.origin_z + u.next() * w])
        .collect();
    let ground = t.source().points(&pts).unwrap();
    if let Some((p, g)) = pts
        .iter()
        .zip(ground)
        .filter_map(|(p, g)| g.map(|g| (p, g.height)))
        .max_by(|a, b| a.1.total_cmp(&b.1))
    {
        out.push((
            format!("high point {g:.0} m ({:.0}, {:.0})", p[0], p[1]),
            p[0],
            p[1],
        ));
    }
    out
}

#[test]
fn the_radar_floor_and_its_users_on_every_terrain() {
    let Some(root) = install_root() else { return };
    let build = dcsterrain_core::decode::install::read_build(&root).unwrap();
    let only: Option<Vec<String>> = std::env::var("DCSTERRAIN_TERRAINS")
        .ok()
        .map(|s| s.split(',').map(str::to_string).collect());
    let ids: Vec<String> = dcsterrain_core::decode::install::terrains(&root)
        .unwrap()
        .into_iter()
        .filter_map(|l| l.id)
        .filter(|id| only.as_ref().is_none_or(|o| o.contains(id)))
        .collect();
    eprintln!("build {} ({} terrains)", build.version, ids.len());
    let mut u = Unit(0x2545_f491_4f6c_dd1d);
    for id in &ids {
        let dir = Scratch(
            std::env::temp_dir().join(format!("dcsterrain-radar-{}-{id}", std::process::id())),
        );
        let cache = Cache::new(&dir.0);
        let t = Terrain::open(&root, id, &cache, true).unwrap();
        let mut times: Vec<[f64; 6]> = Vec::new();
        let (mut raster_err, mut agree, mut judged, mut exact_ok, mut exact_n) =
            (Vec::new(), [0usize; 4], [0usize; 4], 0, 0);
        let mut nest_checked = 0;
        // Per reach: each site's milliseconds and megacycles, and each
        // cell's floor less the exact one.
        let mut reach_ms: [Vec<f64>; 3] = Default::default();
        let mut reach_mc: [Vec<f64>; 3] = Default::default();
        let mut reach_err: [Vec<f64>; 3] = Default::default();
        for (name, sx, sz) in sites(&t, &mut u) {
            let site = json!({"x": sx, "z": sz, "alt_agl": 10});

            // The raster and its bands.
            let (v, floor_ms) = call(
                &t,
                "radar_floor",
                &json!({"site": site, "radius": RADIUS, "geojson": true}),
            );
            let r = &v["raster"];
            let (side, cell) = (r["width"].as_i64().unwrap(), r["cell"].as_f64().unwrap());
            let (ox, oz) = (
                r["origin_x"].as_f64().unwrap(),
                r["origin_z"].as_f64().unwrap(),
            );
            let agl = floats(r["floor_agl"].as_str().unwrap());
            let msl = floats(r["floor_msl"].as_str().unwrap());
            let f = v["geojson"]["features"].as_array().unwrap();
            let bands: Vec<Vec<Vec<[f64; 2]>>> = f[..BANDS.len()]
                .iter()
                .map(|f| rings(&f["geometry"]))
                .collect();
            let shares: Vec<f64> = (v["bands"].as_array().unwrap().iter())
                .map(|b| b["seen_fraction"].as_f64().unwrap())
                .collect();
            assert!(
                shares.windows(2).all(|w| w[0] <= w[1]),
                "{id} {name}: {shares:?}"
            );
            // Up to 300 vertices of each band lie in the next one up.
            for k in 0..BANDS.len() - 1 {
                let all: Vec<[f64; 2]> = bands[k].iter().flatten().copied().collect();
                let stride = (all.len() / 300).max(1);
                for p in all.iter().step_by(stride) {
                    assert!(
                        inside_or_on(&bands[k + 1], *p, 1e-6),
                        "{id} {name}: band {} not inside {} at {p:?}",
                        BANDS[k],
                        BANDS[k + 1]
                    );
                    nest_checked += 1;
                }
            }

            // 100 random cells of the raster: the floor against visible
            // half a meter above and below it, and against the point form.
            let mut cells = Vec::new();
            while cells.len() < 100 {
                let (row, col) = (
                    (u.next() * side as f64) as i64,
                    (u.next() * side as f64) as i64,
                );
                let i = (row * side + col) as usize;
                if !agl[i].is_nan() {
                    let (x, z) = (
                        ox + (row as f64 + 0.5) * cell,
                        oz + (col as f64 + 0.5) * cell,
                    );
                    cells.push((x, z, f64::from(agl[i]), f64::from(msl[i])));
                }
            }
            // At each band, whether the raster puts the cell in it, against
            // whether visible sees a target that high over the mesh there.
            for &(x, z, a, _) in &cells {
                for (k, h) in BANDS.iter().enumerate() {
                    let seen = answer(
                        &t,
                        "visible",
                        &json!({"a": {"x": sx, "z": sz, "alt_agl": 10},
                                "b": {"x": x, "z": z, "alt_agl": h}}),
                    )
                    .unwrap()["visible"]
                        == true;
                    judged[k] += 1;
                    agree[k] += usize::from(seen == (a <= *h));
                }
            }
            let pts: Vec<Value> = cells.iter().map(|c| json!({"x": c.0, "z": c.1})).collect();
            let (p, points_ms) = call(
                &t,
                "radar_floor",
                &json!({"site": site, "radius": RADIUS, "points": pts}),
            );
            for (row, &(x, z, _, m)) in p["points"].as_array().unwrap().iter().zip(&cells) {
                let Some(exact) = row["floor_msl"].as_f64() else {
                    continue;
                };
                raster_err.push(m - exact);
                // The point floor is the least altitude visible passes: a
                // millimeter over it is seen, and 5 cm under it is not,
                // visible letting a line pass 0.1 mm into the ground and so
                // farther under a target the nearer the eye the ground is.
                let seen = |alt: f64| {
                    answer(
                        &t,
                        "visible",
                        &json!({"a": {"x": sx, "z": sz, "alt_agl": 10}, "b": {"x": x, "z": z, "alt": alt}}),
                    )
                    .unwrap()["visible"]
                        == true
                };
                exact_n += 1;
                let floor_agl = row["floor_agl"].as_f64().unwrap();
                exact_ok +=
                    usize::from(seen(exact + 0.001) && (floor_agl < 0.1 || !seen(exact - 0.05)));
            }

            // What the mesh's reach buys: the raster alone, the mesh out to
            // its reach, and the mesh all the way, each timed and against
            // the exact floor at the same cells.
            let exacts: Vec<Option<f64>> = (p["points"].as_array().unwrap().iter())
                .map(|r| r["floor_msl"].as_f64())
                .collect();
            for (j, reach) in REACHES.iter().enumerate() {
                let (c, s) = (cycles(), Instant::now());
                let o = Observer {
                    x: sx,
                    z: sz,
                    alt_agl: 10.0,
                };
                let (m, (rx, rz), rside) = floor_msl_reaching(&t, o, RADIUS, *reach).unwrap();
                reach_ms[j].push(s.elapsed().as_secs_f64() * 1e3);
                reach_mc[j].push((cycles() - c) as f64 / 1e6);
                for (&(x, z, _, _), e) in cells.iter().zip(&exacts) {
                    let Some(e) = e else { continue };
                    let i =
                        ((x - rx) / cell).floor() as i64 * rside + ((z - rz) / cell).floor() as i64;
                    reach_err[j].push(m[i as usize] - e);
                }
            }

            let (_, coverage_ms) = call(
                &t,
                "coverage",
                &json!({"site": site, "envelope": {"radius": RADIUS, "alt_bands_agl": [30, 100, 300]},
                        "geojson": true}),
            );
            let (s, spectrum_ms) = call(
                &t,
                "approach_spectrum",
                &json!({"site": site, "radius": RADIUS, "alt_bands_agl": [30, 100, 300],
                        "geojson": true}),
            );
            // A route in from the radius at 30 m, along the middle of the
            // widest blind arc if there is one, to a kilometer short.
            let arc = s["bands"][0]["blind_arcs"]
                .as_array()
                .unwrap()
                .first()
                .cloned();
            let b = arc.map_or(0.0, |a| {
                let (from, to) = (
                    a["from_deg"].as_f64().unwrap(),
                    a["to_deg"].as_f64().unwrap(),
                );
                (from + (to - from).rem_euclid(360.0) / 2.0).rem_euclid(360.0)
            });
            let (sin, cos) = b.to_radians().sin_cos();
            let route = json!([
                {"x": sx + RADIUS * cos, "z": sz + RADIUS * sin, "alt_agl": 30},
                {"x": sx + 1000.0 * cos, "z": sz + 1000.0 * sin, "alt_agl": 30},
            ]);
            let (w, unmask_ms) = call(
                &t,
                "unmask_profile",
                &json!({"site": site, "route": route, "step": 100}),
            );
            if let Some(fs) = w["first_seen"].as_object() {
                let seen = answer(
                    &t,
                    "visible",
                    &json!({"a": {"x": sx, "z": sz, "alt_agl": 10},
                            "b": {"x": fs["x"], "z": fs["z"], "alt": fs["alt"]}}),
                )
                .unwrap();
                assert_eq!(seen["visible"], true, "{id} {name}: first seen {fs:?}");
            }
            eprintln!(
                "{id} {name}: floor {floor_ms:.0} ms, points {points_ms:.0} ms, coverage \
                 {coverage_ms:.0} ms, spectrum {spectrum_ms:.0} ms, unmask {unmask_ms:.0} ms; \
                 seen at 30/100/300/1000 m {:.3}/{:.3}/{:.3}/{:.3}; route at {b:.0}° first \
                 seen {:?} m out",
                shares[0],
                shares[1],
                shares[2],
                shares[3],
                w["first_seen_dist_from_site"].as_f64().map(f64::round),
            );
            times.push([
                floor_ms,
                points_ms,
                coverage_ms,
                spectrum_ms,
                unmask_ms,
                0.0,
            ]);
        }
        let med = |k: usize| percentile(&times.iter().map(|t| t[k]).collect::<Vec<_>>(), 0.5);
        let abs: Vec<f64> = raster_err.iter().map(|e| e.abs()).collect();
        eprintln!(
            "{id} on {}: {} sites; median ms floor {:.0}, points {:.0}, coverage {:.0}, \
             spectrum {:.0}, unmask {:.0}; {nest_checked} band vertices nest; raster floor \
             agrees with visible at 30/100/300/1000 m on {agree:?} of {judged:?} cells; \
             raster − exact floor \
             p50 {:.2} p95 {:.2} p99 {:.2} m, mean {:+.2} m; point floor exact at {exact_ok} of \
             {exact_n}",
            build.version,
            times.len(),
            med(0),
            med(1),
            med(2),
            med(3),
            med(4),
            percentile(&abs, 0.5),
            percentile(&abs, 0.95),
            percentile(&abs, 0.99),
            raster_err.iter().sum::<f64>() / raster_err.len().max(1) as f64,
        );
        for (j, reach) in REACHES.iter().enumerate() {
            let abs: Vec<f64> = reach_err[j].iter().map(|e| e.abs()).collect();
            eprintln!(
                "{id} mesh out to {reach} m: median {:.0} ms, {:.0} Mcycles; max {:.0} ms, \
                 {:.0} Mcycles; floor − exact p50 {:.2} p95 {:.2} p99 {:.2} max {:.2} m, \
                 mean {:+.2} m",
                percentile(&reach_ms[j], 0.5),
                percentile(&reach_mc[j], 0.5),
                percentile(&reach_ms[j], 1.0),
                percentile(&reach_mc[j], 1.0),
                percentile(&abs, 0.5),
                percentile(&abs, 0.95),
                percentile(&abs, 0.99),
                percentile(&abs, 1.0),
                reach_err[j].iter().sum::<f64>() / reach_err[j].len().max(1) as f64,
            );
        }
        assert!(
            exact_ok == exact_n,
            "{id}: point floor exact at {exact_ok} of {exact_n}"
        );
        assert!(
            agree.iter().sum::<usize>() * 100 >= judged.iter().sum::<usize>() * 95,
            "{id}: the raster agrees with visible at {agree:?} of {judged:?}"
        );
    }
}
