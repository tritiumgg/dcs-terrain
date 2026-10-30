//! `nearest`: the closest roads, rails, airdromes, towns, nodes, beacons or
//! scenery objects to a point.

use super::{Terrain, refused};
use crate::classes::Class;
use crate::decode::rn4::{KIND_RAIL, KIND_ROAD};
use crate::decode::scn5::Pool;
use crate::source::{Source, Tables};
use crate::types::{Issue, Nearby, NearestKind, NearestRequest, NearestResponse};

/// The most results one request returns.
const N_MAX: u32 = 50;
/// The first box a scenery search tries, in meters either side; it doubles
/// until it holds enough.
const SCENERY_FIRST_M: f64 = 250.0;

/// The `n` nearest of `found`, nearest first, ties in the order given.
fn nearest_of(mut found: Vec<Nearby>, n: usize) -> Vec<Nearby> {
    found.sort_by(|a, b| a.distance.total_cmp(&b.distance));
    found.truncate(n);
    found
}

fn at(
    kind: &str,
    id: String,
    name: Option<String>,
    (x, z): (f64, f64),
    from: (f64, f64),
) -> Nearby {
    Nearby {
        kind: kind.into(),
        id,
        name,
        class: None,
        x,
        z,
        distance: (x - from.0).hypot(z - from.1),
    }
}

/// The `n` edges of the network nearest `from`, nearest first, ties in edge
/// order, each at its point closest to `from`: along its polyline and the
/// gaps from its ends to its nodes.
pub(super) fn edges(
    src: &dyn Source,
    kind: u8,
    from: (f64, f64),
    n: usize,
) -> Result<Vec<Nearby>, Vec<Issue>> {
    let g = src.road_graph().map_err(refused)?;
    let label = if kind == KIND_ROAD { "road" } else { "rail" };
    let found = g.nearest(kind, [from.0, from.1], n);
    Ok(found
        .into_iter()
        .map(|(i, q)| {
            let mut f = at(label, i.to_string(), None, (q[0], q[1]), from);
            f.class = Some(g.class(&g.edges[i]).to_string());
            f
        })
        .collect())
}

/// The `n` objects of `want` nearest `from`, searched in a box that doubles
/// until it holds `n` within its half-width or covers the grid.
fn scenery(
    src: &dyn Source,
    want: &dyn Fn(Class) -> bool,
    from: (f64, f64),
    n: usize,
) -> Result<Vec<Nearby>, Vec<Issue>> {
    let g = src.grid();
    let c = f64::from(g.cell_size);
    let corners = [
        (g.origin_x, g.origin_z),
        (g.origin_x + f64::from(g.height) * c, g.origin_z),
        (g.origin_x, g.origin_z + f64::from(g.width) * c),
        (
            g.origin_x + f64::from(g.height) * c,
            g.origin_z + f64::from(g.width) * c,
        ),
    ];
    let farthest = corners
        .iter()
        .map(|&(x, z)| (x - from.0).hypot(z - from.1))
        .fold(0.0, f64::max);
    let mut r = SCENERY_FIRST_M;
    loop {
        let mut found = Vec::new();
        let b = [from.0 - r, from.1 - r, from.0 + r, from.1 + r];
        src.objects_in(Pool::Objects, b, &mut |o| {
            if want(o.class) {
                let mut f = at(
                    "scenery",
                    o.id.to_string(),
                    Some(o.model.to_string()),
                    (o.pos[0], o.pos[2]),
                    from,
                );
                f.class = Some(o.class.name().to_string());
                found.push(f);
            }
        })
        .map_err(refused)?;
        found.sort_by(|a, b| a.id.cmp(&b.id));
        found.dedup_by(|a, b| a.id == b.id);
        // Ties keep the order of their ids. Only objects within the
        // half-width are surely nearer than any
        // outside the box.
        let sure = found.iter().filter(|f| f.distance <= r).count();
        if sure >= n || r >= farthest {
            return Ok(nearest_of(found, n));
        }
        r *= 2.0;
    }
}

/// The indices of the `n` positions nearest `from`, nearest first, ties in
/// the order given; a position that is `None` is never one. Only distances
/// are compared, so a caller builds its answer for the winners alone.
fn nearest_indices(
    positions: impl Iterator<Item = Option<(f64, f64)>>,
    from: (f64, f64),
    n: usize,
) -> Vec<usize> {
    let mut d: Vec<(f64, usize)> = positions
        .enumerate()
        .filter_map(|(k, p)| p.map(|(x, z)| ((x - from.0).hypot(z - from.1), k)))
        .collect();
    let by = |a: &(f64, usize), b: &(f64, usize)| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1));
    if n < d.len() {
        d.select_nth_unstable_by(n, by);
        d.truncate(n);
    }
    d.sort_unstable_by(by);
    d.into_iter().map(|(_, k)| k).collect()
}

