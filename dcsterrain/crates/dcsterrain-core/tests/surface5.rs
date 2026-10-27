//! Opt-in: decodes real `.surface5` files. Set `DCSTERRAIN_INSTALL` to the
//! DCS install root; without it these pass without reading anything, which
//! is what CI does. Run them optimized, `cargo test --release --test
//! surface5`: an unoptimized decode of a whole map takes minutes. The
//! install is only read; the cache they build is a scratch directory.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Instant;

use serde_json::Value;

use dcsterrain_core::cache::Cache;
use dcsterrain_core::decode::install::{self, TERRAINS_DIR};
use dcsterrain_core::decode::surface5;
use dcsterrain_core::decode::surface5::ground::{Prepared, grid_units};
use dcsterrain_core::grid::Grid;
use dcsterrain_core::source::Source;
use dcsterrain_core::source::install::Install;

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
        let r = fs::File::open(&path).unwrap();
        let (mut tris, mut verts, mut runway) = (0, 0, 0);
        // One block at a time: the largest map's mesh is gigabytes.
        for i in 0..file.blocks.len() {
            let b = file
                .read_block(&r, i, &["Runway"])
                .unwrap_or_else(|e| panic!("{dir}: {e}"));
            tris += b.tri.len();
            verts += b.p.len();
            runway += b.subsets.iter().map(|s| s.tri.len()).sum::<usize>();
        }
        let bd = file.bounds();
        eprintln!(
            "{dir}: {} blocks, {tris} triangles, {verts} vertices, {runway} runway triangles, \
             x {}..{} z {}..{}, table of contents {} bytes",
            file.blocks.len(),
            bd.min_x,
            bd.max_x,
            bd.min_z,
            bd.max_z,
            file.encode().len()
        );
        assert!(tris > 0, "{dir}: no triangles");
    }
}

/// Opens every terrain through a fresh cache, then from it five times, and
/// reports how long the build of `mesh.toc` and the opens take. An open
/// reads one header and checks its entry from a stat, so the large
/// terrains, with 450 airfield files, open within 20 ms. The fastest open
/// is held to that, since the other opt-in tests read whole meshes beside
/// this one.
#[test]
fn every_terrain_opens_from_its_cache() {
    let Some(root) = install_root() else { return };
    let cache = Scratch::new("open-cache");
    for listed in install::terrains(&root).unwrap() {
        let Some(id) = listed.id else { continue };
        let ms = |t: Instant| t.elapsed().as_secs_f64() * 1e3;
        let started = Instant::now();
        Install::open(&root, &id, &Cache::new(&cache.0)).unwrap_or_else(|e| panic!("{id}: {e}"));
        let built = ms(started);
        let mut times: Vec<f64> = (0..5)
            .map(|_| {
                let started = Instant::now();
                Install::open(&root, &id, &Cache::new(&cache.0)).unwrap();
                ms(started)
            })
            .collect();
        times.sort_by(f64::total_cmp);
        let fastest = times[0];
        if id == "GermanyCW" || id == "Syria" {
            assert!(fastest < 20.0, "{id} opens in {fastest:.1} ms");
        }
        let g = Install::open(&root, &id, &Cache::new(&cache.0))
            .unwrap()
            .grid();
        let started = Instant::now();
        let map = install::open(&root, &id).unwrap();
        let find = ms(started);
        let tc = Cache::new(&cache.0).terrain(&root, &map);
        let started = Instant::now();
        let o = tc.get(&dcsterrain_core::source::install::MESH_TOC).unwrap();
        let key = ms(started);
        let started = Instant::now();
        let fields = map.stat(&root, &["airfield_rn4", "airfield_rn5"]).unwrap();
        let stat = ms(started);
        let started = Instant::now();
        let bytes = o.payload().unwrap();
        let read = ms(started);
        let started = Instant::now();
        surface5::Surface5::decode(&bytes).unwrap();
        let decode = ms(started);
        eprintln!(
            "{id}: built in {built:.0} ms, opened in {fastest:.1} ms (median {:.1}), {} x {} cells; \
             find {find:.1}, key {key:.1}, {} airfield files stated in {stat:.2}, \
             read {read:.1}, decode {decode:.1} ms",
            times[2],
            g.height,
            g.width,
            fields.len()
        );
    }
}

/// 10 000 points on a 100 by 100 lattice over `[x0, z0, x1, z1]`.
fn lattice([x0, z0, x1, z1]: [f64; 4]) -> Vec<(f64, f64)> {
    let at = |lo: f64, hi: f64, k: u32| lo + (hi - lo) * (f64::from(k) + 0.5) / 100.0;
    (0..100)
        .flat_map(|i| (0..100).map(move |j| (at(x0, x1, i), at(z0, z1, j))))
        .collect()
}

