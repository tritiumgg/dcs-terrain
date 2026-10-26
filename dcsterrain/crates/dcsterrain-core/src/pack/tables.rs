//! The vector tables: airdromes, runways, stands, beacons, radio and the
//! points of interest, loaded from the extract's JSON in one transaction.
//!
//! A null in the extract is NULL here: without the hook a run writes null for
//! the fields only a running DCS knows. A value of the wrong type is a broken
//! writer, and abandons the pack.

use std::fs;

use rusqlite::{Connection, Transaction, params};
use serde_json::{Map, Value};

use super::Error;
use crate::extract::{self, Extract, Failure};

/// Every vector table this module writes, in the order it writes them.
pub const VECTOR: [&str; 6] = ["airdrome", "runway", "stand", "beacon", "radio", "poi"];

// ADR 0049: an airdrome is keyed by `name_id`, which every airdrome has; `id`
// is DCS's own number where a source gave one, and runways and stands carry
// both.
const SCHEMA: &str = "
CREATE TABLE airdrome(id INT UNIQUE, name_id TEXT PRIMARY KEY NOT NULL, code TEXT,
  name TEXT, x REAL, z REAL, lat REAL, lon REAL, civilian INT, abandoned INT,
  class TEXT);
CREATE TABLE runway(airdrome INT, airdrome_name_id TEXT, name TEXT, x1 REAL,
  z1 REAL, x2 REAL, z2 REAL, course REAL, length REAL);
CREATE TABLE stand(airdrome INT, airdrome_name_id TEXT, name TEXT, x REAL,
  z REAL, crossroad_index INT, for_helicopters INT, for_airplanes INT,
  shelter INT, width REAL, length REAL, height REAL);
CREATE TABLE beacon(id TEXT PRIMARY KEY, type INT, callsign TEXT, name TEXT,
  frequency_hz INT, channel INT, x REAL, z REAL, alt REAL, lat REAL, lon REAL);
CREATE TABLE radio(id TEXT PRIMARY KEY, callsign TEXT, roles TEXT,
  hf INT, fm INT, vhf INT, uhf INT);
CREATE TABLE poi(id INTEGER PRIMARY KEY, kind TEXT, name TEXT, x REAL, z REAL);
CREATE VIRTUAL TABLE poi_idx USING rtree(id, minx, maxx, minz, maxz);
";

/// Creates the vector tables and fills them from the extract.
pub fn write(conn: &mut Connection, x: &Extract) -> Result<(), Error> {
    let tx = conn.transaction()?;
    tx.execute_batch(SCHEMA)?;
    airdromes(&tx, x)?;
    runways(&tx, x)?;
    stands(&tx, x)?;
    beacons(&tx, x)?;
    radio(&tx, x)?;
    poi(&tx, x)?;
    tx.commit()?;
    Ok(())
}

fn airdromes(tx: &Transaction, x: &Extract) -> Result<(), Error> {
    let mut insert = tx.prepare(
        "INSERT INTO airdrome(id, name_id, code, name, x, z, lat, lon, civilian,
           abandoned, class) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
    )?;
    for r in rows(x, "airdromes")?.iter() {
        let name_id = r.text("name_id")?.ok_or_else(|| r.bad("name_id", "null"))?;
        insert.execute(params![
            r.int("id")?,
            name_id,
            r.text("code")?,
            r.text("display_name")?,
            r.num("x")?,
            r.num("z")?,
            r.num("lat")?,
            r.num("lon")?,
            r.flag("civilian")?,
            r.flag("abandoned")?,
            r.text("class")?,
        ])?;
    }
    Ok(())
}

fn runways(tx: &Transaction, x: &Extract) -> Result<(), Error> {
    let mut insert = tx.prepare(
        "INSERT INTO runway(airdrome, airdrome_name_id, name, x1, z1, x2, z2, course,
           length) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    )?;
    for r in rows(x, "runways")?.iter() {
        let ends = (
            r.num("edge1_x")?,
            r.num("edge1_z")?,
            r.num("edge2_x")?,
            r.num("edge2_z")?,
        );
        let length = match ends {
            (Some(x1), Some(z1), Some(x2), Some(z2)) => Some((x2 - x1).hypot(z2 - z1)),
            _ => None,
        };
        insert.execute(params![
            r.int("airdrome_id")?,
            r.text("airdrome_name_id")?,
            r.text("name")?,
            ends.0,
            ends.1,
            ends.2,
            ends.3,
            r.num("course")?,
            length,
        ])?;
    }
    Ok(())
}

