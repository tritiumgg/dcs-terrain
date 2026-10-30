//! A terrain of a DCS install as a `Source`, read in place.
//!
//! Opening one reads its table of contents from the cache's `mesh.toc`,
//! built from the `.surface5` file the first time. A tile or point then reads
//! only the blocks under it, each at its offset, and keeps the prepared
//! blocks and the rasterized tiles in small caches, so a batch of requests
//! over one area reads its blocks once. Its scenery is read the same way, the
//! first time it is asked for. Nothing maps a whole file.

use std::collections::HashMap;
use std::fs::File;
use std::hash::Hash;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use rayon::prelude::*;

use super::{
    CELL_SIZE, EachTile, Ground, Identity, Layer, Object, Piece, Refusal, Source, TILE_SIZE,
};
use crate::cache::{Cache, Entry, Opened, Parts, TerrainCache};
use crate::classes::Class;
use crate::decode::install::{self, Map};
use crate::decode::scn5::Pool;
use crate::decode::surface5::ground::{self, Prepared, Tile, cover_of, grid_units, surface_of};
use crate::decode::surface5::{RUNWAY_MATERIAL, Surface5, grid_for, water_class};
use crate::extract::FileEntry;
use crate::grid::Grid;

/// The bytes of prepared blocks kept between requests.
pub const BLOCK_BUDGET: usize = 32 << 20;
/// Up to this many points on a block not held, `points` tests its
/// triangles directly rather than preparing it.
const FEW_POINTS: usize = 32;
/// Rasterized tiles kept between requests, of every layer: about 20 KiB each.
pub const TILE_COUNT: usize = 256;
/// Tiles rasterized together: a few for each core, and few enough that the
/// blocks under them, all held at once, stay a small part of the terrain.
const BAND: usize = 64;
/// The most the blocks under one band may take to read, every subset
/// included: a band over blocks this large is cut shorter, so a terrain of
/// large blocks holds no more of them at once than one of small blocks.
const BAND_READ_BYTES: u64 = 48 << 20;
/// Fewer blocks to prepare or tiles to rasterize than this are done on the
/// calling thread: handing out one costs more than it saves, and the first
/// hand-out of a process starts the threads.
const PARALLEL_MIN: usize = 2;
/// Fewer blocks than this to read for `points` are read on the calling
/// thread: a block a point or two falls on reads only what they need, in
/// less time than waking the other threads takes.
const POINTS_PARALLEL_MIN: usize = 8;
/// Blocks under a line held at once: one for each core of a desktop.
const ALONG_BLOCKS: usize = 16;

mod airdrome_ids;
mod places;
mod roads;
mod scenery;
mod tables;
pub use places::{PLACES, PlacesEntry};
pub use roads::{ROADS_BETWEENNESS, ROADS_GRAPH, RoadsBetweenness, RoadsGraph};
pub use scenery::{SCENE_NODES, SceneNodes, Scenery};
pub use tables::{
    AirdromeRow, IdBy, MAX_CRS_RESIDUAL_M, SceneIndex, TABLES_JSON, Tables, TablesJson, Tower,
};

/// How far past its stated box a block is read for a point or tile. Its
/// vertices lie inside the box to within a centimeter.
pub(super) const BOX_PAD_M: f64 = 1.0;

/// The `.surface5` table of contents.
pub struct MeshToc;

pub static MESH_TOC: MeshToc = MeshToc;

/// The horizon tiles: a directory of one file per tile, each computed the
/// first time a query touches it.
pub struct HorizonTiles;

pub static HORIZON: HorizonTiles = HorizonTiles;

impl Entry for HorizonTiles {
    fn name(&self) -> &str {
        "horizon"
    }

    fn version(&self) -> u32 {
        1
    }

    /// The heights are the mesh's alone.
    fn roles(&self) -> &[&str] {
        &["surface"]
    }

    fn build(&self, _: &TerrainCache, _: &[FileEntry]) -> Result<Vec<u8>, Refusal> {
        Err(Refusal(
            "the horizon is computed a tile at a time, as a query needs it".into(),
        ))
    }
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

    /// 2 keeps only the runway subsets, the one material a block is read
    /// with.
    fn version(&self) -> u32 {
        2
    }

    fn roles(&self) -> &[&str] {
        &["surface"]
    }

