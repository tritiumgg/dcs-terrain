//! `score_site` and `find_sites`: the siting criteria at one site, and the
//! best sites in a region, spaced apart.
//!
//! `find_sites` is a multi-criteria suitability analysis on a lattice, as
//! a GIS weighted overlay runs it: every hard test first, as constraints
//! that remove a site outright, then the weighted sum of the soft scores
//! ranks what is left. Each candidate position keeps its best orientation.
//! The ranked list is thinned by greedy non-maximum suppression: walking it
//! best first, a candidate is kept unless a kept one lies nearer than
//! `spacing`, until `limit` are kept. Ties rank by x, then z, so an answer
//! never depends on the order work finished in.
//!
//! What the answer is exact for, and what it is not:
//!
//! - It is exact on the lattice. Between lattice points nothing is judged,
//!   so a site passing everywhere but on the lattice is not found, and the
//!   best site found lies within `step / √2` of every point of its cell
//!   of the lattice, the best among them included.
//! - Greedy suppression keeps the best candidate, and every candidate kept
//!   is the best of those `spacing` or more from all kept before it. It is
//!   not the set of `limit` spaced candidates with the greatest total
//!   score; finding that is NP-hard, and greedy reaches at least a fifth of
//!   it, because no more than five candidates `spacing` apart fit within
//!   `spacing` of one.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};

use serde_json::json;

use super::criteria::{Mode, Site, evaluate, name};
use super::geojson::{Features, props};
use super::scenery::Rect;
use super::{CELL_SIZE_M, Terrain, invalid, lattice, resolve};
use crate::types::{
    Candidate, Criterion, FindSitesRequest, FindSitesResponse, FindSitesStats, Footprint, Issue,
    ScoreSiteRequest, ScoreSiteResponse, Term,
};

/// The most candidates `find_sites` returns.
pub const LIMIT_MAX: u32 = 200;
/// The most grid cells a region's box may span: the layers a criterion
/// reads are derived over all of it at once, and this is about 100 km
/// square at 50 m.
pub const REGION_CELLS_MAX: f64 = (4u64 << 20) as f64;

/// The weighted sum of the term scores, each weight 1 unless named.
fn weighted(terms: &[Term], weights: &BTreeMap<String, f64>) -> f64 {
    terms
        .iter()
        .map(|t| weights.get(&t.name).copied().unwrap_or(1.0) * t.score)
        .sum()
}

/// The best of one position's sites, which lie in orientation order: one
/// that passes if any does, the highest score among those, the first on a
/// tie.
fn best<'s>(sites: &'s [Site], weights: &BTreeMap<String, f64>) -> (&'s Site, f64) {
    let mut out: Option<(&Site, f64)> = None;
    for s in sites {
        let v = weighted(&s.terms, weights);
        let better = match out {
            None => true,
            Some((b, bv)) => (s.pass, v) > (b.pass, bv),
        };
        if better {
            out = Some((s, v));
        }
    }
    out.expect("a footprint has an orientation")
}

/// The footprint at `site`'s orientation, its corners in order around it.
fn outline(site: &Site, footprint: &Footprint) -> [[f64; 2]; 4] {
    Rect::new(
        [site.x, site.z],
        site.orientation_deg.to_radians(),
        footprint.depth,
        footprint.width,
    )
    .corners()
}

pub fn score_site(terrain: &Terrain, q: ScoreSiteRequest) -> Result<ScoreSiteResponse, Vec<Issue>> {
    let e = evaluate(
        terrain.source(),
        &[[q.x, q.z]],
        &q.footprint,
        &q.criteria,
        Mode::Every,
    )?;
    let (site, score) = best(&e.sites, &BTreeMap::new());
    let geojson = if q.geojson {
        let mut f = Features::new(terrain)?;
        let p = props([
            ("orientation_deg", json!(site.orientation_deg)),
            ("score", json!(score)),
            ("hard_fail", json!(!site.pass)),
        ]);
        f.point([site.x, site.z], p.clone());
        f.polygon(&[outline(site, &q.footprint).to_vec()], p);
        Some(f.finish())
    } else {
        None
    };
    Ok(ScoreSiteResponse {
        provenance: terrain.provenance(),
        orientation_deg: site.orientation_deg,
        score,
        terms: site.terms.clone(),
        hard_fail: !site.pass,
        geojson,
    })
}

/// Ranks best first: the higher score, then the lesser x, then the lesser z.
fn rank(a: &(f64, f64, f64), b: &(f64, f64, f64)) -> Ordering {
    b.0.total_cmp(&a.0)
        .then(a.1.total_cmp(&b.1))
        .then(a.2.total_cmp(&b.2))
}

