//! Opt-in: decodes real `.scn5` files. Set `DCSTERRAIN_INSTALL` to the DCS
//! install root, and `DCSTERRAIN_SCENE_REFERENCE` to the reference decode's
//! output directory to compare against; without them these pass without
//! reading anything, which is what CI does. Run them optimized,
//! `cargo test --release --test scn5`. The install is only read.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde_json::Value;

use dcsterrain_core::decode::install::{self, TERRAINS_DIR};
use dcsterrain_core::decode::scn5::{self, Pool, Scene};
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

/// A fresh directory under the system temp, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir =
            std::env::temp_dir().join(format!("dcsterrain-scn5-{name}-{}", std::process::id()));
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

fn surface(root: &Path, dir: &str) -> (PathBuf, surface5::Surface5) {
    let map = install::read_map(root, dir).unwrap();
    let path = root.join(&map.files.iter().find(|f| f.role == "surface").unwrap().path);
    let file = surface5::open(&path).unwrap_or_else(|e| panic!("{dir}: {e}"));
    (path, file)
}

/// Writes Caucasus's water tiles, then its scenery, and checks what landed.
#[test]
fn caucasus_writes_its_scenery_and_counts() {
    let Some(root) = install_root() else { return };
    let out = TempDir::new("caucasus");
    let (spath, sfile) = surface(&root, "Caucasus");
    let grid = surface5::grid_for(&sfile.bounds());
    let started = Instant::now();
    let tiles = surface5::write_tiles(&sfile, || surface5::reader(&spath), &grid, &out.0).unwrap();
    eprintln!("ground tiles in {:.1} s", started.elapsed().as_secs_f64());

    let path = scene_path(&root, "Caucasus");
    let mut r = scn5::reader(&path).unwrap();
    let s = Scene::open(&mut r, "Caucasus").unwrap();
    let started = Instant::now();
    let w = scn5::write(&s, &mut r, &grid, &tiles, &out.0).unwrap();
    eprintln!(
        "scenery in {:.1} s: {} objects, {} lines, {} trees, {} models, {} count tiles",
        started.elapsed().as_secs_f64(),
        w.objects,
        w.lines,
        w.trees,
        w.models,
        w.tiles.len()
    );
    assert_eq!(w.objects, 917_847 + 18_976);
    assert_eq!((w.lines, w.trees), (1_069_315, 12_967_324));

    let rows = BufReader::new(File::open(out.0.join(scn5::SCENERY_FILE)).unwrap())
        .lines()
        .count() as u64;
    assert_eq!(rows, w.objects);
    let models: Value =
        serde_json::from_str(&fs::read_to_string(out.0.join(scn5::MODELS_FILE)).unwrap()).unwrap();
    let models = models.as_array().unwrap();
    assert_eq!(models.len(), w.models);
    let sum = |k: &str| {
        models
            .iter()
            .map(|m| m["counts"][k].as_u64().unwrap())
            .sum::<u64>()
    };
    assert_eq!(
        (sum("objects"), sum("lines"), sum("trees")),
        (w.objects, w.lines, w.trees)
    );

    for (name, total) in [("trees", w.trees), ("lines", w.lines)] {
        let mut counted = 0u64;
        for t in w.tiles.iter().filter(|t| t.layer == name) {
            let b = fs::read(out.0.join(&t.path)).unwrap();
            counted += b
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .filter(|&c| c != 65535)
                .map(u64::from)
                .sum::<u64>();
        }
        let share = counted as f64 / total as f64;
        eprintln!(
            "{name}: {counted} of {total} counted into covered cells, {:.4} %",
            100.0 * share
        );
        assert!(share > 0.999, "{name}");
    }
}

/// Folds an angle in degrees to its distance from the nearest multiple of
/// 90°: edge lights face along or across their runway.
fn fold(deg: f64) -> f64 {
    let a = deg.rem_euclid(90.0);
    a.min(90.0 - a)
}

/// Nevada's runway lights face along or across their runway. The runways
/// are the reference decode's, `<Map>/airfields.json`, paired ends read
/// from the airfield files. A light is an object or line whose model names
/// one or is an in-pavement light, within 60 m of a centerline and between
/// its ends. Only runways at least 15° off the grid's axes are scored,
/// where an axis read the wrong way round would show.
#[test]
fn nevada_runway_lights_align_with_their_runways() {
    let (Some(root), Some(refdir)) = (install_root(), reference()) else {
        return;
    };
    let airfields: Value = serde_json::from_str(
        &fs::read_to_string(refdir.join("Nevada").join("airfields.json")).unwrap(),
    )
    .unwrap();
    // Each runway as its first end, unit course, length and course.
    let mut runways = Vec::new();
    for rw in airfields
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|a| a["runways"].as_array().unwrap())
    {
        let f = |k: &str| rw[k].as_f64().unwrap();
        let (x1, z1, x2, z2) = (f("edge1_x"), f("edge1_z"), f("edge2_x"), f("edge2_z"));
        let len = (x2 - x1).hypot(z2 - z1);
        let course = (z2 - z1).atan2(x2 - x1).to_degrees();
        if fold(course) >= 15.0 {
            runways.push(([x1, z1], [(x2 - x1) / len, (z2 - z1) / len], len, course));
        }
    }

    let path = scene_path(&root, "Nevada");
    let mut r = scn5::reader(&path).unwrap();
    let s = Scene::open(&mut r, "Nevada").unwrap();
    let (mut scored, mut aligned, mut swapped) = (0u64, 0u64, 0u64);
    let mut models: HashMap<String, u64> = HashMap::new();
    let mut last = None;
    s.walk(&mut r, |i| {
        let model = &s.models[i.model as usize].name;
        // In-pavement lights are lines, and some are objects too: each is
        // scored once.
        let light = model.contains("light") || model.starts_with("in_pavement");
        if i.pool == Pool::Trees || !light || last == Some(i.id) {
            return Ok(());
        }
        last = Some(i.id);
        let Some(&(_, _, _, course)) = runways.iter().find(|(p, u, len, _)| {
            let (dx, dz) = (i.pos[0] - p[0], i.pos[2] - p[1]);
            let along = dx * u[0] + dz * u[1];
            along > 0.0 && along < *len && (dz * u[0] - dx * u[1]).abs() < 60.0
        }) else {
            return Ok(());
        };
        let h = i.rotation.to_degrees();
        scored += 1;
        aligned += u64::from(fold(h - course) < 2.0);
        swapped += u64::from(fold(90.0 - h - course) < 2.0);
        *models.entry(model.clone()).or_default() += 1;
        Ok(())
    })
    .unwrap();
    let mut top: Vec<(String, u64)> = models.into_iter().collect();
    top.sort_by_key(|m| std::cmp::Reverse(m.1));
    top.truncate(6);
    let pct = |n: u64| 100.0 * n as f64 / scored as f64;
    eprintln!(
        "Nevada: {} runways off the axes, {scored} lights on them, {:.1} % within 2° of the course, \
         {:.1} % with the axes swapped; {top:?}",
        runways.len(),
        pct(aligned),
        pct(swapped)
    );
    assert!(scored >= 500, "too few lights scored");
    assert!(pct(aligned) >= 85.0, "lights aligned");
    assert!(pct(swapped) <= 5.0, "lights aligned with the axes swapped");
}