    fn build(&self, cache: &TerrainCache, files: &[FileEntry]) -> Result<Vec<u8>, Refusal> {
        let (root, map) = (cache.install(), cache.map());
        let f = files
            .iter()
            .find(|f| f.role == "surface")
            .ok_or_else(|| Refusal(format!("{} has no surface file", map.theatre)))?;
        let file = File::open(root.join(&f.path)).map_err(|e| refusal(&f.path, e))?;
        let toc = Surface5::open(&file, &f.path).map_err(|e| refusal(&f.path, e))?;
        if toc.blocks.is_empty() {
            return Err(Refusal(format!("{}: the mesh has no blocks", f.path)));
        }
        Ok(toc.keep(&[RUNWAY_MATERIAL]).encode())
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

fn ground_of(g: ground::Ground) -> Ground {
    Ground {
        height: g.h,
        depth: g.depth,
        water: water_class(g.tritype),
        surface: surface_of(g.tritype, g.runway),
        cover: cover_of(g.tritype),
    }
}

/// The tiles of `g` a world rectangle `[x0, z0, x1, z1]` reaches into, as
/// row and column ranges clipped to the grid; `None` when it lies off it.
pub(super) fn tile_span(
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
    // Each tile's start is its fill cursor and ends at the next tile's
    // start; shifting them back by one tile then restores the starts, with
    // no second array the size of the grid.
    let mut ids = vec![0u32; start[n] as usize];
    for (i, s) in spans.iter().enumerate() {
        let Some((rows, cols)) = s else { continue };
        for r in rows.clone() {
            for c in cols.clone() {
                let f = &mut start[r * ntz as usize + c];
                ids[*f as usize] = i as u32;
                *f += 1;
            }
        }
    }
    start.copy_within(0..n, 1);
    start[0] = 0;
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
    cache: TerrainCache,
    /// Opened the first time scenery is asked for, so a scene file no
    /// decoder reads refuses scenery and nothing else. A failure is not
    /// kept: the next request tries again.
    scenery: OnceLock<Scenery>,
    /// Read the first time a table is asked for, likewise.
    tables: OnceLock<Tables>,
    /// `tables.json` as checking the projection opened it, kept for the
    /// first table asked for.
    tables_entry: Mutex<Option<Opened>>,
    /// Read the first time a route or a road distance asks, likewise.
    roads: OnceLock<crate::roads::Graph>,
    betweenness: OnceLock<Vec<f32>>,
    places: OnceLock<crate::places::Places>,
    /// Opened the first time a horizon tile is asked for.
    horizon: OnceLock<Parts>,
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
        let terrain = cache.terrain(root, &map);
        let opened = terrain.get(&MESH_TOC)?;
        let unreadable = |e: &dyn std::fmt::Display| {
            Refusal(format!(
                "{} cannot be read ({e}); `dcsterrain cache clear --terrain {}` rebuilds it",
                opened.path.display(),
                map.theatre
            ))
        };
        let bytes = opened.payload().map_err(|e| unreadable(&e))?;
        let toc = Surface5::decode(&bytes).map_err(|e| unreadable(&e))?;
        let f = &map.surface;
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
            cache: terrain,
            scenery: OnceLock::new(),
            tables: OnceLock::new(),
            tables_entry: Mutex::new(None),
            roads: OnceLock::new(),
            betweenness: OnceLock::new(),
            places: OnceLock::new(),
            horizon: OnceLock::new(),
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

    /// The terrain's scenery, its index read from the cache, built there
    /// first if missing or stale.
    pub fn scenery(&self) -> Result<&Scenery, Refusal> {
        if let Some(s) = self.scenery.get() {
            return Ok(s);
        }
        // Two threads may both open it; the first to finish is kept.
        let s = Scenery::open(&self.root, &self.cache)?;
        Ok(self.scenery.get_or_init(|| s))
    }

    /// The terrain's tables, read from the cache, built there first if
    /// missing or stale.
    pub fn tables(&self) -> Result<&Tables, Refusal> {
        if let Some(t) = self.tables.get() {
            return Ok(t);
        }
        let opened = self.tables_entry.lock().unwrap().take();
        let t = match opened {
            Some(o) => Tables::read(&self.cache, &o)?,
            None => Tables::open(&self.cache)?,
        };
        Ok(self.tables.get_or_init(|| t))
    }

    /// Refuses a projection fitted with a residual over
    /// `MAX_CRS_RESIDUAL_M` unless `allow` accepts it. Reads the projection
    /// alone, leaving the tables to the first request that asks for one.
    pub fn check_crs(&self, allow: bool) -> Result<(), Refusal> {
        let terrain = &self.identity.terrain;
        if let Some(t) = self.tables.get() {
            return t.check_crs(terrain, allow);
        }
        let opened = self.cache.get(&TABLES_JSON)?;
        tables::check_crs(&Tables::read_crs(&self.cache, &opened)?, terrain, allow)?;
        *self.tables_entry.lock().unwrap() = Some(opened);
        Ok(())
    }

    /// The terrain's road and rail graph, read from the cache, built there
    /// first if missing or stale.
    pub fn roads(&self) -> Result<&crate::roads::Graph, Refusal> {
        if let Some(g) = self.roads.get() {
            return Ok(g);
        }
        let g = roads::graph(&self.cache)?;
        Ok(self.roads.get_or_init(|| g))
    }

    /// Each road and rail edge's betweenness, in the graph's edge order,
    /// read from the cache, built there first if missing or stale: minutes
    /// on the largest terrains.
    pub fn betweenness(&self) -> Result<&[f32], Refusal> {
        if let Some(b) = self.betweenness.get() {
            return Ok(b);
        }
        let b = roads::betweenness(&self.cache, self.roads()?)?;
        Ok(self.betweenness.get_or_init(|| b))
    }

    pub fn map(&self) -> &Map {
        &self.map
    }

    /// The terrain's cache directory.
    pub fn cache(&self) -> &TerrainCache {
        &self.cache
    }

    /// The road graph if a request has read it, without reading it.
    pub fn roads_read(&self) -> Option<&crate::roads::Graph> {
        self.roads.get()
    }

    /// The scenery if a request has opened it, without opening it.
    pub fn scenery_read(&self) -> Option<&Scenery> {
        self.scenery.get()
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

    /// The blocks under the segment from `a` to `b`, in block order, found
    /// a tile's length of it at a time so a long diagonal names only the
    /// blocks near it; none for a segment not finite.
    fn along_blocks(&self, a: [f64; 2], b: [f64; 2]) -> Vec<usize> {
        if !a.iter().chain(&b).all(|v| v.is_finite()) {
            return Vec::new();
        }
        let g = &self.grid;
        let tm = f64::from(g.tile_size) * f64::from(g.cell_size);
        let n = ((b[0] - a[0]).hypot(b[1] - a[1]) / tm).ceil().max(1.0) as usize;
        let mut ids = Vec::new();
        for k in 0..n {
            let (s, e) = (k as f64 / n as f64, (k + 1) as f64 / n as f64);
            let x = [a[0] + s * (b[0] - a[0]), a[0] + e * (b[0] - a[0])];
            let z = [a[1] + s * (b[1] - a[1]), a[1] + e * (b[1] - a[1])];
            ids.extend(self.under(
                x[0].min(x[1]),
                x[0].max(x[1]),
                z[0].min(z[1]),
                z[0].max(z[1]),
            ));
        }
        ids.sort_unstable();
        ids.dedup();
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

    /// The blocks under tile `(tx, tz)`, in block order; none off the grid.
    fn tile_under(&self, tx: u32, tz: u32) -> Vec<usize> {
        let g = &self.grid;
        let (ntx, ntz) = g.tiles();
        if tx >= ntx || tz >= ntz {
            return Vec::new();
        }
        let ts = g.tile_size;
        let last = |k: u32, n: u32| ((k + 1) * ts).min(n) - 1;
        let (x0, z0) = g.centre(tx * ts, tz * ts);
        let (x1, z1) = g.centre(last(tx, g.height), last(tz, g.width));
        self.under(x0, x1, z0, z1)
    }

    /// `tiles` cut into runs, each its first tile's index and its tiles: at
    /// most `BAND` tiles, and no more than `BAND_READ_BYTES` of blocks under
    /// them unless a single tile needs more.
    fn bands<'t>(&self, tiles: &'t [(u32, u32)]) -> Vec<(usize, &'t [(u32, u32)])> {
        let mut out = Vec::new();
        let (mut start, mut bytes) = (0, 0);
        let mut seen: Vec<usize> = Vec::new();
        for (k, &(tx, tz)) in tiles.iter().enumerate() {
            let under = self.tile_under(tx, tz);
            let add: u64 = (under.iter())
                .filter(|i| !seen.contains(i))
                .map(|&i| self.toc.blocks[i].read_len())
                .sum();
            if k > start && (k - start == BAND || bytes + add > BAND_READ_BYTES) {
                out.push((start, &tiles[start..k]));
                (start, bytes) = (k, 0);
                seen.clear();
            }
            for i in under {
                if !seen.contains(&i) {
                    bytes += self.toc.blocks[i].read_len();
                    seen.push(i);
                }
            }
        }
        if start < tiles.len() {
            out.push((start, &tiles[start..]));
        }
        out
    }

    /// Blocks `ids`, distinct, in order: those held as they are, the rest
    /// read and prepared on every core when there are enough of them to pay
    /// for it. Each is prepared once, by one thread.
    fn blocks_of(&self, ids: &[usize]) -> Result<Vec<Arc<Prepared>>, Refusal> {
        let mut out: Vec<Option<Arc<Prepared>>> = {
            let mut held = self.blocks.lock().unwrap();
            ids.iter().map(|i| held.get(i)).collect()
        };
        let missing: Vec<usize> = (0..ids.len()).filter(|&k| out[k].is_none()).collect();
        let made: Vec<Arc<Prepared>> = if missing.len() < PARALLEL_MIN {
            (missing.iter())
                .map(|&k| self.block(ids[k]))
                .collect::<Result<_, _>>()?
        } else {
            // Collected whole and then checked in order, so of two blocks that
            // fail the one refused is the first, however the work fell.
            (missing.par_iter())
                .map(|&k| self.block(ids[k]))
                .collect::<Vec<_>>()
                .into_iter()
                .collect::<Result<_, _>>()?
        };
        for (&k, b) in missing.iter().zip(made) {
            out[k] = Some(b);
        }
        Ok(out.into_iter().flatten().collect())
    }

    /// Every layer of tile `(tx, tz)`, rasterized on first use.
    pub fn ground_tile(&self, tx: u32, tz: u32) -> Result<Arc<Tile>, Refusal> {
        if let Some(t) = self.tiles.lock().unwrap().get(&(tx, tz)) {
            return Ok(t);
        }
        let under = self.blocks_of(&self.tile_under(tx, tz))?;
        let refs: Vec<&Prepared> = under.iter().map(|b| b.as_ref()).collect();
        let t = Arc::new(ground::tile(&refs, &self.grid, tx, tz));
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

    /// A band of tiles at a time: the blocks under the band's missing tiles
    /// are prepared together, then the tiles rasterized together, each on
    /// every core. The band keeps its own blocks and tiles while it works,
    /// so a window wider than either cache reads each block and rasterizes
    /// each tile once, and keeps the blocks the next band shares with it.
    fn tiles(&self, layer: Layer, tiles: &[(u32, u32)], f: &mut EachTile) -> Result<(), Refusal> {
        let mut kept: HashMap<usize, Arc<Prepared>> = HashMap::new();
        for (at, band) in self.bands(tiles) {
            let mut got: Vec<Option<Arc<Tile>>> = {
                let mut held = self.tiles.lock().unwrap();
                band.iter().map(|t| held.get(t)).collect()
            };
            let missing: Vec<usize> = (0..band.len()).filter(|&k| got[k].is_none()).collect();
            if missing.len() < PARALLEL_MIN {
                for &k in &missing {
                    got[k] = Some(self.ground_tile(band[k].0, band[k].1)?);
                }
            } else {
                let under: Vec<Vec<usize>> = (missing.iter())
                    .map(|&k| self.tile_under(band[k].0, band[k].1))
                    .collect();
                let mut need: Vec<usize> = under.iter().flatten().copied().collect();
                need.sort_unstable();
                need.dedup();
                kept.retain(|i, _| need.binary_search(i).is_ok());
                let new: Vec<usize> = need.into_iter().filter(|i| !kept.contains_key(i)).collect();
                kept.extend(new.iter().copied().zip(self.blocks_of(&new)?));
                let g = &self.grid;
                let made: Vec<Arc<Tile>> = (missing.par_iter().zip(&under))
                    .map(|(&k, ids)| {
                        let refs: Vec<&Prepared> = ids.iter().map(|i| kept[i].as_ref()).collect();
                        Arc::new(ground::tile(&refs, g, band[k].0, band[k].1))
                    })
                    .collect();
                let mut held = self.tiles.lock().unwrap();
                for (&k, t) in missing.iter().zip(made) {
                    held.put(band[k], t.clone(), 1);
                    got[k] = Some(t);
                }
            }
            for (k, t) in got.into_iter().flatten().enumerate() {
                f(at + k, &t.layer(layer))?;
            }
        }
        Ok(())
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
        Ok(best.map(ground_of))
    }

    /// Block by block: every point's blocks are found first, then each
    /// block is read once and asked about each point on it, and each point
    /// keeps the best answer by rank. Holding one block at a time is
    /// what keeps a terrain whose blocks under one point outgrow the block
    /// budget from reading each block again for every point.
    fn points(&self, points: &[[f64; 2]]) -> Result<Vec<Option<Ground>>, Refusal> {
        let mut on: HashMap<usize, Vec<u32>> = HashMap::new();
        for (k, &[x, z]) in points.iter().enumerate() {
            if x.is_finite() && z.is_finite() {
                for i in self.under(x, x, z, z) {
                    on.entry(i).or_default().push(k as u32);
                }
            }
        }
        let mut order: Vec<usize> = on.keys().copied().collect();
        order.sort_unstable();
        let held: Vec<Option<Arc<Prepared>>> = {
            let mut held = self.blocks.lock().unwrap();
            order.iter().map(|i| held.get(i)).collect()
        };
        let answer = |(&i, held): (&usize, &Option<Arc<Prepared>>)| {
            let ks = &on[&i];
            let ps: Vec<[f64; 2]> = (ks.iter())
                .map(|&k| grid_units(&self.grid, points[k as usize][0], points[k as usize][1]))
                .collect();
            Ok(match held {
                // A block few points fall on is answered without binning
                // it, and not kept: preparing it would cost more than the
                // points save.
                None if ks.len() <= FEW_POINTS => {
                    let raw = (self.toc)
                        .read_block(&self.file, i, &[RUNWAY_MATERIAL])
                        .map_err(|e| refusal(&self.path, e))?;
                    ground::few(&raw, &self.grid, &ps)
                }
                held => {
                    let b = match held {
                        Some(b) => b.clone(),
                        None => self.block(i)?,
                    };
                    ps.iter().map(|&p| (b.point(p), b.on_runway(p))).collect()
                }
            })
        };
        // Each block is read and asked on whichever core is free, and the
        // answers merged in block order, so they are the same however the
        // work falls.
        let unheld = held.iter().filter(|b| b.is_none()).count();
        let answers: Vec<Vec<(Option<ground::Ground>, bool)>> = if unheld < POINTS_PARALLEL_MIN {
            (order.iter().zip(&held))
                .map(answer)
                .collect::<Result<_, Refusal>>()?
        } else {
            (order.par_iter().zip(&held))
                .map(answer)
                .collect::<Vec<_>>()
                .into_iter()
                .collect::<Result<_, Refusal>>()?
        };
        let mut best: Vec<Option<ground::Ground>> = vec![None; points.len()];
        let mut runway = vec![false; points.len()];
        for (i, answers) in order.iter().zip(answers) {
            let ks = &on[i];
            for (&k, (g, r)) in ks.iter().zip(answers) {
                let k = k as usize;
                best[k] = ground::merge(best[k], g);
                runway[k] |= r;
            }
        }
        Ok(best
            .into_iter()
            .zip(runway)
            .map(|(g, r)| {
                g.map(|mut g| {
                    g.runway = r;
                    ground_of(g)
                })
            })
            .collect())
    }

    /// The blocks under the segment are found a tile's length of it at a
    /// time, so a long diagonal reads only the blocks near it, and each is
    /// asked only about the bins the segment crosses.
    fn along(&self, a: [f64; 2], b: [f64; 2]) -> Result<Vec<Piece>, Refusal> {
        let mut out = Vec::new();
        let ids = self.along_blocks(a, b);
        let g = &self.grid;
        let (ga, gb) = (grid_units(g, a[0], a[1]), grid_units(g, b[0], b[1]));
        // The blocks a request has not read yet are prepared together, a
        // few at a time so a long line holds few at once; the pieces then
        // follow in block order.
        for ids in ids.chunks(ALONG_BLOCKS) {
            for b in self.blocks_of(ids)? {
                b.along(ga, gb, &mut out);
            }
        }
        Ok(out)
    }

    /// Block by block, as `points` answers: each block under any of the
    /// segments is prepared once and asked about every segment over it, so
    /// blocks that together outgrow the block budget are not read again
    /// for each segment.
    fn alongs(&self, segments: &[[[f64; 2]; 2]]) -> Result<Vec<Vec<Piece>>, Refusal> {
        let mut on: HashMap<usize, Vec<u32>> = HashMap::new();
        for (k, &[a, b]) in segments.iter().enumerate() {
            for i in self.along_blocks(a, b) {
                on.entry(i).or_default().push(k as u32);
            }
        }
        let mut order: Vec<usize> = on.keys().copied().collect();
        order.sort_unstable();
        let g = &self.grid;
        let answer = |i: &usize| -> Result<Vec<Vec<Piece>>, Refusal> {
            let b = self.block(*i)?;
            Ok((on[i].iter())
                .map(|&k| {
                    let [a, e] = segments[k as usize];
                    let mut out = Vec::new();
                    b.along(
                        grid_units(g, a[0], a[1]),
                        grid_units(g, e[0], e[1]),
                        &mut out,
                    );
                    out
                })
                .collect())
        };
        // Each block on whichever core is free, the pieces then gathered in
        // block order, so they are the same however the work falls.
        let answers: Vec<Vec<Vec<Piece>>> = if order.len() < PARALLEL_MIN {
            order.iter().map(answer).collect::<Result<_, Refusal>>()?
        } else {
            (order.par_iter())
                .map(answer)
                .collect::<Vec<_>>()
                .into_iter()
                .collect::<Result<_, Refusal>>()?
        };
        let mut out: Vec<Vec<Piece>> = (0..segments.len()).map(|_| Vec::new()).collect();
        for (i, answers) in order.iter().zip(answers) {
            for (&k, pieces) in on[i].iter().zip(answers) {
                out[k as usize].extend(pieces);
            }
        }
        Ok(out)
    }

    fn runway_triangles(&self, b: [f64; 4]) -> Result<Vec<[[f64; 2]; 3]>, Refusal> {
        Ok(self.runway_triangles_each(&[b])?.pop().expect("one box"))
    }

    /// Block by block, as `points` answers: each block under any of the
    /// boxes is prepared once and asked about every box over it, so
    /// scattered boxes do not read the same airfield's blocks again as the
    /// block budget turns them over.
    fn runway_triangles_each(
        &self,
        boxes: &[[f64; 4]],
    ) -> Result<Vec<Vec<[[f64; 2]; 3]>>, Refusal> {
        let g = &self.grid;
        let mut on: HashMap<usize, Vec<u32>> = HashMap::new();
        for (k, b) in boxes.iter().enumerate() {
            // The table of contents keeps only the runway subsets, so a
            // block it lists none for is never read.
            for i in self.under(b[0], b[2], b[1], b[3]) {
                if self.toc.blocks[i].has_subsets() {
                    on.entry(i).or_default().push(k as u32);
                }
            }
        }
        let mut order: Vec<usize> = on.keys().copied().collect();
        order.sort_unstable();
        let mut out = vec![Vec::new(); boxes.len()];
        for i in order {
            let p = self.block(i)?;
            for &k in &on[&i] {
                let b = boxes[k as usize];
                let (lo, hi) = (grid_units(g, b[0], b[1]), grid_units(g, b[2], b[3]));
                p.runway_within([lo[0], lo[1], hi[0], hi[1]], &mut out[k as usize]);
            }
        }
        let cs = f64::from(g.cell_size);
        let world = |[r, c]: [f64; 2]| [(r + 0.5) * cs + g.origin_x, (c + 0.5) * cs + g.origin_z];
        Ok(out
            .into_iter()
            .map(|ts| ts.into_iter().map(|t| t.map(world)).collect())
            .collect())
    }

    fn tables(&self) -> Result<&Tables, Refusal> {
        Install::tables(self)
    }

    fn objects_in(
        &self,
        pool: Pool,
        b: [f64; 4],
        f: &mut dyn FnMut(&Object),
    ) -> Result<(), Refusal> {
        let s = self.scenery()?;
        let (classes, models) = (s.classes(), &s.index().models);
        s.each_in(pool, b, |i| {
            let m = &models[i.model as usize];
            f(&Object {
                id: i.id,
                model: &m.name,
                class: classes.of(i.model).class,
                family: classes.of(i.model).family,
                pos: i.pos,
                rotation: i.rotation,
                bbox: m.bbox,
            })
        })
    }

    /// Each tile from the cache, the missing ones computed together and
    /// kept there.
    fn horizon(&self, tiles: &[(u32, u32)]) -> Result<Vec<Vec<u8>>, Refusal> {
        let parts = match self.horizon.get() {
            Some(p) => p,
            None => {
                let p = self.cache.parts(&HORIZON)?;
                self.horizon.get_or_init(|| p)
            }
        };
        let name = |(tx, tz): (u32, u32)| format!("{tx}-{tz}");
        let len = crate::horizon::tile_len(&self.grid);
        let mut out = Vec::with_capacity(tiles.len());
        let mut missing = Vec::new();
        for (i, &t) in tiles.iter().enumerate() {
            match parts.get(&name(t))?.filter(|p| p.len() == len) {
                Some(p) => out.push(p),
                None => {
                    out.push(Vec::new());
                    missing.push(i);
                }
            }
        }
        let want: Vec<(u32, u32)> = missing.iter().map(|&i| tiles[i]).collect();
        if !want.is_empty() {
            parts.prepare()?;
        }
        for (i, p) in missing
            .into_iter()
            .zip(crate::horizon::compute(self, &want)?)
        {
            parts.put(&name(tiles[i]), &p)?;
            out[i] = p;
        }
        Ok(out)
    }

    fn road_graph(&self) -> Result<&crate::roads::Graph, Refusal> {
        self.roads()
    }

    /// Read when its cache entry needs no build: a query never waits the
    /// minutes betweenness takes on the largest terrains, which `index`
    /// spends instead.
    fn road_betweenness(&self) -> Result<Option<&[f32]>, Refusal> {
        if self.betweenness.get().is_none() && !self.cache.ready(&roads::ROADS_BETWEENNESS)? {
            return Ok(None);
        }
        self.betweenness().map(Some)
    }

    /// Read when its cache entry needs no build, as betweenness is: the
    /// walk of every object takes up to a minute, which `index` spends.
    fn places(&self) -> Result<Option<&crate::places::Places>, Refusal> {
        if let Some(p) = self.places.get() {
            return Ok(Some(p));
        }
        if !self.cache.ready(&PLACES)? {
            return Ok(None);
        }
        let p = places::read(&self.cache)?;
        Ok(Some(self.places.get_or_init(|| p)))
    }

    fn structures_in(
        &self,
        b: [f64; 4],
        f: &mut dyn FnMut(f64, f64, Class),
    ) -> Result<(), Refusal> {
        self.scenery()?
            .each_structure_in(Pool::Objects, b, |i, a| f(i.pos[0], i.pos[2], a.class))
    }

    fn instances_in(
        &self,
        pool: Pool,
        b: [f64; 4],
        f: &mut dyn FnMut(f64, f64),
    ) -> Result<(), Refusal> {
        self.scenery()?.each_in(pool, b, |i| f(i.pos[0], i.pos[2]))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::classes::Class;
    use crate::decode::scn5::{self, Pool};
    use crate::decode::surface5::build::{self, f32s, file, tri, u32s, vec3};
    use crate::decode::surface5::{LAKE, ROAD_STRIP};
    use crate::decode::{container, rn4};
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
        let toc = c.0.join("Test").join("mesh.toc");
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
    fn points_block_by_block_are_each_point_alone_whatever_the_budget() {
        // Overlapping blocks, the second a road strip lower down that wins
        // where it runs, and a third off to the north-east alone.
        let mut low = square([ROAD_STRIP, ROAD_STRIP]);
        low.t[1] = -50.0;
        let mut far = square([1024, 1024]);
        far.t = [3000.0, 20.0, 4000.0];
        let root = install(&file(&[square([1024, LAKE]), low, far], 3));
        let c = Scratch::new();
        let t = Install::open(&root.0, "Test", &Cache::new(&c.0))
            .unwrap()
            .with_block_budget(1);
        let mut pts = vec![[f64::NAN, 2500.0], [999.0, 2500.0]];
        for k in 0..400 {
            let f = f64::from(k);
            pts.push([1000.0 + (f * 37.0) % 3000.0, 2000.0 + (f * 53.0) % 3000.0]);
        }
        // Few points to a block take the direct path, many the prepared one.
        let few = t.points(&pts[..12]).unwrap();
        let got = t.points(&pts).unwrap();
        assert_eq!(few, got[..12]);
        assert_eq!(got.len(), pts.len());
        assert!(got.iter().filter(|g| g.is_some()).count() > 50);
        for (p, g) in pts.iter().zip(&got) {
            assert_eq!(*g, t.point(p[0], p[1]).unwrap(), "{p:?}");
        }
        assert!(got.iter().any(|g| g.is_some_and(|g| g.surface == 5)));
        assert!(got.iter().any(|g| g.is_some_and(|g| g.surface == 4)));
    }

    #[test]
    fn alongs_block_by_block_are_each_segment_alone_whatever_the_budget() {
        let mut low = square([ROAD_STRIP, ROAD_STRIP]);
        low.t[1] = -50.0;
        let mut far = square([1024, 1024]);
        far.t = [3000.0, 20.0, 4000.0];
        let root = install(&file(&[square([1024, LAKE]), low, far], 3));
        let c = Scratch::new();
        let t = Install::open(&root.0, "Test", &Cache::new(&c.0))
            .unwrap()
            .with_block_budget(1);
        // A fan of segments from one point, as a radial sweep asks, and one
        // not finite.
        let mut segs = vec![[[f64::NAN, 2500.0], [999.0, 2500.0]]];
        for k in 0..40 {
            let b = f64::from(k) * std::f64::consts::TAU / 40.0;
            segs.push([
                [2000.0, 2500.0],
                [2000.0 + 2500.0 * b.cos(), 2500.0 + 2500.0 * b.sin()],
            ]);
        }
        let got = t.alongs(&segs).unwrap();
        assert_eq!(got.len(), segs.len());
        assert!(got[0].is_empty());
        let key = |p: &Piece| (p.t[0].to_bits(), p.t[1].to_bits(), p.rank);
        for (s, g) in segs.iter().zip(&got) {
            let mut want = t.along(s[0], s[1]).unwrap();
            let mut g = g.clone();
            want.sort_by_key(key);
            g.sort_by_key(key);
            assert_eq!(g, want, "{s:?}");
        }
        assert!(got.iter().filter(|g| g.len() > 1).count() > 20);
    }

    /// `square` stretched `s` times about its corner and moved to `t`.
    fn stretched(t: [f32; 3], s: f32, tritype: [u32; 2]) -> build::Block {
        let mut b = square(tritype);
        b.t = t;
        for v in &mut b.bbox {
            *v *= s;
        }
        let p = vec3(
            "Pbase",
            &[
                [0.0, 100.0, 0.0],
                [1000.0 * s, 200.0, 0.0],
                [0.0, 100.0, 1000.0 * s],
                [1000.0 * s, 50.0, 1000.0 * s],
                [100.0 * s, 110.0, 100.0 * s],
                [300.0 * s, 130.0, 100.0 * s],
                [100.0 * s, 110.0, 300.0 * s],
            ],
        );
        b.arrays[4] = p;
        b
    }

    /// An install over more tiles than a band, under overlapping blocks of
    /// several kinds, some heights a road strip below the rest.
    fn wide() -> Scratch {
        let mut blocks = Vec::new();
        for i in 0..6 {
            for j in 0..6 {
                let kind = [[1024, LAKE], [0, 0], [ROAD_STRIP, ROAD_STRIP]][(i + 2 * j) % 3];
                let t = [4500.0 * i as f32, 10.0 * j as f32, 4500.0 * j as f32];
                blocks.push(stretched(t, 5.0, kind));
            }
        }
        let n = blocks.len() as u32;
        install(&file(&blocks, n))
    }

    #[test]
    fn reads_together_answer_as_reads_one_at_a_time() {
        let root = wide();
        let c = Scratch::new();
        let cache = Cache::new(&c.0);
        // Each answer from its own terrain, so no cache carries one to the
        // other, and a budget of one block, so a band holds what the cache
        // cannot.
        let open = || {
            Install::open(&root.0, "Test", &cache)
                .unwrap()
                .with_block_budget(1)
        };
        let (one, all) = (open(), open());
        let g = one.grid();
        let (ntx, ntz) = g.tiles();
        assert!((ntx * ntz) as usize > BAND, "{ntx} × {ntz} tiles");
        // Every tile, and one past the grid, in the order a window reads.
        let mut want: Vec<(u32, u32)> = (0..ntx)
            .flat_map(|x| (0..ntz).map(move |z| (x, z)))
            .collect();
        want.push((ntx, 0));
        for layer in Layer::ALL {
            let mut seen = 0;
            all.tiles(layer, &want, &mut |i, t| {
                assert_eq!(i, seen);
                seen += 1;
                let (tx, tz) = want[i];
                assert_eq!(
                    t,
                    one.tile(layer, tx, tz).unwrap(),
                    "{layer:?} ({tx}, {tz})"
                );
                Ok(())
            })
            .unwrap();
            assert_eq!(seen, want.len());
        }
        let w = Window::read(&open(), Layer::Height, -3, 5, g.height + 6, g.width - 9).unwrap();
        for r in 0..i64::from(w.rows) {
            for c in 0..i64::from(w.cols) {
                let (row, col) = (w.row0 + r, w.col0 + c);
                let cell = (row >= 0 && row < i64::from(g.height)).then(|| {
                    let (tx, tz, k) = g.tile_of(row as u32, col as u32);
                    one.tile(Layer::Height, tx, tz).unwrap()[k]
                });
                assert_eq!(
                    w.get(row, col),
                    Some(cell.unwrap_or(-32768)),
                    "({row}, {col})"
                );
            }
        }

        // Points across every block, through the parallel path.
        let mut pts = Vec::new();
        for k in 0..3000 {
            let f = f64::from(k);
            pts.push([(f * 37.3) % 28_000.0, (f * 53.9) % 28_000.0]);
        }
        let got = open().points(&pts).unwrap();
        assert!(got.iter().filter(|p| p.is_some()).count() > 2000);
        for (p, g) in pts.iter().zip(&got) {
            assert_eq!(*g, one.point(p[0], p[1]).unwrap(), "{p:?}");
        }

        // A long line, its pieces in block order as one block at a time
        // gives them.
        let (a, b) = ([100.0, 27_000.0], [26_500.0, 300.0]);
        let pieces = open().along(a, b).unwrap();
        let mut ids = one.under(
            a[0].min(b[0]),
            a[0].max(b[0]),
            a[1].min(b[1]),
            a[1].max(b[1]),
        );
        ids.retain(|&i| {
            let mut v = Vec::new();
            let (ga, gb) = (grid_units(&g, a[0], a[1]), grid_units(&g, b[0], b[1]));
            one.block(i).unwrap().along(ga, gb, &mut v);
            !v.is_empty()
        });
        let mut serial = Vec::new();
        let (ga, gb) = (grid_units(&g, a[0], a[1]), grid_units(&g, b[0], b[1]));
        for i in ids {
            one.block(i).unwrap().along(ga, gb, &mut serial);
        }
        assert!(serial.len() > 10, "{}", serial.len());
        assert_eq!(pieces, serial);
    }

    #[test]
    fn a_horizon_tile_is_computed_once_and_kept_across_an_update() {
        let root = install(&file(&[square([1024, LAKE])], 1));
        let c = Scratch::new();
        let cache = Cache::new(&c.0);
        let t = Install::open(&root.0, "Test", &cache).unwrap();
        let made = t.horizon(&[(0, 0)]).unwrap();
        assert_eq!(made, crate::horizon::compute(&t, &[(0, 0)]).unwrap());
        // Some horizon cells see the slope; most lie off the mesh.
        assert!(made[0].iter().any(|&b| b != crate::horizon::UNDEFINED));
        let path = c.0.join("Test").join("horizon").join("0-0");
        let stored = cache.stored("Test").unwrap();
        assert!(stored.entries.iter().any(|(n, s)| n == "horizon" && *s > 0));

        // What is read is what was kept: mark its last byte and open again.
        let mut b = fs::read(&path).unwrap();
        *b.last_mut().unwrap() ^= 1;
        fs::write(&path, &b).unwrap();
        let mut marked = made[0].clone();
        *marked.last_mut().unwrap() ^= 1;
        let t = Install::open(&root.0, "Test", &cache).unwrap();
        assert_eq!(t.horizon(&[(0, 0)]).unwrap(), [marked.clone()]);

        // An update that leaves the surface file alone re-stamps the tile.
        put(
            &root.0,
            "autoupdate.cfg",
            br#"{"version": "1.2.3.5", "timestamp": "20000102-000000"}"#,
        );
        let t = Install::open(&root.0, "Test", &cache).unwrap();
        assert_eq!(t.horizon(&[(0, 0)]).unwrap(), [marked.clone()]);
        let restamped = String::from_utf8_lossy(&fs::read(&path).unwrap()).into_owned();
        assert!(restamped.contains("1.2.3.5"), "{restamped}");

        // A changed surface file has it computed again, the mark gone.
        let mut low = square([0, 0]);
        low.t[1] = -50.0;
        let f = file(&[square([0, 0]), low], 2);
        put(&root.0, "Mods/terrains/Test/Surface/Test.surface5", &f);
        let t = Install::open(&root.0, "Test", &cache).unwrap();
        let again = t.horizon(&[(0, 0)]).unwrap();
        assert_eq!(again, crate::horizon::compute(&t, &[(0, 0)]).unwrap());
        assert_ne!(again, [marked]);

        cache.clear(Some("Test"), true).unwrap();
        assert!(!path.exists());
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

    /// A scene over the square: a house, a tree and a pole, each in its own
    /// pool, and a house placed as a group record.
    fn scene(version: u32) -> Vec<u8> {
        scn5::build::Scene {
            version,
            models: vec![
                ("house", [-5.0, 0.0, -5.0, 5.0, 8.0, 5.0]),
                ("oak", [0.0; 6]),
                ("pole", [-0.5, 0.0, -0.5, 0.5, 9.0, 0.5]),
            ],
            nodes: vec![scn5::build::Node {
                t: [1500.0, 0.0, 2500.0],
                p: vec![[0.0, 100.0, 0.0], [100.0, 100.0, 0.0], [0.0, 100.0, 100.0]],
                model: vec![0, 1, 2],
                axis: vec![[1.0, 0.0]; 3],
            }],
            group: vec![scn5::build::Record {
                model: 0,
                pos: [1800.0, 100.0, 2800.0],
                heading: 0.0,
            }],
            pools: [vec![(0, 0)], vec![(0, 2)], vec![(0, 1)]],
            shift: None,
        }
        .file()
    }

    fn ids(t: &Install, pool: Pool, b: [f64; 4]) -> Vec<u64> {
        let mut out = Vec::new();
        t.scenery()
            .unwrap()
            .each_in(pool, b, |i| out.push(i.id))
            .unwrap();
        out
    }

    #[test]
    fn scenery_answers_a_box_through_the_cache() {
        for version in [8, 9] {
            let root = install(&file(&[square([0, 0])], 1));
            put(
                &root.0,
                "Mods/terrains/Test/Scenes/Test.scn5",
                &scene(version),
            );
            let c = Scratch::new();
            let cache = Cache::new(&c.0);
            let t = Install::open(&root.0, "Test", &cache).unwrap();
            let nodes = c.0.join("Test").join("scene.nodes");
            assert!(!nodes.exists(), "built before scenery was asked for");

            let near = |x: f64, z: f64| [x - 10.0, z - 10.0, x + 10.0, z + 10.0];
            assert_eq!(ids(&t, Pool::Objects, near(1500.0, 2500.0)), [0]);
            assert_eq!(ids(&t, Pool::Trees, near(1600.0, 2500.0)), [1]);
            assert_eq!(ids(&t, Pool::Lines, near(1500.0, 2600.0)), [2]);
            assert_eq!(ids(&t, Pool::Objects, near(1800.0, 2800.0)), [1 << 13]);
            assert!(ids(&t, Pool::Trees, near(1500.0, 2500.0)).is_empty());
            assert!(nodes.is_file());
            let s = t.scenery().unwrap();
            assert_eq!(s.index().version, version);
            assert_eq!(s.index().models[1].name, "oak");

            let map = install::open(&root.0, "Test").unwrap();
            let again = cache.terrain(&root.0, &map).get(&SCENE_NODES).unwrap();
            assert!(!again.built);
        }
    }

    #[test]
    fn a_smoke_volume_is_scenery_but_never_a_structure() {
        let built = scn5::build::Scene {
            version: 9,
            models: vec![
                ("home1_a_new", [-4.0, 0.0, -7.5, 4.0, 8.0, 7.5]),
                ("smoke#big", [-30.0, 0.0, -30.0, 30.0, 300.0, 30.0]),
            ],
            nodes: vec![scn5::build::Node {
                t: [1500.0, 0.0, 2500.0],
                p: vec![[0.0, 100.0, 0.0], [40.0, 100.0, 0.0], [80.0, 100.0, 0.0]],
                model: vec![0, 1, 0],
                axis: vec![[1.0, 0.0]; 3],
            }],
            group: vec![scn5::build::Record {
                model: 1,
                pos: [1520.0, 100.0, 2500.0],
                heading: 0.0,
            }],
            // The builder writes no empty pool; the catalog counts objects alone.
            pools: [vec![(0, 0), (0, 1), (0, 2)], vec![(0, 2)], vec![(0, 1)]],
            shift: None,
        };
        let root = install(&file(&[square([0, 0])], 1));
        put(
            &root.0,
            "Mods/terrains/Test/Scenes/Test.scn5",
            &built.file(),
        );
        let c = Scratch::new();
        let t = Install::open(&root.0, "Test", &Cache::new(&c.0)).unwrap();
        let s = t.scenery().unwrap();
        let b = [1400.0, 2400.0, 1700.0, 2600.0];
        let mut all = Vec::new();
        s.each_in(Pool::Objects, b, |i| all.push(i.model)).unwrap();
        all.sort();
        assert_eq!(all, [0, 0, 1, 1]);
        let mut standing = Vec::new();
        s.each_structure_in(Pool::Objects, b, |i, a| {
            standing.push((i.model, a.class));
        })
        .unwrap();
        assert_eq!(standing, [(0, Class::Building), (0, Class::Building)]);
        let mut objects = Vec::new();
        t.objects_in(Pool::Objects, b, &mut |o| {
            objects.push((o.model.to_string(), o.class))
        })
        .unwrap();
        objects.sort();
        assert_eq!(objects.len(), 4);
        assert_eq!(objects[3], ("smoke#big".to_string(), Class::Effect));

        let cat = s.catalog().unwrap();
        let rows: Vec<_> = cat
            .rows
            .iter()
            .map(|r| (r.model.as_str(), r.count, r.assignment.class))
            .collect();
        assert_eq!(
            rows,
            [
                ("home1_a_new", 2, Class::Building),
                ("smoke#big", 2, Class::Effect)
            ]
        );
        assert_eq!(cat.terrain, "Test");
    }

    #[test]
    fn a_scene_file_no_decoder_reads_refuses_scenery_not_heights() {
        let root = install(&file(&[square([0, 0])], 1));
        let bad = container::build("landscape5::Scene5File", 10, 0);
        put(&root.0, "Mods/terrains/Test/Scenes/Test.scn5", &bad);
        let c = Scratch::new();
        let t = Install::open(&root.0, "Test", &Cache::new(&c.0)).unwrap();
        let Err(e) = t.scenery() else {
            panic!("scenery opened");
        };
        assert!(
            e.0.contains("Test.scn5") && e.0.contains("version 10"),
            "{e}"
        );
        assert_eq!(t.point(1100.0, 2100.0).unwrap().unwrap().surface, 5);
        // A refusal is not kept: once the file reads, so does scenery.
        put(&root.0, "Mods/terrains/Test/Scenes/Test.scn5", &scene(9));
        assert_eq!(ids(&t, Pool::Trees, [1590.0, 2490.0, 1610.0, 2510.0]), [1]);
    }

    #[test]
    fn answers_heights_over_a_taxiway_file_no_decoder_reads() {
        let root = install(&file(&[square([0, 0])], 1));
        let bad = container::build("t5::rn5file", 99, 0);
        put(&root.0, "Mods/terrains/Test/AirfieldsTaxiways/X.rn5", &bad);
        let c = Scratch::new();
        let t = Install::open(&root.0, "Test", &Cache::new(&c.0)).unwrap();
        assert_eq!(t.point(1100.0, 2100.0).unwrap().unwrap().surface, 5);
    }

    /// A road from `(x0, z0)` to `(x1, z1)` at 150 m, its ends junctions `a`
    /// and `b`.
    fn road(a: u32, b: u32, [x0, z0]: [f64; 2], [x1, z1]: [f64; 2]) -> rn4::build::Edge {
        rn4::build::Edge {
            kind: rn4::KIND_ROAD,
            class: 0,
            a,
            b,
            points: vec![[x0, 150.0, z0], [x1, 150.0, z1]],
        }
    }

    #[test]
    fn a_road_crosses_the_water_under_it_even_where_a_road_strip_wins() {
        // A road strip over the whole square, in a second block over the
        // first, wins every point there; the lake is still under it.
        let blocks = [square([1024, LAKE]), square([ROAD_STRIP, ROAD_STRIP])];
        let root = install(&file(&blocks, 2));
        let roads = rn4::build::file(
            &["road"],
            &[
                // Out of the forest half into the lake.
                road(1, 2, [1100.0, 2100.0], [1900.0, 2900.0]),
                // In the forest half only.
                road(3, 4, [1100.0, 2100.0], [1400.0, 2100.0]),
            ],
        );
        put(&root.0, "Mods/terrains/Test/roads/Test.rn4", &roads);
        let c = Scratch::new();
        let t = Install::open(&root.0, "Test", &Cache::new(&c.0)).unwrap();
        assert_eq!(t.point(1900.0, 2900.0).unwrap().unwrap().water, 0);
        let g = t.roads().unwrap();
        let wet: Vec<bool> = g.edges.iter().map(|e| e.crosses_water).collect();
        assert_eq!(wet, [true, false]);
        assert_eq!(g.class(&g.edges[0]), "road");
        // Not built yet, so a query reads none rather than wait for it.
        assert_eq!(t.road_betweenness().unwrap(), None);
        assert_eq!(t.betweenness().unwrap(), [1.0, 0.0]);
        assert_eq!(t.road_betweenness().unwrap(), Some(&[1.0f32, 0.0][..]));
        let again = Install::open(&root.0, "Test", &Cache::new(&c.0)).unwrap();
        assert_eq!(again.road_betweenness().unwrap(), Some(&[1.0f32, 0.0][..]));
        for name in ["roads.graph", "roads.betweenness"] {
            assert!(c.0.join("Test").join(name).is_file(), "{name}");
        }
    }

    #[test]
    fn a_road_file_no_decoder_reads_refuses_roads_not_heights() {
        let root = install(&file(&[square([0, 0])], 1));
        let c = Scratch::new();
        let t = Install::open(&root.0, "Test", &Cache::new(&c.0)).unwrap();
        let e = t.roads().unwrap_err();
        assert!(e.0.contains("Test.rn4"), "{e}");
        assert!(t.point(1100.0, 2100.0).unwrap().is_some());
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
