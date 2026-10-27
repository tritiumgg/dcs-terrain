//! A terrain of a DCS install as a `Source`, read in place.
//!
//! Opening one reads its table of contents from the cache's `mesh.toc`,
//! built from the `.surface5` file the first time. A tile or point then reads
//! only the blocks under it, each at its offset, and keeps the prepared
//! blocks and the rasterized tiles in small caches, so a batch of requests
//! over one area reads its blocks once. Nothing maps the whole file.

use std::collections::HashMap;
use std::fs::File;
use std::hash::Hash;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::{CELL_SIZE, Ground, Identity, Layer, Refusal, Source, TILE_SIZE};
use crate::cache::{Cache, Entry};
use crate::decode::install::{self, Map};
use crate::decode::surface5::ground::{self, Prepared, Tile, cover_of, grid_units, surface_of};
use crate::decode::surface5::{RUNWAY_MATERIAL, Surface5, grid_for, water_class};
use crate::grid::Grid;

/// The bytes of prepared blocks kept between requests.
pub const BLOCK_BUDGET: usize = 32 << 20;
/// Rasterized tiles kept between requests, of every layer: about 20 KiB each.
pub const TILE_COUNT: usize = 256;

/// How far past its stated box a block is read for a point or tile. Its
/// vertices lie inside the box to within a centimeter.
const BOX_PAD_M: f64 = 1.0;

/// The `.surface5` table of contents.
pub struct MeshToc;

pub static MESH_TOC: MeshToc = MeshToc;

fn surface_file(map: &Map) -> Result<&crate::extract::FileEntry, Refusal> {
    map.files
        .iter()
        .find(|f| f.role == "surface")
        .ok_or_else(|| Refusal(format!("{} has no surface file", map.theatre)))
}

/// A decoder's error, naming the file unless the error already does.
fn refusal(path: &str, e: impl std::fmt::Display) -> Refusal {
    let e = e.to_string();
    if e.starts_with(path) {
        Refusal(e)
    } else {
        Refusal(format!("{path}: {e}"))
    }
}

impl Entry for MeshToc {
    fn name(&self) -> &str {
        "mesh.toc"
    }

    fn version(&self) -> u32 {
        1
    }

    fn roles(&self) -> &[&str] {
        &["surface"]
    }

    fn build(&self, root: &Path, map: &Map) -> Result<Vec<u8>, Refusal> {
        let f = surface_file(map)?;
        let file = File::open(root.join(&f.path)).map_err(|e| refusal(&f.path, e))?;
        let toc = Surface5::open(&file, &f.path).map_err(|e| refusal(&f.path, e))?;
        if toc.blocks.is_empty() {
            return Err(Refusal(format!("{}: the mesh has no blocks", f.path)));
        }
        Ok(toc.encode())
    }
}

/// Least recently used values within a budget of their sizes. The last
/// value put is kept whatever its size.
struct Lru<K, V> {
    budget: usize,
    used: usize,
    tick: u64,
    items: HashMap<K, (V, usize, u64)>,
}

impl<K: Eq + Hash + Copy, V: Clone> Lru<K, V> {
    fn new(budget: usize) -> Self {
        Lru {
            budget,
            used: 0,
            tick: 0,
            items: HashMap::new(),
        }
    }

    fn get(&mut self, k: &K) -> Option<V> {
        self.tick += 1;
        let tick = self.tick;
        self.items.get_mut(k).map(|(v, _, t)| {
            *t = tick;
            v.clone()
        })
    }

    fn put(&mut self, k: K, v: V, size: usize) {
        self.tick += 1;
        if let Some((_, old, _)) = self.items.insert(k, (v, size, self.tick)) {
            self.used -= old;
        }
        self.used += size;
        while self.used > self.budget && self.items.len() > 1 {
            let (&oldest, _) = self.items.iter().min_by_key(|(_, (_, _, t))| *t).unwrap();
            let (_, size, _) = self.items.remove(&oldest).unwrap();
            self.used -= size;
        }
    }
}

