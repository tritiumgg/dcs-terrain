//! `airfield_info`: one airdrome's runways, navaids, radio and stands.
//!
//! Runways are the taxi graph's pairs of named ends. Each end is a
//! threshold: its course is the direction a landing on it flies, towards
//! the other end, and the runway's length is threshold to threshold. A
//! course is given true, the normal section's azimuth in the threshold's
//! own east-north plane on the WGS84 ellipsoid through the terrain's
//! projection, and grid, from DCS `x`; they differ by the meridian
//! convergence. There is no magnetic model, so no magnetic course: the
//! designators are the files' own.

use std::cmp::Ordering;

use serde_json::{Value, json};

use super::geojson::{Features, props};
use super::region::known_airdrome;
use super::sightline::highest;
use super::{Terrain, invalid, refused};
use crate::crs::{Tmerc, degrees, grid_bearing, median};
use crate::decode::surface5::ground::SURFACE_RUNWAY;
use crate::source::install::AirdromeRow;
use crate::source::{Ground, Refusal, Source, Tables};
use crate::types::{
    AirfieldInfoRequest, AirfieldInfoResponse, Airframe, Issue, Navaid, RadioInfo, RunwayInfo,
    StandInfo, StandSize, Threshold,
};

/// The stations a width is measured at, as fractions of the way along:
/// clear of the ends, where taxiways and aprons meet the runway.
const WIDTH_STATIONS: [f64; 9] = [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9];
/// How far either side of the centerline the runway's edge is looked for:
/// paint wider than twice this is an apron's, not a runway's, and measures
/// no width at that station.
const WIDTH_REACH_M: f64 = 75.0;
/// A stretch off the paint narrower than this is a seam between its
/// triangles, a crack a point can fall through to the ground beneath, and
/// the runway goes on across it. The coarse step is no longer, so it lands
/// in every stretch that is not a seam; the fine step then finds where the
/// first such stretch begins.
const WIDTH_SEAM_M: f64 = 1.0;
const WIDTH_COARSE_M: f64 = WIDTH_SEAM_M;
const WIDTH_STEP_M: f64 = 0.25;
/// The bisection between the last step on the runway and the first off it
/// stops within this of the edge.
const WIDTH_TOLERANCE_M: f64 = 0.01;

/// The game's beacon types by code, as its shared beacon script names them
/// without their `BEACON_TYPE_` prefix.
const BEACON_TYPES: [(i64, &str); 20] = [
    (1, "vor"),
    (2, "dme"),
    (3, "vor_dme"),
    (4, "tacan"),
    (5, "vortac"),
    (8, "homer"),
    (128, "rsbn"),
    (1024, "broadcast_station"),
    (4104, "airport_homer"),
    (4136, "airport_homer_with_marker"),
    (16408, "ils_far_homer"),
    (16424, "ils_near_homer"),
    (16640, "ils_localizer"),
    (16896, "ils_glideslope"),
    (33024, "prmg_localizer"),
    (33280, "prmg_glideslope"),
    (65536, "nautical_homer"),
    (131328, "icls_localizer"),
    (131584, "icls_glideslope"),
    (262144, "tacan_range"),
];

fn beacon_kind(code: i64) -> Option<&'static str> {
    BEACON_TYPES
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, name)| *name)
}

/// Whether the ground is the runway's paint.
fn on_paint(g: &Option<Ground>) -> bool {
    g.is_some_and(|g| g.surface == SURFACE_RUNWAY)
}

/// The runway's width at each station `stations` gives, over the mesh's
/// paint, as `paint_widths` measures it.
fn runway_widths(
    src: &dyn Source,
    a: [f64; 2],
    b: [f64; 2],
    stations: &[f64],
) -> Result<Vec<Option<f64>>, Refusal> {
    let mut paint = |p: &[[f64; 2]]| Ok(src.points(p)?.iter().map(on_paint).collect());
    paint_widths(&mut paint, a, b, stations)
}

/// Whether each of a batch of points is on the paint.
type Paint<'a> = dyn FnMut(&[[f64; 2]]) -> Result<Vec<bool>, Refusal> + 'a;

