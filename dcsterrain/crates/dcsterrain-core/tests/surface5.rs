//! Opt-in: decodes real `.surface5` files. Set `DCSTERRAIN_INSTALL` to the
//! DCS install root; without it these pass without reading anything, which
//! is what CI does. Run them optimized, `cargo test --release --test
//! surface5`: an unoptimized decode of a whole map takes minutes. The
//! install is only read.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde_json::Value;

use dcsterrain_core::decode::install::{self, TERRAINS_DIR};
use dcsterrain_core::decode::surface5;
use dcsterrain_core::extract::Grid;

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

/// The live extract to compare against, from `DCSTERRAIN_EXTRACT`: the
/// whole-map Caucasus extract the hook wrote, format 1.
fn live_extract() -> Option<PathBuf> {
    match std::env::var_os("DCSTERRAIN_EXTRACT") {
        Some(p) => Some(PathBuf::from(p)),
        None => {
            eprintln!("DCSTERRAIN_EXTRACT is not set; skipped");
            None
        }
    }
}

fn live_tile(dir: &Path, layer: &str, tx: u32, tz: u32) -> Option<Vec<u8>> {
    fs::read(dir.join(format!("tiles/{layer}/{tx}_{tz}.bin"))).ok()
}

/// Rasterizes Caucasus on the live extract's own grid and scores each cell
/// the live extract wrote. A sea tile the live extract wrote without a height
/// tile reads as height 0, as its omission rule says.
#[test]
fn caucasus_matches_the_live_extract() {
    let Some(root) = install_root() else { return };
    let Some(live) = live_extract() else { return };
    let build = install::read_build(&root).unwrap();
    let manifest: Value =
        serde_json::from_str(&fs::read_to_string(live.join("manifest.json")).unwrap()).unwrap();
    eprintln!(
        "DCS {} ({}) against a live extract from {}",
        build.version, build.timestamp, manifest["dcs_build"]
    );
    assert_eq!(
        manifest["theatre"], "Caucasus",
        "the live extract's theatre"
    );
    let g = &manifest["grid"];
    let grid = Grid {
        cell_size: g["cell_size"].as_u64().unwrap() as u32,
        origin_x: g["origin_x"].as_f64().unwrap(),
        origin_z: g["origin_z"].as_f64().unwrap(),
        height: g["height"].as_u64().unwrap() as u32,
        width: g["width"].as_u64().unwrap() as u32,
        tile_size: g["tile_size"].as_u64().unwrap() as u32,
    };
    let map = install::read_map(&root, "Caucasus").unwrap();
    let path = surface_path(&root, &map);
    let file = surface5::open(&path).unwrap();

    let (mut cells, mut exact, mut within_1, mut over_2, mut worst) =
        (0u64, 0u64, 0u64, 0u64, 0i32);
    let (mut land, mut land_exact) = (0u64, 0u64);
    let (mut water_cells, mut water_wrong) = (0u64, 0u64);
    // Cells the live extract wrote as fill that the mesh covers, and the
    // reverse: where the mesh's edge and the sim's disagree.
    let (mut fill_covered, mut covered_fill, mut fill_not_flat) = (0u64, 0u64, 0u64);
    let mut unlisted_covered = 0u64;
    let started = Instant::now();
    surface5::rasterize(
        &file,
        || surface5::reader(&path),
        &grid,
        |t| {
            let n = t.water.len();
            let Some(w) = live_tile(&live, "water", t.tx, t.tz) else {
                unlisted_covered += t.water.iter().filter(|&&v| v != 255).count() as u64;
                return Ok(());
            };
            let h: Vec<i16> = match live_tile(&live, "height", t.tx, t.tz) {
                Some(b) => b
                    .chunks_exact(2)
                    .map(|c| i16::from_le_bytes([c[0], c[1]]))
                    .collect(),
                None => vec![0; n],
            };
            for i in 0..n {
                match (w[i] == 255, t.water[i] == 255) {
                    (true, false) => {
                        fill_covered += 1;
                        // The live fill test took flat 5 m land for no terrain (ADR 0042).
                        fill_not_flat += u64::from(t.height[i] != 5 || t.water[i] != 0);
                    }
                    (false, true) => covered_fill += 1,
                    _ => {}
                }
                if w[i] == 255 || h[i] == -32768 {
                    continue;
                }
                water_cells += 1;
                water_wrong += u64::from(w[i] != t.water[i]);
                if t.water[i] == 255 {
                    continue;
                }
                let e = (i32::from(t.height[i]) - i32::from(h[i])).abs();
                cells += 1;
                exact += u64::from(e == 0);
                within_1 += u64::from(e <= 1);
                over_2 += u64::from(e > 2);
                worst = worst.max(e);
                if w[i] == 0 {
                    land += 1;
                    land_exact += u64::from(e == 0);
                }
            }
            Ok(())
        },
    )
    .unwrap();
    let pct = |a: u64, b: u64| 100.0 * a as f64 / b as f64;
    eprintln!("rasterized in {:.1} s", started.elapsed().as_secs_f64());
    eprintln!(
        "height: {cells} cells, {:.4} % exact, {:.5} % within 1 m, {over_2} over 2 m, worst {worst} m; land {:.4} % exact",
        pct(exact, cells),
        pct(within_1, cells),
        pct(land_exact, land)
    );
    eprintln!("water: {water_wrong} of {water_cells} cells differ");
    eprintln!(
        "coverage: {fill_covered} live fill cells the mesh covers, {covered_fill} live cells outside the mesh, \
         {unlisted_covered} covered cells in tiles the live extract did not write"
    );
    // 99.95 % at two decimals, the figure the reverse-engineered decode reached.
    assert!(pct(exact, cells) >= 99.945, "heights exact");
    assert!(water_wrong <= 67, "water cells that differ");
    assert_eq!(
        fill_not_flat, 0,
        "live fill cells the mesh covers with anything but 5 m land"
    );
    assert_eq!(
        covered_fill + unlisted_covered,
        0,
        "live data cells outside the mesh"
    );
}