/// The `n` airdromes of the tables nearest `from`, nearest first, ties in
/// table order; an airdrome with no position is never one.
pub(super) fn airdromes(tables: &Tables, from: (f64, f64), n: usize) -> Vec<Nearby> {
    let all = &tables.airdromes;
    let near = nearest_indices(all.iter().map(|a| Some((a.x?, a.z?))), from, n);
    near.into_iter()
        .map(|k| {
            let a = &all[k];
            let id = a.id.map_or_else(|| a.name_id.clone(), |id| id.to_string());
            let name = a.display_name.clone().or_else(|| Some(a.name_id.clone()));
            at("airdrome", id, name, (a.x.unwrap(), a.z.unwrap()), from)
        })
        .collect()
}

/// The `n` towns of the tables nearest `from`, nearest first, ties in table
/// order, each by its display name where it has one. Every town is measured,
/// a linear scan: the tables hold under two thousand, a few microseconds'
/// work, where an index would save little.
pub(super) fn towns(tables: &Tables, from: (f64, f64), n: usize) -> Vec<Nearby> {
    let all = &tables.towns;
    let near = nearest_indices(all.iter().map(|t| Some((t.x, t.z))), from, n);
    near.into_iter()
        .map(|k| {
            let t = &all[k];
            let name = t.display_name.clone().unwrap_or_else(|| t.name.clone());
            at("town", k.to_string(), Some(name), (t.x, t.z), from)
        })
        .collect()
}

pub fn nearest(terrain: &Terrain, q: NearestRequest) -> Result<NearestResponse, Vec<Issue>> {
    let src = terrain.source();
    if !(1..=N_MAX).contains(&q.n) {
        return Err(vec![Issue::new(
            "invalid_request",
            &format!("n must be from 1 to {N_MAX}"),
            "n",
        )]);
    }
    if !(q.x.is_finite() && q.z.is_finite()) {
        return Err(vec![Issue::new(
            "invalid_point",
            "x and z must be finite",
            "",
        )]);
    }
    if q.class.is_some() && q.kind != NearestKind::Scenery {
        return Err(vec![Issue::new(
            "invalid_request",
            "class filters scenery only",
            "class",
        )]);
    }
    let from = (q.x, q.z);
    let n = q.n as usize;
    let tables = || src.tables().map_err(refused);
    let results = match q.kind {
        NearestKind::Road => edges(src, KIND_ROAD, from, n)?,
        NearestKind::Rail => edges(src, KIND_RAIL, from, n)?,
        NearestKind::Airdrome => airdromes(tables()?, from, n),
        NearestKind::Town => towns(tables()?, from, n),
        NearestKind::Node => {
            let mut found = Vec::new();
            for (k, node) in tables()?.nodes.iter().enumerate() {
                let id = node.id.map_or_else(|| k.to_string(), |id| id.to_string());
                for (kind, side) in [("node_red", &node.red), ("node_blue", &node.blue)] {
                    if let Some(p) = side {
                        found.push(at(kind, id.clone(), node.name.clone(), (p.x, p.z), from));
                    }
                }
            }
            nearest_of(found, n)
        }
        NearestKind::Beacon => {
            let found = tables()?
                .beacons
                .iter()
                .enumerate()
                .filter_map(|(k, b)| {
                    let id = b.beacon_id.clone().unwrap_or_else(|| k.to_string());
                    let name = b.callsign.clone().or_else(|| b.display_name.clone());
                    Some(at("beacon", id, name, (b.x?, b.z?), from))
                })
                .collect();
            nearest_of(found, n)
        }
        NearestKind::Scenery => {
            let named = match &q.class {
                None => None,
                Some(c) => Some(Class::ALL.into_iter().find(|k| k.name() == c).ok_or_else(
                    || {
                        let names: Vec<_> = Class::ALL.iter().map(|k| k.name()).collect();
                        vec![Issue::new(
                            "unknown_class",
                            &format!("no class `{c}`; the classes are {}", names.join(", ")),
                            "class",
                        )]
                    },
                )?),
            };
            let want = |c: Class| match named {
                Some(k) => c == k,
                None => c.is_structure(),
            };
            scenery(src, &want, from, n)?
        }
    };
    Ok(NearestResponse {
        provenance: terrain.provenance(),
        results,
    })
}

