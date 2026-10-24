//! The extract's scenery: `scenery.jsonl`, one row per object, the models
//! every pool places in `scenery_models.json`, and the `trees` and `lines`
//! layers, a count of each pool's instances per cell.
//!
//! A count is read only where the mesh covers the cell, which the `water`
//! tile at the same address says: a cell whose water is nodata is nodata in
//! the counts too, and an instance there is not counted. A count tile is
//! written only where a covered cell is non-zero; the reader takes an absent
//! one as zero wherever water is not nodata.

use std::collections::{BTreeMap, HashMap};
use std::fs::{self, File};
use std::io::{BufWriter, Read, Seek, Write};
use std::path::Path;

use serde::Serialize;

use super::{Error, Pool, Scene, malformed};
use crate::decode::surface5::{put_tile, tile_entry};
use crate::extract::{Grid, TileEntry, layer};

pub const SCENERY_FILE: &str = "scenery.jsonl";
pub const MODELS_FILE: &str = "scenery_models.json";

/// A count saturates here, below the layers' nodata.
const COUNT_MAX: u16 = 65534;

/// What was written, for the manifest and the run's report.
#[derive(Debug, Clone)]
pub struct Written {
    /// Rows of `scenery.jsonl`: the object pool and the group.
    pub objects: u64,
    /// Every instance of each pool, counted into a cell or not.
    pub lines: u64,
    pub trees: u64,
    /// Rows of `scenery_models.json`.
    pub models: usize,
    /// The `trees` and `lines` tiles, by layer, then tile.
    pub tiles: Vec<TileEntry>,
}

#[derive(Serialize)]
struct Row<'a> {
    id: u64,
    model: &'a str,
    x: f64,
    alt: f64,
    z: f64,
    rotation: f64,
}

#[derive(Serialize)]
struct Counts {
    objects: u64,
    lines: u64,
    trees: u64,
}

#[derive(Serialize)]
struct BoxRow {
    min_x: f32,
    min_y: f32,
    min_z: f32,
    max_x: f32,
    max_y: f32,
    max_z: f32,
}

#[derive(Serialize)]
struct ModelRow<'a> {
    model: &'a str,
    counts: Counts,
    #[serde(rename = "box")]
    bbox: BoxRow,
}

/// One count layer, a tile allocated when an instance first falls in it.
struct CountLayer {
    grid: Grid,
    tiles: HashMap<(u32, u32), Vec<u16>>,
}

impl CountLayer {
    fn new(grid: &Grid) -> CountLayer {
        CountLayer {
            grid: *grid,
            tiles: HashMap::new(),
        }
    }

    /// Counts an instance at world `(x, z)` into its cell, if the grid
    /// holds one there. Cell `(row, col)` spans `cell_size` north and east
    /// of `origin + (row, col) × cell_size`.
    fn add(&mut self, x: f64, z: f64) {
        let g = &self.grid;
        let cs = f64::from(g.cell_size);
        let (row, col) = (
            ((x - g.origin_x) / cs).floor(),
            ((z - g.origin_z) / cs).floor(),
        );
        if !(row >= 0.0 && col >= 0.0 && row < f64::from(g.height) && col < f64::from(g.width)) {
            return;
        }
        let (row, col, ts) = (row as u32, col as u32, g.tile_size);
        let tile = self
            .tiles
            .entry((row / ts, col / ts))
            .or_insert_with(|| vec![0; (ts * ts) as usize]);
        let c = &mut tile[((row % ts) * ts + col % ts) as usize];
        *c = (*c + 1).min(COUNT_MAX);
    }

