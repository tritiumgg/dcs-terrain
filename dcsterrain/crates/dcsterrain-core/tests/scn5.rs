//! Opt-in: decodes real `.scn5` files. Set `DCSTERRAIN_INSTALL` to the DCS
//! install root, and `DCSTERRAIN_SCENE_REFERENCE` to the reference decode's
//! output directory to compare against; without them these pass without
//! reading anything, which is what CI does. Run them optimized,
//! `cargo test --release --test scn5`. The install is only read.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde_json::Value;

use dcsterrain_core::cache::Cache;
use dcsterrain_core::decode::airfield;
use dcsterrain_core::decode::install::{self, TERRAINS_DIR};
use dcsterrain_core::decode::scn5::{self, Index, Instance, Pool, Scene};
use dcsterrain_core::extract::TileEntry;
use dcsterrain_core::grid::{Grid, Window};
use dcsterrain_core::source::install::Install;
use dcsterrain_core::source::{Layer, Source};

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

/// Writes the `water` tiles of `id` on a grid of `tile_size` tiles under
/// `out`, as the scenery writer reads them to tell where the mesh covers,
/// and returns that grid and their entries. A tile wholly outside the mesh
/// is not written.
fn water_tiles(root: &Path, id: &str, tile_size: u32, out: &Path) -> (Grid, Vec<TileEntry>) {
    let cache = TempDir::new("cache");
    let t = Install::open(root, id, &Cache::new(&cache.0))
        .unwrap()
        .with_block_budget(4 << 30);
    let grid = Grid {
        tile_size,
        ..t.grid()
    };
    let mut entries = Vec::new();
    let (ntx, ntz) = grid.tiles();
    let ts = i64::from(tile_size);
    for tx in 0..ntx {
        for tz in 0..ntz {
            let (r, c) = (i64::from(tx) * ts, i64::from(tz) * ts);
            let w = Window::read(&t, Layer::Water, r, c, tile_size, tile_size).unwrap();
            if w.data.iter().all(|&v| v == Layer::Water.nodata()) {
                continue;
            }
            let path = format!("tiles/water/{tx}_{tz}.bin");
            let bytes: Vec<u8> = w.data.iter().map(|&v| v as u8).collect();
            fs::create_dir_all(out.join("tiles/water")).unwrap();
            fs::write(out.join(&path), bytes).unwrap();
            entries.push(TileEntry {
                layer: "water".into(),
                tx,
                tz,
                path,
                min: None,
                max: None,
            });
        }
    }
    (grid, entries)
}

/// Writes Caucasus's water tiles, then its scenery, and checks what landed.
#[test]
fn caucasus_writes_its_scenery_and_counts() {
    let Some(root) = install_root() else { return };
    let out = TempDir::new("caucasus");
    let started = Instant::now();
    let (grid, tiles) = water_tiles(&root, "Caucasus", 256, &out.0);
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
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_le_bytes(*c))
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

/// Every entry of every pool whose box meets one of `boxes`, as (pool, id)
/// per box, from a scan of each pool's whole id and box arrays.
fn scanned(path: &Path, idx: &Index, boxes: &[[f64; 4]]) -> Vec<HashSet<(usize, u64)>> {
    // Each box is listed under every 2 km cell it reaches.
    const CELL: f64 = 2000.0;
    let cell = |v: f64| (v / CELL).floor() as i64;
    let mut grid: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (k, b) in boxes.iter().enumerate() {
        for cx in cell(b[0])..=cell(b[2]) {
            for cz in cell(b[1])..=cell(b[3]) {
                grid.entry((cx, cz)).or_default().push(k);
            }
        }
    }
    let mut out = vec![HashSet::new(); boxes.len()];
    let mut r = scn5::reader(path).unwrap();
    let mut read = |at: u64, n: usize| {
        r.seek(SeekFrom::Start(at)).unwrap();
        let mut b = vec![0u8; n];
        r.read_exact(&mut b).unwrap();
        b
    };
    for (k, p) in idx.pools.iter().enumerate() {
        let mut i = 0;
        while i < p.count {
            let n = (1 << 20).min(p.count - i);
            let ids = read(idx.data_base + p.ids + 8 * i, 8 * n as usize);
            let aabbs = read(idx.data_base + p.aabbs + 24 * i, 24 * n as usize);
            for j in 0..n as usize {
                let f = |c: usize| {
                    f64::from(f32::from_le_bytes(
                        aabbs[24 * j + 4 * c..][..4].try_into().unwrap(),
                    ))
                };
                let a = [f(0), f(2), f(3), f(5)];
                let id = u64::from_le_bytes(ids[8 * j..8 * j + 8].try_into().unwrap());
                let mut seen = HashSet::new();
                for cx in cell(a[0])..=cell(a[2]) {
                    for cz in cell(a[1])..=cell(a[3]) {
                        for &q in grid.get(&(cx, cz)).into_iter().flatten() {
                            let b = boxes[q];
                            if seen.insert(q)
                                && a[0] <= b[2]
                                && b[0] <= a[2]
                                && a[1] <= b[3]
                                && b[1] <= a[3]
                            {
                                out[q].insert((k, id));
                            }
                        }
                    }
                }
            }
            i += n;
        }
    }
    out
}

