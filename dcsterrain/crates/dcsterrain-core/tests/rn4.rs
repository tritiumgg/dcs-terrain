//! Opt-in: decodes real map-wide `.rn4` files. Set `DCSTERRAIN_INSTALL` to
//! the DCS install root, and `DCSTERRAIN_EXTRACT` to the live Caucasus
//! extract to compare against; without them these pass without reading
//! anything, which is what CI does. Run them optimized, `cargo test --release
//! --test rn4`: the scan visits every byte of a 200 MB file. The install is
//! only read.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde_json::Value;

use dcsterrain_core::decode::install::{self, TERRAINS_DIR};
use dcsterrain_core::decode::rn4::{self, KIND_RAIL, KIND_ROAD, Network, Reach};
use dcsterrain_core::decode::surface5;

fn install_root() -> Option<PathBuf> {
    match std::env::var_os("DCSTERRAIN_INSTALL") {
        Some(p) => Some(PathBuf::from(p)),
        None => {
            eprintln!("DCSTERRAIN_INSTALL is not set; skipped");
            None
        }
    }
}

fn live_extract() -> Option<PathBuf> {
    match std::env::var_os("DCSTERRAIN_EXTRACT") {
        Some(p) => Some(PathBuf::from(p)),
        None => {
            eprintln!("DCSTERRAIN_EXTRACT is not set; skipped");
            None
        }
    }
}

