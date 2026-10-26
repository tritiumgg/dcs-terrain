//! Builds a packed file from an extract: one SQLite file holding `meta`, the
//! grids and the tables a query reads.
//!
//! The file is written beside its destination under a `.partial` name and
//! renamed over it only once `VACUUM` has finished, so an interrupted pack
//! leaves the previous file, or nothing, rather than half a file.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OpenFlags, params};
use serde_json::Value;

use crate::extract::{self, Extract, Failure};

mod check;
mod describe;
pub mod grids;
pub mod meta;

pub use check::{Report, check};
pub use describe::describe;

/// The packed file's own format, independent of the extract's.
pub const FORMAT_VERSION: u32 = 1;
/// The base cell sizes a pack may ask for (ADR 0036).
pub const CELL_SIZES: [u32; 2] = [50, 100];
pub const DEFAULT_CELL_SIZE: u32 = 50;
/// Every grid is re-tiled to this; 64 keeps a 10 km sightline to a handful of
/// 8 KiB blobs.
pub const TILE_SIZE: u32 = 64;

/// Set before the first table exists, which is the only time SQLite takes it.
const PAGE_SIZE: u32 = 8192;

const SCHEMA: &str = "
CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT);
CREATE TABLE grid(name TEXT PRIMARY KEY, cell_size REAL, origin_x REAL,
  origin_z REAL, width INT, height INT, dtype TEXT, tile_size INT,
  nodata TEXT, unit TEXT, derived_from TEXT, params TEXT);
CREATE TABLE grid_tile(name TEXT, tx INT, tz INT, data BLOB,
  min_val REAL, max_val REAL, PRIMARY KEY(name, tx, tz)) WITHOUT ROWID;
";

/// What a pack is asked for beyond the extract.
#[derive(Debug, Clone)]
pub struct Options {
    pub cell_size: u32,
    /// Replace an existing file whose `meta` differs.
    pub force: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            cell_size: DEFAULT_CELL_SIZE,
            force: false,
        }
    }
}

/// What a finished pack wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packed {
    pub path: PathBuf,
    pub theatre: String,
    pub dcs_build: String,
    pub cell_size: u32,
    pub grids: usize,
}

