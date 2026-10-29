//! `approach_spectrum`: from each bearing about a site, how near a target
//! at a height above the ground comes before the site sees it.
//!
//! Read off the radar floor: each radial is marched in from the radius
//! every half cell, and a target there is seen where the floor of the cell
//! under it is at or below its height. The original design marched each
//! radial with its own sightlines; the floor is the same geometry, swept
//! once for every bearing and band.

use serde_json::json;

use super::Terrain;
use super::floor::{Floor, check_bands, site};
use super::geojson::{Features, Ring, props};
use crate::types::{
    ApproachSpectrumRequest, ApproachSpectrumResponse, Issue, Radial, Sector, SpectrumBand,
    SpectrumSummary,
};

/// The issue with the range a blind radial must stay masked in to, if it
/// is not finite and at least 0.
pub(super) fn check_min_masked_range(range: f64) -> Option<Issue> {
    (!(range.is_finite() && range >= 0.0)).then(|| {
        Issue::new(
            "invalid_request",
            "the least masked range must be finite and at least 0",
            "min_masked_range",
        )
    })
}

/// One radial at height `h` above the ground, at `bearing` degrees from
/// north towards east.
pub(super) fn radial(fl: &Floor, bearing: f64, h: f64, min_masked_range: f64) -> Radial {
    let (sin, cos) = bearing.to_radians().sin_cos();
    let step = fl.res / 2.0;
    let (mut unmask, mut judged, mut masked) = (None, 0u32, 0u32);
    let mut k = (fl.radius / step).floor() as u64;
    while k > 0 {
        let d = k as f64 * step;
        if let Some(floor) = fl.at(fl.site.0 + d * cos, fl.site.1 + d * sin) {
            judged += 1;
            if floor <= h {
                unmask.get_or_insert(d);
            } else {
                masked += 1;
            }
        }
        k -= 1;
    }
    Radial {
        bearing_deg: bearing,
        unmask_dist: unmask,
        masked_fraction: if judged == 0 {
            0.0
        } else {
            f64::from(masked) / f64::from(judged)
        },
        // A radial with no ground under it is no way in.
        blind: judged > 0 && unmask.is_none_or(|u| u < min_masked_range),
    }
}

/// The summaries over `radials`, in bearing order and `spacing` degrees
/// apart; `round` when they go all the way round, so a run of blind
/// radials may close over north.
fn summarize(radials: &[&Radial], spacing: f64, round: bool) -> SpectrumSummary {
    let n = radials.len();
    let blind: Vec<bool> = radials.iter().map(|r| r.blind).collect();
    let count = blind.iter().filter(|&&b| b).count();
    // Runs of blind radials, first and length; one that closes over north
    // starts at its first radial after a seen one.
    let mut runs: Vec<(usize, usize)> = Vec::new();
    if count == n && round && n > 0 {
        runs.push((0, n));
    } else {
        let start = if round {
            (0..n).find(|&i| !blind[i]).map_or(0, |i| i + 1)
        } else {
            0
        };
        let mut i = 0;
        while i < n {
            let at = (start + i) % n;
            if blind[at] && !(i > 0 && blind[(start + i - 1) % n]) {
                let len = (i..n).take_while(|&j| blind[(start + j) % n]).count();
                runs.push((at, len));
                i += len;
            } else {
                i += 1;
            }
        }
    }
    let arcs = runs
        .iter()
        .map(|&(first, len)| {
            if len == n && round {
                Sector {
                    from_deg: 0.0,
                    to_deg: 360.0,
                }
            } else {
                let last = radials[(first + len - 1) % n].bearing_deg;
                Sector {
                    from_deg: (radials[first].bearing_deg - spacing / 2.0).rem_euclid(360.0),
                    to_deg: (last + spacing / 2.0).rem_euclid(360.0),
                }
            }
        })
        .collect();
    let mut seen: Vec<f64> = radials.iter().filter_map(|r| r.unmask_dist).collect();
    seen.sort_unstable_by(f64::total_cmp);
    let median = match seen.len() {
        0 => None,
        m if m % 2 == 1 => Some(seen[m / 2]),
        m => Some((seen[m / 2 - 1] + seen[m / 2]) / 2.0),
    };
    SpectrumSummary {
        blind_arc_fraction: if n == 0 { 0.0 } else { count as f64 / n as f64 },
        widest_blind_arc_deg: runs
            .iter()
            .map(|&(_, len)| len as f64 * spacing)
            .fold(0.0, f64::max),
        blind_arcs: arcs,
        unmask_dist_min: seen.first().copied(),
        unmask_dist_median: median,
        unmask_dist_max: seen.last().copied(),
    }
}