/// The width of the paint across the segment from `a` to `b` at each
/// station `stations` gives: the centerline there must be on it, and each
/// side's edge is where the first stretch off it of `WIDTH_SEAM_M` or more
/// begins, found to `WIDTH_STEP_M` and then bisected. `None` at a station
/// whose centerline is off the paint, or whose edge lies past
/// `WIDTH_REACH_M`. Each look is one batch for every side at once, so a
/// runway costs a few batches.
fn paint_widths(
    paint: &mut Paint,
    a: [f64; 2],
    b: [f64; 2],
    stations: &[f64],
) -> Result<Vec<Option<f64>>, Refusal> {
    let len = (b[0] - a[0]).hypot(b[1] - a[1]);
    if len == 0.0 {
        return Ok(vec![None; stations.len()]);
    }
    let (ux, uz) = ((b[0] - a[0]) / len, (b[1] - a[1]) / len);
    // Each side of each station: its center, and the unit vector out.
    let sides: Vec<([f64; 2], [f64; 2])> = stations
        .iter()
        .flat_map(|&f| {
            let c = [a[0] + f * (b[0] - a[0]), a[1] + f * (b[1] - a[1])];
            [(c, [-uz, ux]), (c, [uz, -ux])]
        })
        .collect();
    let at = |(c, n): ([f64; 2], [f64; 2]), d: f64| [c[0] + d * n[0], c[1] + d * n[1]];
    let coarse = (WIDTH_REACH_M / WIDTH_COARSE_M).round() as usize;
    let fine = (WIDTH_COARSE_M / WIDTH_STEP_M).round() as usize;
    // The fine steps a stretch off the paint must hold to span a seam.
    let seam = (WIDTH_SEAM_M / WIDTH_STEP_M).round() as usize + 1;
    let pts: Vec<[f64; 2]> = (sides.iter())
        .flat_map(|&s| (0..=coarse).map(move |k| at(s, k as f64 * WIDTH_COARSE_M)))
        .collect();
    let steps: Vec<Vec<bool>> = paint(&pts)?
        .chunks(coarse + 1)
        .map(<[bool]>::to_vec)
        .collect();
    // The first coarse step past `k` off the paint, where the stretch off it
    // may begin.
    let next = |s: usize, k: usize| (k + 1..=coarse).find(|&j| !steps[s][j]);
    // Each side's bracket: the last offset on the runway and the first off
    // it for good, or `None` where the center is off it or nothing leaves
    // it within the reach.
    let mut brackets: Vec<Option<[f64; 2]>> = vec![None; sides.len()];
    // Each side still looked along, and the coarse step it looks back from.
    let mut open: Vec<(usize, usize)> = (0..sides.len())
        .filter(|&s| steps[s][0])
        .filter_map(|s| Some((s, next(s, 0)?)))
        .collect();
    while !open.is_empty() {
        // Fine steps from the coarse one before, out past a seam's width.
        let n = fine + seam;
        let pts: Vec<[f64; 2]> = (open.iter())
            .flat_map(|&(s, k)| {
                let (side, from) = (sides[s], (k - 1) as f64 * WIDTH_COARSE_M);
                (0..n).map(move |j| at(side, from + j as f64 * WIDTH_STEP_M))
            })
            .collect();
        let on = paint(&pts)?;
        let mut still = Vec::new();
        for (&(s, k), g) in open.iter().zip(on.chunks(n)) {
            let off = |j: usize| g[j..j + seam].iter().all(|&g| !g);
            match (1..=fine).find(|&j| g[j - 1] && off(j)) {
                Some(j) => {
                    let edge = (k - 1) as f64 * WIDTH_COARSE_M + j as f64 * WIDTH_STEP_M;
                    brackets[s] = Some([edge - WIDTH_STEP_M, edge]);
                }
                // A seam the coarse step fell into: the paint goes on.
                None => still.extend(next(s, k).map(|k| (s, k))),
            }
        }
        open = still;
    }
    while brackets
        .iter()
        .flatten()
        .any(|[i, o]| o - i > WIDTH_TOLERANCE_M)
    {
        let mids: Vec<[f64; 2]> = (sides.iter().zip(&brackets))
            .filter_map(|(&s, b)| b.map(|[i, o]| at(s, 0.5 * (i + o))))
            .collect();
        let mut on = paint(&mids)?.into_iter();
        for b in brackets.iter_mut().flatten() {
            let m = 0.5 * (b[0] + b[1]);
            if on.next().expect("a point per bracket") {
                b[0] = m;
            } else {
                b[1] = m;
            }
        }
    }
    // An edge is the midpoint of its bracket, within half the tolerance.
    let edge = |b: &Option<[f64; 2]>| b.map(|[i, o]| 0.5 * (i + o));
    Ok(brackets
        .chunks(2)
        .map(|s| Some(edge(&s[0])? + edge(&s[1])?))
        .collect())
}

/// A threshold at `p`, its course towards `other`.
fn threshold(tm: &Tmerc, name: &str, p: [f64; 2], other: [f64; 2], h: Option<f64>) -> Threshold {
    let (lat, lon) = tm.to_latlon(p[0], p[1]);
    Threshold {
        designator: name.to_string(),
        x: p[0],
        z: p[1],
        lat,
        lon,
        height: h,
        course_true_deg: tm.true_bearing(p, other),
        course_grid_deg: grid_bearing(p, other),
    }
}

/// The airdrome's runways, from the tables, each threshold's height and
/// width from the mesh, and the highest ground along their centerlines.
fn runways(
    src: &dyn Source,
    tables: &Tables,
    tm: &Tmerc,
    a: &AirdromeRow,
) -> Result<(Vec<RunwayInfo>, Option<f64>), Refusal> {
    let rows: Vec<_> = (tables.runways.iter())
        .filter(|r| r.airdrome_name_id == a.name_id)
        .collect();
    let ends: Vec<[[f64; 2]; 2]> = rows
        .iter()
        .map(|r| [[r.edge1_x, r.edge1_z], [r.edge2_x, r.edge2_z]])
        .collect();
    let heights = src.points(&ends.iter().flatten().copied().collect::<Vec<_>>())?;
    let pieces = src.alongs(&ends)?;
    let mut top: Option<f64> = None;
    let mut out = Vec::with_capacity(rows.len());
    for (k, (r, &[p1, p2])) in rows.iter().zip(&ends).enumerate() {
        let (h1, h2) = (
            heights[2 * k].map(|g| g.height),
            heights[2 * k + 1].map(|g| g.height),
        );
        for h in [h1, h2]
            .into_iter()
            .chain([highest(&pieces[k]).map(|(h, _)| h)])
        {
            top = match (top, h) {
                (Some(t), Some(h)) => Some(t.max(h)),
                (t, h) => t.or(h),
            };
        }
        let widths = runway_widths(src, p1, p2, &WIDTH_STATIONS)?;
        out.push(RunwayInfo {
            designator: r.name.clone(),
            course_true_deg: tm.true_bearing(p1, p2),
            course_grid_deg: degrees(r.course),
            length: (p2[0] - p1[0]).hypot(p2[1] - p1[1]),
            width: median(widths.into_iter().flatten().collect()),
            thresholds: vec![
                threshold(tm, &r.edge1_name, p1, p2, h1),
                threshold(tm, &r.edge2_name, p2, p1, h2),
            ],
        });
    }
    Ok((out, top))
}

