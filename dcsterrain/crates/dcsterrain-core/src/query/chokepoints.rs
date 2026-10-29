//! `chokepoints`: the edges of a network in a region that its shortest
//! paths lean on most, ranked by the edge betweenness the cache holds.

use serde_json::json;

use super::geojson::{Features, props};
use super::region::resolve;
use super::route::{kind_of, samples_along};
use super::{Terrain, refused};
use crate::layers::{Cells, TPI_300, derive};
use crate::types::{Chokepoint, ChokepointsRequest, ChokepointsResponse, Issue, Point};

/// The most edges one request returns.
const LIMIT_MAX: u32 = 500;

pub fn chokepoints(
    terrain: &Terrain,
    q: ChokepointsRequest,
) -> Result<ChokepointsResponse, Vec<Issue>> {
    if !(1..=LIMIT_MAX).contains(&q.limit) {
        return Err(vec![Issue::new(
            "invalid_request",
            &format!("must be 1 to {LIMIT_MAX}"),
            "limit",
        )]);
    }
    let src = terrain.source();
    let area = resolve(src, &q.region, "region")?;
    let g = src.road_graph().map_err(refused)?;
    let Some(b) = src.road_betweenness().map_err(refused)? else {
        return Err(vec![Issue::new(
            "not_ready",
            &format!(
                "betweenness is still being built; `dcsterrain index --terrain {}` builds it",
                src.identity().terrain
            ),
            "",
        )]);
    };
    let kind = kind_of(q.network);
    let mut found = Vec::new();
    g.edges_near(kind, area.bbox, &mut |i| {
        let e = &g.edges[i];
        let path: Vec<[f64; 2]> = g.path(e).collect();
        if path.windows(2).any(|s| area.meets(s[0], s[1])) {
            found.push(i);
        }
    });
    found.sort_by(|&x, &y| b[y].total_cmp(&b[x]).then(x.cmp(&y)));
    found.truncate(q.limit as usize);
    let grid = src.grid();
    let mut edges = Vec::with_capacity(found.len());
    let mut lines = Vec::with_capacity(found.len());
    for &i in &found {
        let e = &g.edges[i];
        let line = g.piece(e, 0.0, 1.0);
        let samples = samples_along(&line);
        // The relief the edge runs through, over the cells under it.
        let mut bbox = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
        for p in &samples {
            bbox = [
                bbox[0].min(p[0]),
                bbox[1].min(p[1]),
                bbox[2].max(p[0]),
                bbox[3].max(p[1]),
            ];
        }
        let tpi = derive(src, TPI_300, Cells::covering(&grid, bbox)).map_err(refused)?;
        let values: Vec<f64> = samples
            .iter()
            .filter_map(|p| grid.cell_at(p[0], p[1]))
            .filter_map(|(r, c)| tpi.get(r.into(), c.into()))
            .filter(|v| !v.is_nan())
            .map(f64::from)
            .collect();
        let mid = g.piece(e, 0.5, 0.5)[0];
        edges.push(Chokepoint {
            edge_id: i as i64,
            betweenness: f64::from(b[i]),
            class: g.class(e).to_string(),
            crosses_water: e.crosses_water,
            tpi_300_mean: (!values.is_empty())
                .then(|| values.iter().sum::<f64>() / values.len() as f64),
            midpoint: Point {
                x: mid[0],
                z: mid[1],
            },
        });
        lines.push(line);
    }
    let geojson = if q.geojson {
        let mut f = Features::new(terrain)?;
        for (rank, (c, line)) in edges.iter().zip(&lines).enumerate() {
            f.line(
                line,
                props([
                    ("rank", json!(rank + 1)),
                    ("edge_id", json!(c.edge_id)),
                    ("betweenness", json!(c.betweenness)),
                    ("class", json!(c.class)),
                    ("crosses_water", json!(c.crosses_water)),
                ]),
            );
        }
        Some(f.finish())
    } else {
        None
    };
    Ok(ChokepointsResponse {
        provenance: terrain.provenance(),
        edges,
        geojson,
    })
}

#[cfg(test)]
mod tests {
    use super::super::answer;
    use super::super::geojson::tests::assert_valid;
    use super::super::tests::synth;
    use serde_json::json;

    #[test]
    fn the_bridge_ranks_first_over_the_whole_terrain() {
        let t = synth();
        let all = json!({"Box": {"min_x": -1e6, "min_z": -1e6, "max_x": 1e6, "max_z": 1e6}});
        let v = answer(
            &t,
            "chokepoints",
            &json!({"region": all, "network": "roads", "geojson": true}),
        )
        .unwrap();
        let edges = v["edges"].as_array().unwrap();
        assert_eq!(edges.len(), 11);
        assert_eq!(edges[0]["crosses_water"], true);
        assert_eq!(edges[0]["betweenness"], 1.0);
        assert!(edges[1]["betweenness"].as_f64().unwrap() < 1.0);
        assert!(edges.iter().all(|e| e["tpi_300_mean"].is_number()));
        assert_valid(&v["geojson"]);
        let rail = answer(
            &t,
            "chokepoints",
            &json!({"region": all, "network": "railroads", "limit": 5}),
        )
        .unwrap();
        assert_eq!(rail["edges"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn a_region_holds_the_edges_that_reach_into_it() {
        let t = synth();
        let s = match &t {
            super::Terrain::Synth(s) => s.form(),
            _ => unreachable!(),
        };
        // A box over the bridge alone, short of the roads either side of it.
        let (x, z) = s.abs(crate::synth::ROAD_EW_NORTH_M, crate::synth::RIVER_EAST_M);
        let r = json!({"Circle": {"x": x, "z": z, "radius": 50.0}});
        let v = answer(&t, "chokepoints", &json!({"region": r, "network": "roads"})).unwrap();
        let edges = v["edges"].as_array().unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0]["crosses_water"], true);
        let e = answer(
            &t,
            "chokepoints",
            &json!({"region": r, "network": "roads", "limit": 0}),
        )
        .unwrap_err();
        assert_eq!(e[0].code, "invalid_request");
    }
}
