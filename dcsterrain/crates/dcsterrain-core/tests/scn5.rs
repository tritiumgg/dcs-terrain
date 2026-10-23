//! Opt-in: decodes real `.scn5` files. Set `DCSTERRAIN_INSTALL` to the DCS
//! install root; without it these pass without reading anything, which is
//! what CI does. Run them optimized, `cargo test --release --test scn5`. The
//! install is only read.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use dcsterrain_core::decode::install::{self, TERRAINS_DIR};
use dcsterrain_core::decode::scn5::{self, Pool, Scene};

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

/// The reference decode's output directory, holding
/// `<Map>/scenery_pool<k>_objects.tsv`: the decode the formats were
/// measured with, one object a row, `id model x y z heading_deg obb_w obb_d
/// height`, model uppercase, positions to 2 decimals, heading to 1.
fn reference() -> Option<PathBuf> {
    match std::env::var_os("DCSTERRAIN_SCENE_REFERENCE") {
        Some(p) => Some(PathBuf::from(p)),
        None => {
            eprintln!("DCSTERRAIN_SCENE_REFERENCE is not set; skipped");
            None
        }
    }
}

fn reference_objects(dir: &Path, map: &str) -> Option<PathBuf> {
    fs::read_dir(dir.join(map))
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            let n = p.file_name().unwrap().to_string_lossy();
            n.starts_with("scenery_pool") && n.ends_with("_objects.tsv")
        })
}

/// Every object-pool row of the reference against this decoder's, by id.
fn compare_objects(root: &Path, refdir: &Path, map: &str) {
    let Some(tsv) = reference_objects(refdir, map) else {
        panic!("{map}: no objects table under {}", refdir.display());
    };
    let path = scene_path(root, map);
    let mut r = scn5::reader(&path).unwrap();
    let s = Scene::open(&mut r, map).unwrap();
    let mut ours = HashMap::new();
    s.walk(&mut r, |i| {
        if i.pool == Pool::Objects && (i.id >> s.shift()) < s.nodes.len() as u64 {
            ours.insert(i.id, i.clone());
        }
        Ok(())
    })
    .unwrap();
    let text = fs::read_to_string(&tsv).unwrap();
    let mut rows = 0;
    for line in text.lines().skip(1) {
        let f: Vec<&str> = line.split('\t').collect();
        let id: u64 = f[0].parse().unwrap();
        let o = ours
            .get(&id)
            .unwrap_or_else(|| panic!("{map}: id {id} is not decoded"));
        let m = &s.models[o.model as usize];
        let b = m.bbox;
        let ext = |lo: usize, hi: usize| format!("{:.2}", f64::from(b[hi] - b[lo]));
        let got = [
            m.name.to_uppercase(),
            format!("{:.2}", o.pos[0]),
            format!("{:.2}", o.pos[1]),
            format!("{:.2}", o.pos[2]),
            ext(0, 3),
            ext(2, 5),
            ext(1, 4),
        ];
        let want = [f[1], f[2], f[3], f[4], f[6], f[7], f[8]];
        assert_eq!(got, want.map(String::from), "{map}: id {id}");
        // Both round to a tenth, and a value near a boundary rounds apart.
        let h: f64 = f[5].parse().unwrap();
        let d = (o.rotation.to_degrees() - h).rem_euclid(360.0);
        assert!(
            d.min(360.0 - d) <= 0.1 + 1e-9,
            "{map}: id {id} heading {} against {h}",
            o.rotation.to_degrees()
        );
        rows += 1;
    }
    assert_eq!(
        rows,
        ours.len(),
        "{map}: the reference lists {rows} objects and the decode {}",
        ours.len()
    );
    eprintln!("{map}: {rows} objects equal the reference");
}

#[test]
fn caucasus_objects_equal_the_reference_decode() {
    let (Some(root), Some(refdir)) = (install_root(), reference()) else {
        return;
    };
    compare_objects(&root, &refdir, "Caucasus");
}

#[test]
fn nevada_objects_equal_the_reference_decode() {
    let (Some(root), Some(refdir)) = (install_root(), reference()) else {
        return;
    };
    compare_objects(&root, &refdir, "Nevada");
}

/// The ids a map's `NotInstances.lua` lists, `"t:<id>"` each.
fn not_instances(root: &Path, dir: &str) -> Option<Vec<u64>> {
    let map = root.join(TERRAINS_DIR).join(dir);
    let file = fs::read_dir(&map).ok()?.flatten().find(|e| {
        e.file_name()
            .to_string_lossy()
            .eq_ignore_ascii_case("notinstances.lua")
    })?;
    let text = fs::read_to_string(file.path()).unwrap();
    Some(
        text.split("\"t:")
            .skip(1)
            .map(|s| s[..s.find('"').unwrap()].parse().unwrap())
            .collect(),
    )
}

#[test]
fn every_id_not_instances_lists_is_a_group_record() {
    let Some(root) = install_root() else { return };
    for dir in maps(&root) {
        let Some(listed) = not_instances(&root, &dir) else {
            continue;
        };
        let s = open(&root, &dir);
        let first = (s.nodes.len() as u64) << s.shift();
        let group: Vec<u64> = (first..first + s.group.len() as u64).collect();
        let missing = listed
            .iter()
            .filter(|id| group.binary_search(id).is_err())
            .count();
        eprintln!(
            "{dir}: {} listed, {} group records, {missing} listed ids not in the group",
            listed.len(),
            group.len()
        );
        assert_eq!(missing, 0, "{dir}");
        if dir == "Caucasus" {
            assert_eq!(listed, group);
        }
    }
}

#[test]
fn every_installed_scene_walks_with_each_pool_counted() {
    let Some(root) = install_root() else { return };
    for dir in maps(&root) {
        let path = scene_path(&root, &dir);
        let mut r = scn5::reader(&path).unwrap();
        let s = Scene::open(&mut r, &dir).unwrap();
        let mut counts = HashMap::new();
        s.walk(&mut r, |i| {
            *counts.entry(i.pool).or_insert(0u64) += 1;
            Ok(())
        })
        .unwrap_or_else(|e| panic!("{dir}: {e}"));
        let got = Pool::ALL.map(|p| counts.get(&p).copied().unwrap_or(0));
        let want = [
            s.pools[0].count + s.group.len() as u64,
            s.pools[1].count,
            s.pools[2].count,
        ];
        eprintln!("{dir}: shift {}, {got:?}", s.shift());
        assert_eq!(got, want, "{dir}");
    }
}