fn stands(tx: &Transaction, x: &Extract) -> Result<(), Error> {
    let mut insert = tx.prepare(
        "INSERT INTO stand(airdrome, airdrome_name_id, name, x, z, crossroad_index,
           for_helicopters, for_airplanes, shelter, width, length, height)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
    )?;
    for r in rows(x, "stands")?.iter() {
        let p = r.object("params")?;
        insert.execute(params![
            r.int("airdrome_id")?,
            r.text("airdrome_name_id")?,
            r.text("name")?,
            r.num("x")?,
            r.num("z")?,
            r.int("crossroad_index")?,
            p.as_ref()
                .map(|p| p.flag("FOR_HELICOPTERS"))
                .transpose()?
                .flatten(),
            p.as_ref()
                .map(|p| p.flag("FOR_AIRPLANES"))
                .transpose()?
                .flatten(),
            p.as_ref().map(|p| p.flag("SHELTER")).transpose()?.flatten(),
            p.as_ref().map(|p| p.num("WIDTH")).transpose()?.flatten(),
            p.as_ref().map(|p| p.num("LENGTH")).transpose()?.flatten(),
            p.as_ref()
                .map(|p| p.num_text("HEIGHT"))
                .transpose()?
                .flatten(),
        ])?;
    }
    Ok(())
}

fn beacons(tx: &Transaction, x: &Extract) -> Result<(), Error> {
    let mut insert = tx.prepare(
        "INSERT INTO beacon(id, type, callsign, name, frequency_hz, channel, x, z, alt,
           lat, lon) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
    )?;
    for r in rows(x, "beacons")?.iter() {
        insert.execute(params![
            r.text("beacon_id")?,
            r.int("type")?,
            r.text("callsign")?,
            r.text("display_name")?,
            r.int("frequency_hz")?,
            r.int("channel")?,
            r.num("x")?,
            r.num("z")?,
            r.num("alt")?,
            r.num("lat")?,
            r.num("lon")?,
        ])?;
    }
    Ok(())
}

fn radio(tx: &Transaction, x: &Extract) -> Result<(), Error> {
    let mut insert = tx.prepare(
        "INSERT INTO radio(id, callsign, roles, hf, fm, vhf, uhf)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )?;
    for r in rows(x, "radio")?.iter() {
        let callsign = r
            .object("callsigns")?
            .map(|c| c.callsign())
            .transpose()?
            .flatten();
        let roles = match r.get("roles") {
            Value::Null => None,
            v @ Value::Array(_) => Some(v.to_string()),
            _ => return Err(r.bad("roles", "not an array")),
        };
        let f = r.object("frequencies_hz")?;
        let band = |k| {
            f.as_ref()
                .map(|f| f.int(k))
                .transpose()
                .map(Option::flatten)
        };
        insert.execute(params![
            r.text("radio_id")?,
            callsign,
            roles,
            band("hf")?,
            band("fm")?,
            band("vhf")?,
            band("uhf")?,
        ])?;
    }
    Ok(())
}

/// Towns as `town`, then two rows per node, its red side and its blue, each
/// with the node's name; the R-tree holds every row as a point.
fn poi(tx: &Transaction, x: &Extract) -> Result<(), Error> {
    let mut insert = tx.prepare("INSERT INTO poi(kind, name, x, z) VALUES (?1, ?2, ?3, ?4)")?;
    for r in rows(x, "towns")?.iter() {
        let (px, pz) = r.position()?;
        insert.execute(params!["town", r.text("name")?, px, pz])?;
    }
    for r in rows(x, "nodes")?.iter() {
        let name = r.text("name")?;
        for (side, kind) in [("red", "node_red"), ("blue", "node_blue")] {
            let p = r.object(side)?.ok_or_else(|| r.bad(side, "null"))?;
            let (px, pz) = p.position()?;
            insert.execute(params![kind, name, px, pz])?;
        }
    }
    tx.execute(
        "INSERT INTO poi_idx(id, minx, maxx, minz, maxz) SELECT id, x, x, z, z FROM poi",
        [],
    )?;
    Ok(())
}

/// The rows of one of the extract's array tables.
struct Rows {
    file: String,
    rows: Vec<Value>,
}

impl Rows {
    fn iter(&self) -> impl Iterator<Item = Row<'_>> {
        self.rows.iter().zip(1..).map(|(v, n)| Row {
            file: &self.file,
            at: format!("row {n}"),
            // The extract's check has seen every row is an object.
            obj: v.as_object(),
        })
    }
}

