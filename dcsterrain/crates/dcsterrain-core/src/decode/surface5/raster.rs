//! The `height`, `water` and `surface` tile layers, from the finest mesh's
//! winning triangle at each cell centre.
//!
//! Tiles are made a band of tile rows at a time. Only the blocks under the
//! band are held, and each is read once per band it reaches; a block spans
//! a few tiles, so most are read two or three times. Within a band, tiles
//! are rasterized in parallel.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read, Seek};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use super::mesh::{Block, Error, Surface5};
use super::sample::{ROAD_STRIP, cover, rank, round_height, water_class};
use crate::extract::{Bounds, CELL_SIZE_M, Grid, TILE_SIZE, TileEntry, layer};

/// The material whose subset marks a runway.
pub const RUNWAY_MATERIAL: &str = "Runway";

/// `surface` codes. SHALLOW_WATER, 2, is never written: nothing in the file
/// is known to mark it.
pub const SURFACE_LAND: u8 = 1;
pub const SURFACE_WATER: u8 = 3;
pub const SURFACE_ROAD: u8 = 4;
pub const SURFACE_RUNWAY: u8 = 5;

const HEIGHT_NODATA: i16 = -32768;
const WATER_NODATA: u8 = 255;
const SURFACE_NODATA: u8 = 0;
const WATER_SEA: u8 = 2;

/// The grid of an extract whose authored bounds are `b`: its origin and
/// extent snapped outward to whole cells.
pub fn grid_for(b: &Bounds) -> Grid {
    let cs = f64::from(CELL_SIZE_M);
    let (x0, z0) = ((b.min_x / cs).floor() * cs, (b.min_z / cs).floor() * cs);
    let (x1, z1) = ((b.max_x / cs).ceil() * cs, (b.max_z / cs).ceil() * cs);
    Grid {
        cell_size: CELL_SIZE_M,
        origin_x: x0,
        origin_z: z0,
        height: ((x1 - x0) / cs).round() as u32,
        width: ((z1 - z0) / cs).round() as u32,
        tile_size: TILE_SIZE,
    }
}

/// A block ready to rasterize: its corners in grid units, where cell
/// `(row, col)` has its centre at `(row, col)`.
struct Prepared {
    index: usize,
    rc: Vec<[f64; 2]>,
    h: Vec<f64>,
    tri: Vec<[u32; 3]>,
    tritype: Vec<u32>,
    runway: Vec<[u32; 3]>,
}

fn grid_units(g: &Grid, x: f64, z: f64) -> [f64; 2] {
    let cs = f64::from(g.cell_size);
    [(x - g.origin_x) / cs - 0.5, (z - g.origin_z) / cs - 0.5]
}

fn prepare(b: Block, g: &Grid) -> Prepared {
    let n = b.p.len() as u32;
    let world: Vec<[f64; 3]> = (0..n).map(|i| b.world(i)).collect();
    let runway = b
        .subsets
        .iter()
        .filter(|s| s.material == RUNWAY_MATERIAL)
        .flat_map(|s| s.tri.iter().copied())
        .collect();
    Prepared {
        index: b.index,
        rc: world.iter().map(|v| grid_units(g, v[0], v[2])).collect(),
        h: world.iter().map(|v| v[1]).collect(),
        tri: b.tri,
        tritype: b.tritype,
        runway,
    }
}

/// One tile of each of the three layers, row-major with columns fastest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tile {
    pub tx: u32,
    pub tz: u32,
    pub height: Vec<i16>,
    pub water: Vec<u8>,
    pub surface: Vec<u8>,
}