/// Whether `bearing` lies on the arc clockwise from `s.from_deg` to
/// `s.to_deg`, the whole circle when the two are a turn apart or equal.
fn in_sector(s: &Sector, bearing: f64) -> bool {
    let span = (s.to_deg - s.from_deg).rem_euclid(360.0);
    span == 0.0 || (bearing - s.from_deg).rem_euclid(360.0) <= span
}

/// The wedge from the site over the arc `s`, out to `radius`: a circle when
/// the arc goes all the way round.
pub(super) fn wedge(site: (f64, f64), radius: f64, s: &Sector) -> Ring {
    let span = (s.to_deg - s.from_deg).rem_euclid(360.0);
    let whole = span == 0.0;
    let span = if whole { 360.0 } else { span };
    let n = (span / 2.0).ceil().max(1.0) as u32;
    let mut ring: Ring = if whole {
        Vec::new()
    } else {
        vec![[site.0, site.1]]
    };
    let last = if whole { n - 1 } else { n };
    for k in 0..=last {
        let b = (s.from_deg + span * f64::from(k) / f64::from(n)).to_radians();
        ring.push([site.0 + radius * b.cos(), site.1 + radius * b.sin()]);
    }
    ring
}

pub fn approach_spectrum(
    terrain: &Terrain,
    q: ApproachSpectrumRequest,
) -> Result<ApproachSpectrumResponse, Vec<Issue>> {
    let mut issues: Vec<Issue> = check_bands(&q.alt_bands_agl, "alt_bands_agl")
        .into_iter()
        .collect();
    if !(q.azimuth_step_deg.is_finite() && q.azimuth_step_deg >= 0.1 && q.azimuth_step_deg <= 90.0)
    {
        issues.push(Issue::new(
            "invalid_request",
            "the azimuth step must be at least 0.1 and at most 90 degrees",
            "azimuth_step_deg",
        ));
    }
    issues.extend(check_min_masked_range(q.min_masked_range));
    if let Some(s) = &q.sector
        && !(s.from_deg.is_finite() && s.to_deg.is_finite())
    {
        issues.push(Issue::new(
            "invalid_request",
            "a sector's bearings must be finite",
            "sector",
        ));
    }
    let fl = Floor::sweep(
        terrain,
        q.site,
        ["site", "radius"],
        q.radius,
        q.resolution,
        issues,
    )?;
    // As many radials as divide the circle evenly at about the step asked.
    let n = (360.0 / q.azimuth_step_deg).round().max(1.0) as u32;
    let spacing = 360.0 / f64::from(n);
    let bands: Vec<SpectrumBand> = q
        .alt_bands_agl
        .iter()
        .map(|&h| {
            let radials: Vec<Radial> = (0..n)
                .map(|k| {
                    let b = f64::from(k) * spacing;
                    radial(&fl, b, h, q.min_masked_range)
                })
                .collect();
            let all: Vec<&Radial> = radials.iter().collect();
            let summary = summarize(&all, spacing, true);
            let sector_summary = q.sector.map(|s| {
                let mine: Vec<&Radial> = radials
                    .iter()
                    .filter(|r| in_sector(&s, r.bearing_deg))
                    .collect();
                let whole = mine.len() == radials.len();
                // Over a part of the circle, its radials from the sector's
                // first bearing on, so a run does not close over north.
                let first = mine
                    .iter()
                    .position(|r| (r.bearing_deg - s.from_deg).rem_euclid(360.0) < spacing)
                    .unwrap_or(0);
                let mut ordered = mine.clone();
                ordered.rotate_left(if whole { 0 } else { first });
                summarize(&ordered, spacing, whole)
            });
            SpectrumBand {
                alt_agl: h,
                radials,
                summary,
                sector_summary,
            }
        })
        .collect();
    let geojson = if q.geojson {
        let mut f = Features::new(terrain)?;
        for b in &bands {
            for s in &b.summary.blind_arcs {
                f.polygon(
                    &[wedge(fl.site, fl.radius, s)],
                    props([
                        ("alt_agl", json!(b.alt_agl)),
                        ("from_deg", json!(s.from_deg)),
                        ("to_deg", json!(s.to_deg)),
                    ]),
                );
            }
        }
        site(&mut f, q.site);
        Some(f.finish())
    } else {
        None
    };
    Ok(ApproachSpectrumResponse {
        provenance: terrain.provenance(),
        bands,
        geojson,
    })
}

