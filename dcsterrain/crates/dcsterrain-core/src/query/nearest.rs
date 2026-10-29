//! `nearest`: the closest roads, rails, airdromes, towns, nodes, beacons or
//! scenery objects to a point.

use super::{Terrain, refused};
use crate::classes::Class;
use crate::decode::rn4::{KIND_RAIL, KIND_ROAD};
use crate::source::Source;
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

/// The closest point to `p` on the segment `a`–`b`.
fn closest_on(a: [f64; 2], b: [f64; 2], p: [f64; 2]) -> [f64; 2] {
    let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
    let len2 = dx * dx + dz * dz;
    let t = if len2 > 0.0 {
        (((p[0] - a[0]) * dx + (p[1] - a[1]) * dz) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    [a[0] + t * dx, a[1] + t * dz]
}

/// Each edge of the network, the point on it closest to `from`: along its
/// polyline and the gaps from its ends to its nodes.
fn edges(src: &dyn Source, kind: u8, from: (f64, f64)) -> Result<Vec<Nearby>, Vec<Issue>> {
    let g = src.road_graph().map_err(refused)?;
    let p = [from.0, from.1];
    let label = if kind == KIND_ROAD { "road" } else { "rail" };
    let mut out = Vec::new();
    for (i, e) in g.edges.iter().enumerate().filter(|(_, e)| e.kind == kind) {
        let mut path = Vec::with_capacity(g.line(e).len() + 2);
        path.push(g.nodes[e.a as usize]);
        path.extend(g.line(e).iter().map(|q| [f64::from(q[0]), f64::from(q[1])]));
        path.push(g.nodes[e.b as usize]);
        let mut best = path[0];
        let mut best_d = f64::INFINITY;
        for s in path.windows(2) {
            let c = closest_on(s[0], s[1], p);
            let d = (c[0] - p[0]).hypot(c[1] - p[1]);
            if d < best_d {
                (best, best_d) = (c, d);
            }
        }
        let mut n = at(label, i.to_string(), None, (best[0], best[1]), from);
        n.class = Some(g.class(e).to_string());
        out.push(n);
    }
    Ok(out)
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
        src.objects_in(b, &mut |o| {
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
        NearestKind::Road => nearest_of(edges(src, KIND_ROAD, from)?, n),
        NearestKind::Rail => nearest_of(edges(src, KIND_RAIL, from)?, n),
        NearestKind::Airdrome => {
            let found = tables()?
                .airdromes
                .iter()
                .filter_map(|a| {
                    let id = a.id.map_or_else(|| a.name_id.clone(), |id| id.to_string());
                    let name = a.display_name.clone().or_else(|| Some(a.name_id.clone()));
                    Some(at("airdrome", id, name, (a.x?, a.z?), from))
                })
                .collect();
            nearest_of(found, n)
        }
        NearestKind::Town => {
            let found = tables()?
                .towns
                .iter()
                .enumerate()
                .map(|(k, t)| {
                    let name = t.display_name.clone().unwrap_or_else(|| t.name.clone());
                    at("town", k.to_string(), Some(name), (t.x, t.z), from)
                })
                .collect();
            nearest_of(found, n)
        }
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