/// The tiles of `g` a world rectangle `[x0, z0, x1, z1]` reaches into, as
/// row and column ranges clipped to the grid; `None` when it lies off it.
fn tile_span(
    g: &Grid,
    [x0, z0, x1, z1]: [f64; 4],
) -> Option<(
    std::ops::RangeInclusive<usize>,
    std::ops::RangeInclusive<usize>,
)> {
    let tm = f64::from(g.tile_size) * f64::from(g.cell_size);
    let (ntx, ntz) = g.tiles();
    let span = |lo: f64, hi: f64, origin: f64, n: u32| {
        let lo = ((lo - origin) / tm).floor().max(0.0);
        let hi = ((hi - origin) / tm).floor().min(f64::from(n) - 1.0);
        // A NaN fails the comparison and lands here too.
        (lo <= hi).then_some(lo as usize..=hi as usize)
    };
    Some((
        span(x0, x1, g.origin_x, ntx)?,
        span(z0, z1, g.origin_z, ntz)?,
    ))
}

/// For each tile of `g`, row-major, the blocks whose box, padded, reaches
/// into it: a tile or point then looks at a handful of blocks rather than
/// every block of the terrain, thirteen thousand on the largest.
fn tile_index(g: &Grid, boxes: &[[f64; 4]]) -> (Vec<u32>, Vec<u32>) {
    let (ntx, ntz) = g.tiles();
    let n = ntx as usize * ntz as usize;
    let p = BOX_PAD_M;
    let spans: Vec<_> = boxes
        .iter()
        .map(|b| tile_span(g, [b[0] - p, b[1] - p, b[2] + p, b[3] + p]))
        .collect();
    let mut start = vec![0u32; n + 1];
    for (rows, cols) in spans.iter().flatten() {
        for r in rows.clone() {
            for c in cols.clone() {
                start[r * ntz as usize + c + 1] += 1;
            }
        }
    }
    for k in 1..start.len() {
        start[k] += start[k - 1];
    }
    let mut fill = start.clone();
    let mut ids = vec![0u32; start[n] as usize];
    for (i, s) in spans.iter().enumerate() {
        let Some((rows, cols)) = s else { continue };
        for r in rows.clone() {
            for c in cols.clone() {
                let f = &mut fill[r * ntz as usize + c];
                ids[*f as usize] = i as u32;
                *f += 1;
            }
        }
    }
    (start, ids)
}

/// An install terrain: its files, its table of contents, and the blocks and
/// tiles read so far.
pub struct Install {
    identity: Identity,
    root: PathBuf,
    map: Map,
    path: String,
    file: File,
    toc: Surface5,
    boxes: Vec<[f64; 4]>,
    grid: Grid,
    /// The blocks whose padded box reaches into each tile, row-major over
    /// the tiles: tile k's are `tile_blocks[tile_start[k]..tile_start[k + 1]]`.
    tile_start: Vec<u32>,
    tile_blocks: Vec<u32>,
    blocks: Mutex<Lru<usize, Arc<Prepared>>>,
    tiles: Mutex<Lru<(u32, u32), Arc<Tile>>>,
}

/// The sizes of the surface, road and scene files, joined by `-`.
fn fingerprint(map: &Map) -> String {
    let size = |role: &str| {
        map.files
            .iter()
            .find(|f| f.role == role)
            .map_or_else(|| "0".to_string(), |f| f.size.to_string())
    };
    format!("{}-{}-{}", size("surface"), size("roads"), size("scene"))
}