/// `n` 2 km boxes over `id`, half centred on objects and half anywhere in
/// the terrain, each answered through the install's scenery and checked
/// against a scan of every pool box, and each instance against the walk.
fn boxes_equal_a_full_walk(root: &Path, id: &str, n: usize) {
    let cache = TempDir::new(&format!("boxes-{id}"));
    let t = Install::open(root, id, &Cache::new(&cache.0)).unwrap();
    let started = Instant::now();
    let s = t.scenery().unwrap();
    let size = fs::metadata(
        cache
            .0
            .join(id)
            .join(&t.map().build.version)
            .join("scene.nodes"),
    )
    .unwrap()
    .len();
    eprintln!(
        "{id}: scene.nodes {:.2} MiB, built and read in {:.2} s",
        size as f64 / 1048576.0,
        started.elapsed().as_secs_f64()
    );
    let idx = s.index();
    let path = root.join(
        &t.map()
            .files
            .iter()
            .find(|f| f.role == "scene")
            .unwrap()
            .path,
    );

    let mut seed = 11u64;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let g = t.grid();
    let (x0, z0) = (g.origin_x, g.origin_z);
    let (dx, dz) = (
        f64::from(g.height * g.cell_size),
        f64::from(g.width * g.cell_size),
    );
    let mut objects = Vec::new();
    s.each_in(Pool::Objects, [x0, z0, x0 + dx, z0 + dz], |i| {
        objects.push((i.pos[0], i.pos[2]))
    })
    .unwrap();
    let boxes: Vec<[f64; 4]> = (0..n)
        .map(|k| {
            let (x, z) = if k < n / 2 {
                objects[(next() % objects.len() as u64) as usize]
            } else {
                let f = |v: u64| (v >> 11) as f64 / (1u64 << 53) as f64;
                (x0 + f(next()) * dx, z0 + f(next()) * dz)
            };
            [x - 1000.0, z - 1000.0, x + 1000.0, z + 1000.0]
        })
        .collect();

    let mut lat = Vec::new();
    let mut got: Vec<HashMap<(usize, u64), Instance>> = Vec::new();
    for &b in &boxes {
        let mut here = HashMap::new();
        let started = Instant::now();
        for (k, pool) in Pool::ALL.into_iter().enumerate() {
            s.each_in(pool, b, |i| {
                assert!(
                    here.insert((k, i.id), i.clone()).is_none(),
                    "{id}: {} twice",
                    i.id
                );
            })
            .unwrap();
        }
        lat.push(started.elapsed().as_secs_f64() * 1e3);
        got.push(here);
    }
    lat.sort_by(f64::total_cmp);
    let hits: usize = got.iter().map(HashMap::len).sum();
    eprintln!(
        "{id}: {n} boxes, {hits} instances, all three pools per box in p50 {:.2} p95 {:.2} max {:.2} ms",
        lat[n / 2],
        lat[n * 95 / 100],
        lat[n - 1]
    );

    let mut want = scanned(&path, idx, &boxes);
    let first_group = (idx.nodes.len() as u64) << idx.shift;
    let needed: HashSet<(usize, u64)> = got.iter().flat_map(|h| h.keys().copied()).collect();
    let mut walked: HashMap<(usize, u64), Instance> = HashMap::new();
    let mut r = scn5::reader(&path).unwrap();
    let scene = Scene::open(&mut r, id).unwrap();
    scene
        .walk(&mut r, |i| {
            let k = Pool::ALL.iter().position(|&p| p == i.pool).unwrap();
            if i.id >= first_group {
                let bbox = &scene.models[i.model as usize].bbox;
                let f = scn5::footprint(bbox, i.pos, i.rotation);
                for (q, b) in boxes.iter().enumerate() {
                    if f[0] <= b[2] && b[0] <= f[2] && f[1] <= b[3] && b[1] <= f[3] {
                        want[q].insert((k, i.id));
                    }
                }
            }
            if needed.contains(&(k, i.id)) {
                walked.insert((k, i.id), i.clone());
            }
            Ok(())
        })
        .unwrap();
    for (q, (g, w)) in got.iter().zip(&want).enumerate() {
        let ids: HashSet<(usize, u64)> = g.keys().copied().collect();
        assert_eq!(ids.len(), w.len(), "{id}: box {q} {:?}", boxes[q]);
        assert!(
            ids == *w,
            "{id}: box {q} {:?} differs from the scan",
            boxes[q]
        );
        for (key, i) in g {
            assert_eq!(Some(i), walked.get(key), "{id}: box {q}");
        }
    }
    eprintln!("{id}: every box equals the scan and every instance the walk");
}

