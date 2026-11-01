//! Scores DCS's replies against the plan and states the verdict.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use crate::plan::{HeightCase, MgrsCase, Plan, RouteCase, SightCase};
use crate::stats::{distribution, wilson};

/// The accuracy targets. A height and a reference are held at every point,
/// the agreement rates at their own figures.
pub const HEIGHT_MAX_M: f64 = 0.5;
pub const WATER_AGREEMENT: f64 = 1.0;
pub const SIGHT_AGREEMENT: f64 = 0.99;
pub const ROUTE_WITHIN: f64 = 0.05;
pub const ROUTE_SHARE: f64 = 0.95;
pub const MGRS_MAX_M: f64 = 2.0;

const EARTH_RADIUS_M: f64 = 6_371_008.8;
const SQUARE_M: f64 = 100_000.0;

/// The values of every reply of `kind`, in order, each batch checked against
/// the plan's terrain and build.
fn replies(dir: &Path, kind: &str, plan: &Plan, want: usize) -> Result<Vec<Value>, String> {
    let mut out = Vec::new();
    let mut i = 0;
    while out.len() < want {
        let path = dir.join("replies").join(format!("{kind}-{i:03}.json"));
        let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let v: Value =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        if v["terrain"] != plan.terrain.as_str() || v["build"] != plan.dcs_build.as_str() {
            return Err(format!(
                "{} is from {} on {}, the plan is {} on {}",
                path.display(),
                v["terrain"],
                v["build"],
                plan.terrain,
                plan.dcs_build
            ));
        }
        let values = v["values"].as_array().ok_or("a reply has no values")?;
        if values.is_empty() {
            return Err(format!("{} holds nothing", path.display()));
        }
        out.extend(values.iter().cloned());
        i += 1;
    }
    if out.len() != want {
        return Err(format!("{kind}: {} replies for {want} cases", out.len()));
    }
    Ok(out)
}

/// Whether a distribution's largest error is at most `max`. An error that is
/// no number is not within anything.
fn at_most(d: &Value, max: f64) -> bool {
    d["max"].as_f64().is_some_and(|m| m <= max)
}

/// Whether DCS's name or number for a surface is water.
fn is_water(surface: &str) -> bool {
    matches!(surface, "water" | "sea" | "lake" | "river" | "2" | "3")
}

fn heights(cases: &[HeightCase], live: &[Value]) -> Value {
    let mut all = Vec::new();
    let mut strata: BTreeMap<&str, Vec<f64>> = BTreeMap::new();
    let (mut same, mut n, mut missing) = (0, 0, 0);
    let mut worst = Vec::new();
    for (c, v) in cases.iter().zip(live) {
        n += 1;
        same += usize::from(
            v[1].as_str()
                .is_some_and(|s| is_water(s) == (c.water != 0.0)),
        );
        let Some(h) = v[0].as_f64() else {
            missing += 1;
            continue;
        };
        let e = c.height - h;
        all.push(e);
        strata.entry(&c.stratum).or_default().push(e);
        worst.push((e.abs(), c));
    }
    worst.sort_by(|a, b| b.0.total_cmp(&a.0));
    let strata: BTreeMap<&str, Value> = strata.iter().map(|(k, v)| (*k, distribution(v))).collect();
    let d = distribution(&all);
    let height_pass = !all.is_empty() && missing == 0 && at_most(&d, HEIGHT_MAX_M);
    let water = same as f64 / n.max(1) as f64;
    let (lo, hi) = wilson(same, n);
    let worst: Vec<Value> = worst
        .iter()
        .take(10)
        .map(|(e, c)| json!({"x": c.x, "z": c.z, "stratum": c.stratum, "error": e}))
        .collect();
    json!({
        "height": {"all": d, "strata": strata, "missing": missing, "worst": worst,
                   "target_max_m": HEIGHT_MAX_M, "pass": height_pass},
        "water": {"n": n, "agreement": water, "interval_95": [lo, hi],
                  "target": WATER_AGREEMENT, "pass": n > 0 && water >= WATER_AGREEMENT},
    })
}

