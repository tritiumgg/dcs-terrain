//! The base grids, `water`, `height` and `surface`, re-tiled from the
//! extract's tiles to the pack's and, at a coarser cell size, resampled.
//!
//! A tile is left out as the extract leaves it out, judged on the packed
//! `water` tile: with no cell on the mesh no layer is stored, and an all-sea
//! one stands for its `height` and `surface`.

use rusqlite::Connection;
use serde_json::{Value, json};

use super::{Error, GridRow, TILE_SIZE, Tile, write_grid};
use crate::extract::{CELL_SIZE_M, Dtype, Extract, Failure, Layer, MANIFEST, layer};
use crate::grid::{ExtractTiles, Tiles, Window};

/// `water` first: its tiles decide which tiles of the others are stored.
pub const BASE: [&str; 3] = ["water", "height", "surface"];

const WATER_SEA: i32 = 2;
const HEIGHT_SEA: i32 = 0;
const SURFACE_WATER: i32 = 3;

/// Cells that a grid of `n` extract cells spans at `cell_size`: the same
/// ground from the same origin, the last cell overhanging where the count is
/// odd.
pub fn cells(n: u32, cell_size: u32) -> u32 {
    (n * CELL_SIZE_M).div_ceil(cell_size)
}

/// What a packed `water` tile says about the tiles of the same place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// Every cell is off the mesh: nothing is stored.
    Off,
    /// Every cell is sea: only `water` is stored.
    Sea,
    Mixed,
}

/// Writes the base grids, each in its own transaction, and returns how many.
pub fn write_base(conn: &mut Connection, x: &Extract, cell_size: u32) -> Result<usize, Error> {
    let src = ExtractTiles::new(x);
    let g = x.manifest.grid;
    let f = cell_size / CELL_SIZE_M;
    let (rows, cols) = (cells(g.height, cell_size), cells(g.width, cell_size));
    let (tiles_x, tiles_z) = (rows.div_ceil(TILE_SIZE), cols.div_ceil(TILE_SIZE));
    let omit_sea = x.manifest.omit_sea_tiles;
    // A band of tile rows read as one window, as tall as an extract tile
    // where it can be, so each extract tile is read once.
    let band = (g.tile_size / (TILE_SIZE * f)).max(1);
    let mut kinds = vec![Kind::Mixed; (tiles_x * tiles_z) as usize];

    for name in BASE {
        let layer = layer(name).expect("a base layer is a format layer");
        let unit = x
            .manifest
            .layers
            .get(name)
            .map(|l| l.unit.clone())
            .ok_or_else(|| {
                Error::Extract(vec![Failure::new(MANIFEST, format!("layers lacks {name}"))])
            })?;
        let row = GridRow {
            name: name.into(),
            cell_size: f64::from(cell_size),
            origin_x: g.origin_x,
            origin_z: g.origin_z,
            width: cols,
            height: rows,
            dtype: layer.dtype.name().into(),
            tile_size: TILE_SIZE,
            nodata: layer.nodata.to_string(),
            unit,
            derived_from: None,
            params: params(layer, f),
        };
        let kinds = &mut kinds;
        let src = &src;
        let tiles = (0..tiles_x).step_by(band as usize).flat_map(move |tx0| {
            let n = band.min(tiles_x - tx0);
            match resample_band(src, layer, f, tx0, n, tiles_z) {
                Err(e) => vec![Err(Error::Extract(vec![e]))],
                Ok(band) => band
                    .into_iter()
                    .filter(|(tx, tz, s)| {
                        let k = &mut kinds[(tx * tiles_z + tz) as usize];
                        if layer.name == "water" {
                            *k = kind(s);
                        }
                        is_stored(*k, layer, omit_sea, s)
                    })
                    .map(|(tx, tz, s)| Ok(encode(layer, tx, tz, &s)))
                    .collect(),
            }
        });
        write_grid(conn, &row, tiles)?;
    }
    Ok(BASE.len())
}

