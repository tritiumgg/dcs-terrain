//! Opt-in: decodes real `.surface5` files. Set `DCSTERRAIN_INSTALL` to the
//! DCS install root; without it these pass without reading anything, which
//! is what CI does. Run them optimized, `cargo test --release --test
//! surface5`: an unoptimized decode of a whole map takes minutes. The
//! install is only read.

use std::fs;
use std::path::{Path, PathBuf};

use dcsterrain_core::decode::install::{self, TERRAINS_DIR};
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

fn surface_path(root: &Path, map: &install::Map) -> PathBuf {
    let f = map.files.iter().find(|f| f.role == "surface").unwrap();
    root.join(&f.path)
}

#[test]
fn every_map_reads_its_finest_mesh() {
    let Some(root) = install_root() else { return };
    let build = install::read_build(&root).unwrap();
    eprintln!("DCS {} ({})", build.version, build.timestamp);
    for dir in maps(&root) {
        let map = install::read_map(&root, &dir).unwrap();
        let path = surface_path(&root, &map);
        let file = surface5::open(&path).unwrap_or_else(|e| panic!("{dir}: {e}"));
        let mut r = surface5::reader(&path).unwrap();
        let (mut tris, mut verts, mut runway) = (0, 0, 0);
        // One block at a time: the largest map's mesh is gigabytes.
        for i in 0..file.blocks.len() {
            let b = file
                .read_block(&mut r, i, &["Runway"])
                .unwrap_or_else(|e| panic!("{dir}: {e}"));
            tris += b.tri.len();
            verts += b.p.len();
            runway += b.subsets.iter().map(|s| s.tri.len()).sum::<usize>();
        }
        let bd = file.bounds();
        eprintln!(
            "{dir}: {} blocks, {tris} triangles, {verts} vertices, {runway} runway triangles, \
             x {}..{} z {}..{}",
            file.blocks.len(),
            bd.min_x,
            bd.max_x,
            bd.min_z,
            bd.max_z
        );
        assert!(tris > 0, "{dir}: no triangles");
    }
}