/// Greedy non-maximum suppression: of `ranked` positions, best first, the
/// indices of those kept, each `spacing` or more from every one kept before
/// it, at most `limit`. The kept ones are hashed on a grid of side
/// `spacing`, so a position nearer than that to a kept one lies in the
/// nine buckets about its own, each holding at most four kept ones: each
/// test is constant time.
pub(crate) fn suppress(ranked: &[[f64; 2]], spacing: f64, limit: usize) -> Vec<usize> {
    let mut kept = Vec::new();
    if spacing <= 0.0 {
        kept.extend((0..ranked.len()).take(limit));
        return kept;
    }
    let key = |p: [f64; 2]| {
        (
            (p[0] / spacing).floor() as i64,
            (p[1] / spacing).floor() as i64,
        )
    };
    let mut grid: HashMap<(i64, i64), Vec<[f64; 2]>> = HashMap::new();
    for (i, &p) in ranked.iter().enumerate() {
        if kept.len() == limit {
            break;
        }
        let (r, c) = key(p);
        let near = (-1..=1).any(|dr| {
            (-1..=1).any(|dc| {
                grid.get(&(r.saturating_add(dr), c.saturating_add(dc)))
                    .is_some_and(|b| b.iter().any(|q| (q[0] - p[0]).hypot(q[1] - p[1]) < spacing))
            })
        });
        if !near {
            grid.entry((r, c)).or_default().push(p);
            kept.push(i);
        }
    }
    kept
}

pub fn find_sites(terrain: &Terrain, q: FindSitesRequest) -> Result<FindSitesResponse, Vec<Issue>> {
    let src = terrain.source();
    let step = q.step.unwrap_or(2.0 * CELL_SIZE_M);
    if !(step >= CELL_SIZE_M && step.is_finite()) {
        return Err(invalid(
            &format!("step must be at least the cell size, {CELL_SIZE_M} m"),
            "step",
        ));
    }
    if !spacing_ok(q.spacing) {
        return Err(invalid(
            "spacing must be a finite length, 0 or more",
            "spacing",
        ));
    }
    if !(1..=LIMIT_MAX).contains(&q.limit) {
        return Err(invalid(
            &format!("limit must be from 1 to {LIMIT_MAX}"),
            "limit",
        ));
    }
    check_weights(&q.criteria, &q.weights)?;
    let area = resolve(src, &q.region, "region")?;
    let [x0, z0, x1, z1] = area.bbox;
    let cells =
        ((x1 - x0) / CELL_SIZE_M).ceil().max(1.0) * ((z1 - z0) / CELL_SIZE_M).ceil().max(1.0);
    if cells > REGION_CELLS_MAX {
        return Err(vec![Issue::new(
            "too_large",
            &format!(
                "the region's box spans {cells:.0} cells, over the {REGION_CELLS_MAX:.0} a search \
                 may, about 100 km square; split the region"
            ),
            "region",
        )]);
    }
    // Under the cap on cells, and a step of at least a cell, the lattice
    // holds fewer points than the cap on its rows.
    let cands = lattice(&area, step, u32::MAX)?;
    let e = evaluate(src, &cands, &q.footprint, &q.criteria, Mode::Prune)?;

    // Each position's best orientation; its sites lie together.
    let mut best_sites: Vec<(&Site, f64)> = Vec::new();
    let mut i = 0;
    while i < e.sites.len() {
        let (x, z) = (e.sites[i].x, e.sites[i].z);
        let n = e.sites[i..]
            .iter()
            .take_while(|s| s.x == x && s.z == z)
            .count();
        best_sites.push(best(&e.sites[i..i + n], &q.weights));
        i += n;
    }
    best_sites.sort_by(|a, b| rank(&(a.1, a.0.x, a.0.z), &(b.1, b.0.x, b.0.z)));
    let at: Vec<[f64; 2]> = best_sites.iter().map(|(s, _)| [s.x, s.z]).collect();
    let kept = suppress(&at, q.spacing, q.limit as usize);
    let candidates: Vec<Candidate> = kept
        .iter()
        .map(|&k| {
            let (s, score) = best_sites[k];
            Candidate {
                x: s.x,
                z: s.z,
                orientation_deg: s.orientation_deg,
                score,
                terms: s.terms.clone(),
            }
        })
        .collect();
    let geojson = if q.geojson {
        let mut f = Features::new(terrain)?;
        for (n, &k) in kept.iter().enumerate() {
            let (s, score) = best_sites[k];
            let p = props([
                ("rank", json!(n + 1)),
                ("score", json!(score)),
                ("orientation_deg", json!(s.orientation_deg)),
            ]);
            f.point([s.x, s.z], p.clone());
            f.polygon(&[outline(s, &q.footprint).to_vec()], p);
        }
        Some(f.finish())
    } else {
        None
    };
    Ok(FindSitesResponse {
        provenance: terrain.provenance(),
        candidates,
        stats: FindSitesStats {
            cells_considered: e.stats.candidates,
            cells_passed: e.stats.candidates_passed,
            tiles_skipped: e.stats.tiles_skipped,
        },
        geojson,
    })
}

fn spacing_ok(s: f64) -> bool {
    s.is_finite() && s >= 0.0
}

/// Each weight finite and 0 or more, and naming a criterion asked for or
/// `valid`, which is always applied.
fn check_weights(
    criteria: &[Criterion],
    weights: &BTreeMap<String, f64>,
) -> Result<(), Vec<Issue>> {
    let mut issues = Vec::new();
    for (k, &w) in weights {
        let path = format!("weights.{k}");
        if !(w.is_finite() && w >= 0.0) {
            issues.push(Issue::new(
                "invalid_request",
                "a weight must be finite, 0 or more",
                &path,
            ));
        }
        if k != "valid" && !criteria.iter().any(|c| name(c) == *k) {
            issues.push(Issue::new(
                "invalid_request",
                &format!("no criterion `{k}` is asked for"),
                &path,
            ));
        }
    }
    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

#[cfg(test)]
mod tests;
