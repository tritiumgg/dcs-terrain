//! Opt-in: decodes real `.scn5` files. Set `DCSTERRAIN_INSTALL` to the DCS
//! install root; without it these pass without reading anything, which is
//! what CI does. Run them optimized, `cargo test --release --test scn5`. The
//! install is only read.

use std::fs;
use std::path::{Path, PathBuf};

use dcsterrain_core::decode::install::{self, TERRAINS_DIR};
use dcsterrain_core::decode::scn5::{self, Scene};

fn install_root() -> Option<PathBuf> {
    match std::env::var_os("DCSTERRAIN_INSTALL") {
        Some(p) => Some(PathBuf::from(p)),
        None => {
            eprintln!("DCSTERRAIN_INSTALL is not set; skipped");
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

fn scene_path(root: &Path, dir: &str) -> PathBuf {
    let map = install::read_map(root, dir).unwrap_or_else(|e| panic!("{dir}: {e}"));
    root.join(&map.files.iter().find(|f| f.role == "scene").unwrap().path)
}

fn open(root: &Path, dir: &str) -> Scene {
    scn5::open(&scene_path(root, dir)).unwrap_or_else(|e| panic!("{dir}: {e}"))
}

#[test]
fn every_installed_scene_opens() {
    let Some(root) = install_root() else { return };
    for dir in maps(&root) {
        let s = open(&root, &dir);
        let pools: Vec<u64> = s.pools.iter().map(|p| p.count).collect();
        eprintln!(
            "{dir}: version {}, {} models, {} nodes, {} instances, group {}, pools {pools:?}",
            s.version,
            s.models.len(),
            s.nodes.len(),
            s.instances(),
            s.group.len()
        );
        assert!(pools.iter().all(|&c| c > 0), "{dir}");
    }
}

#[test]
fn caucasus_and_nevada_hold_the_measured_counts() {
    let Some(root) = install_root() else { return };
    let c = open(&root, "Caucasus");
    assert_eq!((c.version, c.models.len(), c.nodes.len()), (9, 615, 25_283));
    assert_eq!(c.instances(), 15_602_807);
    assert_eq!(c.group.len(), 18_976);
    let pools: Vec<u64> = c.pools.iter().map(|p| p.count).collect();
    assert_eq!(pools, [917_847, 1_069_315, 12_967_324]);

    let n = open(&root, "Nevada");
    assert_eq!((n.version, n.models.len(), n.nodes.len()), (8, 679, 13_434));
    assert!(n.group.is_empty());
    let pools: Vec<u64> = n.pools.iter().map(|p| p.count).collect();
    assert_eq!(pools, [553_400, 8_551_832, 889_299]);
}