#[cfg(test)]
mod tests {
    use super::super::floor::tests::{profile, site};
    use super::super::geojson::tests::assert_valid;
    use super::super::tests::synth;
    use super::super::{Terrain, answer};
    use serde_json::{Value, json};

    fn ask(t: &Terrain, extra: Value) -> Value {
        let (x, z) = site();
        let mut q = json!({
            "site": {"x": x, "z": z, "alt_agl": 10}, "radius": 12000,
            "alt_bands_agl": [30], "azimuth_step_deg": 1, "min_masked_range": 7000,
        });
        q.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        answer(t, "approach_spectrum", &q).unwrap()
    }

    /// The range a target 30 m up coming in from 12 km is first seen at
    /// on the closed-form ground, and whether it stays masked in to 7 km.
    fn predicted(t: &Terrain, bearing: f64) -> (Option<f64>, bool) {
        let p = profile(t, bearing, 12000.0);
        let unmask = p.iter().rev().find(|(_, f)| *f <= 30.0).map(|(d, _)| *d);
        (unmask, unmask.is_none_or(|u| u < 7000.0))
    }

    #[test]
    fn the_blind_arc_lies_behind_the_hill_as_wide_as_its_geometry() {
        let t = synth();
        let v = ask(&t, json!({"geojson": true}));
        let band = &v["bands"][0];
        let radials = band["radials"].as_array().unwrap();
        assert_eq!(radials.len(), 360);
        // The geometry's blind bearings, every quarter degree.
        let blind: Vec<f64> = (0..1440)
            .map(|k| f64::from(k) / 4.0)
            .filter(|&b| predicted(&t, b).1)
            .collect();
        let width = blind.len() as f64 / 4.0;
        assert!(blind.iter().all(|b| (45.0..135.0).contains(b)), "{blind:?}");
        let arcs = band["blind_arcs"].as_array().unwrap();
        assert_eq!(arcs.len(), 1, "{arcs:?}");
        let (from, to) = (
            arcs[0]["from_deg"].as_f64().unwrap(),
            arcs[0]["to_deg"].as_f64().unwrap(),
        );
        // East, behind the hill, and as wide as the geometry to a radial
        // either side.
        assert!(from < 90.0 && to > 90.0, "{from} {to}");
        let widest = band["widest_blind_arc_deg"].as_f64().unwrap();
        assert!((widest - width).abs() <= 2.0, "{widest} against {width}");
        assert!(width > 10.0, "{width}");
        assert_eq!(band["blind_arc_fraction"], widest / 360.0);
        // Where the target is seen, it is first seen where the geometry
        // says, to a cell or two.
        let (mut both, mut near, mut differ) = (0, 0, 0);
        for r in radials {
            let b = r["bearing_deg"].as_f64().unwrap();
            let (want, blind) = predicted(&t, b);
            differ += usize::from(r["blind"] != blind);
            if let (Some(got), Some(want)) = (r["unmask_dist"].as_f64(), want) {
                both += 1;
                near += usize::from((got - want).abs() <= 100.0);
            }
        }
        assert!(near as f64 >= 0.95 * both as f64, "{near} of {both}");
        // Only a radial at each edge of the arc may be judged otherwise.
        assert!(differ <= 2, "{differ}");
        let g = &v["geojson"];
        assert_valid(g);
        assert_eq!(g["features"][0]["geometry"]["type"], "Polygon");
        assert_eq!(g["features"][0]["properties"]["from_deg"], from);
    }