/// Whether `f` fits a stand of this size and use: no where any known size
/// or use fails it, `None` where none does but one is unknown.
fn fits(f: &Airframe, s: &StandInfo) -> Option<bool> {
    let usable = if f.helicopter {
        s.helicopters
    } else {
        s.airplanes
    };
    let checks = [
        s.width.map(|w| f.width <= w),
        s.length.map(|l| f.length <= l),
        s.height.map(|h| f.height <= h),
        usable,
    ];
    // One known failure settles it, whatever else is unknown.
    if checks.contains(&Some(false)) {
        Some(false)
    } else if checks.contains(&None) {
        None
    } else {
        Some(true)
    }
}

/// Counts of the stands by width and length, widest first, then longest;
/// stands of no known size last.
fn sizes(stands: &[StandInfo]) -> Vec<StandSize> {
    let mut out: Vec<StandSize> = Vec::new();
    for s in stands {
        match out
            .iter_mut()
            .find(|c| c.width == s.width && c.length == s.length)
        {
            Some(c) => c.count += 1,
            None => out.push(StandSize {
                width: s.width,
                length: s.length,
                count: 1,
            }),
        }
    }
    let wider = |a: Option<f64>, b: Option<f64>| match (a, b) {
        (Some(a), Some(b)) => b.total_cmp(&a),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    };
    out.sort_by(|a, b| wider(a.width, b.width).then(wider(a.length, b.length)));
    out
}

fn check(f: &Airframe) -> Result<(), Vec<Issue>> {
    for (name, v) in [
        ("width", f.width),
        ("length", f.length),
        ("height", f.height),
    ] {
        if !(v.is_finite() && v >= 0.0) {
            return Err(invalid(
                "a size must be finite and not negative",
                &format!("fits.{name}"),
            ));
        }
    }
    Ok(())
}

pub fn airfield_info(
    terrain: &Terrain,
    q: AirfieldInfoRequest,
) -> Result<AirfieldInfoResponse, Vec<Issue>> {
    if let Some(f) = &q.fits {
        check(f)?;
    }
    let src = terrain.source();
    let tables = src.tables().map_err(refused)?;
    let tm = tables.crs.tmerc();
    let a = known_airdrome(tables, &q.airdrome, "airdrome")?;
    let (runways, centerline_top) = runways(src, tables, &tm, a).map_err(refused)?;

    let navaids: Vec<Navaid> = (tables.beacons.iter())
        .filter(|b| {
            b.beacon_id
                .as_ref()
                .is_some_and(|id| a.beacon_ids.contains(id))
        })
        .map(|b| Navaid {
            id: b.beacon_id.clone(),
            kind: b.kind.and_then(beacon_kind).map(str::to_string),
            kind_code: b.kind,
            callsign: b.callsign.clone(),
            frequency_hz: b.frequency_hz,
            channel: b.channel,
            x: b.x,
            z: b.z,
            alt: b.alt,
        })
        .collect();
    let radio: Vec<RadioInfo> = (tables.radio.iter())
        .filter(|r| {
            r.radio_id
                .as_ref()
                .is_some_and(|id| a.radio_ids.contains(id))
        })
        .map(|r| RadioInfo {
            id: r.radio_id.clone(),
            callsign: (r.callsigns.get("en"))
                .or_else(|| r.callsigns.values().next())
                .cloned(),
            hf_hz: r.frequencies_hz.hf,
            fm_hz: r.frequencies_hz.fm,
            vhf_hz: r.frequencies_hz.vhf,
            uhf_hz: r.frequencies_hz.uhf,
        })
        .collect();
    let mut stands: Vec<StandInfo> = (tables.stands.iter())
        .filter(|s| s.airdrome_name_id == a.name_id)
        .map(|s| StandInfo {
            name: s.name.clone(),
            x: s.x,
            z: s.z,
            class: s.stand_class.clone(),
            width: s.params.WIDTH,
            length: s.params.LENGTH,
            height: (s.params.HEIGHT.as_deref())
                .and_then(|h| h.trim().parse().ok())
                .filter(|h: &f64| h.is_finite()),
            helicopters: s.params.FOR_HELICOPTERS,
            airplanes: s.params.FOR_AIRPLANES,
            shelter: s.params.SHELTER,
            fits: None,
        })
        .collect();
    let mut stands_fitting = None;
    if let Some(f) = &q.fits {
        for s in &mut stands {
            s.fits = fits(f, s);
        }
        stands_fitting = Some(stands.iter().filter(|s| s.fits == Some(true)).count() as u32);
    }
    let elevation = match centerline_top {
        Some(top) => Some(top),
        None => {
            let at: Vec<[f64; 2]> = stands.iter().map(|s| [s.x, s.z]).collect();
            (src.points(&at).map_err(refused)?.into_iter())
                .flatten()
                .map(|g| g.height)
                .reduce(f64::max)
        }
    };

    let geojson = if q.geojson {
        let mut f = Features::new(terrain)?;
        for r in &runways {
            let line: Vec<[f64; 2]> = r.thresholds.iter().map(|t| [t.x, t.z]).collect();
            f.line(
                &line,
                props([
                    ("designator", json!(r.designator)),
                    ("course_true_deg", json!(r.course_true_deg)),
                    ("length", json!(r.length)),
                    ("width", json!(r.width)),
                ]),
            );
        }
        for s in &stands {
            let Value::Object(mut p) = json!(s) else {
                unreachable!("a stand serializes to an object")
            };
            p.remove("x");
            p.remove("z");
            f.point([s.x, s.z], p);
        }
        Some(f.finish())
    } else {
        None
    };
    Ok(AirfieldInfoResponse {
        provenance: terrain.provenance(),
        id: a.id,
        name_id: a.name_id.clone(),
        name: a.display_name.clone().unwrap_or_else(|| a.name_id.clone()),
        x: a.x,
        z: a.z,
        lat: a.lat,
        lon: a.lon,
        elevation,
        runways,
        navaids,
        radio,
        stand_sizes: sizes(&stands),
        stands,
        stands_fitting,
        geojson,
    })
}

