//! An airdrome's id where `Radio.lua`'s label gives none, joined by
//! position: a spatial join of the stations that carry an id in their name,
//! `airfieldN_k`, to the airdromes, then a unanimous vote per `N`.
//!
//! Each station votes for one airdrome or for none. `N` is kept for an
//! airdrome only when every station of `N` votes for it, no other `N` is
//! kept for it, and no airdrome holds `N` already. So a join never moves an
//! id, never gives two airdromes one id, and gives none where its stations
//! disagree: a station that votes wrongly costs an id, never a wrong one,
//! unless every station of `N` lies at the wrong airdrome.
//!
//! The airdromes and the stations each number at most a few hundred on a
//! terrain, so each vote scans every airdrome: exact, with no index to
//! build, and both joins take about a millisecond on the largest terrain.

use std::collections::{BTreeMap, BTreeSet};

/// How far a beacon of an airdrome lies from its reference point, at most:
/// an outer marker sits 7 to 11 km out on the approach.
pub const BEACON_REACH_M: f64 = 15_000.0;

/// How far a station's scene object, its tower, lies outside the box of
/// its airdrome's runway ends and stands, at most. Measured on 2.9.30.28738,
/// every tower of an airdrome `Radio.lua` labels lies within 520 m of its
/// own box, and no other box lies as near it.
pub const TOWER_REACH_M: f64 = 1_000.0;

/// The index of the nearest of `distances` within `reach`, `None` standing
/// for a site that has no position; `None` when none is within reach, or
/// when two lie at the same distance, since then the station names neither.
pub fn nearest(distances: impl Iterator<Item = Option<f64>>, reach: f64) -> Option<usize> {
    let mut best: Option<(f64, usize)> = None;
    let mut tied = false;
    for (i, d) in distances.enumerate() {
        let Some(d) = d.filter(|d| *d <= reach) else {
            continue;
        };
        match best {
            Some((b, _)) if d > b => {}
            Some((b, _)) if d == b => tied = true,
            _ => {
                best = Some((d, i));
                tied = false;
            }
        }
    }
    best.filter(|_| !tied).map(|(_, i)| i)
}

/// The id each of `sites` takes from `votes`, each `(N, site)` one station
/// of `N` voting for a site or, with `None`, for none. `taken` are the ids
/// some site holds already; those are never given again.
pub fn unanimous(
    sites: usize,
    votes: impl IntoIterator<Item = (i64, Option<usize>)>,
    taken: &BTreeSet<i64>,
) -> Vec<Option<i64>> {
    // `N` to the one site all its stations voted for so far, or `None` once
    // two of them disagree or one voted for none.
    let mut of: BTreeMap<i64, Option<usize>> = BTreeMap::new();
    for (n, site) in votes {
        if taken.contains(&n) {
            continue;
        }
        of.entry(n)
            .and_modify(|s| {
                if *s != site {
                    *s = None;
                }
            })
            .or_insert(site);
    }
    let mut won: Vec<Vec<i64>> = vec![Vec::new(); sites];
    for (n, s) in of {
        if let Some(s) = s {
            won[s].push(n);
        }
    }
    won.into_iter()
        .map(|ns| match ns.as_slice() {
            [n] => Some(*n),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const R: f64 = BEACON_REACH_M;

    fn nearest_site(sites: &[Option<[f64; 2]>], p: [f64; 2], reach: f64) -> Option<usize> {
        let d = sites
            .iter()
            .map(|s| s.map(|s| (s[0] - p[0]).hypot(s[1] - p[1])));
        nearest(d, reach)
    }

    #[test]
    fn a_station_names_the_nearest_site_within_reach() {
        let sites = [Some([0.0, 0.0]), None, Some([10_000.0, 0.0])];
        assert_eq!(nearest_site(&sites, [1000.0, 0.0], R), Some(0));
        assert_eq!(nearest_site(&sites, [9000.0, 500.0], R), Some(2));
        assert_eq!(nearest_site(&sites, [0.0, 16_000.0], R), None);
        // Halfway between two names neither.
        assert_eq!(nearest_site(&sites, [5000.0, 0.0], R), None);
    }

    #[test]
    fn keeps_an_id_only_where_every_station_agrees() {
        let none = BTreeSet::new();
        // 5's stations agree on site 0; 6's split; 7 has one that names none.
        let votes = [
            (5, Some(0)),
            (5, Some(0)),
            (6, Some(1)),
            (6, Some(2)),
            (7, Some(2)),
            (7, None),
        ];
        assert_eq!(unanimous(3, votes, &none), [Some(5), None, None]);
        // An id a site holds already is never given to another.
        assert_eq!(unanimous(3, votes, &[5].into()), [None, None, None]);
        // A site two ids agree on takes neither.
        let votes = [(5, Some(0)), (8, Some(0)), (9, Some(1))];
        assert_eq!(unanimous(2, votes, &none), [None, Some(9)]);
    }

    /// A tiny generator, so the comparison runs the same everywhere.
    struct Lcg(u64);

    impl Lcg {
        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            self.0 >> 33
        }
    }

    /// The join written the slow way: for each candidate pair of a site and
    /// an id, every station of the id is checked against every site.
    fn oracle(
        sites: &[Option<[f64; 2]>],
        stations: &[(i64, [f64; 2])],
        taken: &BTreeSet<i64>,
    ) -> Vec<Option<i64>> {
        let d = |s: [f64; 2], p: [f64; 2]| (s[0] - p[0]).hypot(s[1] - p[1]);
        let names = |site: usize, p: [f64; 2]| {
            let Some(s) = sites[site] else { return false };
            d(s, p) <= R
                && (0..sites.len()).all(|j| j == site || sites[j].is_none_or(|t| d(t, p) > d(s, p)))
        };
        let ids: BTreeSet<i64> = stations.iter().map(|(n, _)| *n).collect();
        (0..sites.len())
            .map(|site| {
                let won: Vec<i64> = ids
                    .iter()
                    .copied()
                    .filter(|n| !taken.contains(n))
                    .filter(|n| {
                        stations
                            .iter()
                            .filter(|(m, _)| m == n)
                            .all(|(_, p)| names(site, *p))
                    })
                    .collect();
                match won.as_slice() {
                    [n] => Some(*n),
                    _ => None,
                }
            })
            .collect()
    }

    #[test]
    fn agrees_with_the_join_written_the_slow_way() {
        let mut g = Lcg(7);
        let mut kept = 0;
        for _ in 0..500 {
            let n_sites = 1 + g.next() as usize % 8;
            let at = |g: &mut Lcg| (g.next() % 60) as f64 * 1000.0;
            let sites: Vec<Option<[f64; 2]>> = (0..n_sites)
                .map(|_| (!g.next().is_multiple_of(5)).then(|| [at(&mut g), at(&mut g)]))
                .collect();
            let stations: Vec<(i64, [f64; 2])> = (0..g.next() % 12)
                .map(|_| ((g.next() % 6) as i64, [at(&mut g), at(&mut g)]))
                .collect();
            let taken: BTreeSet<i64> = (0..6).filter(|_| g.next().is_multiple_of(6)).collect();
            let votes = stations
                .iter()
                .map(|&(n, p)| (n, nearest_site(&sites, p, R)));
            let got = unanimous(sites.len(), votes, &taken);
            assert_eq!(
                got,
                oracle(&sites, &stations, &taken),
                "{sites:?} {stations:?}"
            );
            kept += got.iter().flatten().count();
        }
        // The cases reach the kept branch, not only the empty one.
        assert!(kept > 100, "{kept}");
    }
}
