//! Opt-in: builds the road graph and its betweenness from real terrains.
//! Set `DCSTERRAIN_INSTALL` to the DCS install root; without it these pass
//! without reading anything, which is what CI does. Run them optimized,
//! `cargo test --release --test roads -- --nocapture --test-threads 1`:
//! GermanyColdWar's betweenness takes minutes. The install is only read.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use dcsterrain_core::cache::Cache;
use dcsterrain_core::decode::install;
use dcsterrain_core::decode::rn4::{KIND_RAIL, KIND_ROAD};
use dcsterrain_core::roads::Graph;
use dcsterrain_core::source::install::{Install, ROADS_BETWEENNESS, ROADS_GRAPH};

fn install_root() -> Option<PathBuf> {
    match std::env::var_os("DCSTERRAIN_INSTALL") {
        Some(p) => Some(PathBuf::from(p)),
        None => {
            eprintln!("DCSTERRAIN_INSTALL is not set; skipped");
            None
        }
    }
}

/// A fresh directory under the system temp, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir =
            std::env::temp_dir().join(format!("dcsterrain-roads-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn report(id: &str, g: &Graph) {
    for kind in [KIND_ROAD, KIND_RAIL] {
        let edges: Vec<_> = g.edges.iter().filter(|e| e.kind == kind).collect();
        let wet = edges.iter().filter(|e| e.crosses_water).count();
        let km: f64 = edges.iter().map(|e| e.length).sum::<f64>() / 1000.0;
        eprintln!(
            "{id}: kind {kind}: {} edges, {wet} crossing water, {km:.0} km",
            edges.len()
        );
    }
    eprintln!("{id}: {} nodes, classes {:?}", g.nodes.len(), g.classes);
}

/// Builds `id`'s graph and betweenness into a fresh cache, returning the
/// seconds each took and the entries' sizes in MiB.
fn build(root: &Path, id: &str) -> (f64, f64, f64, f64) {
    let cache = TempDir::new(id);
    let t = Install::open(root, id, &Cache::new(&cache.0)).unwrap();
    let start = Instant::now();
    let g = t.roads().unwrap_or_else(|e| panic!("{id}: {e}"));
    let graph_s = start.elapsed().as_secs_f64();
    report(id, g);
    let start = Instant::now();
    let b = t.betweenness().unwrap_or_else(|e| panic!("{id}: {e}"));
    let between_s = start.elapsed().as_secs_f64();
    assert_eq!(b.len(), g.edges.len());
    assert!(b.iter().all(|v| (0.0..=1.0).contains(v)));
    for kind in [KIND_ROAD, KIND_RAIL] {
        let top = g
            .edges
            .iter()
            .zip(b)
            .filter(|(e, _)| e.kind == kind)
            .map(|(_, v)| *v)
            .fold(0.0f32, f32::max);
        assert!(top == 1.0 || top == 0.0, "{id}: kind {kind} peaks at {top}");
    }
    let size = |name: &str| {
        fs::metadata(cache.0.join(id).join(name)).unwrap().len() as f64 / (1 << 20) as f64
    };
    let sizes = (size(ROADS_GRAPH_NAME), size(ROADS_BETWEENNESS_NAME));
    eprintln!(
        "{id}: graph {graph_s:.2} s, {:.1} MiB; betweenness {between_s:.1} s, {:.1} MiB",
        sizes.0, sizes.1
    );
    (graph_s, between_s, sizes.0, sizes.1)
}

const ROADS_GRAPH_NAME: &str = "roads.graph";
const ROADS_BETWEENNESS_NAME: &str = "roads.betweenness";

#[test]
fn caucasus_builds_its_graph_under_ten_seconds() {
    let Some(root) = install_root() else { return };
    let (graph_s, ..) = build(&root, "Caucasus");
    assert!(graph_s < 10.0, "{graph_s} s");
}

#[test]
fn germany_completes_its_betweenness() {
    let Some(root) = install_root() else { return };
    build(&root, "GermanyCW");
}

/// The key of a raw entry, rewritten with its first line replaced: the
/// header is the magic, the u32 version, the u32 key length, the key and the
/// u64 payload length, then the payload.
fn rewrite_build(path: &Path, line: &str) {
    let b = fs::read(path).unwrap();
    let key_len = u32::from_le_bytes(b[12..16].try_into().unwrap()) as usize;
    let key = std::str::from_utf8(&b[16..16 + key_len]).unwrap();
    let (_, rest) = key.split_once('\n').unwrap();
    let key = format!("{line}\n{rest}");
    let mut out = b[..12].to_vec();
    out.extend_from_slice(&(key.len() as u32).to_le_bytes());
    out.extend_from_slice(key.as_bytes());
    out.extend_from_slice(&b[16 + key_len..]);
    fs::write(path, out).unwrap();
}

#[test]
fn an_update_with_the_road_files_unchanged_re_stamps_the_graph() {
    let Some(root) = install_root() else { return };
    let cache = TempDir::new("restamp");
    let map = install::open(&root, "Caucasus").unwrap();
    let t = Cache::new(&cache.0).terrain(&root, &map);
    let start = Instant::now();
    assert!(t.get(&ROADS_GRAPH).unwrap().built);
    assert!(t.get(&ROADS_BETWEENNESS).unwrap().built);
    let built = start.elapsed().as_secs_f64();
    let graph = fs::read(t.dir().join(ROADS_GRAPH_NAME)).unwrap();
    // As an entry made on the build before this one would read.
    for name in [ROADS_GRAPH_NAME, ROADS_BETWEENNESS_NAME] {
        rewrite_build(&t.dir().join(name), "build 2.9.29.27468 20250101-000000");
    }
    let start = Instant::now();
    assert!(
        !t.get(&ROADS_GRAPH).unwrap().built,
        "re-stamped, not rebuilt"
    );
    assert!(!t.get(&ROADS_BETWEENNESS).unwrap().built);
    let restamped = start.elapsed().as_secs_f64();
    eprintln!(
        "Caucasus on {}: built in {built:.2} s, re-stamped in {:.1} ms",
        map.build.version,
        restamped * 1000.0
    );
    assert_eq!(fs::read(t.dir().join(ROADS_GRAPH_NAME)).unwrap(), graph);
}