#[cfg(test)]
mod tests {
    use super::super::answer;
    use super::super::tests::synth;
    use super::*;
    use crate::synth::*;
    use serde_json::{Value, json};

    fn ask(t: &Terrain, req: Value) -> Vec<Value> {
        answer(t, "nearest", &req).unwrap()["results"]
            .as_array()
            .unwrap()
            .clone()
    }

    fn form(t: &Terrain) -> Theatre {
        match t {
            Terrain::Synth(s) => *s.form(),
            _ => unreachable!(),
        }
    }

    #[test]
    fn the_nearest_road_is_the_closest_point_on_it() {
        let t = synth();
        // 300 m north of the east-west road, west of every crossing.
        let (x, z) = form(&t).abs(ROAD_EW_NORTH_M + 300.0, 6000.0);
        let r = ask(&t, json!({"kind": "road", "x": x, "z": z}));
        assert_eq!(r.len(), 1);
        assert!(
            (r[0]["distance"].as_f64().unwrap() - 300.0).abs() < 1e-3,
            "{r:?}"
        );
        assert!((r[0]["z"].as_f64().unwrap() - z).abs() < 1e-3);
        assert!(r[0]["class"].is_string());
        let rail = ask(&t, json!({"kind": "rail", "x": x, "z": z}));
        let want = 300.0 + RAIL_SOUTH_OFFSET_M;
        assert!(
            (rail[0]["distance"].as_f64().unwrap() - want).abs() < 1e-3,
            "{rail:?}"
        );
        assert_eq!(rail[0]["kind"], "rail");
    }