fn maps(root: &Path) -> Vec<String> {
    let mut out: Vec<String> = fs::read_dir(root.join(TERRAINS_DIR))
        .unwrap()
        .flatten()
        .filter(|e| e.path().join(install::ENTRY_FILE).is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    out.sort();
    out
}

fn decode(root: &Path, dir: &str) -> Network {
    let map = install::read_map(root, dir).unwrap_or_else(|e| panic!("{dir}: {e}"));
    let path = |role: &str| root.join(&map.files.iter().find(|f| f.role == role).unwrap().path);
    let surface = surface5::open(&path("surface")).unwrap_or_else(|e| panic!("{dir}: {e}"));
    rn4::read(&path("roads"), &Reach::around(&surface)).unwrap_or_else(|e| panic!("{dir}: {e}"))
}

/// A rail class, by its name: `rw` on Caucasus, `rail…` and `…_rail`
/// elsewhere.
fn is_rail(class: &str) -> bool {
    class == "rw" || class.contains("rail")
}

/// Edge counts by kind and class name.
fn census(net: &Network) -> BTreeMap<(u8, String), usize> {
    let mut out = BTreeMap::new();
    for e in &net.edges {
        *out.entry((e.kind, net.class(e).to_string())).or_default() += 1;
    }
    out
}

fn quantile(sorted: &[f64], q: f64) -> f64 {
    sorted[((sorted.len() - 1) as f64 * q).round() as usize]
}

#[test]
fn every_map_decodes_with_rail_classes_on_rail_edges() {
    let Some(root) = install_root() else { return };
    let build = install::read_build(&root).unwrap();
    eprintln!("DCS {} ({})", build.version, build.timestamp);
    let mut wrong = Vec::new();
    for dir in maps(&root) {
        let net = decode(&root, &dir);
        let points: usize = net.edges.iter().map(|e| e.points.len()).sum();
        eprintln!(
            "{dir}: {} classes, {} edges, {points} vertices",
            net.classes.len(),
            net.edges.len()
        );
        for ((kind, class), n) in census(&net) {
            eprintln!("  kind {kind}  {class:24} {n}");
            if kind == KIND_RAIL && !is_rail(&class) {
                wrong.push(format!("{dir}: {n} rail edges of class {class}"));
            }
            // The kind, not the name, decides the table: Marianas lays its
            // narrow-gauge line as roads.
            if kind == KIND_ROAD && is_rail(&class) {
                eprintln!("  note: {n} road edges of a rail class, written as roads");
            }
        }
        assert!(
            net.edges.iter().any(|e| e.kind == KIND_ROAD),
            "{dir}: no road edges"
        );
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// The horizontal distance from `p` to the segment `a`–`b`.
fn to_segment(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
    let len2 = dx * dx + dz * dz;
    let t = if len2 == 0.0 {
        0.0
    } else {
        (((p[0] - a[0]) * dx + (p[1] - a[1]) * dz) / len2).clamp(0.0, 1.0)
    };
    let (cx, cz) = (a[0] + t * dx - p[0], a[1] + t * dz - p[1]);
    (cx * cx + cz * cz).sqrt()
}

/// A segment's two ends, [x, z].
type Segment = ([f64; 2], [f64; 2]);

/// The segments of one kind, bucketed on a square lattice so a point looks
/// only at those near it.
struct Segments {
    cell: f64,
    buckets: HashMap<(i64, i64), Vec<Segment>>,
}

impl Segments {
    fn new(net: &Network, kind: u8, cell: f64) -> Segments {
        let mut buckets: HashMap<(i64, i64), Vec<_>> = HashMap::new();
        let key = |v: f64| (v / cell).floor() as i64;
        for e in net.edges.iter().filter(|e| e.kind == kind) {
            for w in e.points.windows(2) {
                let (a, b) = ([w[0][0], w[0][2]], [w[1][0], w[1][2]]);
                for i in key(a[0].min(b[0]))..=key(a[0].max(b[0])) {
                    for j in key(a[1].min(b[1]))..=key(a[1].max(b[1])) {
                        buckets.entry((i, j)).or_default().push((a, b));
                    }
                }
            }
        }
        Segments { cell, buckets }
    }

    /// The distance to the nearest segment within a cell's reach, or
    /// infinity where there is none.
    fn nearest(&self, p: [f64; 2]) -> f64 {
        let (i, j) = (
            (p[0] / self.cell).floor() as i64,
            (p[1] / self.cell).floor() as i64,
        );
        let mut best = f64::INFINITY;
        for di in -1..=1 {
            for dj in -1..=1 {
                for &(a, b) in self.buckets.get(&(i + di, j + dj)).into_iter().flatten() {
                    best = best.min(to_segment(p, a, b));
                }
            }
        }
        best
    }
}

/// Every point of every path the live router returned, from a format 1
/// `roads.jsonl` or `railroads.jsonl`.
fn router_points(path: &Path) -> Vec<[f64; 2]> {
    let mut out = Vec::new();
    for line in BufReader::new(fs::File::open(path).unwrap()).lines() {
        let row: Value = serde_json::from_str(&line.unwrap()).unwrap();
        if row["kind"] != "path" {
            continue;
        }
        for p in row["points"].as_array().unwrap() {
            out.push([p[0].as_f64().unwrap(), p[1].as_f64().unwrap()]);
        }
    }
    out
}

/// Distances from each router point to the nearest decoded edge of its
/// kind, sorted.
fn distances(net: &Network, kind: u8, points: &[[f64; 2]]) -> Vec<f64> {
    let segments = Segments::new(net, kind, 100.0);
    let mut d: Vec<f64> = points.iter().map(|&p| segments.nearest(p)).collect();
    d.sort_by(f64::total_cmp);
    d
}

fn report(what: &str, d: &[f64]) {
    let far = d.iter().filter(|v| v.is_infinite()).count();
    eprintln!(
        "{what}: {} router points, p50 {:.2} m, p90 {:.2} m, p99 {:.2} m, {far} beyond 100 m",
        d.len(),
        quantile(d, 0.5),
        quantile(d, 0.9),
        quantile(d, 0.99)
    );
}

#[test]
fn caucasus_router_points_lie_on_decoded_edges() {
    let Some(root) = install_root() else { return };
    let Some(live) = live_extract() else { return };
    let manifest: Value =
        serde_json::from_str(&fs::read_to_string(live.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(
        manifest["theatre"], "Caucasus",
        "the live extract's theatre"
    );
    let build = install::read_build(&root).unwrap();
    eprintln!(
        "DCS {} ({}) against a live extract from {}",
        build.version, build.timestamp, manifest["dcs_build"]
    );
    let net = decode(&root, "Caucasus");
    let roads = distances(&net, KIND_ROAD, &router_points(&live.join("roads.jsonl")));
    let rail = distances(
        &net,
        KIND_RAIL,
        &router_points(&live.join("railroads.jsonl")),
    );
    report("roads", &roads);
    report("rail", &rail);
    assert!(!roads.is_empty() && !rail.is_empty(), "no router points");
    // ADR 0043: the router drives a lane or a track off the centerline.
    assert!(quantile(&roads, 0.9) <= 2.0, "road router points at p90");
    assert!(quantile(&rail, 0.9) <= 3.0, "rail router points at p90");
}

/// The ends that name each junction, road and rail together.
fn junctions(net: &Network) -> HashMap<u32, Vec<[f64; 2]>> {
    let mut ends: HashMap<u32, Vec<[f64; 2]>> = HashMap::new();
    for e in net.edges.iter().filter(|e| e.kind != 0) {
        let (first, last) = (e.points[0], e.points[e.points.len() - 1]);
        for (id, p) in [(e.a, first), (e.b, last)] {
            if let Some(id) = id {
                ends.entry(id).or_default().push([p[0], p[2]]);
            }
        }
    }
    ends
}

/// Edges sharing an id end within a junction of each other (ADR 0044): each
/// map's catch-all id is cleared, and what is left spans at most a few
/// kilometers.
#[test]
fn every_map_has_one_catch_all_and_tight_junctions() {
    let Some(root) = install_root() else { return };
    for dir in maps(&root) {
        let net = decode(&root, &dir);
        let cleared = net
            .edges
            .iter()
            .filter(|e| e.kind != 0)
            .map(|e| usize::from(e.a.is_none()) + usize::from(e.b.is_none()))
            .sum::<usize>();
        let mut spreads: Vec<f64> = junctions(&net)
            .values()
            .filter(|l| l.len() > 1)
            .map(|l| rn4::spread(l))
            .collect();
        spreads.sort_by(f64::total_cmp);
        eprintln!(
            "{dir}: catch-all {:?}, {cleared} ends written null; {} shared junctions, spread p50 {:.0} m, p90 {:.0} m, p99 {:.0} m, widest {:.0} m",
            net.catch_all,
            spreads.len(),
            quantile(&spreads, 0.5),
            quantile(&spreads, 0.9),
            quantile(&spreads, 0.99),
            spreads[spreads.len() - 1]
        );
        assert_eq!(net.catch_all.len(), 1, "{dir}: catch-all ids");
    }
}

/// `a` is read at the first vertex: where two ends coincide, they name one
/// junction under that reading.
#[test]
fn caucasus_ends_that_coincide_share_their_id() {
    let Some(root) = install_root() else { return };
    let net = decode(&root, "Caucasus");
    let key = |p: [f64; 3]| ((p[0] * 2.0).round() as i64, (p[2] * 2.0).round() as i64);
    let mut at: HashMap<(i64, i64), Vec<Option<u32>>> = HashMap::new();
    for e in net.edges.iter().filter(|e| e.kind != 0) {
        at.entry(key(e.points[0])).or_default().push(e.a);
        at.entry(key(e.points[e.points.len() - 1]))
            .or_default()
            .push(e.b);
    }
    let shared: Vec<_> = at.values().filter(|ids| ids.len() > 1).collect();
    let agree = shared
        .iter()
        .filter(|ids| ids.iter().all(|i| *i == ids[0]))
        .count();
    eprintln!("{agree} of {} coinciding ends share their id", shared.len());
    assert!(!shared.is_empty(), "no ends coincide");
    assert!(
        agree * 10 >= shared.len() * 9,
        "coinciding ends that share an id"
    );
    let spreads: Vec<f64> = junctions(&net).values().map(|l| rn4::spread(l)).collect();
    let mut s = spreads;
    s.sort_by(f64::total_cmp);
    assert!(quantile(&s, 0.99) <= 250.0, "junction spread at p99");
}