#[cfg(test)]
mod tests {
    use super::super::answer;
    use super::super::geojson::tests::assert_valid;
    use super::super::tests::synth;
    use super::*;
    use crate::crs::tests::{geodesic_azimuth, turn};
    use crate::synth::*;

    fn ask(t: &Terrain, q: Value) -> Result<Value, Vec<Issue>> {
        answer(t, "airfield_info", &q)
    }

    fn ends(t: &Terrain) -> [[f64; 2]; 2] {
        let Terrain::Synth(s) = t else { unreachable!() };
        s.form().runway_ends().map(|(x, z)| [x, z])
    }

    #[test]
    fn the_runway_is_the_synthetic_one_with_true_and_grid_courses() {
        let t = synth();
        let v = ask(&t, json!({"airdrome": AIRDROME_ID})).unwrap();
        let tm = t.source().tables().unwrap().crs.tmerc();
        let [w, e] = ends(&t);
        let r = &v["runways"][0];
        assert_eq!(v["runways"].as_array().unwrap().len(), 1);
        assert_eq!(
            r["designator"],
            format!("{RUNWAY_EDGE1_NAME}-{RUNWAY_EDGE2_NAME}")
        );
        assert_eq!(r["length"], RUNWAY_LENGTH_M);
        assert_eq!(r["course_grid_deg"], 90.0);
        // True against Vincenty's geodesic, threshold to threshold.
        let geodesic = |a: [f64; 2], b: [f64; 2]| {
            let ((la, lo), (lb, lob)) = (tm.to_latlon(a[0], a[1]), tm.to_latlon(b[0], b[1]));
            geodesic_azimuth(la, lo, lb, lob)
        };
        let course = r["course_true_deg"].as_f64().unwrap();
        assert!(turn(course, geodesic(w, e)) < 1e-5, "{course}");
        // The synthetic projection's meridian is off the runway's, so the
        // two courses differ by its convergence.
        assert!(turn(course, 90.0) > 0.1, "{course}");
        let th = &r["thresholds"];
        assert_eq!(th[0]["designator"], RUNWAY_EDGE1_NAME);
        assert_eq!(th[1]["designator"], RUNWAY_EDGE2_NAME);
        assert_eq!(th[1]["course_grid_deg"], 270.0);
        let back = th[1]["course_true_deg"].as_f64().unwrap();
        assert!(turn(back, geodesic(e, w)) < 1e-5, "{back}");
        for (k, p) in [w, e].into_iter().enumerate() {
            let g = t.source().point(p[0], p[1]).unwrap().unwrap();
            assert_eq!(th[k]["height"], g.height);
            assert_eq!(
                [th[k]["x"].as_f64().unwrap(), th[k]["z"].as_f64().unwrap()],
                p
            );
        }
    }

    #[test]
    fn the_width_is_the_runway_surfaces_as_a_millimeter_scan_finds_it() {
        let t = synth();
        let v = ask(&t, json!({"airdrome": AIRDROME_ID})).unwrap();
        let width = v["runways"][0]["width"].as_f64().unwrap();
        assert!(
            (width - RUNWAY_WIDTH_M).abs() <= WIDTH_TOLERANCE_M,
            "{width}"
        );
        // Brute force: every millimeter across a station, counted.
        let [w, e] = ends(&t);
        let src = t.source();
        for f in [0.1, 0.5, 0.9] {
            let c = [w[0] + f * (e[0] - w[0]), w[1] + f * (e[1] - w[1])];
            let pts: Vec<[f64; 2]> = (-60_000..=60_000)
                .map(|mm| [c[0] + f64::from(mm) / 1000.0, c[1]])
                .collect();
            let on = (src.points(&pts).unwrap().iter())
                .filter(|g| on_paint(g))
                .count();
            let scan = on as f64 / 1000.0;
            let at = (runway_widths(src, w, e, &[f]).unwrap().pop())
                .flatten()
                .unwrap();
            assert!(
                (at - scan).abs() <= WIDTH_TOLERANCE_M + 0.001,
                "{at} against {scan}"
            );
        }
        // Slanted across the runway, the paint runs 120 m, each side's edge
        // past the near pass.
        let (nx, nz) = (
            RUNWAY_WIDTH_M / 120.0,
            (1.0 - (RUNWAY_WIDTH_M / 120.0).powi(2)).sqrt(),
        );
        let c = [(w[0] + e[0]) / 2.0, (w[1] + e[1]) / 2.0];
        let (a, b) = (
            [c[0] - 100.0 * nz, c[1] + 100.0 * nx],
            [c[0] + 100.0 * nz, c[1] - 100.0 * nx],
        );
        let slant = runway_widths(src, a, b, &[0.5]).unwrap()[0].unwrap();
        assert!((slant - 120.0).abs() <= WIDTH_TOLERANCE_M, "{slant}");
        // Off the runway's surface, no width.
        let off = [[w[0] + 500.0, w[1]], [e[0] + 500.0, e[1]]];
        assert_eq!(runway_widths(src, off[0], off[1], &[0.5]).unwrap(), [None]);
    }

    /// Paint across x either side of a centerline along z: `on` for the
    /// offsets x on it.
    fn across(on: impl Fn(f64) -> bool + Copy) -> Option<f64> {
        let mut paint = |p: &[[f64; 2]]| Ok(p.iter().map(|p| on(p[0])).collect());
        paint_widths(&mut paint, [0.0, 0.0], [0.0, 100.0], &[0.5]).unwrap()[0]
    }