/// How a grid was made from the extract's 50 m cells, empty when it is
/// those cells.
fn params(layer: Layer, f: u32) -> Value {
    if f == 1 {
        return json!({});
    }
    let rule = match layer.name {
        "height" => "mean",
        _ => "largest",
    };
    json!({"resampled_from_m": CELL_SIZE_M, "rule": rule})
}

/// Every tile of `n` tile rows from `tx0`, as `(tx, tz, samples)` with
/// columns fastest.
fn resample_band(
    src: &impl Tiles,
    layer: Layer,
    f: u32,
    tx0: u32,
    n: u32,
    tiles_z: u32,
) -> Result<Vec<(u32, u32, Vec<i32>)>, Failure> {
    let ts = TILE_SIZE as usize;
    let f = f as usize;
    let row0 = i64::from(tx0 * TILE_SIZE) * f as i64;
    let w = Window::read(
        src,
        layer,
        row0,
        0,
        n * TILE_SIZE * f as u32,
        tiles_z * TILE_SIZE * f as u32,
    )?;
    let at = |r: usize, c: usize| w.data[r * w.cols as usize + c];
    let mut out = Vec::with_capacity((n * tiles_z) as usize);
    for dtx in 0..n as usize {
        for tz in 0..tiles_z as usize {
            let s = (0..ts * ts)
                .map(|i| {
                    // The tile's cell, in cells of the source window.
                    let r = (dtx * ts + i / ts) * f;
                    let c = (tz * ts + i % ts) * f;
                    reduce(layer, (0..f * f).map(|j| at(r + j / f, c + j % f)))
                })
                .collect();
            out.push((tx0 + dtx as u32, tz as u32, s));
        }
    }
    Ok(out)
}

/// One packed cell from the extract cells it covers: nodata where none is on
/// the mesh; else `height` their mean, rounded half up as the extract rounds,
/// and `water` and `surface` the largest code, which is the precedence each
/// is decided by at a cell (ADR 0048). Over one cell each is that cell.
fn reduce(layer: Layer, vals: impl Iterator<Item = i32>) -> i32 {
    let covered = vals.filter(|&v| v != layer.nodata);
    if layer.name != "height" {
        return covered.max().unwrap_or(layer.nodata);
    }
    let (sum, n) = covered.fold((0i64, 0i64), |(s, n), v| (s + i64::from(v), n + 1));
    if n == 0 {
        return layer.nodata;
    }
    // floor(sum / n + 0.5), exactly.
    (2 * sum + n).div_euclid(2 * n) as i32
}

/// Whether a layer's tile is stored, given what its `water` tile is.
fn is_stored(k: Kind, layer: Layer, omit_sea: bool, s: &[i32]) -> bool {
    match k {
        Kind::Off => false,
        // Only where the reader rebuilds it exactly: a coarse cell is sea
        // with land in it.
        Kind::Sea => layer.name == "water" || !omit_sea || s.iter().any(|&v| v != sea(layer)),
        Kind::Mixed => true,
    }
}

/// What a reader puts in a sea tile's place.
fn sea(layer: Layer) -> i32 {
    match layer.name {
        "height" => HEIGHT_SEA,
        _ => SURFACE_WATER,
    }
}

fn kind(water: &[i32]) -> Kind {
    let nodata = layer("water").expect("a format layer").nodata;
    if water.iter().all(|&v| v == nodata) {
        Kind::Off
    } else if water.iter().all(|&v| v == WATER_SEA) {
        Kind::Sea
    } else {
        Kind::Mixed
    }
}