impl Install {
    /// The terrain whose `entry.lua` declares `id`, its table of contents
    /// read from `cache`, built there first if missing or stale.
    pub fn open(root: &Path, id: &str, cache: &Cache) -> Result<Install, Refusal> {
        let map = install::open(root, id)?;
        let opened = cache.terrain(root, &map).get(&MESH_TOC)?;
        let unreadable = |e: &dyn std::fmt::Display| {
            Refusal(format!(
                "{} cannot be read ({e}); `dcsterrain cache clear --terrain {}` rebuilds it",
                opened.path.display(),
                map.theatre
            ))
        };
        let bytes = opened.payload().map_err(|e| unreadable(&e))?;
        let toc = Surface5::decode(&bytes).map_err(|e| unreadable(&e))?;
        let f = surface_file(&map)?;
        let path = f.path.clone();
        let file = File::open(root.join(&path)).map_err(|e| refusal(&path, e))?;
        // The file can change between the key's check and this open; the
        // offsets are then another file's.
        let size = file.metadata().map_err(|e| refusal(&path, e))?.len();
        if size != toc.file_len() {
            return Err(Refusal(format!(
                "{path} changed while it was being opened ({size} bytes, where its table of \
                 contents was read from {}); open it again",
                toc.file_len()
            )));
        }
        let boxes: Vec<[f64; 4]> = toc.blocks.iter().map(|b| b.world_box()).collect();
        let grid = grid_for(&toc.bounds(), CELL_SIZE, TILE_SIZE);
        let (tile_start, tile_blocks) = tile_index(&grid, &boxes);
        Ok(Install {
            identity: Identity {
                terrain: map.theatre.clone(),
                dcs_build: map.build.version.clone(),
                fingerprint: fingerprint(&map),
            },
            root: root.to_path_buf(),
            map,
            path,
            file,
            toc,
            boxes,
            grid,
            tile_start,
            tile_blocks,
            blocks: Mutex::new(Lru::new(BLOCK_BUDGET)),
            tiles: Mutex::new(Lru::new(TILE_COUNT)),
        })
    }

    /// Keeps up to `bytes` of prepared blocks rather than the default.
    pub fn with_block_budget(self, bytes: usize) -> Install {
        *self.blocks.lock().unwrap() = Lru::new(bytes);
        self
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn map(&self) -> &Map {
        &self.map
    }

    /// The blocks whose box, padded, overlaps `[x0, x1] × [z0, z1]`, in
    /// block order, found through the tiles the rectangle touches.
    fn under(&self, x0: f64, x1: f64, z0: f64, z1: f64) -> Vec<usize> {
        let Some((rows, cols)) = tile_span(&self.grid, [x0, z0, x1, z1]) else {
            return Vec::new();
        };
        let ntz = self.grid.tiles().1 as usize;
        let mut ids: Vec<usize> = Vec::new();
        for r in rows {
            for c in cols.clone() {
                let k = r * ntz + c;
                let (a, b) = (self.tile_start[k] as usize, self.tile_start[k + 1] as usize);
                ids.extend(self.tile_blocks[a..b].iter().map(|&i| i as usize));
            }
        }
        ids.sort_unstable();
        ids.dedup();
        let p = BOX_PAD_M;
        ids.retain(|&i| {
            let [bx0, bz0, bx1, bz1] = self.boxes[i];
            bx0 - p <= x1 && bx1 + p >= x0 && bz0 - p <= z1 && bz1 + p >= z0
        });
        ids
    }

    /// Block `i`, prepared, read on first use.
    fn block(&self, i: usize) -> Result<Arc<Prepared>, Refusal> {
        if let Some(b) = self.blocks.lock().unwrap().get(&i) {
            return Ok(b);
        }
        // Read outside the lock, so other threads go on answering from the
        // blocks already held.
        let b = self
            .toc
            .read_block(&self.file, i, &[RUNWAY_MATERIAL])
            .map_err(|e| refusal(&self.path, e))?;
        let b = Arc::new(Prepared::new(b, &self.grid));
        self.blocks.lock().unwrap().put(i, b.clone(), b.bytes());
        Ok(b)
    }

    /// Every layer of tile `(tx, tz)`, rasterized on first use.
    pub fn ground_tile(&self, tx: u32, tz: u32) -> Result<Arc<Tile>, Refusal> {
        if let Some(t) = self.tiles.lock().unwrap().get(&(tx, tz)) {
            return Ok(t);
        }
        let g = &self.grid;
        let (ntx, ntz) = g.tiles();
        let mut under = Vec::new();
        if tx < ntx && tz < ntz {
            let ts = g.tile_size;
            let last = |k: u32, n: u32| ((k + 1) * ts).min(n) - 1;
            let (x0, z0) = g.centre(tx * ts, tz * ts);
            let (x1, z1) = g.centre(last(tx, g.height), last(tz, g.width));
            for i in self.under(x0, x1, z0, z1) {
                under.push(self.block(i)?);
            }
        }
        let refs: Vec<&Prepared> = under.iter().map(|b| b.as_ref()).collect();
        let t = Arc::new(ground::tile(&refs, g, tx, tz));
        self.tiles.lock().unwrap().put((tx, tz), t.clone(), 1);
        Ok(t)
    }
}

impl Source for Install {
    fn identity(&self) -> &Identity {
        &self.identity
    }