    #[test]
    fn a_seam_is_bridged_and_a_wider_stretch_off_the_paint_is_an_edge() {
        let close = |w: Option<f64>, want: f64| {
            let w = w.unwrap();
            assert!((w - want).abs() <= WIDTH_TOLERANCE_M, "{w} against {want}");
        };
        close(across(|x| x.abs() <= 25.0), 50.0);
        // A crack a coarse step lands in, and one between the steps.
        close(
            across(|x| x.abs() <= 25.0 && !(10.0..10.5).contains(&x)),
            50.0,
        );
        close(
            across(|x| x.abs() <= 25.0 && !(12.3..12.8).contains(&x)),
            50.0,
        );
        // Under a meter off it is still a seam; two meters is the edge.
        close(
            across(|x| x.abs() <= 25.0 && !(20.0..20.9).contains(&x)),
            50.0,
        );
        close(
            across(|x| x.abs() <= 25.0 && !(20.0..22.0).contains(&x)),
            45.0,
        );
        // Paint past the reach on one side, or none at the center: no width.
        assert_eq!(across(|x| (-25.0..=80.0).contains(&x)), None);
        assert_eq!(across(|x| x.abs() <= 25.0 && x.abs() > 0.1), None);
    }

    #[test]
    fn the_elevation_is_the_highest_ground_along_the_centerline() {
        let t = synth();
        let v = ask(&t, json!({"airdrome": AIRDROME_NAME_ID})).unwrap();
        let elevation = v["elevation"].as_f64().unwrap();
        let [w, e] = ends(&t);
        let pts: Vec<[f64; 2]> = (0..=25_000)
            .map(|k| {
                let f = f64::from(k) / 25_000.0;
                [w[0] + f * (e[0] - w[0]), w[1] + f * (e[1] - w[1])]
            })
            .collect();
        let scan = (t.source().points(&pts).unwrap().into_iter())
            .flatten()
            .map(|g| g.height)
            .reduce(f64::max)
            .unwrap();
        // The scan steps 10 cm, so it may stand a little under.
        assert!(
            elevation >= scan - 1e-9 && elevation - scan < 0.01,
            "{elevation} against {scan}"
        );
    }

    #[test]
    fn an_id_its_digits_and_its_name_id_answer_alike() {
        let t = synth();
        let mut by = [
            json!(AIRDROME_ID),
            json!(AIRDROME_ID.to_string()),
            json!(AIRDROME_NAME_ID),
        ]
        .map(|a| ask(&t, json!({ "airdrome": a })).unwrap());
        for v in &mut by {
            v.as_object_mut().unwrap().remove("elapsed_ms");
        }
        assert_eq!(by[0], by[1]);
        assert_eq!(by[0], by[2]);
        assert_eq!(by[0]["id"], AIRDROME_ID);
        assert_eq!(by[0]["name"], AIRDROME_DISPLAY_NAME);
        let code = |a: Value| {
            ask(&t, json!({ "airdrome": a })).unwrap_err()[0]
                .code
                .clone()
        };
        assert_eq!(code(json!(99)), "unknown_airdrome");
        assert_eq!(code(json!("Nowhere")), "unknown_airdrome");
    }

    #[test]
    fn navaids_radio_and_stands_are_the_airdromes_own() {
        let t = synth();
        let v = ask(&t, json!({"airdrome": AIRDROME_ID})).unwrap();
        let navaids = v["navaids"].as_array().unwrap();
        assert_eq!(navaids.len(), BEACON_COUNT as usize);
        assert_eq!(navaids[0]["id"], beacon_id(1));
        assert_eq!(navaids[0]["channel"], BEACON_1_CHANNEL);
        assert_eq!(navaids[1]["frequency_hz"], BEACON_2_FREQUENCY_HZ as f64);
        assert_eq!(navaids[1]["channel"], Value::Null);
        let radio = &v["radio"][0];
        assert_eq!(radio["vhf_hz"], RADIO_VHF_HZ as f64);
        assert_eq!(radio["uhf_hz"], RADIO_UHF_HZ as f64);
        let stands = v["stands"].as_array().unwrap();
        assert_eq!(stands.len(), STAND_COUNT as usize);
        assert_eq!(stands[0]["width"], STAND_WIDTH_M);
        assert_eq!(stands[0]["fits"], Value::Null);
        assert_eq!(
            v["stand_sizes"],
            json!([{"width": STAND_WIDTH_M, "length": STAND_LENGTH_M, "count": STAND_COUNT}])
        );
        assert_eq!(v["stands_fitting"], Value::Null);
    }

    fn stand(width: f64, height: Option<f64>, helicopters: bool, airplanes: bool) -> StandInfo {
        StandInfo {
            name: "1".into(),
            x: 0.0,
            z: 0.0,
            class: None,
            width: Some(width),
            length: Some(width),
            height,
            helicopters: Some(helicopters),
            airplanes: Some(airplanes),
            shelter: None,
            fits: None,
        }
    }

    #[test]
    fn an_airframe_fits_a_stand_big_enough_for_its_kind() {
        let jet = Airframe {
            width: 15.0,
            length: 20.0,
            height: 5.0,
            helicopter: false,
        };
        let helo = Airframe {
            helicopter: true,
            ..jet
        };
        assert_eq!(fits(&jet, &stand(22.0, Some(8.0), false, true)), Some(true));
        assert_eq!(
            fits(&jet, &stand(18.0, Some(8.0), false, true)),
            Some(false)
        );
        assert_eq!(
            fits(&jet, &stand(22.0, Some(4.0), false, true)),
            Some(false)
        );
        assert_eq!(
            fits(&helo, &stand(22.0, Some(8.0), false, true)),
            Some(false)
        );
        assert_eq!(
            fits(&helo, &stand(22.0, Some(8.0), true, false)),
            Some(true)
        );
        // An unknown height leaves the answer unknown.
        assert_eq!(fits(&jet, &stand(22.0, None, false, true)), None);
        // But a known failure settles it.
        assert_eq!(fits(&jet, &stand(18.0, None, false, true)), Some(false));
        assert_eq!(fits(&helo, &stand(22.0, None, false, true)), Some(false));
        // Exactly the stand's size fits.
        assert_eq!(fits(&jet, &stand(20.0, Some(5.0), false, true)), Some(true));
    }