fn rows(x: &Extract, table: &str) -> Result<Rows, Error> {
    let fail = |file: &str, why: String| Error::Extract(vec![Failure::new(file, why)]);
    let file = x
        .manifest
        .tables
        .get(table)
        .ok_or_else(|| fail(extract::MANIFEST, format!("tables lacks {table}")))?;
    let text = fs::read_to_string(x.dir.join(file))
        .map_err(|e| fail(file, format!("cannot read: {e}")))?;
    match serde_json::from_str(&text) {
        Ok(Value::Array(rows)) => Ok(Rows {
            file: file.clone(),
            rows,
        }),
        Ok(_) => Err(fail(file, "not a JSON array".into())),
        Err(e) => Err(fail(file, format!("not JSON: {e}"))),
    }
}

/// One object of a table, or an object nested in one, and where it is.
struct Row<'a> {
    file: &'a str,
    at: String,
    obj: Option<&'a Map<String, Value>>,
}

impl<'a> Row<'a> {
    fn get(&self, k: &str) -> &'a Value {
        self.obj.and_then(|o| o.get(k)).unwrap_or(&Value::Null)
    }

    fn bad(&self, k: &str, why: &str) -> Error {
        let why = format!("{}: {k} is {why}", self.at);
        Error::Extract(vec![Failure::new(self.file, why)])
    }

    fn num(&self, k: &str) -> Result<Option<f64>, Error> {
        match self.get(k) {
            Value::Null => Ok(None),
            v => v
                .as_f64()
                .map(Some)
                .ok_or_else(|| self.bad(k, "not a number")),
        }
    }

    /// A whole number, which a writer may have spelled with a fraction of 0.
    fn int(&self, k: &str) -> Result<Option<i64>, Error> {
        match self.get(k) {
            Value::Null => Ok(None),
            v => v
                .as_i64()
                .or_else(|| {
                    v.as_f64()
                        .filter(|f| f.fract() == 0.0 && f.abs() < 2f64.powi(53))
                        .map(|f| f as i64)
                })
                .map(Some)
                .ok_or_else(|| self.bad(k, "not a whole number")),
        }
    }

    /// A number the source spells as text, as the assets file does a stand's
    /// height; empty text is no value.
    fn num_text(&self, k: &str) -> Result<Option<f64>, Error> {
        match self.get(k) {
            Value::String(s) if s.trim().is_empty() => Ok(None),
            Value::String(s) => s
                .trim()
                .parse()
                .map(Some)
                .map_err(|_| self.bad(k, "text that is not a number")),
            _ => self.num(k),
        }
    }

    fn text(&self, k: &str) -> Result<Option<&'a str>, Error> {
        match self.get(k) {
            Value::Null => Ok(None),
            Value::String(s) => Ok(Some(s)),
            _ => Err(self.bad(k, "not text")),
        }
    }

    fn flag(&self, k: &str) -> Result<Option<bool>, Error> {
        match self.get(k) {
            Value::Null => Ok(None),
            Value::Bool(b) => Ok(Some(*b)),
            _ => Err(self.bad(k, "not true or false")),
        }
    }

    fn object(&self, k: &str) -> Result<Option<Row<'a>>, Error> {
        match self.get(k) {
            Value::Null => Ok(None),
            Value::Object(o) => Ok(Some(Row {
                file: self.file,
                at: format!("{} {k}", self.at),
                obj: Some(o),
            })),
            _ => Err(self.bad(k, "not an object")),
        }
    }

    /// A point of interest is nothing without a place.
    fn position(&self) -> Result<(f64, f64), Error> {
        let need = |k| self.num(k)?.ok_or_else(|| self.bad(k, "null"));
        Ok((need("x")?, need("z")?))
    }

    /// The English callsign, or else the first language's by name; a map
    /// with none has no callsign.
    fn callsign(&self) -> Result<Option<&'a str>, Error> {
        if let Some(en) = self.text("en")? {
            return Ok(Some(en));
        }
        let Some(first) = self.obj.and_then(|o| o.keys().min()) else {
            return Ok(None);
        };
        self.text(first)
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use rusqlite::OpenFlags;

    use super::*;
    use crate::pack::{Options, pack};
    use crate::scratch::Scratch;
    use crate::synth;

    fn synth_extract(s: &Scratch) -> PathBuf {
        let dir = s.0.join("extract");
        let t = synth::Theatre::new(synth::DEFAULT_SEED, synth::DEFAULT_SIZE_KM, 50).unwrap();
        synth::write(&t, &dir).unwrap();
        dir
    }

    fn packed(s: &Scratch, dir: &Path) -> Connection {
        let out = s.0.join("x.sqlite");
        pack(dir, Some(&out), &Options::default()).unwrap();
        Connection::open_with_flags(&out, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap()
    }

    fn count(conn: &Connection, table: &str) -> usize {
        let n: i64 = conn
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap();
        n as usize
    }

    fn read(dir: &Path, name: &str) -> Value {
        serde_json::from_str(&fs::read_to_string(dir.join(name)).unwrap()).unwrap()
    }

    fn edit(dir: &Path, name: &str, f: impl FnOnce(&mut Value)) {
        let mut v = read(dir, name);
        f(&mut v);
        fs::write(dir.join(name), v.to_string()).unwrap();
    }

    fn len(v: &Value) -> usize {
        v.as_array().unwrap().len()
    }

    #[test]
    fn row_and_r_tree_counts_match_the_extract() {
        let s = Scratch::new();
        let dir = synth_extract(&s);
        let conn = packed(&s, &dir);
        for (table, file) in [
            ("airdrome", "airdromes.json"),
            ("runway", "runways.json"),
            ("stand", "stands.json"),
            ("beacon", "beacons.json"),
            ("radio", "radio.json"),
        ] {
            assert_eq!(count(&conn, table), len(&read(&dir, file)), "{table}");
        }
        let towns = len(&read(&dir, "towns.json"));
        let nodes = len(&read(&dir, "nodes.json"));
        assert_eq!(towns, synth::TOWNS_COUNT as usize);
        assert_eq!(nodes, synth::NODES_COUNT as usize);
        assert_eq!(count(&conn, "poi"), towns + 2 * nodes);
        assert_eq!(count(&conn, "poi_idx"), count(&conn, "poi"));
        let kinds: Vec<(String, i64)> = conn
            .prepare("SELECT kind, count(*) FROM poi GROUP BY kind ORDER BY kind")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let n = |k: usize| k as i64;
        assert_eq!(
            kinds,
            [
                ("node_blue".into(), n(nodes)),
                ("node_red".into(), n(nodes)),
                ("town".into(), n(towns)),
            ]
        );
        // Every R-tree box holds its row's point; the box is f32, rounded out.
        let outside: i64 = conn
            .query_row(
                "SELECT count(*) FROM poi p JOIN poi_idx i ON i.id = p.id
                 WHERE NOT (i.minx <= p.x AND p.x <= i.maxx AND i.minz <= p.z AND p.z <= i.maxz)",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(outside, 0);
    }

    #[test]
    fn writes_each_column_from_its_source() {
        let s = Scratch::new();
        let dir = synth_extract(&s);
        let conn = packed(&s, &dir);

        let a = &read(&dir, "airdromes.json")[0];
        let row: (i64, String, String, String, f64, f64, bool, String) = conn
            .query_row(
                "SELECT id, name_id, code, name, x, lat, civilian, class FROM airdrome",
                [],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                        r.get(7)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(row.0, i64::from(synth::AIRDROME_ID));
        assert_eq!(row.1, synth::AIRDROME_NAME_ID);
        assert_eq!(row.2, synth::AIRDROME_CODE);
        assert_eq!(row.3, synth::AIRDROME_DISPLAY_NAME);
        assert_eq!(
            (row.4, row.5),
            (a["x"].as_f64().unwrap(), a["lat"].as_f64().unwrap())
        );
        assert!(!row.6);
        assert_eq!(row.7, "airfield");

        let (name, length, course): (String, f64, f64) = conn
            .query_row("SELECT name, length, course FROM runway", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap();
        assert_eq!(
            name,
            format!("{}-{}", synth::RUNWAY_EDGE1_NAME, synth::RUNWAY_EDGE2_NAME)
        );
        assert!((length - synth::RUNWAY_LENGTH_M).abs() < 1e-6, "{length}");
        assert_eq!(course, synth::RUNWAY_COURSE_RAD);

        let stand: (bool, bool, Option<bool>, f64, Option<f64>) = conn
            .query_row(
                "SELECT for_helicopters, for_airplanes, shelter, width, height FROM stand
                 WHERE crossroad_index = 0",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap();
        // The synthetic stand's height is empty text, as a class with no
        // height gives it.
        assert_eq!(stand, (false, true, None, synth::STAND_WIDTH_M, None));

        let channels: Vec<Option<i64>> = conn
            .prepare("SELECT channel FROM beacon ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(channels, [Some(i64::from(synth::BEACON_1_CHANNEL)), None]);

        let radio: (String, String, i64) = conn
            .query_row("SELECT callsign, roles, uhf FROM radio", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap();
        assert_eq!(radio.0, synth::AIRDROME_DISPLAY_NAME);
        assert_eq!(radio.1, r#"["ground","tower","approach"]"#);
        assert_eq!(radio.2, synth::RADIO_UHF_HZ as i64);
    }

    #[test]
    fn a_stand_height_given_as_text_is_a_number() {
        let s = Scratch::new();
        let dir = synth_extract(&s);
        edit(&dir, "stands.json", |v| {
            v[0]["params"]["HEIGHT"] = "18".into()
        });
        let conn = packed(&s, &dir);
        let h: Option<f64> = conn
            .query_row(
                "SELECT height FROM stand WHERE crossroad_index = 0",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(h, Some(18.0));
    }

    /// Without the hook an airdrome may have no `id`; its runways and stands
    /// still find it by `name_id`.
    #[test]
    fn an_airdrome_with_no_id_is_joined_by_name_id() {
        let s = Scratch::new();
        let dir = synth_extract(&s);
        edit(&dir, "airdromes.json", |v| v[0]["id"] = Value::Null);
        for file in ["runways.json", "stands.json"] {
            edit(&dir, file, |v| {
                for r in v.as_array_mut().unwrap() {
                    r["airdrome_id"] = Value::Null;
                }
            });
        }
        let conn = packed(&s, &dir);
        let id: Option<i64> = conn
            .query_row("SELECT id FROM airdrome", [], |r| r.get(0))
            .unwrap();
        assert_eq!(id, None);
        for table in ["runway", "stand"] {
            let joined: i64 = conn
                .query_row(
                    &format!(
                        "SELECT count(*) FROM {table} t
                         JOIN airdrome a ON a.name_id = t.airdrome_name_id"
                    ),
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(joined as usize, count(&conn, table), "{table}");
        }
    }

    #[test]
    fn a_value_of_the_wrong_type_abandons_the_pack() {
        let s = Scratch::new();
        let dir = synth_extract(&s);
        edit(&dir, "beacons.json", |v| {
            v[1]["frequency_hz"] = "fast".into()
        });
        let out = s.0.join("x.sqlite");
        match pack(&dir, Some(&out), &Options::default()).unwrap_err() {
            Error::Extract(f) => {
                assert_eq!(f[0].path, "beacons.json");
                assert!(f[0].message.contains("row 2: frequency_hz"), "{f:?}");
            }
            e => panic!("{e}"),
        }
        assert!(!out.exists());
    }

    #[test]
    fn a_point_of_interest_needs_a_place() {
        let s = Scratch::new();
        let dir = synth_extract(&s);
        edit(&dir, "nodes.json", |v| v[0]["blue"]["z"] = Value::Null);
        match pack(&dir, Some(&s.0.join("x.sqlite")), &Options::default()).unwrap_err() {
            Error::Extract(f) => assert!(f[0].message.contains("row 1 blue: z is null"), "{f:?}"),
            e => panic!("{e}"),
        }
    }
}