    /// Every edge of the network at its closest point, by brute force: the
    /// search the index replaced, kept to check it against.
    fn every_edge(src: &dyn Source, kind: u8, from: (f64, f64)) -> Vec<Nearby> {
        let g = src.road_graph().unwrap();
        let p = [from.0, from.1];
        let label = if kind == KIND_ROAD { "road" } else { "rail" };
        let closest_on = |a: [f64; 2], b: [f64; 2]| {
            let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
            let len2 = dx * dx + dz * dz;
            let t = if len2 > 0.0 {
                (((p[0] - a[0]) * dx + (p[1] - a[1]) * dz) / len2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            [a[0] + t * dx, a[1] + t * dz]
        };
        let mut out = Vec::new();
        for (i, e) in g.edges.iter().enumerate().filter(|(_, e)| e.kind == kind) {
            let mut path = vec![g.nodes[e.a as usize]];
            path.extend(g.line(e).iter().map(|q| [f64::from(q[0]), f64::from(q[1])]));
            path.push(g.nodes[e.b as usize]);
            let (mut best, mut best_d) = (path[0], f64::INFINITY);
            for s in path.windows(2) {
                let c = closest_on(s[0], s[1]);
                let d = (c[0] - p[0]).hypot(c[1] - p[1]);
                if d < best_d {
                    (best, best_d) = (c, d);
                }
            }
            let mut n = at(label, i.to_string(), None, (best[0], best[1]), from);
            n.class = Some(g.class(e).to_string());
            out.push(n);
        }
        out
    }

    #[test]
    fn the_nearest_edges_are_those_every_edge_ranks_first() {
        let t = synth();
        let f = form(&t);
        let size = f.size_m();
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut unit = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        for k in 0..300 {
            // Across the terrain and past its edges, some on a coarse lattice
            // so that edges tie.
            let (mut north, mut east) = ((unit() * 1.4 - 0.2) * size, (unit() * 1.4 - 0.2) * size);
            if k % 3 == 0 {
                (north, east) = (
                    (north / 500.0).round() * 500.0,
                    (east / 500.0).round() * 500.0,
                );
            }
            let from = f.abs(north, east);
            for kind in [KIND_ROAD, KIND_RAIL] {
                let all = every_edge(t.source(), kind, from);
                for n in [1, 2, 5, 50] {
                    let want = nearest_of(all.clone(), n);
                    let got = edges(t.source(), kind, from, n).unwrap();
                    assert_eq!(
                        format!("{got:?}"),
                        format!("{want:?}"),
                        "{from:?} {kind} {n}"
                    );
                }
            }
        }
    }

    #[test]
    fn indices_rank_by_distance_then_order_given() {
        let p = [
            Some((3.0, 0.0)),
            None,
            Some((0.0, 1.0)),
            Some((-1.0, 0.0)),
            Some((0.0, 3.0)),
        ];
        let rank = |n| nearest_indices(p.into_iter(), (0.0, 0.0), n);
        assert_eq!(rank(1), [2]);
        assert_eq!(rank(2), [2, 3]);
        assert_eq!(rank(3), [2, 3, 0]);
        assert_eq!(rank(9), [2, 3, 0, 4]);
    }

    #[test]
    fn tables_kinds_rank_by_distance() {
        let t = synth();
        let f = form(&t);
        let (x, z) = f.town(1);
        let towns = ask(&t, json!({"kind": "town", "x": x, "z": z + 10.0, "n": 3}));
        assert_eq!(towns.len(), TOWNS_COUNT as usize);
        assert_eq!(towns[0]["name"], town_name(2));
        assert!((towns[0]["distance"].as_f64().unwrap() - 10.0).abs() < 1e-9);
        let d: Vec<f64> = towns
            .iter()
            .map(|r| r["distance"].as_f64().unwrap())
            .collect();
        assert!(d.windows(2).all(|w| w[0] <= w[1]), "{d:?}");

        let [red, blue] = f.node(0);
        let nodes = ask(
            &t,
            json!({"kind": "node", "x": blue.0, "z": blue.1, "n": 2}),
        );
        assert_eq!(nodes[0]["kind"], "node_blue");
        assert_eq!(nodes[1]["kind"], "node_red");
        assert!((nodes[1]["x"].as_f64().unwrap() - red.0).abs() < 1e-9);

        let [w, _] = f.runway_ends();
        let b = ask(&t, json!({"kind": "beacon", "x": w.0, "z": w.1}));
        assert_eq!(b[0]["id"], beacon_id(1));
        let a = ask(&t, json!({"kind": "airdrome", "x": 0, "z": 0}));
        assert_eq!(a[0]["id"], AIRDROME_ID.to_string());
        assert_eq!(a[0]["name"], AIRDROME_DISPLAY_NAME);
    }

    #[test]
    fn the_nearest_scenery_is_found_however_far_and_filtered_by_class() {
        let t = synth();
        let f = form(&t);
        let (x, z) = f.abs(SCENERY_INDUSTRIAL_NORTH_M, SCENERY_INDUSTRIAL_EAST_M);
        let r = ask(
            &t,
            json!({"kind": "scenery", "x": x, "z": z, "n": 2, "class": "industrial"}),
        );
        assert_eq!(r.len(), 2);
        assert_eq!(r[0]["distance"], 0.0);
        assert_eq!(r[0]["name"], SCENERY_INDUSTRIAL_MODEL_A.to_lowercase());
        assert_eq!(r[1]["class"], "industrial");
        // From a far corner the search widens until it finds the poles.
        let (x, z) = f.abs(3000.0, 67000.0);
        let r = ask(
            &t,
            json!({"kind": "scenery", "x": x, "z": z, "class": "misc"}),
        );
        assert_eq!(r[0]["name"], SCENERY_POLE_MODEL.to_lowercase());
        // Every pole is a candidate, so the nearest is the easternmost.
        let (_, pz) = f.abs(
            0.0,
            SCENERY_POLE_FIRST_EAST_M + 13.0 * SCENERY_POLE_SPACING_M,
        );
        assert!((r[0]["z"].as_f64().unwrap() - pz).abs() < 1e-6);
    }

    #[test]
    fn a_bad_n_or_class_is_an_issue() {
        let t = synth();
        let code = |req: Value| answer(&t, "nearest", &req).unwrap_err()[0].code.clone();
        assert_eq!(
            code(json!({"kind": "town", "x": 0, "z": 0, "n": 0})),
            "invalid_request"
        );
        assert_eq!(
            code(json!({"kind": "town", "x": 0, "z": 0, "n": 51})),
            "invalid_request"
        );
        assert_eq!(
            code(json!({"kind": "scenery", "x": 0, "z": 0, "class": "castle"})),
            "unknown_class"
        );
        assert_eq!(
            code(json!({"kind": "town", "x": 0, "z": 0, "class": "misc"})),
            "invalid_request"
        );
        assert_eq!(
            code(json!({"kind": "place", "x": 0, "z": 0})),
            "invalid_request"
        );
    }
}