    /// Writes each tile with a covered non-zero cell, nodata where `water`
    /// is, and returns their entries in tile order.
    fn write(
        self,
        name: &str,
        water: &BTreeMap<(u32, u32), &TileEntry>,
        dir: &Path,
    ) -> Result<Vec<TileEntry>, Error> {
        let water_nodata = layer("water").unwrap().nodata as u8;
        let nodata = layer(name).unwrap().nodata as u16;
        let cells = (self.grid.tile_size * self.grid.tile_size) as usize;
        let mut keys: Vec<(u32, u32)> = self.tiles.keys().copied().collect();
        keys.sort_unstable();
        let mut out = Vec::new();
        for (tx, tz) in keys {
            // No water tile: every cell is outside the mesh.
            let Some(w) = water.get(&(tx, tz)) else {
                continue;
            };
            let wb = fs::read(dir.join(&w.path))?;
            if wb.len() != cells {
                return Err(malformed(format!(
                    "{} holds {} bytes, where a tile of {} cells has {cells}",
                    w.path,
                    wb.len(),
                    cells
                )));
            }
            let counts = &self.tiles[&(tx, tz)];
            let tile: Vec<u16> = counts
                .iter()
                .zip(&wb)
                .map(|(&c, &w)| if w == water_nodata { nodata } else { c })
                .collect();
            if !tile.iter().any(|&c| c != 0 && c != nodata) {
                continue;
            }
            let e = tile_entry(name, tx, tz, tile.iter().map(|&v| i64::from(v)));
            let bytes: Vec<u8> = tile.iter().flat_map(|v| v.to_le_bytes()).collect();
            put_tile(dir, &e.path, &bytes)?;
            out.push(e);
        }
        Ok(out)
    }
}

/// Writes `path` through a `.tmp` renamed into place.
fn put_file(
    path: &Path,
    f: impl FnOnce(&mut BufWriter<File>) -> Result<(), Error>,
) -> Result<(), Error> {
    let tmp = path.with_file_name(format!(
        "{}.tmp",
        path.file_name().unwrap().to_string_lossy()
    ));
    let mut out = BufWriter::new(File::create(&tmp)?);
    f(&mut out)?;
    out.into_inner().map_err(|e| e.into_error())?.sync_all()?;
    fs::rename(&tmp, path)?;
    Ok(())
}

fn json_err(e: serde_json::Error) -> Error {
    Error::Io(e.into())
}