fn sights(cases: &[SightCase], live: &[Value]) -> Value {
    let mut sets: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    let mut strata: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    let mut wrong = Vec::new();
    for (c, v) in cases.iter().zip(live) {
        let same = v.as_bool() == Some(c.visible);
        let s = sets.entry(&c.set).or_default();
        s.0 += usize::from(same);
        s.1 += 1;
        if c.set == "random" {
            let s = strata.entry(&c.stratum).or_default();
            s.0 += usize::from(same);
            s.1 += 1;
        }
        if !same {
            wrong.push(
                json!({"set": c.set, "stratum": c.stratum, "a": c.a, "b": c.b,
                              "tool": c.visible, "clearance_m": c.clearance_m}),
            );
        }
    }
    let summary = |&(same, n): &(usize, usize)| {
        let (lo, hi) = wilson(same, n);
        json!({"n": n, "agreement": same as f64 / n.max(1) as f64, "interval_95": [lo, hi]})
    };
    let sets_json: BTreeMap<&str, Value> = sets.iter().map(|(k, v)| (*k, summary(v))).collect();
    let strata: BTreeMap<&str, Value> = strata.iter().map(|(k, v)| (*k, summary(v))).collect();
    let (same, n) = sets.get("random").copied().unwrap_or((0, 0));
    wrong.truncate(40);
    json!({"sets": sets_json, "strata": strata, "disagreements": wrong,
           "target": SIGHT_AGREEMENT,
           "pass": n > 0 && same as f64 / n as f64 >= SIGHT_AGREEMENT})
}

fn routes(cases: &[RouteCase], live: &[Value]) -> Value {
    let mut errors: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let mut none = 0;
    let mut within = 0;
    let mut all = Vec::new();
    let mut worst = Vec::new();
    for (c, v) in cases.iter().zip(live) {
        let Some(length) = v[0].as_f64().filter(|l| *l > 0.0) else {
            none += 1;
            continue;
        };
        let e = c.length / length - 1.0;
        all.push(e);
        within += usize::from(e.abs() <= ROUTE_WITHIN);
        errors
            .entry(format!("{}/{}", c.network, c.stratum))
            .or_default()
            .push(e);
        worst.push((e.abs(), c, length));
    }
    worst.sort_by(|a, b| b.0.total_cmp(&a.0));
    let strata: BTreeMap<&String, Value> =
        errors.iter().map(|(k, v)| (k, distribution(v))).collect();
    let share = within as f64 / all.len().max(1) as f64;
    let worst: Vec<Value> = worst
        .iter()
        .take(10)
        .map(|(_, c, l)| {
            json!({"network": c.network, "from": c.from, "to": c.to, "tool": c.length, "dcs": l})
        })
        .collect();
    json!({"ratio_minus_1": distribution(&all), "strata": strata, "no_path": none,
           "share_within": share, "worst": worst,
           "target_within": ROUTE_WITHIN, "target_share": ROUTE_SHARE,
           "pass": !all.is_empty() && none == 0 && share >= ROUTE_SHARE})
}

/// The ground distance between a reference and a zone, easting and northing,
/// taken within the 100 km square: across a square's edge the letters differ
/// and the digits wrap, which is a meter or two and not 100 km. Two zones
/// share no grid, so `None`.
fn gap(tool: &str, zone: &str, e: f64, n: f64) -> Option<f64> {
    let mut p = tool.split(' ');
    let (z, _, te, tn) = (p.next()?, p.next()?, p.next()?, p.next()?);
    if z != zone {
        return None;
    }
    let wrap = |d: f64| (d + SQUARE_M / 2.0).rem_euclid(SQUARE_M) - SQUARE_M / 2.0;
    let de = wrap(te.parse::<f64>().ok()? - e.floor());
    let dn = wrap(tn.parse::<f64>().ok()? - n.floor());
    Some(de.hypot(dn))
}

/// Whether a reference and DCS's digits lie across a 100 km square's edge, the
/// one place their letters may rightly differ.
fn across_edge(tool: &str, e: f64, n: f64) -> bool {
    let mut p = tool.split(' ').skip(2);
    let digits = |t: Option<&str>| t.and_then(|t| t.parse::<f64>().ok());
    let (Some(te), Some(tn)) = (digits(p.next()), digits(p.next())) else {
        return false;
    };
    (te - e.floor()).abs() > SQUARE_M / 2.0 || (tn - n.floor()).abs() > SQUARE_M / 2.0
}

