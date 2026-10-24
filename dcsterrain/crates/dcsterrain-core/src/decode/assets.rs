//! A map's `misc/assets.assets`: one record per stand class, giving the
//! stand's outline and who may use it. The file has no container header.
//!
//! ```text
//! a record   u32 len, len bytes          the stand class, e.g. stand_shelter
//!            …                           its category, among a few words
//!            u32 9, "DummySize", str     the first key and value
//!            (str key, str value) …      height, aircraftTypes, …, any order
//!            u32 5, "stand", u32 4, "true"   the last pair
//!            geometry                    f64 triples; the outline is the
//!                                        ones at y 0, in meters
//! ```
//!
//! Only the key and value pairs are framed in a way that is understood, so
//! a record is found by its last pair, its name read from the strings
//! before its first, and its outline picked out of the bytes after.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

/// The last pair of a stand record.
const STAND_MARK: &[u8] = b"\x05\x00\x00\x00stand\x04\x00\x00\x00true";
/// The first key of every record.
const FIRST_KEY: &[u8] = b"\x09\x00\x00\x00DummySize";
/// How far before its last pair a record's first key lies.
const KEYS_BEFORE: usize = 3000;
/// How far before its first key a record's name lies.
const NAME_BEFORE: usize = 300;
const MAX_STR: usize = 300;
/// An outline vertex lies this far from the stand's origin. The icon
/// geometry stays within 2 m, and no stand is a kilometer across.
const MIN_REACH_M: f64 = 2.5;
const MAX_REACH_M: f64 = 1000.0;
/// A coordinate this small and not zero is bytes that are not a float.
const TINY: f64 = 1e-6;

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Malformed(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "cannot be read: {e}"),
            Error::Malformed(m) => f.write_str(m),
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

/// What a stand class allows, as the game reports it for each stand.
#[derive(Debug, Clone, PartialEq)]
pub struct StandParams {
    /// The outline's extent across, x, and along, z, in meters.
    pub width: f64,
    pub length: f64,
    /// As the file writes it, or empty where it gives none.
    pub height: String,
    pub for_airplanes: bool,
    pub for_helicopters: bool,
}

/// The string at `at`, if a printable one is there, and where it ends.
fn str_at(b: &[u8], at: usize) -> Option<(&str, usize)> {
    let len = u32::from_le_bytes(b.get(at..at + 4)?.try_into().unwrap()) as usize;
    if !(1..=MAX_STR).contains(&len) {
        return None;
    }
    let s = b.get(at + 4..at + 4 + len)?;
    s.iter()
        .all(|c| c.is_ascii_graphic() || *c == b' ')
        .then(|| (std::str::from_utf8(s).unwrap(), at + 4 + len))
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn rfind(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).rposition(|w| w == needle)
}

/// The string ending latest in `b[from..to]` that is not `except`.
fn last_str_before<'a>(b: &'a [u8], from: usize, to: usize, except: &str) -> Option<&'a str> {
    (from..to)
        .filter_map(|p| str_at(b, p).filter(|&(s, end)| end <= to && s != except))
        .max_by_key(|&(_, end)| end)
        .map(|(s, _)| s)
}

/// The outline's extent across and along, from the triples at `y` 0 in
/// `b`, at any offset.
fn extent(b: &[u8]) -> Option<(f64, f64)> {
    let f = |at: usize| f64::from_le_bytes(b[at..at + 8].try_into().unwrap());
    let ok = |v: f64| v.is_finite() && (v == 0.0 || v.abs() >= TINY);
    let (mut min_x, mut max_x, mut min_z, mut max_z) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    let mut any = false;
    for at in 0..b.len().saturating_sub(23) {
        let (x, y, z) = (f(at), f(at + 8), f(at + 16));
        let reach = x.abs().max(z.abs());
        if y == 0.0 && ok(x) && ok(z) && reach > MIN_REACH_M && reach <= MAX_REACH_M {
            (min_x, max_x) = (min_x.min(x), max_x.max(x));
            (min_z, max_z) = (min_z.min(z), max_z.max(z));
            any = true;
        }
    }
    any.then_some((max_x - min_x, max_z - min_z))
}

/// Every stand record in the file, by stand class.
pub fn parse(b: &[u8]) -> Result<BTreeMap<String, StandParams>, Error> {
    let mut out = BTreeMap::new();
    let mut from = 0;
    while let Some(k) = find(&b[from..], STAND_MARK) {
        let mark = from + k;
        from = mark + STAND_MARK.len();
        let window = mark.saturating_sub(KEYS_BEFORE);
        let first = window
            + rfind(&b[window..mark], FIRST_KEY).ok_or_else(|| {
                Error::Malformed(format!(
                    "the stand record ending at byte {mark} has no DummySize key before it"
                ))
            })?;
        let mut pairs = BTreeMap::new();
        let mut at = first;
        while at < mark {
            let pair = str_at(b, at).and_then(|(k, end)| Some((k, str_at(b, end)?)));
            let Some((key, (value, end))) = pair else {
                return Err(Error::Malformed(format!(
                    "the stand record at byte {first} has no key and value at byte {at}"
                )));
            };
            pairs.insert(key, value);
            at = end;
        }
        let category = pairs.get("assetCategory").copied().unwrap_or("");
        let name = last_str_before(b, first.saturating_sub(NAME_BEFORE), first, category)
            .ok_or_else(|| {
                Error::Malformed(format!(
                    "the stand record at byte {first} has no name before it"
                ))
            })?;
        let end = find(&b[from..], FIRST_KEY).map_or(b.len(), |k| from + k);
        let (width, length) = extent(&b[from..end])
            .ok_or_else(|| Error::Malformed(format!("stand class {name} has no outline")))?;
        let aircraft = pairs.get("aircraftTypes").copied();
        out.entry(name.to_string()).or_insert(StandParams {
            width,
            length,
            height: pairs.get("height").map_or(String::new(), |h| h.to_string()),
            for_airplanes: aircraft != Some("helicopters"),
            for_helicopters: aircraft != Some("airplanes"),
        });
    }
    if out.is_empty() {
        return Err(Error::Malformed("no stand record was found".into()));
    }
    Ok(out)
}