/// The cells of tile `(tx, tz)` a triangle's corners can cover: row and
/// column ranges, clipped to the tile and the grid.
fn cells(
    g: &Grid,
    tx: u32,
    tz: u32,
    corners: [[f64; 2]; 3],
) -> Option<(std::ops::RangeInclusive<u32>, std::ops::RangeInclusive<u32>)> {
    let ts = g.tile_size;
    let span = |k: usize, start: u32, limit: u32| {
        let lo = corners.iter().map(|c| c[k]).fold(f64::INFINITY, f64::min);
        let hi = corners
            .iter()
            .map(|c| c[k])
            .fold(f64::NEG_INFINITY, f64::max);
        let end = (start + ts).min(limit);
        let lo = lo.ceil().max(f64::from(start));
        let hi = hi.floor().min(f64::from(end) - 1.0);
        (lo <= hi).then_some(lo as u32..=hi as u32)
    };
    Some((span(0, tx * ts, g.height)?, span(1, tz * ts, g.width)?))
}

fn raster_tile(blocks: &[&Prepared], g: &Grid, tx: u32, tz: u32) -> Tile {
    let ts = g.tile_size;
    let n = (ts * ts) as usize;
    const NONE: u64 = u64::MAX;
    let mut best = vec![NONE; n];
    let mut h = vec![0f64; n];
    let mut ty = vec![0u32; n];
    let mut runway = vec![false; n];
    let at = |r: u32, c: u32| ((r - tx * ts) * ts + (c - tz * ts)) as usize;

    for b in blocks {
        for (t, tri) in b.tri.iter().enumerate() {
            let p = tri.map(|k| b.rc[k as usize]);
            let Some((rows, cols)) = cells(g, tx, tz, p) else {
                continue;
            };
            let k = rank(b.tritype[t], b.index, t);
            for r in rows {
                for c in cols.clone() {
                    let Some((u, v)) = cover(p[0], p[1], p[2], [f64::from(r), f64::from(c)]) else {
                        continue;
                    };
                    let i = at(r, c);
                    if best[i] == NONE || k > best[i] {
                        let z = tri.map(|k| b.h[k as usize]);
                        best[i] = k;
                        h[i] = z[0] + u * (z[1] - z[0]) + v * (z[2] - z[0]);
                        ty[i] = b.tritype[t];
                    }
                }
            }
        }
        for tri in &b.runway {
            let p = tri.map(|k| b.rc[k as usize]);
            let Some((rows, cols)) = cells(g, tx, tz, p) else {
                continue;
            };
            for r in rows {
                for c in cols.clone() {
                    if cover(p[0], p[1], p[2], [f64::from(r), f64::from(c)]).is_some() {
                        runway[at(r, c)] = true;
                    }
                }
            }
        }
    }

    let mut tile = Tile {
        tx,
        tz,
        height: vec![HEIGHT_NODATA; n],
        water: vec![WATER_NODATA; n],
        surface: vec![SURFACE_NODATA; n],
    };
    for i in 0..n {
        if best[i] == NONE {
            continue;
        }
        tile.height[i] = round_height(h[i]);
        let w = water_class(ty[i]);
        tile.water[i] = w;
        tile.surface[i] = if runway[i] {
            SURFACE_RUNWAY
        } else if ty[i] & ROAD_STRIP != 0 {
            SURFACE_ROAD
        } else if w != 0 {
            SURFACE_WATER
        } else {
            SURFACE_LAND
        };
    }
    tile
}

/// Runs `f` on `0..n` across the machine's threads, results in order. The
/// first error stops the rest.
fn parallel<T: Send>(
    n: usize,
    f: impl Fn(usize) -> Result<T, Error> + Sync,
) -> Result<Vec<T>, Error> {
    let workers = thread::available_parallelism()
        .map_or(1, |k| k.get())
        .min(n.max(1));
    let next = AtomicUsize::new(0);
    let out: Mutex<Vec<Option<T>>> = Mutex::new((0..n).map(|_| None).collect());
    let failed: Mutex<Option<Error>> = Mutex::new(None);
    thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= n || failed.lock().unwrap().is_some() {
                        return;
                    }
                    match f(i) {
                        Ok(v) => out.lock().unwrap()[i] = Some(v),
                        Err(e) => {
                            failed.lock().unwrap().get_or_insert(e);
                            return;
                        }
                    }
                }
            });
        }
    });
    if let Some(e) = failed.into_inner().unwrap() {
        return Err(e);
    }
    Ok(out
        .into_inner()
        .unwrap()
        .into_iter()
        .map(Option::unwrap)
        .collect())
}

