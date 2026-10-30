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
use crate::crs::{Tmerc, grid_bearing};
use crate::decode::surface5::ground::SURFACE_RUNWAY;
use crate::source::install::AirdromeRow;
use crate::source::{Refusal, Source, Tables};
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

/// The runway's width at each station `stations` gives: the ground there
/// must be runway, and each side's edge is where the first stretch off it
/// of `WIDTH_SEAM_M` or more begins, found to `WIDTH_STEP_M` and then
/// bisected. `None` at a station whose centerline is not runway, or whose
/// edge lies past `WIDTH_REACH_M`. Each look is one batch of points for
/// every side at once, so a runway costs a few batches.
fn runway_widths(
    src: &dyn Source,
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
    let on = |g: &Option<crate::source::Ground>| g.is_some_and(|g| g.surface == SURFACE_RUNWAY);
    let coarse = (WIDTH_REACH_M / WIDTH_COARSE_M).round() as usize;
    let fine = (WIDTH_COARSE_M / WIDTH_STEP_M).round() as usize;
    let seam = (WIDTH_SEAM_M / WIDTH_STEP_M).ceil() as usize;
    let pts: Vec<[f64; 2]> = (sides.iter())
        .flat_map(|&s| (0..=coarse).map(move |k| at(s, k as f64 * WIDTH_COARSE_M)))
        .collect();
    let steps: Vec<Vec<bool>> = (src.points(&pts)?.chunks(coarse + 1))
        .map(|g| g.iter().map(on).collect())
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
        let n = fine + seam + 1;
        let pts: Vec<[f64; 2]> = (open.iter())
            .flat_map(|&(s, k)| {
                let (side, from) = (sides[s], (k - 1) as f64 * WIDTH_COARSE_M);
                (0..n).map(move |j| at(side, from + j as f64 * WIDTH_STEP_M))
            })
            .collect();
        let ground = src.points(&pts)?;
        let mut still = Vec::new();
        for (&(s, k), g) in open.iter().zip(ground.chunks(n)) {
            let off = |j: usize| g[j..j + seam].iter().all(|g| !on(g));
            match (1..=fine).find(|&j| on(&g[j - 1]) && off(j)) {
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
        let mut g = src.points(&mids)?.into_iter();
        for b in brackets.iter_mut().flatten() {
            let m = 0.5 * (b[0] + b[1]);
            if on(&g.next().expect("a point per bracket")) {
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

/// The median of the values given, the mean of the middle two of an even
/// count; `None` of none.
fn median(mut v: Vec<f64>) -> Option<f64> {
    v.sort_by(f64::total_cmp);
    let n = v.len();
    match n {
        0 => None,
        _ if n % 2 == 1 => Some(v[n / 2]),
        _ => Some(0.5 * (v[n / 2 - 1] + v[n / 2])),
    }
}

/// Degrees in [0, 360) of radians from north towards east.
fn degrees(rad: f64) -> f64 {
    rad.to_degrees().rem_euclid(360.0) % 360.0 + 0.0
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

/// Whether `f` fits a stand of this size and use; `None` where a size or
/// use it needs is unknown.
fn fits(f: &Airframe, s: &StandInfo) -> Option<bool> {
    let room = f.width <= s.width? && f.length <= s.length? && f.height <= s.height?;
    let usable = if f.helicopter {
        s.helicopters?
    } else {
        s.airplanes?
    };
    Some(room && usable)
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
            height: (s.params.HEIGHT.as_deref()).and_then(|h| h.trim().parse().ok()),
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
    let elevation = match (runways.is_empty(), centerline_top) {
        (false, top) => top,
        (true, _) => {
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
    use crate::decode::surface5::ground::SURFACE_RUNWAY;
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
                .filter(|g| g.is_some_and(|g| g.surface == SURFACE_RUNWAY))
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

