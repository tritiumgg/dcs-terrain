//! An install terrain's road and rail graph: `roads.graph`, built from the
//! road file in one pass through a window and from the ground under each
//! edge, and `roads.betweenness`, built from the graph at low priority.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use super::{BOX_PAD_M, MESH_TOC, refusal, tile_span};
use crate::cache::{Entry, Opened, TerrainCache};
use crate::decode::rn4::{self, KIND_RAIL, KIND_ROAD, Reach};
use crate::decode::surface5::grid_for;
use crate::decode::surface5::ground::{Prepared, grid_units};
use crate::decode::surface5::{ROAD_STRIP, Surface5, water_class};
use crate::extract::FileEntry;
use crate::grid::Grid;
use crate::roads::{self, Graph, Stretch, Water};
use crate::source::{CELL_SIZE, Refusal, TILE_SIZE};

/// The graph of the road file, with each edge's water.
pub struct RoadsGraph;

pub static ROADS_GRAPH: RoadsGraph = RoadsGraph;

const GRAPH_VERSION: u32 = 1;

impl Entry for RoadsGraph {
    fn name(&self) -> &str {
        "roads.graph"
    }

    fn version(&self) -> u32 {
        GRAPH_VERSION
    }

    /// The surface too, for the water under each edge.
    fn roles(&self) -> &[&str] {
        &["roads", "surface"]
    }

    fn build(&self, cache: &TerrainCache, files: &[FileEntry]) -> Result<Vec<u8>, Refusal> {
        let (root, map) = (cache.install(), cache.map());
        let f = files
            .iter()
            .find(|f| f.role == "roads")
            .ok_or_else(|| Refusal(format!("{} has no road file", map.theatre)))?;
        let (toc, surface) = ground(cache)?;
        let file = File::open(root.join(&f.path)).map_err(|e| refusal(&f.path, e))?;
        let mut stretches = Vec::new();
        let mut ends = Vec::new();
        let classes = rn4::scan(file, &f.path, &Reach::around(&toc), |e| {
            if e.kind != KIND_ROAD && e.kind != KIND_RAIL {
                return;
            }
            ends.extend(rn4::ends(e.a, e.b, &e.points));
            stretches.push(Stretch {
                kind: e.kind,
                class: e.class,
                a: e.a,
                b: e.b,
                points: e.points.iter().map(|p| [p[0], p[2]]).collect(),
            });
        })
        .map_err(|e| refusal(&f.path, e))?;
        // Each map has one id whose ends are spread across it; joined
        // through one node it would weld the map's dead ends together.
        let wide = rn4::catch_alls(ends);
        for s in &mut stretches {
            for end in [&mut s.a, &mut s.b] {
                if end.is_some_and(|id| wide.binary_search(&id).is_ok()) {
                    *end = None;
                }
            }
        }
        let mut water = Blocks {
            toc: &toc,
            file: &surface,
            path: &map.surface.path,
        };
        Ok(roads::build(classes, stretches, &mut water)?.encode())
    }
}

/// The surface's table of contents from the cache, and its file, refused if
/// it changed since the table was read.
fn ground(cache: &TerrainCache) -> Result<(Surface5, File), Refusal> {
    let opened = cache.get(&MESH_TOC)?;
    let bytes = opened
        .payload()
        .map_err(|e| refusal(&opened.path.display().to_string(), e))?;
    let toc =
        Surface5::decode(&bytes).map_err(|e| refusal(&opened.path.display().to_string(), e))?;
    let path = &cache.map().surface.path;
    let file = File::open(cache.install().join(path)).map_err(|e| refusal(path, e))?;
    let size = file.metadata().map_err(|e| refusal(path, e))?.len();
    if size != toc.file_len() {
        return Err(Refusal(format!(
            "{path} changed while the road graph was being built; build it again"
        )));
    }
    Ok((toc, file))
}

/// The water under points, read a block at a time: each block the points
/// fall in is read and prepared once, and tests every point in its box.
struct Blocks<'a> {
    toc: &'a Surface5,
    file: &'a File,
    path: &'a str,
}

impl Water for Blocks<'_> {
    fn on_water(&mut self, points: &[[f32; 2]]) -> Result<Vec<bool>, Refusal> {
        let grid = grid_for(&self.toc.bounds(), CELL_SIZE, TILE_SIZE);
        let (start, ids) = by_tile(&grid, points);
        let ntz = grid.tiles().1 as usize;
        let mut wet = vec![false; points.len()];
        let mut here = Vec::new();
        for (i, blk) in self.toc.blocks.iter().enumerate() {
            let [x0, z0, x1, z1] = blk.world_box();
            let p = BOX_PAD_M;
            let (x0, z0, x1, z1) = (x0 - p, z0 - p, x1 + p, z1 + p);
            let Some((rows, cols)) = tile_span(&grid, [x0, z0, x1, z1]) else {
                continue;
            };
            here.clear();
            for r in rows {
                for c in cols.clone() {
                    let k = r * ntz + c;
                    for &s in &ids[start[k] as usize..start[k + 1] as usize] {
                        let [x, z] = points[s as usize].map(f64::from);
                        if !wet[s as usize] && (x0..=x1).contains(&x) && (z0..=z1).contains(&z) {
                            here.push(s);
                        }
                    }
                }
            }
            if here.is_empty() {
                continue;
            }
            let b = self
                .toc
                .read_block(self.file, i, &[])
                .map_err(|e| refusal(self.path, e))?;
            if !b
                .tritype
                .iter()
                .any(|&t| water_class(t) != 0 && t & ROAD_STRIP == 0)
            {
                continue;
            }
            let b = Prepared::water(b, &grid);
            for &s in &here {
                let [x, z] = points[s as usize].map(f64::from);
                wet[s as usize] |= b.water_under(grid_units(&grid, x, z));
            }
        }
        Ok(wet)
    }
}