    fn grid(&self) -> Grid {
        self.grid
    }

    fn tile(&self, layer: Layer, tx: u32, tz: u32) -> Result<Vec<i32>, Refusal> {
        Ok(self.ground_tile(tx, tz)?.layer(layer))
    }

    fn point(&self, x: f64, z: f64) -> Result<Option<Ground>, Refusal> {
        if !x.is_finite() || !z.is_finite() {
            return Ok(None);
        }
        let p = grid_units(&self.grid, x, z);
        let blocks = self
            .under(x, x, z, z)
            .into_iter()
            .map(|i| self.block(i))
            .collect::<Result<Vec<_>, _>>()?;
        let best = ground::point(blocks.iter().map(|b| b.as_ref()), p);
        Ok(best.map(|g| Ground {
            height: g.h,
            water: water_class(g.tritype),
            surface: surface_of(g.tritype, g.runway),
            cover: cover_of(g.tritype),
        }))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::decode::container;
    use crate::decode::surface5::build::{self, f32s, file, tri, u32s, vec3};
    use crate::decode::surface5::{LAKE, ROAD_STRIP};
    use crate::grid::Window;
    use crate::scratch::Scratch;
    use crate::source::Cover;

    fn put(root: &Path, rel: &str, bytes: &[u8]) {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, bytes).unwrap();
    }

    /// A kilometer square at (1000, 2000) in two triangles: the south-west
    /// half forest rising from 100 m to 200 m northward, the north-east half
    /// a lake 50 m deep over a 50 m bed at its far corner. A runway subset
    /// covers a triangle near the south-west corner.
    fn square(tritype: [u32; 2]) -> build::Block {
        build::Block {
            t: [1000.0, 0.0, 2000.0],
            bbox: [0.0, 0.0, 0.0, 1000.0, 300.0, 1000.0],
            arrays: vec![
                u32s("TRITYPE", &tritype),
                tri(&[[0, 1, 2], [1, 3, 2]], Some(("Surface5.0", "Land"))),
                u32s("baseIndex", &[4, 5, 6]),
                tri(&[[0, 1, 2]], Some(("Runway5.0", "Runway"))),
                vec3(
                    "Pbase",
                    &[
                        [0.0, 100.0, 0.0],
                        [1000.0, 200.0, 0.0],
                        [0.0, 100.0, 1000.0],
                        [1000.0, 50.0, 1000.0],
                        [100.0, 110.0, 100.0],
                        [300.0, 130.0, 100.0],
                        [100.0, 110.0, 300.0],
                    ],
                ),
                f32s("depth", &[0.0, 0.0, 0.0, 50.0, 0.0, 0.0, 0.0]),
            ],
        }
    }