#[derive(Debug)]
pub enum Error {
    /// The extract fails its own check; every failure is listed.
    Extract(Vec<Failure>),
    CellSize(u32),
    /// A file is already there and its `meta` differs in these keys.
    Exists {
        path: PathBuf,
        differs: Vec<String>,
    },
    Sqlite(rusqlite::Error),
    Io(io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Extract(failures) => {
                write!(
                    f,
                    "the extract fails its check: {} problems",
                    failures.len()
                )
            }
            Error::CellSize(c) => write!(f, "cell size {c} m is not one of 50 or 100"),
            Error::Exists { path, differs } => write!(
                f,
                "{} exists and its meta differs in {}; pass --force to replace it",
                path.display(),
                differs.join(", ")
            ),
            Error::Sqlite(e) => write!(f, "{e}"),
            Error::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Error::Sqlite(e)
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

/// The name `pack` gives its output when none is asked for (ADR 0023).
pub fn default_name(theatre: &str, dcs_build: &str, cell_size: u32) -> String {
    format!("{theatre}-{dcs_build}-{cell_size}m.sqlite")
}

/// Packs the extract at `dir` into `out`, or into the default name in the
/// current directory.
pub fn pack(dir: &Path, out: Option<&Path>, opts: &Options) -> Result<Packed, Error> {
    if !CELL_SIZES.contains(&opts.cell_size) {
        return Err(Error::CellSize(opts.cell_size));
    }
    extract::check(dir).map_err(Error::Extract)?;
    let x = Extract::open(dir).map_err(|f| Error::Extract(vec![f]))?;
    let config = read_config(&x).map_err(|f| Error::Extract(vec![f]))?;
    let m = &x.manifest;
    let rows = meta::from_extract(&x, &config, opts.cell_size, &utc_now())
        .map_err(|why| Error::Extract(vec![Failure::new("config.json", why)]))?;

    let path = match out {
        Some(p) => p.to_path_buf(),
        None => PathBuf::from(default_name(&m.theatre, &m.dcs_build, opts.cell_size)),
    };
    if path.is_dir() {
        let why = format!("{} is a directory, not a file to write", path.display());
        return Err(Error::Io(io::Error::other(why)));
    }
    if path.exists() && !opts.force {
        let differs = differs_from(&path, &rows);
        if !differs.is_empty() {
            return Err(Error::Exists { path, differs });
        }
    }

    let mut partial = path.clone().into_os_string();
    partial.push(".partial");
    let partial = PathBuf::from(partial);
    // A crashed pack may have left one.
    let _ = fs::remove_file(&partial);
    let written = write(&partial, &rows, &x, opts.cell_size)
        .and_then(|grids| Ok(fs::rename(&partial, &path).map(|()| grids)?));
    let grids = match written {
        Ok(n) => n,
        Err(e) => {
            let _ = fs::remove_file(&partial);
            return Err(e);
        }
    };
    Ok(Packed {
        path,
        theatre: m.theatre.clone(),
        dcs_build: m.dcs_build.clone(),
        cell_size: opts.cell_size,
        grids,
    })
}

fn write(
    path: &Path,
    rows: &[(&'static str, Option<String>)],
    x: &Extract,
    cell_size: u32,
) -> Result<usize, Error> {
    let mut conn = Connection::open(path)?;
    // Nothing reads the file until the rename, so a crash costs only the
    // partial file, and a journal would protect nothing.
    conn.execute_batch(&format!(
        "PRAGMA page_size = {PAGE_SIZE}; PRAGMA journal_mode = OFF; PRAGMA synchronous = OFF;"
    ))?;
    conn.execute_batch(SCHEMA)?;
    let tx = conn.transaction()?;
    {
        let mut insert = tx.prepare("INSERT INTO meta(key, value) VALUES (?1, ?2)")?;
        for (k, v) in rows {
            insert.execute(params![k, v])?;
        }
    }
    tx.commit()?;
    let grids = grids::write_base(&mut conn, x, cell_size)?;
    // Lays every tile's overflow pages out contiguously, so a window read is
    // a short sequential run.
    conn.execute_batch("VACUUM")?;
    conn.close().map_err(|(_, e)| e)?;
    Ok(grids)
}

/// One `grid` row.
#[derive(Debug, Clone, PartialEq)]
pub struct GridRow {
    pub name: String,
    pub cell_size: f64,
    pub origin_x: f64,
    pub origin_z: f64,
    pub width: u32,
    pub height: u32,
    pub dtype: String,
    pub tile_size: u32,
    pub nodata: String,
    pub unit: String,
    /// The grid this one is computed from at query time, with no tiles.
    pub derived_from: Option<String>,
    /// The parameters that produced the grid, as a JSON object.
    pub params: Value,
}

/// One stored tile: raw little-endian samples, row-major.
#[derive(Debug, Clone, PartialEq)]
pub struct Tile {
    pub tx: u32,
    pub tz: u32,
    pub data: Vec<u8>,
    /// Over the samples that are not nodata; `None` when every one is.
    pub min: Option<f64>,
    pub max: Option<f64>,
}

/// Writes a grid's row and every tile in one transaction, the tiles made as
/// they are written. With no journal a failed transaction cannot be relied
/// on to roll back, so a failure here, in SQLite or in making a tile,
/// abandons the whole pack, whose partial file is then removed.
pub fn write_grid<E: From<rusqlite::Error>>(
    conn: &mut Connection,
    row: &GridRow,
    tiles: impl IntoIterator<Item = Result<Tile, E>>,
) -> Result<usize, E> {
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO grid(name, cell_size, origin_x, origin_z, width, height, dtype,
           tile_size, nodata, unit, derived_from, params)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            row.name,
            row.cell_size,
            row.origin_x,
            row.origin_z,
            row.width,
            row.height,
            row.dtype,
            row.tile_size,
            row.nodata,
            row.unit,
            row.derived_from,
            row.params.to_string(),
        ],
    )?;
    let mut n = 0;
    {
        let mut insert = tx.prepare(
            "INSERT INTO grid_tile(name, tx, tz, data, min_val, max_val)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;
        for t in tiles {
            let t = t?;
            insert.execute(params![row.name, t.tx, t.tz, t.data, t.min, t.max])?;
            n += 1;
        }
    }
    tx.commit()?;
    Ok(n)
}

fn read_config(x: &Extract) -> Result<Value, Failure> {
    let name = x
        .manifest
        .tables
        .get("config")
        .ok_or_else(|| Failure::new(extract::MANIFEST, "tables lacks config"))?;
    let text = fs::read_to_string(x.dir.join(name))
        .map_err(|e| Failure::new(name, format!("cannot read: {e}")))?;
    serde_json::from_str(&text).map_err(|e| Failure::new(name, format!("not JSON: {e}")))
}

/// The `meta` keys in which the file at `path` differs from `rows`; every key
/// when it is not a packed file at all. `packed_at` and `dcsterrain_version`
/// are not compared, so repacking the same extract replaces it, while a file
/// stamped with validation figures is kept unless forced.
fn differs_from(path: &Path, rows: &[(&'static str, Option<String>)]) -> Vec<String> {
    let all = || rows.iter().map(|(k, _)| k.to_string()).collect();
    let Ok(old) = read_meta(path) else {
        return all();
    };
    let skip = |k: &str| matches!(k, "packed_at" | "dcsterrain_version");
    let mut differs: Vec<String> = rows
        .iter()
        .filter(|(k, v)| {
            !skip(k) && old.iter().find(|(ok, _)| ok == k).map(|(_, ov)| ov) != Some(v)
        })
        .map(|(k, _)| k.to_string())
        .collect();
    differs.extend(
        old.iter()
            .filter(|(k, _)| !rows.iter().any(|(nk, _)| nk == k))
            .map(|(k, _)| k.clone()),
    );
    differs
}

/// Every `meta` row of a packed file, opened read-only.
pub(crate) fn read_meta(path: &Path) -> rusqlite::Result<Vec<(String, Option<String>)>> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    conn.execute_batch("PRAGMA query_only = 1")?;
    let mut stmt = conn.prepare("SELECT key, value FROM meta")?;
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect()
}

/// Now, as `YYYY-MM-DDTHH:MM:SSZ`.
fn utc_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    utc(secs)
}

fn utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Howard Hinnant's days-to-civil, from a proleptic Gregorian count of
    // days since 1970-01-01.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratch::Scratch;
    use crate::synth;

    fn synth_extract(s: &Scratch) -> PathBuf {
        let dir = s.0.join("extract");
        let t = synth::Theatre::new(synth::DEFAULT_SEED, synth::DEFAULT_SIZE_KM, 50).unwrap();
        synth::write(&t, &dir).unwrap();
        dir
    }

    #[test]
    fn formats_utc() {
        assert_eq!(utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(utc(1_793_017_199), "2026-10-26T12:19:59Z");
    }

    #[test]
    fn writes_the_pragmas_schema_meta_and_base_grids() {
        let s = Scratch::new();
        let dir = synth_extract(&s);
        let out = s.0.join("synth.sqlite");
        let p = pack(&dir, Some(&out), &Options::default()).unwrap();
        assert_eq!(p.path, out);
        assert_eq!(p.theatre, synth::THEATRE);
        assert!(!s.0.join("synth.sqlite.partial").exists());

        let conn = Connection::open_with_flags(&out, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let page: u32 = conn
            .query_row("PRAGMA page_size", [], |r| r.get(0))
            .unwrap();
        assert_eq!(page, PAGE_SIZE);
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_schema WHERE type = 'table' ORDER BY name")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(tables, ["grid", "grid_tile", "meta"]);
        let n: i64 = conn
            .query_row("SELECT count(*) FROM grid", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n as usize, grids::BASE.len());
        assert_eq!(p.grids, grids::BASE.len());
        let n: i64 = conn
            .query_row("SELECT count(*) FROM meta", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n as usize, meta::KEYS.len());
    }

    #[test]
    fn refuses_a_cell_size_other_than_50_or_100() {
        let s = Scratch::new();
        let dir = synth_extract(&s);
        let opts = Options {
            cell_size: 75,
            ..Options::default()
        };
        let e = pack(&dir, Some(&s.0.join("x.sqlite")), &opts).unwrap_err();
        assert!(matches!(e, Error::CellSize(75)), "{e}");
    }

    #[test]
    fn refuses_an_extract_that_fails_its_check() {
        let s = Scratch::new();
        let dir = synth_extract(&s);
        fs::remove_file(dir.join("config.json")).unwrap();
        let out = s.0.join("x.sqlite");
        let e = pack(&dir, Some(&out), &Options::default()).unwrap_err();
        assert!(matches!(e, Error::Extract(_)), "{e}");
        assert!(!out.exists());
    }

    #[test]
    fn replaces_its_own_file_but_not_a_stamped_or_foreign_one() {
        let s = Scratch::new();
        let dir = synth_extract(&s);
        let out = s.0.join("x.sqlite");
        pack(&dir, Some(&out), &Options::default()).unwrap();
        // The same extract again differs only in when it was packed.
        pack(&dir, Some(&out), &Options::default()).unwrap();

        let conn = Connection::open(&out).unwrap();
        conn.execute(
            "UPDATE meta SET value = '1.5' WHERE key = 'validation_height_rmse'",
            [],
        )
        .unwrap();
        drop(conn);
        match pack(&dir, Some(&out), &Options::default()).unwrap_err() {
            Error::Exists { differs, .. } => assert_eq!(differs, ["validation_height_rmse"]),
            e => panic!("{e}"),
        }
        let forced = Options {
            force: true,
            ..Options::default()
        };
        pack(&dir, Some(&out), &forced).unwrap();

        let foreign = s.0.join("notes.txt");
        fs::write(&foreign, "not a database").unwrap();
        assert!(matches!(
            pack(&dir, Some(&foreign), &Options::default()).unwrap_err(),
            Error::Exists { .. }
        ));
        assert_eq!(fs::read_to_string(&foreign).unwrap(), "not a database");
    }

    #[test]
    fn refuses_a_directory_as_the_output_and_leaves_nothing() {
        let s = Scratch::new();
        let dir = synth_extract(&s);
        let out = s.0.join("taken");
        fs::create_dir(&out).unwrap();
        let forced = Options {
            force: true,
            ..Options::default()
        };
        let e = pack(&dir, Some(&out), &forced).unwrap_err();
        assert!(e.to_string().contains("is a directory"), "{e}");
        assert!(!s.0.join("taken.partial").exists());
    }

    #[test]
    fn writes_a_grid_row_and_its_tiles() {
        let s = Scratch::new();
        let dir = synth_extract(&s);
        let out = s.0.join("x.sqlite");
        pack(&dir, Some(&out), &Options::default()).unwrap();
        let mut conn = Connection::open(&out).unwrap();
        let row = GridRow {
            name: "scratch".into(),
            cell_size: 50.0,
            origin_x: 0.0,
            origin_z: 0.0,
            width: 128,
            height: 64,
            dtype: "i16".into(),
            tile_size: TILE_SIZE,
            nodata: "-32768".into(),
            unit: "m".into(),
            derived_from: None,
            params: serde_json::json!({}),
        };
        let tile = |tz| {
            Ok::<_, rusqlite::Error>(Tile {
                tx: 0,
                tz,
                data: vec![0; 64 * 64 * 2],
                min: Some(0.0),
                max: Some(0.0),
            })
        };
        assert_eq!(write_grid(&mut conn, &row, [tile(0), tile(1)]).unwrap(), 2);

        let n: i64 = conn
            .query_row(
                "SELECT count(*) FROM grid_tile WHERE name = 'scratch'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 2);

        // A duplicate tile is an error, which abandons the pack.
        let again = GridRow {
            name: "scratch2".into(),
            ..row
        };
        assert!(write_grid(&mut conn, &again, [tile(0), tile(0)]).is_err());
    }
}