/// The world range of cell centres along one axis of tile index `k`.
fn centres(origin: f64, cs: f64, ts: u32, k: u32, limit: u32) -> (f64, f64) {
    let first = k * ts;
    let last = ((k + 1) * ts).min(limit) - 1;
    (
        origin + (f64::from(first) + 0.5) * cs,
        origin + (f64::from(last) + 0.5) * cs,
    )
}

/// Rasterizes every tile of `grid` from `file`, handing each to `sink` in
/// order of `tx`, then `tz`. `open` gives a fresh reader of the file for each
/// block read, so threads never share one.
pub fn rasterize<R, O, S>(file: &Surface5, open: O, grid: &Grid, mut sink: S) -> Result<(), Error>
where
    R: Read + Seek,
    O: Fn() -> io::Result<R> + Sync,
    S: FnMut(Tile) -> Result<(), Error>,
{
    let (ntx, ntz) = grid.tiles();
    if ntx == 0 || ntz == 0 {
        return Ok(());
    }
    let cs = f64::from(grid.cell_size);
    let boxes: Vec<[f64; 4]> = file.blocks.iter().map(|b| b.world_box()).collect();
    let mut held: BTreeMap<usize, Arc<Prepared>> = BTreeMap::new();
    for tx in 0..ntx {
        let (x0, x1) = centres(grid.origin_x, cs, grid.tile_size, tx, grid.height);
        let (z0, _) = centres(grid.origin_z, cs, grid.tile_size, 0, grid.width);
        let (_, z1) = centres(grid.origin_z, cs, grid.tile_size, ntz - 1, grid.width);
        let band: Vec<usize> = (0..boxes.len())
            .filter(|&i| {
                let [bx0, bz0, bx1, bz1] = boxes[i];
                bx0 <= x1 && bx1 >= x0 && bz0 <= z1 && bz1 >= z0
            })
            .collect();
        held.retain(|i, _| band.binary_search(i).is_ok());
        let missing: Vec<usize> = band
            .iter()
            .copied()
            .filter(|i| !held.contains_key(i))
            .collect();
        let read = parallel(missing.len(), |k| {
            let mut r = open()?;
            let b = file.read_block(&mut r, missing[k], &[RUNWAY_MATERIAL])?;
            Ok(Arc::new(prepare(b, grid)))
        })?;
        for b in read {
            held.insert(b.index, b);
        }
        let tiles = parallel(ntz as usize, |tz| {
            let tz = tz as u32;
            let (z0, z1) = centres(grid.origin_z, cs, grid.tile_size, tz, grid.width);
            let under: Vec<&Prepared> = held
                .values()
                .filter(|b| {
                    let [_, bz0, _, bz1] = boxes[b.index];
                    bz0 <= z1 && bz1 >= z0
                })
                .map(|b| b.as_ref())
                .collect();
            Ok(raster_tile(&under, grid, tx, tz))
        })?;
        for t in tiles {
            sink(t)?;
        }
    }
    Ok(())
}

fn put(dir: &Path, rel: &str, bytes: &[u8]) -> io::Result<()> {
    let path = dir.join(rel);
    fs::create_dir_all(path.parent().unwrap())?;
    let tmp = dir.join(format!("{rel}.tmp"));
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, &path)
}

fn entry(name: &str, tx: u32, tz: u32, values: impl Iterator<Item = i64>) -> TileEntry {
    let nodata = i64::from(layer(name).unwrap().nodata);
    let present: Vec<i64> = values.filter(|&v| v != nodata).collect();
    TileEntry {
        layer: name.to_string(),
        tx,
        tz,
        path: format!("tiles/{name}/{tx}_{tz}.bin"),
        min: present.iter().copied().min(),
        max: present.iter().copied().max(),
    }
}