fn encode(layer: Layer, tx: u32, tz: u32, s: &[i32]) -> Tile {
    let mut data = Vec::with_capacity(s.len() * layer.dtype.bytes() as usize);
    for &v in s {
        match layer.dtype {
            Dtype::U8 => data.push(v as u8),
            Dtype::I16 => data.extend_from_slice(&(v as i16).to_le_bytes()),
            Dtype::U16 => data.extend_from_slice(&(v as u16).to_le_bytes()),
        }
    }
    let present = || s.iter().copied().filter(|&v| v != layer.nodata);
    Tile {
        tx,
        tz,
        data,
        min: present().min().map(f64::from),
        max: present().max().map(f64::from),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::{Grid, Samples, check};
    use crate::pack::{Options, pack};
    use crate::scratch::Scratch;
    use crate::synth;
    use rusqlite::{OpenFlags, params};
    use std::fs;
    use std::path::{Path, PathBuf};

    fn l(name: &str) -> Layer {
        layer(name).unwrap()
    }

    fn theatre() -> synth::Theatre {
        synth::Theatre::new(synth::DEFAULT_SEED, synth::DEFAULT_SIZE_KM, CELL_SIZE_M).unwrap()
    }

    fn extract(s: &Scratch) -> PathBuf {
        let dir = s.0.join("extract");
        synth::write(&theatre(), &dir).unwrap();
        dir
    }

    fn packed(s: &Scratch, dir: &Path, cell_size: u32) -> PathBuf {
        let out = s.0.join(format!("{cell_size}.sqlite"));
        let opts = Options {
            cell_size,
            ..Options::default()
        };
        pack(dir, Some(&out), &opts).unwrap();
        out
    }

    /// A packed file's tiles, read as the grid module reads an extract's, so
    /// an absent tile is rebuilt by the same rules.
    struct Packed {
        conn: Connection,
        grid: Grid,
    }

    impl Packed {
        fn open(path: &Path) -> Packed {
            let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
            let grid = conn
                .query_row(
                    "SELECT cell_size, origin_x, origin_z, height, width, tile_size
                     FROM grid WHERE name = 'height'",
                    [],
                    |r| {
                        Ok(Grid {
                            cell_size: r.get::<_, f64>(0)? as u32,
                            origin_x: r.get(1)?,
                            origin_z: r.get(2)?,
                            height: r.get(3)?,
                            width: r.get(4)?,
                            tile_size: r.get(5)?,
                        })
                    },
                )
                .unwrap();
            Packed { conn, grid }
        }

        fn keys(&self, name: &str) -> Vec<(u32, u32)> {
            self.conn
                .prepare("SELECT tx, tz FROM grid_tile WHERE name = ?1 ORDER BY tx, tz")
                .unwrap()
                .query_map([name], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap()
        }
    }

    impl Tiles for Packed {
        fn grid(&self) -> &Grid {
            &self.grid
        }

        fn stored(&self, layer: Layer, tx: u32, tz: u32) -> Result<Option<Samples>, Failure> {
            let blob = self.conn.query_row(
                "SELECT data FROM grid_tile WHERE name = ?1 AND tx = ?2 AND tz = ?3",
                params![layer.name, tx, tz],
                |r| r.get::<_, Vec<u8>>(0),
            );
            match blob {
                Ok(b) => Ok(Some(Samples::decode(layer.dtype, &b))),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => panic!("{e}"),
            }
        }
    }

    /// A closed-form cell of a layer, nodata off the theatre.
    fn truth(t: &synth::Theatre, layer: Layer, row: u32, col: u32) -> i32 {
        if row >= t.cells() || col >= t.cells() {
            return layer.nodata;
        }
        let c = t.cell(row, col);
        match layer.name {
            "height" => i32::from(c.height),
            "water" => i32::from(c.water),
            _ => i32::from(c.surface),
        }
    }

    /// Every cell of every base grid, the last tiles' padding included,
    /// against `want`.
    fn assert_grid(p: &Packed, want: impl Fn(Layer, u32, u32) -> i32) {
        let (tiles_x, tiles_z) = p.grid.tiles();
        let ts = TILE_SIZE;
        for name in BASE {
            let layer = l(name);
            // Each tile whole, so the padding past the grid is checked too.
            for tx in 0..tiles_x {
                for tz in 0..tiles_z {
                    let t = crate::grid::tile(p, layer, tx, tz).unwrap();
                    for i in 0..(ts * ts) as usize {
                        let (r, c) = (tx * ts + i as u32 / ts, tz * ts + i as u32 % ts);
                        let on = r < p.grid.height && c < p.grid.width;
                        let want = if on { want(layer, r, c) } else { layer.nodata };
                        assert_eq!(t.get(i), want, "{name} ({r}, {c})");
                    }
                }
            }
        }
    }

    /// `height` and `surface` are stored exactly where `water` is not all
    /// sea; every value was checked against the closed form already.
    fn assert_sea_omitted(p: &Packed) {
        let water = p.keys("water");
        let all_sea = |&(tx, tz): &(u32, u32)| {
            let s = p.stored(l("water"), tx, tz).unwrap().unwrap();
            s.iter().all(|v| v == WATER_SEA)
        };
        let land: Vec<_> = water.iter().copied().filter(|k| !all_sea(k)).collect();
        assert!(land.len() < water.len(), "no tile is all sea");
        for name in ["height", "surface"] {
            assert_eq!(p.keys(name), land, "{name}");
        }
    }

    fn assert_min_max(p: &Packed) {
        let mut stmt = p
            .conn
            .prepare("SELECT name, tx, tz, data, min_val, max_val FROM grid_tile")
            .unwrap();
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, u32>(1)?,
                    r.get::<_, u32>(2)?,
                    r.get::<_, Vec<u8>>(3)?,
                    r.get::<_, Option<f64>>(4)?,
                    r.get::<_, Option<f64>>(5)?,
                ))
            })
            .unwrap();
        for row in rows {
            let (name, tx, tz, data, min, max) = row.unwrap();
            let layer = l(&name);
            let s = Samples::decode(layer.dtype, &data);
            assert_eq!(s.len(), (TILE_SIZE * TILE_SIZE) as usize);
            let present = || s.iter().filter(|&v| v != layer.nodata).map(f64::from);
            let at = format!("{name} ({tx}, {tz})");
            assert_eq!(min, present().reduce(f64::min), "{at}");
            assert_eq!(max, present().reduce(f64::max), "{at}");
            assert!(min.is_some(), "{at} is stored with nothing on the mesh");
        }
    }

    #[test]
    fn at_50_m_every_cell_is_the_extracts() {
        let s = Scratch::new();
        let dir = extract(&s);
        let p = Packed::open(&packed(&s, &dir, 50));
        let t = theatre();
        assert_eq!((p.grid.height, p.grid.width), (t.cells(), t.cells()));
        assert_eq!(p.grid.tiles(), (22, 22));
        assert_grid(&p, |layer, r, c| truth(&t, layer, r, c));
        assert_min_max(&p);
        assert_sea_omitted(&p);

        // No tile is off the mesh, so every water tile is stored. The
        // extract's all-sea tile (1, 1) is 4 by 4 of these, and they hold no
        // height or surface.
        let all: Vec<(u32, u32)> = (0..22).flat_map(|x| (0..22).map(move |z| (x, z))).collect();
        assert_eq!(p.keys("water"), all);
        let sea = |&(x, z): &(u32, u32)| (4..8).contains(&x) && (4..8).contains(&z);
        for name in ["height", "surface"] {
            let kept = p.keys(name);
            assert!(!kept.iter().any(sea), "{name}");
        }
    }

    #[test]
    fn at_100_m_a_cell_is_the_four_it_covers() {
        let s = Scratch::new();
        let dir = extract(&s);
        let p = Packed::open(&packed(&s, &dir, 100));
        let t = theatre();
        assert_eq!((p.grid.height, p.grid.width), (700, 700));
        assert_eq!(p.grid.cell_size, 100);
        assert_eq!(p.grid.tiles(), (11, 11));
        assert_grid(&p, |layer, r, c| {
            let four: Vec<i32> = (0..4)
                .map(|j| truth(&t, layer, 2 * r + j / 2, 2 * c + j % 2))
                .filter(|&v| v != layer.nodata)
                .collect();
            match (layer.name, four.len()) {
                (_, 0) => layer.nodata,
                ("height", n) => {
                    let mean = f64::from(four.iter().sum::<i32>()) / n as f64;
                    (mean + 0.5).floor() as i32
                }
                _ => *four.iter().max().unwrap(),
            }
        });
        assert_min_max(&p);
        assert_sea_omitted(&p);
        // The extract's all-sea tile is 2 by 2 of these.
        assert_eq!(p.keys("water").len(), 121);
    }

    #[test]
    fn a_tile_off_the_mesh_is_stored_in_no_grid() {
        let s = Scratch::new();
        let dir = extract(&s);
        // The synthetic extract has none, so tile (2, 2) is taken out of its
        // manifest and off the disk.
        let path = dir.join(MANIFEST);
        let mut m: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        m["tiles"].as_array_mut().unwrap().retain(|t| {
            let here = t["tx"] == 2 && t["tz"] == 2;
            if here {
                fs::remove_file(dir.join(t["path"].as_str().unwrap())).unwrap();
            }
            !here
        });
        fs::write(&path, m.to_string()).unwrap();
        check(&dir).unwrap();

        for (cell_size, off) in [(50, 8..12), (100, 4..6)] {
            let p = Packed::open(&packed(&s, &dir, cell_size));
            for name in BASE {
                let kept = p.keys(name);
                let gone = |&(x, z): &(u32, u32)| off.contains(&x) && off.contains(&z);
                assert_eq!(kept.iter().filter(|k| gone(k)).count(), 0, "{name}");
                assert!(kept.contains(&(off.start - 1, off.start)), "{name}");
                let t = crate::grid::tile(&p, l(name), off.start, off.start).unwrap();
                assert!(t.iter().all(|v| v == l(name).nodata), "{name}");
            }
        }
    }

    #[test]
    fn a_sea_tile_is_left_out_only_where_the_reader_rebuilds_it() {
        let (h, w, s) = (l("height"), l("water"), l("surface"));
        assert_eq!(kind(&[2, 2, 2, 2]), Kind::Sea);
        assert_eq!(kind(&[255, 255, 255, 255]), Kind::Off);
        assert_eq!(kind(&[2, 2, 2, 255]), Kind::Mixed);
        assert_eq!(kind(&[2, 0, 2, 2]), Kind::Mixed);
        for omit in [true, false] {
            assert!(is_stored(Kind::Sea, w, omit, &[2; 4]));
            assert!(!is_stored(Kind::Off, w, omit, &[255; 4]));
            assert!(is_stored(Kind::Mixed, h, omit, &[0; 4]));
            // A coarse cell that is sea with land among its four keeps its
            // mean height.
            assert!(is_stored(Kind::Sea, h, omit, &[0, 0, 3, 0]));
            assert!(is_stored(Kind::Sea, s, omit, &[3, 3, 1, 3]));
        }
        assert!(!is_stored(Kind::Sea, h, true, &[0; 4]));
        assert!(!is_stored(Kind::Sea, s, true, &[3; 4]));
        assert!(is_stored(Kind::Sea, h, false, &[0; 4]));
        assert!(is_stored(Kind::Sea, s, false, &[3; 4]));
    }

    #[test]
    fn reduces_cells_by_mean_and_by_largest_code() {
        let (h, w, s) = (l("height"), l("water"), l("surface"));
        let r = |layer, v: &[i32]| reduce(layer, v.iter().copied());
        assert_eq!(r(h, &[7]), 7);
        assert_eq!(r(h, &[-7]), -7);
        assert_eq!(r(h, &[1, 2]), 2);
        assert_eq!(r(h, &[-1, -2]), -1);
        assert_eq!(r(h, &[10, 11, 11, 11]), 11);
        assert_eq!(r(h, &[10, 10, 10, 11]), 10);
        assert_eq!(r(h, &[-32768, 4, -32768, 6]), 5);
        assert_eq!(r(h, &[-32768; 4]), -32768);
        assert_eq!(r(h, &[32767; 4]), 32767);
        assert_eq!(r(w, &[0, 1, 2, 255]), 2);
        assert_eq!(r(w, &[0, 3, 2, 1]), 3);
        assert_eq!(r(w, &[255, 0, 255, 255]), 0);
        assert_eq!(r(w, &[255; 4]), 255);
        assert_eq!(r(s, &[1, 3, 4, 0]), 4);
        assert_eq!(r(s, &[0; 4]), 0);
        assert_eq!(r(s, &[5, 1, 1, 1]), 5);
    }
}