fn great_circle(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (la, lb) = (a.0.to_radians(), b.0.to_radians());
    let (dla, dlo) = (lb - la, (b.1 - a.1).to_radians());
    let h = (dla / 2.0).sin().powi(2) + la.cos() * lb.cos() * (dlo / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_M * h.sqrt().asin()
}

fn mgrs(cases: &[MgrsCase], live: &[Value]) -> Value {
    let (mut ends, mut projection, mut converter) = (Vec::new(), Vec::new(), Vec::new());
    let (mut other_zone, mut same_letters, mut wrong_letters) = (0, 0, 0);
    let num = |v: &Value, i: usize| v[i].as_f64().unwrap_or(f64::NAN);
    for (c, v) in cases.iter().zip(live) {
        projection.push(great_circle((c.lat, c.lon), (num(v, 0), num(v, 1))));
        let zone = v[2].as_str().unwrap_or("");
        let square = v[3].as_str().unwrap_or("");
        let same = c.mgrs.starts_with(&format!("{zone} {square} "));
        same_letters += usize::from(same);
        wrong_letters += usize::from(!same && !across_edge(&c.mgrs, num(v, 4), num(v, 5)));
        match gap(&c.mgrs, zone, num(v, 4), num(v, 5)) {
            Some(g) => ends.push(g),
            None => other_zone += 1,
        }
        let zone = v[6].as_str().unwrap_or("");
        converter.push(gap(&c.mgrs, zone, num(v, 8), num(v, 9)).unwrap_or(f64::MAX));
    }
    let d = distribution(&ends);
    let pass = !ends.is_empty() && other_zone == 0 && wrong_letters == 0 && at_most(&d, MGRS_MAX_M);
    json!({"end_to_end_m": d, "projection_m": distribution(&projection),
           "converter_m": distribution(&converter), "other_zone": other_zone,
           "same_letters": same_letters, "wrong_letters": wrong_letters, "n": cases.len(),
           "target_max_m": MGRS_MAX_M, "pass": pass})
}

/// The civil date and time of `seconds` since the epoch, as a bare UTC
/// stamp; the calendar is Howard Hinnant's `civil_from_days`.
fn stamp(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64 + 719_468;
    let era = days.div_euclid(146_097);
    let doe = days.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    let s = seconds % 86_400;
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}Z", s / 3600, s / 60 % 60)
}

/// The figures of a finished sortie; their `pass` says whether every target
/// held.
pub fn report(plan: &Plan, dir: &Path) -> Result<Value, String> {
    let ground = heights(
        &plan.heights,
        &replies(dir, "heights", plan, plan.heights.len())?,
    );
    let sight = sights(
        &plan.sights,
        &replies(dir, "sights", plan, plan.sights.len())?,
    );
    let route = routes(
        &plan.routes,
        &replies(dir, "routes", plan, plan.routes.len())?,
    );
    let grid = mgrs(&plan.mgrs, &replies(dir, "mgrs", plan, plan.mgrs.len())?);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs();
    let pass = [&ground["height"], &ground["water"], &sight, &route, &grid]
        .iter()
        .all(|f| f["pass"] == true);
    Ok(json!({
        "terrain": plan.terrain,
        "dcs_build": plan.dcs_build,
        "terrain_fingerprint": plan.terrain_fingerprint,
        "validated_at": stamp(now),
        "seed": plan.seed,
        "pass": pass,
        "height_rmse_m": ground["height"]["all"]["rmse"],
        "los_agreement": sight["sets"]["random"]["agreement"],
        "route_ratio_p95": route["ratio_minus_1"]["p95"],
        "height": ground["height"],
        "water": ground["water"],
        "visible": sight,
        "route": route,
        "mgrs": grid,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamp_reads_the_calendar() {
        assert_eq!(stamp(0), "1970-01-01T00:00Z");
        assert_eq!(stamp(951_782_400 + 86_399), "2000-02-29T23:59Z");
    }

    #[test]
    fn gap_measures_across_a_square_edge() {
        assert_eq!(gap("37T GG 99999 00000", "37T", 0.0, 0.0), Some(1.0));
        assert_eq!(gap("37T GG 1 1", "38T", 1.0, 1.0), None);
    }

    #[test]
    fn only_digits_that_wrap_lie_across_an_edge() {
        assert!(across_edge("37T GG 99999 00000", 0.0, 0.0));
        assert!(!across_edge("37T GG 12345 00000", 12_345.5, 0.0));
    }

    #[test]
    fn no_number_is_within_a_target() {
        assert!(!at_most(&json!({"max": null}), 2.0));
        assert!(at_most(&json!({"max": 2.0}), 2.0));
    }

    #[test]
    fn a_degree_of_latitude_is_about_111_km() {
        let d = great_circle((0.0, 0.0), (1.0, 0.0));
        assert!((d - 111_195.0).abs() < 1.0, "{d}");
    }
}