    /// An install holding one terrain, `Test`, whose surface file is `surface`.
    fn install(surface: &[u8]) -> Scratch {
        let s = Scratch::new();
        let m = "Mods/terrains/Test";
        put(
            &s.0,
            "autoupdate.cfg",
            br#"{"version": "1.2.3.4", "timestamp": "20000101-000000"}"#,
        );
        put(
            &s.0,
            &format!("{m}/entry.lua"),
            b"theatre = { ['id'] = \"Test\" }\n",
        );
        put(&s.0, &format!("{m}/Surface/Test.surface5"), surface);
        let rn4 = container::build("landscape4::lRoadNetwork", 5, 0);
        put(&s.0, &format!("{m}/roads/Test.rn4"), &rn4);
        let scn5 = container::build("landscape5::Scene5File", 9, 0);
        put(&s.0, &format!("{m}/Scenes/Test.scn5"), &scn5);
        s
    }

    #[test]
    fn a_built_block_answers_tiles_and_points_through_the_cache() {
        let root = install(&file(&[square([1024, LAKE])], 1));
        let c = Scratch::new();
        let cache = Cache::new(&c.0);
        let t = Install::open(&root.0, "Test", &cache).unwrap();
        let toc = c.0.join("Test").join("1.2.3.4").join("mesh.toc");
        assert!(toc.is_file());
        assert_eq!(t.identity().terrain, "Test");
        let g = t.grid();
        assert_eq!(
            (g.origin_x, g.origin_z, g.height, g.width),
            (1000.0, 2000.0, 20, 20)
        );
        assert_eq!(g.tile_size, TILE_SIZE);

        // (1025, 2025): 102.5 m rounds up, forest.
        let at = |layer| t.tile(layer, 0, 0).unwrap()[0];
        assert_eq!(at(Layer::Height), 103);
        assert_eq!(at(Layer::Cover), Cover::Forest.code());
        let p = t.point(1200.0, 2200.0).unwrap().unwrap();
        assert_eq!((p.surface, p.cover, p.water), (5, Cover::Forest, 0));
        assert!((p.height - 120.0).abs() < 1e-9, "{}", p.height);
        let lake = t.point(1990.0, 2990.0).unwrap().unwrap();
        assert_eq!((lake.water, lake.surface, lake.cover), (1, 3, Cover::Water));
        assert_eq!(t.point(999.0, 2500.0).unwrap(), None);
        assert_eq!(t.point(f64::NAN, 2500.0).unwrap(), None);

        // Every cell is the point at its center.
        for layer in Layer::ALL {
            let w = Window::read(&t, layer, 0, 0, 20, 20).unwrap();
            for r in 0..20 {
                for c in 0..20 {
                    let (x, z) = g.centre(r, c);
                    let p = t.point(x, z).unwrap().unwrap();
                    let want = match layer {
                        Layer::Height => (p.height as f32 + 0.5).floor() as i32,
                        Layer::Water => p.water.into(),
                        Layer::Surface => p.surface.into(),
                        Layer::Cover => p.cover.code(),
                    };
                    assert_eq!(w.get(r.into(), c.into()), Some(want), "({r}, {c})");
                }
            }
        }

        // A second open reads the entry rather than building it again.
        let map = install::open(&root.0, "Test").unwrap();
        let again = cache.terrain(&root.0, &map).get(&MESH_TOC).unwrap();
        assert!(!again.built);
    }

    #[test]
    fn a_changed_surface_file_rebuilds_the_table_of_contents() {
        let root = install(&file(&[square([0, 0])], 1));
        let c = Scratch::new();
        let cache = Cache::new(&c.0);
        let t = Install::open(&root.0, "Test", &cache).unwrap();
        assert_eq!(t.point(1100.0, 2100.0).unwrap().unwrap().surface, 5);
        // Two blocks now, the second a road strip 50 m lower.
        let mut low = square([ROAD_STRIP, ROAD_STRIP]);
        low.t[1] = -50.0;
        let f = file(&[square([0, 0]), low], 2);
        put(&root.0, "Mods/terrains/Test/Surface/Test.surface5", &f);
        let t = Install::open(&root.0, "Test", &cache).unwrap();
        let p = t.point(1025.0, 2600.0).unwrap().unwrap();
        assert_eq!(p.surface, 4);
        assert!((p.height - 52.5).abs() < 1e-9, "{}", p.height);
    }