/// Writes the `height`, `water` and `surface` tiles of `grid` under `dir`,
/// each as a `.tmp` renamed into place, and returns their manifest entries
/// by layer, then tile. A tile wholly outside the mesh is written in no
/// layer, and one wholly sea only in `water`.
pub fn write_tiles<R, O>(
    file: &Surface5,
    open: O,
    grid: &Grid,
    dir: &Path,
) -> Result<Vec<TileEntry>, Error>
where
    R: Read + Seek,
    O: Fn() -> io::Result<R> + Sync,
{
    let mut entries = Vec::new();
    rasterize(file, open, grid, |t| {
        if t.water.iter().all(|&w| w == WATER_NODATA) {
            return Ok(());
        }
        let (tx, tz) = (t.tx, t.tz);
        let w = entry("water", tx, tz, t.water.iter().map(|&v| i64::from(v)));
        put(dir, &w.path, &t.water)?;
        entries.push(w);
        // Omitted only where the rebuilt tile, height 0 and surface WATER,
        // would be what was rasterized: a sea tile that differs keeps its data.
        let sea = t.water.iter().all(|&w| w == WATER_SEA)
            && t.height.iter().all(|&h| h == 0)
            && t.surface.iter().all(|&s| s == SURFACE_WATER);
        if sea {
            return Ok(());
        }
        let h = entry("height", tx, tz, t.height.iter().map(|&v| i64::from(v)));
        let bytes: Vec<u8> = t.height.iter().flat_map(|v| v.to_le_bytes()).collect();
        put(dir, &h.path, &bytes)?;
        entries.push(h);
        let s = entry("surface", tx, tz, t.surface.iter().map(|&v| i64::from(v)));
        put(dir, &s.path, &t.surface)?;
        entries.push(s);
        Ok(())
    })?;
    let order = ["height", "water", "surface"];
    entries.sort_by_key(|e| (order.iter().position(|l| *l == e.layer), e.tx, e.tz));
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::super::build::{self, f32s, file, tri, u32s, vec3};
    use super::super::sample::{LAKE, SEA};
    use super::*;
    use crate::decode::surface5::Mesh;
    use crate::scratch::Scratch;

    /// A kilometre square at the origin in two triangles: the south-west
    /// half land rising from 100 m in the south to 200 m in the north, the
    /// north-east half a lake. A runway subset covers a triangle near the
    /// south-west corner, over three vertices the full list does not use.
    fn square(tritype: [u32; 2]) -> build::Block {
        build::Block {
            t: [0.0, 0.0, 0.0],
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

    /// One tile of open sea, at the origin.
    fn sea() -> build::Block {
        let s = 12800.0;
        build::Block {
            t: [0.0, 0.0, 0.0],
            bbox: [0.0, -40.0, 0.0, s, 0.0, s],
            arrays: vec![
                u32s("TRITYPE", &[SEA, SEA]),
                tri(&[[0, 1, 2], [1, 3, 2]], Some(("Sea5.0", "Sea"))),
                vec3(
                    "Pbase",
                    &[
                        [0.0, -40.0, 0.0],
                        [s, -40.0, 0.0],
                        [0.0, -40.0, s],
                        [s, -40.0, s],
                    ],
                ),
                f32s("depth", &[40.0; 4]),
            ],
        }
    }

    fn tiles(f: &[u8], grid: &Grid) -> Vec<Tile> {
        let file = Surface5::open(&mut Cursor::new(f), "t").unwrap();
        let mut out = Vec::new();
        rasterize(
            &file,
            || Ok(Cursor::new(f)),
            grid,
            |t| {
                out.push(t);
                Ok(())
            },
        )
        .unwrap();
        out
    }

    fn one(blocks: &[build::Block]) -> (Vec<u8>, Grid) {
        let f = file(blocks, blocks.len() as u32);
        let file = Surface5::open(&mut Cursor::new(&f[..]), "t").unwrap();
        let grid = grid_for(&file.bounds());
        (f, grid)
    }

    fn cell(t: &Tile, r: usize, c: usize) -> (i16, u8, u8) {
        let i = r * 256 + c;
        (t.height[i], t.water[i], t.surface[i])
    }

    #[test]
    fn the_grid_is_the_bounds_snapped_outward() {
        let g = grid_for(&Bounds {
            min_x: -490048.7,
            min_z: -570000.0,
            max_x: 270000.0,
            max_z: 960039.4,
        });
        assert_eq!((g.origin_x, g.origin_z), (-490050.0, -570000.0));
        assert_eq!((g.height, g.width), (15201, 30601));
        assert_eq!((g.cell_size, g.tile_size), (50, 256));
    }

    #[test]
    fn rasterizes_a_two_triangle_block() {
        let (f, grid) = one(&[square([0, LAKE])]);
        assert_eq!((grid.height, grid.width), (20, 20));
        let t = &tiles(&f, &grid)[0];
        // (25, 25): 102.5 m rounds up.
        assert_eq!(cell(t, 0, 0), (103, 0, SURFACE_LAND));
        // (125, 125) lies inside the runway subset.
        assert_eq!(cell(t, 2, 2), (113, 0, SURFACE_RUNWAY));
        // (975, 975), the lake: its surface, not its bed.
        assert_eq!(cell(t, 19, 19).1, 1);
        assert_eq!(cell(t, 19, 19).2, SURFACE_WATER);
        // A centre on the shared diagonal goes to the later triangle.
        assert_eq!(cell(t, 9, 10).1, 1);
        // Past the grid's edge is nodata, as is anything outside the mesh.
        assert_eq!(
            cell(t, 20, 0),
            (HEIGHT_NODATA, WATER_NODATA, SURFACE_NODATA)
        );
    }

    #[test]
    fn every_cell_is_what_the_sampler_gives_at_its_centre() {
        let (f, grid) = one(&[square([0, LAKE])]);
        let t = &tiles(&f, &grid)[0];
        let mesh: Mesh = Surface5::open(&mut Cursor::new(&f[..]), "t")
            .unwrap()
            .read_all(&mut Cursor::new(&f[..]), &[])
            .unwrap();
        for r in 0..20 {
            for c in 0..20 {
                let (x, z) = (f64::from(r) * 50.0 + 25.0, f64::from(c) * 50.0 + 25.0);
                let hit = mesh.sample(x, z).unwrap();
                let (h, w, _) = cell(t, r as usize, c as usize);
                assert_eq!(h, round_height(hit.h), "({r}, {c})");
                assert_eq!(w, water_class(hit.tritype), "({r}, {c})");
            }
        }
    }

    #[test]
    fn a_road_strip_wins_over_a_later_block() {
        let mut road = square([ROAD_STRIP, ROAD_STRIP]);
        road.arrays[3] = tri(&[], Some(("Runway5.0", "Runway")));
        // The later block is 50 m lower everywhere.
        let mut low = square([0, 0]);
        low.t[1] = -50.0;
        let (f, grid) = one(&[road, low]);
        let t = &tiles(&f, &grid)[0];
        assert_eq!(cell(t, 0, 0), (103, 0, SURFACE_ROAD));

        let (f, grid) = one(&[square([0, 0]), {
            let mut low = square([0, 0]);
            low.t[1] = -50.0;
            low
        }]);
        assert_eq!(cell(&tiles(&f, &grid)[0], 0, 0).0, 53);
    }

    #[test]
    fn blocks_across_bands_and_partial_tiles_rasterize_as_sampled() {
        // Two 20 km blocks, the second offset 10 km north and east: a 30 km
        // grid of 600 cells, so three bands of tiles, the last partial on
        // both axes, and blocks held from one band into the next.
        let block = |t: [f32; 3], tritype: [u32; 2]| {
            let s = 20000.0;
            build::Block {
                t,
                bbox: [0.0, 0.0, 0.0, s, 1000.0, s],
                arrays: vec![
                    u32s("TRITYPE", &tritype),
                    tri(&[[0, 1, 2], [1, 3, 2]], None),
                    vec3(
                        "Pbase",
                        &[
                            [0.0, 0.0, 0.0],
                            [s, 1000.0, 0.0],
                            [0.0, 500.0, s],
                            [s, 200.0, s],
                        ],
                    ),
                ],
            }
        };
        let blocks = [
            block([0.0; 3], [0, LAKE]),
            block([10000.0, 0.0, 10000.0], [ROAD_STRIP, 0]),
        ];
        let (f, grid) = one(&blocks);
        assert_eq!((grid.height, grid.width, grid.tiles()), (600, 600, (3, 3)));
        let mesh = Surface5::open(&mut Cursor::new(&f[..]), "t")
            .unwrap()
            .read_all(&mut Cursor::new(&f[..]), &[])
            .unwrap();
        let tiles = tiles(&f, &grid);
        assert_eq!(tiles.len(), 9);
        for t in &tiles {
            for i in 0..256 * 256 {
                let (r, c) = (t.tx * 256 + i / 256, t.tz * 256 + i % 256);
                let (x, z) = (f64::from(r) * 50.0 + 25.0, f64::from(c) * 50.0 + 25.0);
                let got = (
                    t.height[i as usize],
                    t.water[i as usize],
                    t.surface[i as usize],
                );
                let want = match mesh.sample(x, z) {
                    _ if r >= 600 || c >= 600 => (HEIGHT_NODATA, WATER_NODATA, SURFACE_NODATA),
                    None => (HEIGHT_NODATA, WATER_NODATA, SURFACE_NODATA),
                    Some(h) => {
                        let w = water_class(h.tritype);
                        let s = if h.tritype & ROAD_STRIP != 0 {
                            SURFACE_ROAD
                        } else if w != 0 {
                            SURFACE_WATER
                        } else {
                            SURFACE_LAND
                        };
                        (round_height(h.h), w, s)
                    }
                };
                assert_eq!(got, want, "({r}, {c})");
            }
        }
    }

    #[test]
    fn a_lake_wins_over_a_later_land_block() {
        let mut land = square([0, 0]);
        land.t[1] = 50.0;
        let (f, grid) = one(&[square([LAKE, LAKE]), land]);
        let t = &tiles(&f, &grid)[0];
        assert_eq!(cell(t, 0, 0), (103, 1, SURFACE_WATER));
    }

    #[test]
    fn writes_each_layer_and_omits_what_reads_back_without_it() {
        let s = Scratch::new();
        let (f, mut grid) = one(&[sea()]);
        // A second tile east of the mesh, wholly outside it.
        grid.width = 300;
        let file = Surface5::open(&mut Cursor::new(&f[..]), "t").unwrap();
        let entries = write_tiles(&file, || Ok(Cursor::new(&f[..])), &grid, &s.0).unwrap();
        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, ["tiles/water/0_0.bin"]);
        assert_eq!((entries[0].min, entries[0].max), (Some(2), Some(2)));
        let water = fs::read(s.0.join("tiles/water/0_0.bin")).unwrap();
        assert_eq!(water.len(), 256 * 256);
        assert!(!s.0.join("tiles/water/0_0.bin.tmp").exists());

        // A sea tile whose surface is not at 0 m keeps its height.
        let s = Scratch::new();
        let mut high = sea();
        high.arrays[3] = f32s("depth", &[41.0; 4]);
        let (f, grid) = one(&[high]);
        let file = Surface5::open(&mut Cursor::new(&f[..]), "t").unwrap();
        let entries = write_tiles(&file, || Ok(Cursor::new(&f[..])), &grid, &s.0).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].max, Some(1));

        let s = Scratch::new();
        let (f, grid) = one(&[square([0, LAKE])]);
        let file = Surface5::open(&mut Cursor::new(&f[..]), "t").unwrap();
        let entries = write_tiles(&file, || Ok(Cursor::new(&f[..])), &grid, &s.0).unwrap();
        let layers: Vec<&str> = entries.iter().map(|e| e.layer.as_str()).collect();
        assert_eq!(layers, ["height", "water", "surface"]);
        let h = fs::read(s.0.join(&entries[0].path)).unwrap();
        assert_eq!(i16::from_le_bytes([h[0], h[1]]), 103);
        assert_eq!(entries[2].max, Some(i64::from(SURFACE_RUNWAY)));
    }
}