/// The points in each tile of `g`, row-major: tile k's are
/// `ids[start[k]..start[k + 1]]`. A point past the grid is in none.
fn by_tile(g: &Grid, points: &[[f32; 2]]) -> (Vec<u32>, Vec<u32>) {
    let (ntx, ntz) = g.tiles();
    let n = ntx as usize * ntz as usize;
    let tile = |p: &[f32; 2]| {
        let [x, z] = p.map(f64::from);
        tile_span(g, [x, z, x, z]).map(|(r, c)| *r.start() * ntz as usize + *c.start())
    };
    let tiles: Vec<Option<usize>> = points.iter().map(tile).collect();
    let mut start = vec![0u32; n + 1];
    for k in tiles.iter().flatten() {
        start[k + 1] += 1;
    }
    for k in 1..start.len() {
        start[k] += start[k - 1];
    }
    let mut fill = start.clone();
    let mut ids = vec![0u32; start[n] as usize];
    for (i, k) in tiles.iter().enumerate() {
        if let Some(k) = k {
            ids[fill[*k] as usize] = i as u32;
            fill[*k] += 1;
        }
    }
    (start, ids)
}

/// Each edge's betweenness in its network, in the graph's edge order.
pub struct RoadsBetweenness;

pub static ROADS_BETWEENNESS: RoadsBetweenness = RoadsBetweenness;

impl Entry for RoadsBetweenness {
    fn name(&self) -> &str {
        "roads.betweenness"
    }

    /// Its rows follow the graph's edges, so a new graph layout is a new
    /// version of this entry too.
    fn version(&self) -> u32 {
        GRAPH_VERSION * 100 + 1
    }

    fn roles(&self) -> &[&str] {
        ROADS_GRAPH.roles()
    }

    fn build(&self, cache: &TerrainCache, _: &[FileEntry]) -> Result<Vec<u8>, Refusal> {
        let g = graph(cache)?;
        // At most two threads, so a build beside a DCS server leaves it room.
        let threads = crate::threads::cap().min(2);
        let b = roads::betweenness(&g, roads::SOURCES, threads);
        let mut out = Vec::with_capacity(4 + 4 * b.len());
        out.extend_from_slice(&(b.len() as u32).to_le_bytes());
        for v in b {
            out.extend_from_slice(&v.to_le_bytes());
        }
        Ok(out)
    }
}

fn unreadable(path: &Path, theatre: &str, e: impl std::fmt::Display) -> Refusal {
    Refusal(format!(
        "{} cannot be read ({e}); `dcsterrain cache clear --terrain {theatre}` rebuilds it",
        path.display()
    ))
}

/// The road graph, from the cache, built there first if missing or stale.
pub fn graph(cache: &TerrainCache) -> Result<Graph, Refusal> {
    let opened = cache.get(&ROADS_GRAPH)?;
    let bad = |e: &dyn std::fmt::Display| unreadable(&opened.path, &cache.map().theatre, e);
    let bytes = payload(&opened).map_err(|e| bad(&e))?;
    Graph::decode(&bytes).map_err(|e| bad(&e.0))
}

/// An entry's payload read into a buffer of its size, allocated once: the
/// largest terrain's graph is over a hundred MiB, which a growing buffer
/// copies several times over on the way.
fn payload(opened: &Opened) -> std::io::Result<Vec<u8>> {
    let len = usize::try_from(opened.len).map_err(std::io::Error::other)?;
    let mut out = vec![0u8; len];
    let mut f = &opened.file;
    f.seek(SeekFrom::Start(opened.offset))?;
    f.read_exact(&mut out)?;
    Ok(out)
}

/// Each edge's betweenness for `g`, from the cache, built there first if
/// missing or stale: minutes on the largest terrains.
pub fn betweenness(cache: &TerrainCache, g: &Graph) -> Result<Vec<f32>, Refusal> {
    let opened = cache.get(&ROADS_BETWEENNESS)?;
    let bad = |e: &dyn std::fmt::Display| unreadable(&opened.path, &cache.map().theatre, e);
    let b = opened.payload().map_err(|e| bad(&e))?;
    let n = b
        .get(..4)
        .map(|n| u32::from_le_bytes(n.try_into().unwrap()) as usize)
        .ok_or_else(|| bad(&"it is empty"))?;
    if n != g.edges.len() || b.len() != 4 + 4 * n {
        return Err(bad(&format!(
            "it holds {n} edges, where the graph has {}",
            g.edges.len()
        )));
    }
    Ok(b[4..]
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
        .collect())
}