    #[test]
    fn the_tile_index_lists_every_block_a_brute_search_finds() {
        let g = Grid {
            cell_size: 50,
            origin_x: -1000.0,
            origin_z: 500.0,
            height: 300,
            width: 200,
            tile_size: 64,
        };
        // A block within one tile, one across many, one on a tile edge, and
        // one past the grid.
        let boxes = [
            [0.0, 1000.0, 100.0, 1100.0],
            [-1000.0, 500.0, 9000.0, 9000.0],
            [2200.0, 3700.0, 2300.0, 3800.0],
            [20000.0, 20000.0, 21000.0, 21000.0],
        ];
        let (start, ids) = tile_index(&g, &boxes);
        let brute = |x0: f64, x1: f64, z0: f64, z1: f64| -> Vec<usize> {
            let p = BOX_PAD_M;
            (0..boxes.len())
                .filter(|&i| {
                    let [bx0, bz0, bx1, bz1] = boxes[i];
                    bx0 - p <= x1 && bx1 + p >= x0 && bz0 - p <= z1 && bz1 + p >= z0
                })
                .collect()
        };
        let (_ntx, ntz) = g.tiles();
        let indexed = |x0: f64, x1: f64, z0: f64, z1: f64| -> Vec<usize> {
            let Some((rows, cols)) = tile_span(&g, [x0, z0, x1, z1]) else {
                return Vec::new();
            };
            let mut v: Vec<usize> = Vec::new();
            for r in rows {
                for c in cols.clone() {
                    let k = r * ntz as usize + c;
                    v.extend(
                        ids[start[k] as usize..start[k + 1] as usize]
                            .iter()
                            .map(|&i| i as usize),
                    );
                }
            }
            v.sort_unstable();
            v.dedup();
            v.retain(|&i| brute(x0, x1, z0, z1).contains(&i));
            v
        };
        for k in 0..400 {
            let x = -1100.0 + f64::from(k) * 41.3;
            let z = 400.0 + f64::from(k * 13 % 400) * 26.1;
            assert_eq!(indexed(x, x, z, z), brute(x, x, z, z), "({x}, {z})");
            assert_eq!(
                indexed(x, x + 3000.0, z, z + 500.0),
                brute(x, x + 3000.0, z, z + 500.0)
                    .into_iter()
                    .filter(|&i| i != 3)
                    .collect::<Vec<_>>(),
                "({x}, {z}) box"
            );
        }
        // Past the grid nothing is listed, the far block included.
        assert_eq!(
            indexed(20500.0, 20500.0, 20500.0, 20500.0),
            Vec::<usize>::new()
        );
    }

    #[test]
    fn refuses_a_surface_file_no_decoder_reads() {
        let root = install(&container::build("landscape5::Surface5File", 6, 0));
        let c = Scratch::new();
        let Err(e) = Install::open(&root.0, "Test", &Cache::new(&c.0)) else {
            panic!("opened");
        };
        assert!(e.0.contains("version 6"), "{e}");
    }

    #[test]
    fn the_least_recently_used_goes_first_and_the_last_put_stays() {
        let mut l = Lru::new(10);
        l.put(1, 'a', 4);
        l.put(2, 'b', 4);
        assert_eq!(l.get(&1), Some('a'));
        l.put(3, 'c', 4);
        assert_eq!(
            (l.get(&1), l.get(&2), l.get(&3)),
            (Some('a'), None, Some('c'))
        );
        l.put(4, 'd', 50);
        assert_eq!((l.get(&1), l.get(&4)), (None, Some('d')));
        assert_eq!(l.used, 50);
        l.put(4, 'e', 2);
        assert_eq!((l.used, l.get(&4)), (2, Some('e')));
    }
}