    #[test]
    fn sizes_count_widest_first_and_unknown_last() {
        let mut none = stand(0.0, None, true, true);
        (none.width, none.length) = (None, None);
        let s = [
            stand(10.0, None, true, true),
            none,
            stand(30.0, None, true, true),
            stand(10.0, None, true, true),
        ];
        let got: Vec<_> = sizes(&s).iter().map(|c| (c.width, c.count)).collect();
        assert_eq!(got, [(Some(30.0), 1), (Some(10.0), 2), (None, 1)]);
    }

    #[test]
    fn a_bad_size_is_an_issue() {
        let t = synth();
        let q = json!({"airdrome": 1, "fits": {"width": -1.0, "length": 10.0, "height": 3.0}});
        assert_eq!(ask(&t, q).unwrap_err()[0].path, "fits.width");
        let q = json!({"airdrome": 1, "fits": {"width": 10.0, "length": 10.0, "height": 3.0}});
        assert_eq!(ask(&t, q).unwrap()["stands_fitting"], 0);
    }

    #[test]
    fn geojson_holds_the_runway_and_each_stand() {
        let t = synth();
        let v = ask(&t, json!({"airdrome": 1, "geojson": true})).unwrap();
        assert_valid(&v["geojson"]);
        let f = v["geojson"]["features"].as_array().unwrap();
        assert_eq!(f.len(), 1 + STAND_COUNT as usize);
        assert_eq!(f[0]["geometry"]["type"], "LineString");
        assert_eq!(f[0]["properties"]["length"], RUNWAY_LENGTH_M);
        assert_eq!(f[1]["geometry"]["type"], "Point");
    }