    #[test]
    fn a_sector_summarizes_only_its_radials() {
        let t = synth();
        let whole = &ask(&t, json!({}))["bands"][0];
        let east = &ask(&t, json!({"sector": {"from_deg": 30, "to_deg": 150}}))["bands"][0];
        let s = &east["sector_summary"];
        assert_eq!(s["widest_blind_arc_deg"], whole["widest_blind_arc_deg"]);
        assert_eq!(s["blind_arcs"], whole["blind_arcs"]);
        let fraction = s["blind_arc_fraction"].as_f64().unwrap();
        let n = whole["blind_arc_fraction"].as_f64().unwrap() * 360.0;
        assert!((fraction - n / 121.0).abs() < 1e-9, "{fraction}");
        // Over north, the west of the hill: nothing blind.
        let north = &ask(&t, json!({"sector": {"from_deg": 300, "to_deg": 20}}))["bands"][0];
        let s = &north["sector_summary"];
        assert_eq!(s["blind_arc_fraction"], 0.0);
        assert_eq!(s["blind_arcs"], json!([]));
        assert!(s["unmask_dist_min"].as_f64().unwrap() <= s["unmask_dist_max"].as_f64().unwrap());
    }

    #[test]
    fn refuses_a_step_too_fine_and_a_bad_range() {
        let t = synth();
        let (x, z) = site();
        let e = answer(
            &t,
            "approach_spectrum",
            &json!({"site": {"x": x, "z": z, "alt_agl": 10}, "radius": 5000,
                    "alt_bands_agl": [30], "azimuth_step_deg": 1e-9, "min_masked_range": -1}),
        )
        .unwrap_err();
        let paths: Vec<_> = e.iter().map(|i| i.path.as_str()).collect();
        assert_eq!(paths, ["azimuth_step_deg", "min_masked_range"]);
    }

    #[test]
    fn runs_of_blind_radials_close_over_north_and_a_full_circle_is_one_arc() {
        use super::{summarize, wedge};
        use crate::types::{Radial, Sector};
        let r = |b: f64, blind: bool, d: Option<f64>| Radial {
            bearing_deg: b,
            unmask_dist: d,
            masked_fraction: 0.0,
            blind,
        };
        let radials = [
            r(0.0, true, None),
            r(90.0, false, Some(4.0)),
            r(180.0, false, Some(1.0)),
            r(270.0, true, Some(2.0)),
        ];
        let all: Vec<&Radial> = radials.iter().collect();
        let s = summarize(&all, 90.0, true);
        assert_eq!(
            s.blind_arcs,
            [Sector {
                from_deg: 225.0,
                to_deg: 45.0
            }]
        );
        assert_eq!((s.widest_blind_arc_deg, s.blind_arc_fraction), (180.0, 0.5));
        assert_eq!(s.unmask_dist_median, Some(2.0));
        let blind: Vec<Radial> = (0..4).map(|k| r(f64::from(k) * 90.0, true, None)).collect();
        let all: Vec<&Radial> = blind.iter().collect();
        let s = summarize(&all, 90.0, true);
        assert_eq!(s.blind_arcs.len(), 1);
        assert_eq!(
            (s.blind_arcs[0].from_deg, s.blind_arcs[0].to_deg),
            (0.0, 360.0)
        );
        assert_eq!(s.unmask_dist_median, None);
        // The whole circle draws as a ring without the site.
        let w = wedge((0.0, 0.0), 10.0, &s.blind_arcs[0]);
        assert_eq!(w.len(), 180);
        assert!(w.iter().all(|p| (p[0].hypot(p[1]) - 10.0).abs() < 1e-9));
    }
}