/// Writes the scenery tables and count tiles of `scene` under `dir`, read
/// through `r`. `water` is the extract's `water` tiles, already under `dir`.
pub fn write<R: Read + Seek>(
    scene: &Scene,
    r: &mut R,
    grid: &Grid,
    water: &[TileEntry],
    dir: &Path,
) -> Result<Written, Error> {
    let mut placed = vec![[0u64; 3]; scene.models.len()];
    let mut lines = CountLayer::new(grid);
    let mut trees = CountLayer::new(grid);
    let mut objects = 0;
    put_file(&dir.join(SCENERY_FILE), |out| {
        scene.walk(r, |i| {
            let k = Pool::ALL.iter().position(|&p| p == i.pool).unwrap();
            placed[i.model as usize][k] += 1;
            match i.pool {
                Pool::Objects => {
                    let row = Row {
                        id: i.id,
                        model: &scene.models[i.model as usize].name,
                        x: i.pos[0],
                        alt: i.pos[1],
                        z: i.pos[2],
                        rotation: i.rotation,
                    };
                    serde_json::to_writer(&mut *out, &row).map_err(json_err)?;
                    out.write_all(b"\n")?;
                    objects += 1;
                }
                Pool::Lines => lines.add(i.pos[0], i.pos[2]),
                Pool::Trees => trees.add(i.pos[0], i.pos[2]),
            }
            Ok(())
        })
    })?;

    let mut rows: Vec<ModelRow> = scene
        .models
        .iter()
        .zip(&placed)
        .filter(|(_, c)| c.iter().any(|&n| n > 0))
        .map(|(m, c)| {
            let b = m.bbox;
            ModelRow {
                model: &m.name,
                counts: Counts {
                    objects: c[0],
                    lines: c[1],
                    trees: c[2],
                },
                bbox: BoxRow {
                    min_x: b[0],
                    min_y: b[1],
                    min_z: b[2],
                    max_x: b[3],
                    max_y: b[4],
                    max_z: b[5],
                },
            }
        })
        .collect();
    rows.sort_by(|a, b| a.model.cmp(b.model));
    put_file(&dir.join(MODELS_FILE), |out| {
        serde_json::to_writer_pretty(&mut *out, &rows).map_err(json_err)?;
        out.write_all(b"\n")?;
        Ok(())
    })?;

    let water: BTreeMap<(u32, u32), &TileEntry> = water
        .iter()
        .filter(|e| e.layer == "water")
        .map(|e| ((e.tx, e.tz), e))
        .collect();
    let total = |k: usize| placed.iter().map(|c| c[k]).sum::<u64>();
    let mut tiles = trees.write("trees", &water, dir)?;
    tiles.extend(lines.write("lines", &water, dir)?);
    Ok(Written {
        objects,
        lines: total(1),
        trees: total(2),
        models: rows.len(),
        tiles,
    })
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use serde_json::Value;

    use super::super::tests::scene;
    use super::*;
    use crate::scratch::Scratch;

    /// One tile of 4 × 4 cells of 100 m, its origin under the built scene's
    /// first node.
    fn grid() -> Grid {
        Grid {
            cell_size: 100,
            origin_x: -150_000.0,
            origin_z: 449_900.0,
            height: 4,
            width: 4,
            tile_size: 4,
        }
    }

    /// A water tile at (0, 0), land but for the cells given as nodata.
    fn water(dir: &Path, outside: &[usize]) -> Vec<TileEntry> {
        let mut w = vec![0u8; 16];
        for &c in outside {
            w[c] = 255;
        }
        let e = tile_entry("water", 0, 0, w.iter().map(|&v| i64::from(v)));
        put_tile(dir, &e.path, &w).unwrap();
        vec![e]
    }

    fn run(built: &super::super::build::Scene, outside: &[usize]) -> (Scratch, Written) {
        let s = Scratch::new();
        fs::create_dir_all(&s.0).unwrap();
        let w = water(&s.0, outside);
        let b = built.file();
        let mut r = Cursor::new(&b[..]);
        let scene = Scene::open(&mut r, "X.scn5").unwrap();
        let written = write(&scene, &mut r, &grid(), &w, &s.0).unwrap();
        (s, written)
    }

    fn tile(dir: &Path, layer: &str) -> Option<Vec<u16>> {
        let b = fs::read(dir.join(format!("tiles/{layer}/0_0.bin"))).ok()?;
        Some(
            b.as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_le_bytes(*c))
                .collect(),
        )
    }

    #[test]
    fn writes_each_object_and_group_record_as_a_row() {
        let (s, w) = run(&scene(9), &[]);
        assert_eq!((w.objects, w.lines, w.trees), (4, 1, 2));
        let text = fs::read_to_string(s.0.join(SCENERY_FILE)).unwrap();
        let rows: Vec<Value> = text
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        let ids: Vec<u64> = rows.iter().map(|r| r["id"].as_u64().unwrap()).collect();
        assert_eq!(ids, [0, 1 << 13 | 1, 2 << 13, (2 << 13) + 1]);
        let r = &rows[0];
        assert_eq!(r["model"], "sklad_new");
        assert_eq!(
            (r["x"].as_f64(), r["alt"].as_f64(), r["z"].as_f64()),
            (Some(-149_989.5), Some(22.25), Some(449_997.0))
        );
        assert_eq!(r["rotation"], 0.0);
        let keys: Vec<&String> = r.as_object().unwrap().keys().collect();
        assert_eq!(keys.len(), 6);
        assert_eq!(rows[2]["model"], "wire");
        assert!(!s.0.join(format!("{SCENERY_FILE}.tmp")).exists());
    }

    #[test]
    fn lists_every_placed_model_with_its_counts_and_box() {
        let (s, w) = run(&scene(8), &[]);
        let rows: Value =
            serde_json::from_str(&fs::read_to_string(s.0.join(MODELS_FILE)).unwrap()).unwrap();
        let rows = rows.as_array().unwrap();
        assert_eq!(w.models, 3);
        let names: Vec<&str> = rows.iter().map(|r| r["model"].as_str().unwrap()).collect();
        assert_eq!(names, ["oak_1", "sklad_new", "wire"]);
        let sklad = &rows[1];
        assert_eq!(
            sklad["counts"],
            serde_json::json!({"objects": 3, "lines": 0, "trees": 0})
        );
        assert_eq!(
            sklad["box"],
            serde_json::json!({"min_x": -5.0, "min_y": 0.0, "min_z": -10.0, "max_x": 5.0, "max_y": 8.0, "max_z": 10.0})
        );
        assert_eq!(rows[0]["counts"]["trees"], 2);
        assert_eq!(rows[0]["box"]["max_y"], 0.0);
        assert_eq!(
            rows[2]["counts"],
            serde_json::json!({"objects": 1, "lines": 1, "trees": 0})
        );
    }

    #[test]
    fn omits_a_model_no_pool_places() {
        let mut built = scene(9);
        built.models.push(("unused", [1.0; 6]));
        let (s, w) = run(&built, &[]);
        assert_eq!(w.models, 3);
        assert!(
            !fs::read_to_string(s.0.join(MODELS_FILE))
                .unwrap()
                .contains("unused")
        );
    }

    #[test]
    fn counts_trees_and_lines_into_their_cells() {
        // From the grid's origin, trees at (20, 104) and (250, 350), the line
        // at (30, 105).
        let mut built = scene(9);
        built.nodes[1].t = [-150_000.0, 0.0, 449_900.0];
        built.nodes[1].p[0] = [250.0, 30.0, 350.0];
        let (s, w) = run(&built, &[]);
        let trees = tile(&s.0, "trees").unwrap();
        let mut want = vec![0u16; 16];
        want[1] = 1;
        want[2 * 4 + 3] = 1;
        assert_eq!(trees, want);
        let lines = tile(&s.0, "lines").unwrap();
        assert_eq!(lines.iter().map(|&c| u32::from(c)).sum::<u32>(), 1);
        assert_eq!(lines[1], 1);
        let layers: Vec<&str> = w.tiles.iter().map(|t| t.layer.as_str()).collect();
        assert_eq!(layers, ["trees", "lines"]);
        assert_eq!((w.tiles[0].min, w.tiles[0].max), (Some(0), Some(1)));
    }

    #[test]
    fn a_cell_outside_the_mesh_is_nodata_and_drops_its_count() {
        let mut built = scene(9);
        built.nodes[1].t = [-150_000.0, 0.0, 449_900.0];
        built.nodes[1].p[0] = [250.0, 30.0, 350.0];
        let (s, _) = run(&built, &[2 * 4 + 3, 15]);
        let trees = tile(&s.0, "trees").unwrap();
        assert_eq!(
            (trees[1], trees[2 * 4 + 3], trees[15], trees[0]),
            (1, 65535, 65535, 0)
        );
    }

    #[test]
    fn writes_no_tile_whose_covered_cells_are_all_zero() {
        // The line and one tree fall in a cell outside the mesh, the other
        // tree outside the grid: every covered cell counts zero.
        let mut built = scene(9);
        built.nodes[1].t = [0.0; 3];
        let (s, w) = run(&built, &[1]);
        assert!(tile(&s.0, "lines").is_none());
        assert!(tile(&s.0, "trees").is_none());
        assert!(w.tiles.is_empty());
        assert_eq!((w.lines, w.trees), (1, 2));
    }

    #[test]
    fn a_count_saturates_below_nodata() {
        let mut l = CountLayer::new(&grid());
        for _ in 0..70_000 {
            l.add(-149_950.0, 449_950.0);
        }
        assert_eq!(l.tiles[&(0, 0)][0], COUNT_MAX);
        l.add(-160_000.0, 449_950.0);
        l.add(-149_950.0, 449_900.0 + 400.0);
        assert_eq!(l.tiles.len(), 1);
    }
}