    #[test]
    fn a_median_is_the_middle_value() {
        assert_eq!(median(vec![]), None);
        assert_eq!(median(vec![3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(median(vec![4.0, 1.0, 2.0, 3.0]), Some(2.5));
    }
}

/// Opt-in: `airfield_info` for every airdrome of every terrain of a real
/// install, each runway checked against its table row, its true course
/// against Vincenty's geodesic, its width against a scan across it every
/// 2 cm and the elevation against a scan along every centerline every
/// meter. Set `DCSTERRAIN_INSTALL` to the install root; without it this
/// passes reading nothing, which is what CI does. Run it with `--release`
/// and `--nocapture` for the figures. The install is only read; the cache
/// is a scratch directory.
#[cfg(test)]
mod install_tests {
    use std::path::PathBuf;
    use std::time::Instant;

    use super::*;
    use crate::cache::Cache;
    use crate::crs::tests::{geodesic_azimuth, turn};
    use crate::decode::install::terrains;
    use crate::query::route::install_tests::percentile;
    use crate::scratch::Scratch;
    use crate::threads::cpu_cycles;
    use crate::types::AirdromeRef;

    /// The scan across a runway steps this far.
    const SCAN_STEP_M: f64 = 0.02;

    /// Wall milliseconds and millions of CPU cycles since `since`.
    fn spent(since: (Instant, u64)) -> (f64, f64) {
        let cycles = cpu_cycles().saturating_sub(since.1) as f64 / 1e6;
        (since.0.elapsed().as_secs_f64() * 1000.0, cycles)
    }

    fn now() -> (Instant, u64) {
        (Instant::now(), cpu_cycles())
    }

    /// Brute force: the run of runway surface through `c` along `n`,
    /// sampled every `SCAN_STEP_M` out to the width search's reach and
    /// across any seam narrower than `WIDTH_SEAM_M`, as many samples as it
    /// spans times the step; `None` where `c` is off the runway or the run
    /// reaches the reach.
    fn scan_width(src: &dyn Source, c: [f64; 2], n: [f64; 2]) -> Option<f64> {
        let k = ((WIDTH_REACH_M + WIDTH_SEAM_M) / SCAN_STEP_M).round() as i64;
        let reach = (WIDTH_REACH_M / SCAN_STEP_M).round() as usize;
        let seam = (WIDTH_SEAM_M / SCAN_STEP_M).round() as usize + 1;
        let pts: Vec<[f64; 2]> = (-k..=k)
            .map(|i| {
                let d = i as f64 * SCAN_STEP_M;
                [c[0] + d * n[0], c[1] + d * n[1]]
            })
            .collect();
        let g: Vec<bool> = src.points(&pts).unwrap().iter().map(on_paint).collect();
        let mid = k as usize;
        if !g[mid] {
            return None;
        }
        // The first sample off the paint that begins a seam's width off it,
        // out from the center each way.
        let out = |step: &dyn Fn(usize) -> usize| {
            (1..=reach).find(|&d| (0..seam).all(|s| !g[step(d + s)]))
        };
        let lo = out(&|d| mid - d)?;
        let hi = out(&|d| mid + d)?;
        Some((lo + hi - 1) as f64 * SCAN_STEP_M)
    }

    /// The runway number a designator starts with, 1 to 36.
    fn number(designator: &str) -> Option<f64> {
        let digits: String = designator
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        let n: u32 = digits.parse().ok()?;
        (1..=36).contains(&n).then_some(f64::from(n))
    }

    /// A signed angle in (-180, 180].
    fn signed(deg: f64) -> f64 {
        let d = deg.rem_euclid(360.0);
        if d > 180.0 { d - 360.0 } else { d }
    }

    #[test]
    fn every_airdrome_answers_on_every_installed_terrain() {
        let Some(root) = std::env::var_os("DCSTERRAIN_INSTALL").map(PathBuf::from) else {
            eprintln!("DCSTERRAIN_INSTALL is not set; skipped");
            return;
        };
        // Five threads, so the runs beside this one keep their share.
        crate::threads::init(Some(5));
        println!(
            "| terrain | build | airdromes | answered | no id | no runway | runways | course, length = tables | true off geodesic max ° | convergence max ° | widths | width p50 (min–max) m | width off scan max m | station spread p50 / max m | elevation over scan max m | declination from designators ° | designator off p95 / max ° | thresholds on runway | unnamed navaid kinds | stands |"
        );
        println!(
            "|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|"
        );
        let mut timing = Vec::new();
        let mut outliers = Vec::new();
        let scratch = Scratch::new();
        let cache = Cache::new(&scratch.0);
        for listed in terrains(&root).unwrap() {
            let Some(id) = listed.id else {
                println!("| {} | declares no id |", listed.dir);
                continue;
            };
            let clock = now();
            let t = match Terrain::open(&root, &id, &cache, true) {
                Ok(t) => t,
                Err(e) => {
                    println!("| {id} | refused: {} |", e.0);
                    continue;
                }
            };
            let src = t.source();
            let opened = spent(clock);
            let build = src.identity().dcs_build.clone();
            let clock = now();
            let tables = src.tables().unwrap();
            let prime = spent(clock);
            let tm = tables.crs.tmerc();

            let (mut no_id, mut no_runway, mut n_runways, mut n_stands) = (0, 0, 0, 0);
            let (mut true_off, mut convergence, mut width_off, mut elev_over) =
                (0.0f64, 0.0f64, 0.0f64, 0.0f64);
            let (mut widths, mut spreads, mut residuals) = (Vec::new(), Vec::new(), Vec::new());
            let (mut painted, mut ends, mut unnamed) = (0, 0, 0);
            let (mut cold, mut warm) = (Vec::new(), Vec::new());
            let mut width_spent = (0.0, 0.0);
            for a in &tables.airdromes {
                let by =
                    a.id.map_or_else(|| AirdromeRef::Name(a.name_id.clone()), AirdromeRef::Id);
                let q = AirfieldInfoRequest {
                    airdrome: by,
                    fits: None,
                    geojson: false,
                };
                let clock = now();
                let v = airfield_info(&t, q.clone())
                    .unwrap_or_else(|e| panic!("{id} {}: {e:?}", a.name_id));
                cold.push(spent(clock));
                let clock = now();
                let again = airfield_info(&t, q).unwrap();
                warm.push(spent(clock));
                assert_eq!(v.runways, again.runways, "{id} {}: twice alike", a.name_id);
                let by_name = AirfieldInfoRequest {
                    airdrome: AirdromeRef::Name(a.name_id.clone()),
                    fits: None,
                    geojson: false,
                };
                assert_eq!(airfield_info(&t, by_name).unwrap().name_id, a.name_id);
                assert_eq!(v.name_id, a.name_id);
                no_id += usize::from(a.id.is_none());
                no_runway += usize::from(v.runways.is_empty());

                let rows: Vec<_> = (tables.runways.iter())
                    .filter(|r| r.airdrome_name_id == a.name_id)
                    .collect();
                assert_eq!(v.runways.len(), rows.len(), "{id} {}", a.name_id);
                let mut along: Vec<[f64; 2]> = Vec::new();
                for (r, row) in v.runways.iter().zip(&rows) {
                    let (p1, p2) = ([row.edge1_x, row.edge1_z], [row.edge2_x, row.edge2_z]);
                    let what = format!("{id} {} {}", a.name_id, r.designator);
                    // The answer is the table's, and the table's course its
                    // own ends'.
                    assert!(
                        turn(degrees(row.course), grid_bearing(p1, p2)) < 1e-9,
                        "{what}"
                    );
                    assert_eq!(r.length, (p2[0] - p1[0]).hypot(p2[1] - p1[1]), "{what}");
                    assert_eq!(r.designator, row.name, "{what}");
                    let th = &r.thresholds;
                    assert_eq!(
                        [th[0].x, th[0].z, th[1].x, th[1].z],
                        [p1[0], p1[1], p2[0], p2[1]]
                    );
                    for (k, (p, o)) in [(p1, p2), (p2, p1)].into_iter().enumerate() {
                        let ((la, lo), (lb, lob)) =
                            (tm.to_latlon(p[0], p[1]), tm.to_latlon(o[0], o[1]));
                        let off = turn(th[k].course_true_deg, geodesic_azimuth(la, lo, lb, lob));
                        assert!(off < 1e-5, "{what}: {off}");
                        true_off = true_off.max(off);
                        convergence =
                            convergence.max(turn(th[k].course_true_deg, th[k].course_grid_deg));
                        if let Some(n) = number(&th[k].designator) {
                            residuals.push((
                                signed(th[k].course_true_deg - 10.0 * n),
                                format!("{what} {}", th[k].designator),
                            ));
                        }
                        ends += 1;
                        painted += usize::from(on_paint(&src.point(p[0], p[1]).unwrap()));
                    }
                    // Width against the scan, station by station.
                    let len = r.length;
                    let n = [-(p2[1] - p1[1]) / len, (p2[0] - p1[0]) / len];
                    let clock = now();
                    let each = runway_widths(src, p1, p2, &WIDTH_STATIONS).unwrap();
                    let w = spent(clock);
                    width_spent = (width_spent.0 + w.0, width_spent.1 + w.1);
                    for (&f, w) in WIDTH_STATIONS.iter().zip(&each) {
                        let c = [p1[0] + f * (p2[0] - p1[0]), p1[1] + f * (p2[1] - p1[1])];
                        let scan = scan_width(src, c, n);
                        match (w, scan) {
                            (Some(w), Some(s)) => {
                                let off = (w - s).abs();
                                assert!(
                                    off <= WIDTH_TOLERANCE_M + SCAN_STEP_M + 1e-6,
                                    "{what} at {f}: {w} against {s}"
                                );
                                width_off = width_off.max(off);
                            }
                            (None, None) => {}
                            _ => panic!("{what} at {f}: {w:?} against the scan's {scan:?}"),
                        }
                    }
                    let got: Vec<f64> = each.iter().flatten().copied().collect();
                    if let Some(w) = r.width {
                        widths.push(w);
                        let (lo, hi) = got
                            .iter()
                            .fold((f64::MAX, f64::MIN), |(l, h), &v| (l.min(v), h.max(v)));
                        spreads.push(hi - lo);
                    }
                    let steps = len.ceil() as usize;
                    along.extend((0..=steps).map(|i| {
                        let f = i as f64 / steps as f64;
                        [p1[0] + f * (p2[0] - p1[0]), p1[1] + f * (p2[1] - p1[1])]
                    }));
                }
                n_runways += rows.len();
                // The elevation against a scan along every centerline, or
                // over the stands where there is no runway.
                let scan_at = |pts: &[[f64; 2]]| {
                    (src.points(pts).unwrap().into_iter())
                        .flatten()
                        .map(|g| g.height)
                        .reduce(f64::max)
                };
                if v.runways.is_empty() {
                    let at: Vec<[f64; 2]> = v.stands.iter().map(|s| [s.x, s.z]).collect();
                    assert_eq!(v.elevation, scan_at(&at), "{id} {}", a.name_id);
                } else if let (Some(e), Some(s)) = (v.elevation, scan_at(&along)) {
                    assert!(
                        e >= s - 1e-6,
                        "{id} {}: {e} under the scan's {s}",
                        a.name_id
                    );
                    // The scan steps a meter, so it may stand a little under a crest.
                    assert!(e - s < 0.05, "{id} {}: {e} over the scan's {s}", a.name_id);
                    elev_over = elev_over.max(e - s);
                }
                let stands = (tables.stands.iter())
                    .filter(|s| s.airdrome_name_id == a.name_id)
                    .count();
                assert_eq!(v.stands.len(), stands, "{id} {}", a.name_id);
                n_stands += stands;
                let beacons = (tables.beacons.iter())
                    .filter(|b| {
                        b.beacon_id
                            .as_ref()
                            .is_some_and(|i| a.beacon_ids.contains(i))
                    })
                    .count();
                assert_eq!(v.navaids.len(), beacons, "{id} {}", a.name_id);
                unnamed += v
                    .navaids
                    .iter()
                    .filter(|n| n.kind_code.is_some() && n.kind.is_none())
                    .count();
            }
            assert_eq!(painted, ends, "{id}: every threshold on the paint");
            // The designators' declination: the median of each threshold's
            // true course less its number's tens of degrees.
            let decl = {
                let mut r: Vec<f64> = residuals.iter().map(|r| r.0).collect();
                r.sort_by(f64::total_cmp);
                r.get(r.len() / 2).copied().unwrap_or(f64::NAN)
            };
            let offs: Vec<f64> = residuals.iter().map(|r| signed(r.0 - decl).abs()).collect();
            for (r, off) in residuals.iter().zip(&offs) {
                if *off > 5.0 {
                    outliers.push(format!("{} off by {off:.1}°", r.1));
                }
            }
            let mm = |v: &[f64]| {
                if v.is_empty() {
                    return "none".to_string();
                }
                let (lo, hi) = v
                    .iter()
                    .fold((f64::MAX, f64::MIN), |(l, h), &x| (l.min(x), h.max(x)));
                format!("{:.1} ({lo:.1}–{hi:.1})", percentile(v, 0.5))
            };
            println!(
                "| {id} | {build} | {} | {} | {no_id} | {no_runway} | {n_runways} | all | {true_off:.1e} | {convergence:.2} | {} of {n_runways} | {} | {width_off:.3} | {:.2} / {:.2} | {elev_over:.3} | {decl:+.1} | {:.1} / {:.1} | {painted} of {ends} | {unnamed} | {n_stands} |",
                tables.airdromes.len(),
                tables.airdromes.len(),
                widths.len(),
                mm(&widths),
                percentile(&spreads, 0.5),
                percentile(&spreads, 1.0),
                percentile(&offs, 0.95),
                percentile(&offs, 1.0),
            );
            let col = |v: &[(f64, f64)], k: usize, p: f64| {
                percentile(
                    &v.iter()
                        .map(|t| if k == 0 { t.0 } else { t.1 })
                        .collect::<Vec<_>>(),
                    p,
                )
            };
            timing.push(format!(
                "| {id} | {:.0} / {:.0} | {:.0} / {:.0} | {:.1} / {:.1} | {:.1} / {:.1} | {:.1} / {:.1} | {:.1} / {:.1} | {:.0} % / {:.0} % |",
                opened.0, opened.1, prime.0, prime.1,
                col(&cold, 0, 0.5), col(&cold, 1, 0.5),
                col(&cold, 0, 1.0), col(&cold, 1, 1.0),
                col(&warm, 0, 0.5), col(&warm, 1, 0.5),
                col(&warm, 0, 1.0), col(&warm, 1, 1.0),
                100.0 * width_spent.0 / warm.iter().map(|t| t.0).sum::<f64>(),
                100.0 * width_spent.1 / warm.iter().map(|t| t.1).sum::<f64>(),
            ));
        }
        println!();
        println!(
            "| terrain | open ms / Mcycles | tables | cold p50 ms / Mcycles | cold max | warm p50 | warm max | widths of warm, wall / cycles |"
        );
        println!("|---|---|---|---|---|---|---|---|");
        for row in timing {
            println!("{row}");
        }
        println!();
        for o in outliers {
            println!("- {o}");
        }
    }
}