/// Every installed terrain builds its index, within the 10 s a cache entry
/// is given, and answers a box at its center.
#[test]
fn every_installed_scene_builds_its_index() {
    let Some(root) = install_root() else { return };
    for dir in maps(&root) {
        let map = install::read_map(&root, &dir).unwrap();
        let cache = TempDir::new(&format!("index-{dir}"));
        let t = Install::open(&root, &map.theatre, &Cache::new(&cache.0)).unwrap();
        let started = Instant::now();
        let s = t.scenery().unwrap_or_else(|e| panic!("{dir}: {e}"));
        let built = started.elapsed().as_secs_f64();
        let size = fs::metadata(
            cache
                .0
                .join(&map.theatre)
                .join(&map.build.version)
                .join("scene.nodes"),
        )
        .unwrap()
        .len();
        let g = t.grid();
        let (x, z) = g.centre(g.height / 2, g.width / 2);
        let started = Instant::now();
        let mut n = 0;
        for pool in Pool::ALL {
            s.each_in(
                pool,
                [x - 1000.0, z - 1000.0, x + 1000.0, z + 1000.0],
                |_| n += 1,
            )
            .unwrap();
        }
        eprintln!(
            "{dir}: scene.nodes {:.2} MiB in {built:.2} s; {n} instances in 2 km at the center in {:.2} ms",
            size as f64 / 1048576.0,
            started.elapsed().as_secs_f64() * 1e3
        );
        assert!(built < 10.0, "{dir}");
    }
}

#[test]
fn caucasus_boxes_equal_a_full_walk() {
    let Some(root) = install_root() else { return };
    boxes_equal_a_full_walk(&root, "Caucasus", 400);
}

#[test]
fn nevada_boxes_equal_a_full_walk() {
    let Some(root) = install_root() else { return };
    boxes_equal_a_full_walk(&root, "Nevada", 400);
}

/// Folds an angle in degrees to its distance from the nearest multiple of
/// 90°: edge lights face along or across their runway.
fn fold(deg: f64) -> f64 {
    let a = deg.rem_euclid(90.0);
    a.min(90.0 - a)
}

/// Nevada's runway lights face along or across their runway, as the airfield
/// files pair its ends. A light is an object or line whose model names one
/// or is an in-pavement light, within 60 m of a centerline and between its
/// ends. Only runways at least 15° off the grid's axes are scored, where an
/// axis read the wrong way round would show.
#[test]
fn nevada_runway_lights_align_with_their_runways() {
    let Some(root) = install_root() else { return };
    let map = install::read_map(&root, "Nevada").unwrap();
    let decoded = airfield::decode(&root, &install::classify(&root, &map.files).unwrap()).unwrap();
    // Each runway as its first end, unit course, length and course.
    let mut runways = Vec::new();
    for rw in &decoded.runways {
        let (x1, z1, x2, z2) = (rw.edge1_x, rw.edge1_z, rw.edge2_x, rw.edge2_z);
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