/// Milliseconds for the fastest of three passes of `f` over `points`, and
/// what the last pass counted.
fn timed(points: &[(f64, f64)], f: impl Fn(f64, f64) -> bool) -> (f64, usize) {
    let mut best = (f64::INFINITY, 0);
    for _ in 0..3 {
        let started = Instant::now();
        let n = points.iter().filter(|&&(x, z)| f(x, z)).count();
        best = (best.0.min(started.elapsed().as_secs_f64() * 1e3), n);
    }
    best
}

/// Over the block with the most runway triangles, the runway test costs no
/// more than finding the winning triangle, because a point looks only at the
/// runway triangles near it, where it once tried every one. Asks every
/// terrain, and reports the size of the table of contents the cache keeps
/// and the time for the same lattice over a block of like size with no
/// runway.
#[test]
fn points_over_an_airfield_answer_as_fast_as_points_elsewhere() {
    let Some(root) = install_root() else { return };
    let build = install::read_build(&root).unwrap();
    eprintln!("DCS {} ({})", build.version, build.timestamp);
    let cache = Scratch::new("runway-cache");
    for listed in install::terrains(&root).unwrap() {
        let Some(id) = listed.id else { continue };
        let map = install::open(&root, &id).unwrap();
        let path = root.join(&map.surface.path);
        let file = surface5::open(&path).unwrap();
        let r = fs::File::open(&path).unwrap();
        let mut runway: Vec<(usize, usize)> = (0..file.blocks.len())
            .map(|i| {
                let b = file.read_block(&r, i, &["Runway"]).unwrap();
                (b.subsets.iter().map(|s| s.tri.len()).sum::<usize>(), i)
            })
            .collect();
        runway.sort_unstable();
        let (most, busiest) = *runway.last().unwrap();
        if most == 0 {
            continue;
        }
        let size = |i: usize| {
            let [x0, z0, x1, z1] = file.blocks[i].world_box();
            (x1 - x0) * (z1 - z0)
        };
        let plain = runway
            .iter()
            .filter(|&&(n, _)| n == 0)
            .min_by(|a, b| {
                let d = |i| (size(i) / size(busiest)).ln().abs();
                d(a.1).total_cmp(&d(b.1))
            })
            .map(|&(_, i)| i)
            .unwrap();

        let t = Install::open(&root, &id, &Cache::new(&cache.0)).unwrap();
        let toc = fs::metadata(
            cache
                .0
                .join(&map.theatre)
                .join(&build.version)
                .join("mesh.toc"),
        )
        .unwrap()
        .len();
        let g = t.grid();
        let over = lattice(file.blocks[busiest].world_box());
        let (all, on) = timed(&over, |x, z| {
            t.point(x, z).unwrap().is_some_and(|p| p.surface == 5)
        });
        let (elsewhere, _) = timed(&lattice(file.blocks[plain].world_box()), |x, z| {
            t.point(x, z).unwrap().is_some()
        });
        let b = Prepared::new(file.read_block(&r, busiest, &["Runway"]).unwrap(), &g);
        let (ground, _) = timed(&over, |x, z| b.point(grid_units(&g, x, z)).is_some());
        let (strip, _) = timed(&over, |x, z| b.on_runway(grid_units(&g, x, z)));
        eprintln!(
            "{id}: mesh.toc {:.2} MB; block {busiest}, {most} runway triangles, {on} of the \
             points on a runway: {all:.1} ms, of which its winning triangle {ground:.1} ms and \
             its runway {strip:.1} ms; block {plain}, no runway: {elsewhere:.1} ms",
            toc as f64 / 1e6
        );
        assert!(
            on > 0,
            "{id}: no point over its busiest airfield is on a runway"
        );
        assert!(
            strip < ground.max(0.5),
            "{id}: the runway test takes {strip:.1} ms, the winning triangle {ground:.1} ms"
        );
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

/// A fresh directory under the system temp, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("dcsterrain-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Tallies of a comparison, summed across threads.
#[derive(Default)]
struct Score {
    cells: u64,
    exact: u64,
    within_1: u64,
    over_2: u64,
    worst: i32,
    land: u64,
    land_exact: u64,
    water_cells: u64,
    water_wrong: u64,
    /// Cells the live extract wrote as fill that the mesh covers.
    fill_covered: u64,
    /// Of those, the ones that are not flat 5 m land.
    fill_not_flat: u64,
    /// Live data cells the mesh leaves uncovered.
    covered_fill: u64,
    /// Covered cells in tiles the live extract did not write.
    unlisted_covered: u64,
}

impl Score {
    fn add(&mut self, o: &Score) {
        self.cells += o.cells;
        self.exact += o.exact;
        self.within_1 += o.within_1;
        self.over_2 += o.over_2;
        self.worst = self.worst.max(o.worst);
        self.land += o.land;
        self.land_exact += o.land_exact;
        self.water_cells += o.water_cells;
        self.water_wrong += o.water_wrong;
        self.fill_covered += o.fill_covered;
        self.fill_not_flat += o.fill_not_flat;
        self.covered_fill += o.covered_fill;
        self.unlisted_covered += o.unlisted_covered;
    }
}

/// The install's height and water over tile `(tx, tz)` of the live
/// extract's grid, row-major, read from the install's own smaller tiles.
/// `(dr, dc)` is where the live grid's first cell lies on the install's.
fn install_cells(
    t: &Install,
    live: &Grid,
    tx: u32,
    tz: u32,
    (dr, dc): (i64, i64),
) -> (Vec<i16>, Vec<u8>) {
    let ls = i64::from(live.tile_size);
    let mine = t.grid();
    let ms = i64::from(mine.tile_size);
    let n = (ls * ls) as usize;
    let (mut h, mut w) = (vec![-32768i16; n], vec![255u8; n]);
    let (r0, c0) = (i64::from(tx) * ls + dr, i64::from(tz) * ls + dc);
    // The live grid's last tiles reach past its edge, where it wrote nodata.
    let (rl, rh) = (
        r0.max(0),
        (r0 + ls)
            .min(i64::from(mine.height))
            .min(i64::from(live.height) + dr),
    );
    let (cl, ch) = (
        c0.max(0),
        (c0 + ls)
            .min(i64::from(mine.width))
            .min(i64::from(live.width) + dc),
    );
    if rl >= rh || cl >= ch {
        return (h, w);
    }
    for mx in rl / ms..=(rh - 1) / ms {
        for mz in cl / ms..=(ch - 1) / ms {
            let g = t.ground_tile(mx as u32, mz as u32).unwrap();
            for r in rl.max(mx * ms)..rh.min((mx + 1) * ms) {
                for c in cl.max(mz * ms)..ch.min((mz + 1) * ms) {
                    let from = ((r - mx * ms) * ms + (c - mz * ms)) as usize;
                    let to = ((r - r0) * ls + (c - c0)) as usize;
                    h[to] = g.height[from];
                    w[to] = g.water[from];
                }
            }
        }
    }
    (h, w)
}

/// Scores the cells of one live tile against the install, cell by cell: the
/// live extract's tiles are 256 cells and the install's 64.
fn score_tile(t: &Install, dir: &Path, live: &Grid, tx: u32, tz: u32, d: (i64, i64)) -> Score {
    let mut s = Score::default();
    let (th, tw) = install_cells(t, live, tx, tz, d);
    let Some(w) = live_tile(dir, "water", tx, tz) else {
        s.unlisted_covered = tw.iter().filter(|&&v| v != 255).count() as u64;
        return s;
    };
    // A sea tile the live extract wrote without a height tile reads as
    // height 0, as its omission rule says.
    let h: Vec<i16> = match live_tile(dir, "height", tx, tz) {
        Some(b) => b
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| i16::from_le_bytes(*c))
            .collect(),
        None => vec![0; tw.len()],
    };
    for i in 0..tw.len() {
        match (w[i] == 255, tw[i] == 255) {
            (true, false) => {
                s.fill_covered += 1;
                // The live fill test took flat 5 m land for no terrain, though it is ground.
                s.fill_not_flat += u64::from(th[i] != 5 || tw[i] != 0);
            }
            (false, true) => s.covered_fill += 1,
            _ => {}
        }
        if w[i] == 255 || h[i] == -32768 {
            continue;
        }
        s.water_cells += 1;
        s.water_wrong += u64::from(w[i] != tw[i]);
        if tw[i] == 255 {
            continue;
        }
        let e = (i32::from(th[i]) - i32::from(h[i])).abs();
        s.cells += 1;
        s.exact += u64::from(e == 0);
        s.within_1 += u64::from(e <= 1);
        s.over_2 += u64::from(e > 2);
        s.worst = s.worst.max(e);
        if w[i] == 0 {
            s.land += 1;
            s.land_exact += u64::from(e == 0);
        }
    }
    s
}

/// Opens Caucasus through a fresh cache, times a second open from the
/// cache, then scores every cell of the live extract's grid against the
/// install's tiles, in memory.
#[test]
fn caucasus_matches_the_live_extract() {
    let Some(root) = install_root() else { return };
    let Some(dir) = live_extract() else { return };
    let build = install::read_build(&root).unwrap();
    let manifest: Value =
        serde_json::from_str(&fs::read_to_string(dir.join("manifest.json")).unwrap()).unwrap();
    eprintln!(
        "DCS {} ({}) against a live extract from {}",
        build.version, build.timestamp, manifest["dcs_build"]
    );
    assert_eq!(
        manifest["theatre"], "Caucasus",
        "the live extract's theatre"
    );
    let g = &manifest["grid"];
    let live = Grid {
        cell_size: g["cell_size"].as_u64().unwrap() as u32,
        origin_x: g["origin_x"].as_f64().unwrap(),
        origin_z: g["origin_z"].as_f64().unwrap(),
        height: g["height"].as_u64().unwrap() as u32,
        width: g["width"].as_u64().unwrap() as u32,
        tile_size: g["tile_size"].as_u64().unwrap() as u32,
    };

    let cache = Scratch::new("surface5-cache");
    let started = Instant::now();
    Install::open(&root, "Caucasus", &Cache::new(&cache.0)).unwrap();
    let ms = |t: Instant| t.elapsed().as_secs_f64() * 1e3;
    eprintln!("first open, building mesh.toc: {:.0} ms", ms(started));
    let started = Instant::now();
    let t = Install::open(&root, "Caucasus", &Cache::new(&cache.0)).unwrap();
    let open_ms = ms(started);
    eprintln!("second open, reading mesh.toc: {open_ms:.1} ms");
    // Room for every block the threads hold at once; the default budget is
    // for one query, not a whole terrain at full speed.
    let t = t.with_block_budget(4 << 30);

    let mine = t.grid();
    assert_eq!(mine.cell_size, live.cell_size);
    let cs = f64::from(mine.cell_size);
    let offset = |a: f64, b: f64| {
        let d = (a - b) / cs;
        assert_eq!(d, d.round(), "the grids' origins differ by a whole cell");
        d as i64
    };
    let d = (
        offset(live.origin_x, mine.origin_x),
        offset(live.origin_z, mine.origin_z),
    );

    let (ntx, ntz) = live.tiles();
    let tiles: Vec<(u32, u32)> = (0..ntx)
        .flat_map(|tx| (0..ntz).map(move |tz| (tx, tz)))
        .collect();
    let next = AtomicUsize::new(0);
    let total = Mutex::new(Score::default());
    let started = Instant::now();
    thread::scope(|sc| {
        for _ in 0..thread::available_parallelism().map_or(1, |n| n.get()) {
            sc.spawn(|| {
                let mut s = Score::default();
                while let Some(&(tx, tz)) = tiles.get(next.fetch_add(1, Ordering::Relaxed)) {
                    s.add(&score_tile(&t, &dir, &live, tx, tz, d));
                }
                total.lock().unwrap().add(&s);
            });
        }
    });
    let s = total.into_inner().unwrap();
    let pct = |a: u64, b: u64| 100.0 * a as f64 / b as f64;
    eprintln!("compared in {:.1} s", started.elapsed().as_secs_f64());
    eprintln!(
        "height: {} cells, {:.4} % exact, {:.5} % within 1 m, {} over 2 m, worst {} m; \
         land {:.4} % exact",
        s.cells,
        pct(s.exact, s.cells),
        pct(s.within_1, s.cells),
        s.over_2,
        s.worst,
        pct(s.land_exact, s.land)
    );
    eprintln!("water: {} of {} cells differ", s.water_wrong, s.water_cells);
    eprintln!(
        "coverage: {} live fill cells the mesh covers, {} live cells outside the mesh, \
         {} covered cells in tiles the live extract did not write",
        s.fill_covered, s.covered_fill, s.unlisted_covered
    );
    // 99.95 % at two decimals, the figure the reverse-engineered decode reached.
    assert!(pct(s.exact, s.cells) >= 99.945, "heights exact");
    assert!(s.water_wrong <= 67, "water cells that differ");
    assert_eq!(
        s.fill_not_flat, 0,
        "live fill cells the mesh covers with anything but 5 m land"
    );
    assert_eq!(
        s.covered_fill + s.unlisted_covered,
        0,
        "live data cells outside the mesh"
    );
    assert!(
        open_ms < 20.0,
        "an open from the cache took {open_ms:.1} ms"
    );
}