/// Reads and decodes the `assets.assets` at `path`.
pub fn read(path: &Path) -> Result<BTreeMap<String, StandParams>, Error> {
    parse(&fs::read(path)?)
}

#[cfg(test)]
pub mod build {
    //! `assets.assets` records as the decoder reads them, for tests.

    use super::STAND_MARK;

    fn s(b: &mut Vec<u8>, v: &str) {
        b.extend_from_slice(&(v.len() as u32).to_le_bytes());
        b.extend_from_slice(v.as_bytes());
    }

    fn triple(b: &mut Vec<u8>, v: [f64; 3]) {
        for x in v {
            b.extend_from_slice(&x.to_le_bytes());
        }
    }

    /// A record as the file lays one out: name, a zero, category, a pair
    /// count, the pairs, then an icon and the outline among other floats.
    pub fn record(b: &mut Vec<u8>, name: &str, pairs: &[(&str, &str)], outline: &[[f64; 2]]) {
        s(b, name);
        b.extend_from_slice(&[0; 4]);
        s(b, "crosstaxiway");
        b.extend_from_slice(&((pairs.len() + 3) as u32).to_le_bytes());
        s(b, "DummySize");
        s(b, "1.059147");
        for (k, v) in pairs {
            s(b, k);
            s(b, v);
        }
        s(b, "assetCategory");
        s(b, "crosstaxiway");
        b.extend_from_slice(STAND_MARK);
        b.extend_from_slice(&[0x9A, 0x99, 0x99, 0x99, 0x99, 0x99, 0xB9, 0x3F, 1, 0, 0]);
        triple(b, [1.5, 0.0, -2.0]);
        triple(b, [3.0, 1.0, 50.0]);
        triple(b, [3.0, 0.0, 1e-9]);
        for &[x, z] in outline {
            triple(b, [x, 0.0, z]);
        }
        b.extend_from_slice(&[0; 7]);
    }
}

#[cfg(test)]
mod tests {
    use super::build::record;
    use super::*;

    #[test]
    fn reads_each_stand_class_with_its_outline_and_permissions() {
        let mut b = vec![0; 20];
        let square = [
            [-20.0, -21.5],
            [20.0, -21.5],
            [20.0, 21.557],
            [-20.0, 21.557],
        ];
        record(
            &mut b,
            "stand_40x40m_end",
            &[("height", "18"), ("aircraftTypes", "airplanes")],
            &square,
        );
        record(
            &mut b,
            "hellicopter_no_taxiway",
            &[("aircraftTypes", "helicopters")],
            &[[-9.0, -10.25], [9.0, 10.25]],
        );
        record(
            &mut b,
            "stand_end_all",
            &[("height", "5.5")],
            &[[-12.0, 0.0], [12.0, 13.0]],
        );
        let p = parse(&b).unwrap();
        assert_eq!(
            p["stand_40x40m_end"],
            StandParams {
                width: 40.0,
                length: 43.057,
                height: "18".into(),
                for_airplanes: true,
                for_helicopters: false,
            }
        );
        let heli = &p["hellicopter_no_taxiway"];
        assert_eq!(
            (heli.width, heli.length, heli.height.as_str()),
            (18.0, 20.5, "")
        );
        assert_eq!((heli.for_airplanes, heli.for_helicopters), (false, true));
        let all = &p["stand_end_all"];
        assert_eq!(
            (all.width, all.length, all.height.as_str()),
            (24.0, 13.0, "5.5")
        );
        assert_eq!((all.for_airplanes, all.for_helicopters), (true, true));
    }

    #[test]
    fn refuses_a_record_it_cannot_frame() {
        assert_eq!(
            parse(&[0; 64]).unwrap_err().to_string(),
            "no stand record was found"
        );
        let mut b = vec![0; 8];
        b.extend_from_slice(STAND_MARK);
        let e = parse(&b).unwrap_err().to_string();
        assert!(e.ends_with("has no DummySize key before it"), "{e}");

        let mut b = Vec::new();
        record(&mut b, "stand_x", &[], &[]);
        assert_eq!(
            parse(&b).unwrap_err().to_string(),
            "stand class stand_x has no outline"
        );
    }
}
