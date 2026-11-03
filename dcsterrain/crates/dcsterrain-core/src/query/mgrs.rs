//! MGRS references on WGS84, through UTM: the zone and latitude band, the
//! 100 km square's two letters, and the easting and northing within it to
//! the meter, each truncated as MGRS writes them; and a reference read back
//! to the square it names.

use crate::crs::{forward, inverse};

const BANDS: &[u8] = b"CDEFGHJKLMNPQRSTUVWX";
const COLUMNS: [&[u8]; 3] = [b"ABCDEFGH", b"JKLMNPQR", b"STUVWXYZ"];
const ROWS: &[u8] = b"ABCDEFGHJKLMNPQRSTUV";
const FALSE_EASTING: f64 = 500_000.0;
const FALSE_NORTHING_SOUTH: f64 = 10_000_000.0;
const SQUARE: f64 = 100_000.0;
/// The rows' letters repeat every 2 000 km of northing.
const ROW_CYCLE: f64 = 2_000_000.0;

/// The UTM zone of a position, with Norway's and Svalbard's exceptions.
fn zone(lat: f64, lon: f64) -> i32 {
    let lon = (lon + 180.0).rem_euclid(360.0) - 180.0;
    if (56.0..64.0).contains(&lat) && (3.0..12.0).contains(&lon) {
        return 32;
    }
    if (72.0..=84.0).contains(&lat) && (0.0..42.0).contains(&lon) {
        return match lon {
            l if l < 9.0 => 31,
            l if l < 21.0 => 33,
            l if l < 33.0 => 35,
            _ => 37,
        };
    }
    ((lon + 180.0) / 6.0).floor() as i32 % 60 + 1
}

fn band(lat: f64) -> u8 {
    BANDS[(((lat + 80.0) / 8.0).floor() as usize).min(BANDS.len() - 1)]
}

fn meridian(zone: i32) -> f64 {
    f64::from(zone * 6 - 183)
}

/// Whether MGRS covers `lat`: polar grids take over past 80° S and 84° N.
fn covered(lat: f64, lon: f64) -> bool {
    lat.is_finite() && lon.is_finite() && (-80.0..=84.0).contains(&lat)
}

/// The UTM easting and northing of `lat`, `lon` in `zone`, false offsets
/// included.
fn utm(lat: f64, lon: f64, zone: i32) -> (f64, f64) {
    let dlon = (lon - meridian(zone) + 180.0).rem_euclid(360.0) - 180.0;
    let (n, e) = forward(lat, dlon);
    let northing = if lat < 0.0 {
        FALSE_NORTHING_SOUTH + n
    } else {
        n
    };
    (FALSE_EASTING + e, northing)
}

/// The reference of `lat`, `lon` in degrees, such as `31N AA 66021 00000`;
/// `None` outside 80° S to 84° N, where MGRS uses polar grids, or for a
/// position that is not finite.
pub fn mgrs(lat: f64, lon: f64) -> Option<String> {
    if !covered(lat, lon) {
        return None;
    }
    let z = zone(lat, lon);
    let (easting, northing) = utm(lat, lon, z);
    let col = (easting / SQUARE).floor() as usize;
    let column = *COLUMNS[((z - 1) % 3) as usize].get(col.checked_sub(1)?)?;
    let shift = if z % 2 == 0 { 5 } else { 0 };
    let row = ROWS[((northing / SQUARE).floor() as usize + shift) % ROWS.len()];
    let within = |v: f64| (v.rem_euclid(SQUARE)).floor() as u32;
    Some(format!(
        "{z}{} {}{} {:05} {:05}",
        band(lat) as char,
        column as char,
        row as char,
        within(easting),
        within(northing)
    ))
}

/// The square an MGRS reference names, read back.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MgrsSquare {
    /// The center of the part of the square that lies in its zone and band,
    /// in degrees: inside the square, and never more than half its side
    /// from any point the reference could have been written for.
    pub lat: f64,
    pub lon: f64,
    /// The digits each of the easting and the northing carries, 1 to 5.
    pub digits: u32,
    /// The side of the square in meters: 10 km for one digit, 1 m for five.
    pub side: f64,
}

/// Reads an MGRS reference: the zone, its band, the square's two letters,
/// then an even count of 2 to 10 digits, easting before northing. Spaces
/// may stand anywhere or nowhere, and letters are of either case. The
/// error says what is wrong, as the end of a sentence about the reference.
pub fn parse_mgrs(text: &str) -> Result<MgrsSquare, String> {
    let chars: Vec<char> = text
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| c.to_ascii_uppercase())
        .collect();
    let zone_len = chars.iter().take_while(|c| c.is_ascii_digit()).count();
    if !(1..=2).contains(&zone_len) {
        return Err("an MGRS grid starts with its zone, 1 to 60".into());
    }
    let letters = (chars[zone_len..].iter())
        .take_while(|c| c.is_ascii_alphabetic())
        .count();
    if letters != 3 {
        return Err(
            "an MGRS grid needs three letters after its zone: the band, then the square".into(),
        );
    }
    if let Some(bad) = (chars[zone_len + 3..].iter()).find(|c| !c.is_ascii_digit()) {
        return Err(format!(
            "only digits may follow the square's letters, not `{bad}`"
        ));
    }
    // Every character is an ASCII digit or letter from here.
    let s: Vec<u8> = chars.iter().map(|&c| c as u8).collect();
    let zone: i32 = std::str::from_utf8(&s[..zone_len])
        .expect("digits are ASCII")
        .parse()
        .expect("one or two digits parse");
    if !(1..=60).contains(&zone) {
        return Err(format!("the zone must be 1 to 60, not {zone}"));
    }
    let rest = &s[zone_len..];
    let (b, c, r) = (rest[0], rest[1], rest[2]);
    let digits = &rest[3..];
    if digits.is_empty() || digits.len() % 2 == 1 || digits.len() > 10 {
        return Err(format!(
            "an MGRS grid needs an even number of digits, 2 to 10, and this one has {}",
            digits.len()
        ));
    }
    let band_k = BANDS
        .iter()
        .position(|&x| x == b)
        .ok_or_else(|| format!("`{}` is not a latitude band", b as char))?;
    let set = COLUMNS[((zone - 1) % 3) as usize];
    let col = set.iter().position(|&x| x == c).ok_or_else(|| {
        format!(
            "zone {zone}'s squares have no column `{}`; its columns are {}",
            c as char,
            std::str::from_utf8(set).expect("letters are ASCII")
        )
    })?;
    let row = ROWS
        .iter()
        .position(|&x| x == r)
        .ok_or_else(|| format!("`{}` is not a square's row", r as char))?;

    let k = digits.len() / 2;
    let side = 10f64.powi(5 - k as i32);
    let number = |d: &[u8]| -> f64 {
        let v: u32 = std::str::from_utf8(d).unwrap().parse().unwrap();
        f64::from(v) * side
    };
    let east0 = (col + 1) as f64 * SQUARE + number(&digits[..k]);
    let shift = if zone % 2 == 0 { 5 } else { 0 };
    let north_in_cycle = ((row + ROWS.len() - shift) % ROWS.len()) as f64 * SQUARE;
    let north_low = north_in_cycle + number(&digits[k..]);

    // The band says which 2 000 km cycle of rows is meant: one cycle is
    // 18° of latitude, and a band 8°, so at most one lies near it.
    let south = b < b'N';
    let lo = -80.0 + 8.0 * band_k as f64;
    let hi = if b == b'X' { 84.0 } else { lo + 8.0 };
    let at = |e: f64, n: f64| -> (f64, f64) {
        let true_n = if south { n - FALSE_NORTHING_SOUTH } else { n };
        let (lat, dlon) = inverse(true_n, e - FALSE_EASTING);
        (
            lat,
            (dlon + meridian(zone) + 180.0).rem_euclid(360.0) - 180.0,
        )
    };
    let center = (east0 + side / 2.0, north_low + side / 2.0);
    let cycle = (0..5)
        .map(|j| north_low + j as f64 * ROW_CYCLE)
        .find(|&n| {
            let (lat, _) = at(center.0, n + side / 2.0);
            (lo - 1.0..=hi + 1.0).contains(&lat)
        })
        .ok_or_else(|| {
            format!(
                "square {}{} of zone {zone} lies nowhere in band {}",
                c as char, r as char, b as char
            )
        })?;

    // A square cut by its zone's or band's edge holds a part written with
    // other letters; the point is the middle of the part these name.
    let inside = |e: f64, n: f64| {
        let (lat, lon) = at(e, n);
        covered(lat, lon) && self::zone(lat, lon) == zone && band(lat) == b
    };
    let (mut e, mut n) = (center.0, cycle + side / 2.0);
    if !inside(e, n) {
        let part = Part {
            zone,
            band: (lo, hi),
            corner: (east0, cycle),
            side,
        };
        (e, n) = part.middle(&inside).ok_or_else(|| {
            format!(
                "square {}{} of zone {zone} lies outside band {}",
                c as char, r as char, b as char
            )
        })?;
    }
    let (lat, lon) = at(e, n);
    Ok(MgrsSquare {
        lat,
        lon,
        digits: k as u32,
        side,
    })
}

/// The longitudes `zone` spans at `lat`, with Norway's and Svalbard's
/// exceptions, as `zone` assigns them.
fn zone_span(zone: i32, lat: f64) -> (f64, f64) {
    let exception = if (56.0..64.0).contains(&lat) {
        match zone {
            31 => Some((0.0, 3.0)),
            32 => Some((3.0, 12.0)),
            _ => None,
        }
    } else if (72.0..=84.0).contains(&lat) {
        match zone {
            31 => Some((0.0, 9.0)),
            33 => Some((9.0, 21.0)),
            35 => Some((21.0, 33.0)),
            37 => Some((33.0, 42.0)),
            _ => None,
        }
    } else {
        None
    };
    exception.unwrap_or_else(|| (meridian(zone) - 3.0, meridian(zone) + 3.0))
}

/// The part of a square, `side` meters across from its `corner`, that lies
/// in `zone` and between the latitudes `band`.
struct Part {
    zone: i32,
    band: (f64, f64),
    corner: (f64, f64),
    side: f64,
}

impl Part {
    /// The mean of the part's corners, which lies in it: the band's and the
    /// zone's edges are near straight across a square, so the part is
    /// convex. Its corners are the square's corners in it, the points where
    /// the square's sides cross into it, and the band's and zone's corners in
    /// the square. `None` when the square holds none of it.
    fn middle(&self, inside: &dyn Fn(f64, f64) -> bool) -> Option<(f64, f64)> {
        // Within the square, a hair inside its sides: a reference names the
        // square's west and south edges but not its east and north.
        let inset = self.side * 1e-6;
        let (e0, n0) = (self.corner.0 + inset, self.corner.1 + inset);
        let (e1, n1) = (
            self.corner.0 + self.side - inset,
            self.corner.1 + self.side - inset,
        );
        let corners = [(e0, n0), (e1, n0), (e1, n1), (e0, n1)];
        let mut found: Vec<(f64, f64)> = Vec::new();
        // Across one square an edge of the band or the zone crosses a side at
        // most once, so a few samples a side find every crossing.
        const SAMPLES: usize = 8;
        for k in 0..4 {
            let (a, b) = (corners[k], corners[(k + 1) % 4]);
            let along = |t: f64| (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
            let mut was = inside(a.0, a.1);
            if was {
                found.push(a);
            }
            for j in 1..=SAMPLES {
                let t = j as f64 / SAMPLES as f64;
                let p = along(t);
                let now = inside(p.0, p.1);
                if now != was {
                    // Halve the step between the two samples down to a
                    // micrometer, keeping the end that lies in the part.
                    let (mut out_t, mut in_t) = if now {
                        (t - 1.0 / SAMPLES as f64, t)
                    } else {
                        (t, t - 1.0 / SAMPLES as f64)
                    };
                    for _ in 0..40 {
                        let mid = (out_t + in_t) / 2.0;
                        let q = along(mid);
                        if inside(q.0, q.1) {
                            in_t = mid;
                        } else {
                            out_t = mid;
                        }
                    }
                    found.push(along(in_t));
                }
                was = now;
            }
        }
        // About a centimeter, so a corner of the band and the zone is inside
        // both.
        const NUDGE_DEG: f64 = 1e-7;
        for lat in [self.band.0 + NUDGE_DEG, self.band.1 - NUDGE_DEG] {
            let (west, east) = zone_span(self.zone, lat);
            for lon in [west + NUDGE_DEG, east - NUDGE_DEG] {
                let (e, n) = utm(lat, lon, self.zone);
                let within = (e0..=e1).contains(&e) && (n0..=n1).contains(&n);
                if within && inside(e, n) {
                    found.push((e, n));
                }
            }
        }
        let first = *found.first()?;
        let k = found.len() as f64;
        let mean = (found.iter()).fold((0.0, 0.0), |s, p| (s.0 + p.0 / k, s.1 + p.1 / k));
        Some(if inside(mean.0, mean.1) { mean } else { first })
    }
}

/// `reference`, as `mgrs` writes it, cut to `digits` of easting and of
/// northing.
pub fn truncate_mgrs(reference: &str, digits: u32) -> String {
    let mut parts: Vec<&str> = reference.split(' ').collect();
    for p in parts.iter_mut().skip(2) {
        *p = &p[..(digits as usize).min(p.len())];
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_references() {
        assert_eq!(mgrs(0.0, 0.0).unwrap(), "31N AA 66021 00000");
        // The Washington Monument, given to four places, about 10 m.
        assert!(mgrs(38.8895, -77.0352).unwrap().starts_with("18S UJ 2348"));
        // South of the equator the northing is counted from 10 000 km.
        let s = mgrs(-33.8568, 151.2153).unwrap();
        assert!(s.starts_with("56H LH"), "{s}");
        assert_eq!(&mgrs(60.0, 5.0).unwrap()[..3], "32V");
        assert_eq!(&mgrs(78.0, 15.0).unwrap()[..3], "33X");
        assert_eq!(mgrs(85.0, 0.0), None);
        assert_eq!(mgrs(f64::NAN, 0.0), None);
    }

    /// Every reference `mgrs` writes, cut to any precision, reads back to a
    /// point `mgrs` writes the same at that precision, within the square.
    #[test]
    fn a_reference_reads_back_to_its_square_at_every_precision() {
        let mut lat = -79.9;
        while lat < 84.0 {
            let mut lon = -179.7;
            while lon < 180.0 {
                let full = mgrs(lat, lon).unwrap();
                for d in 1..=5 {
                    let typed = truncate_mgrs(&full, d);
                    let sq = parse_mgrs(&typed).unwrap_or_else(|e| panic!("{typed}: {e}"));
                    assert_eq!(sq.digits, d);
                    let back = truncate_mgrs(&mgrs(sq.lat, sq.lon).unwrap(), d);
                    assert_eq!(back, typed, "{lat} {lon}");
                }
                lon += 7.37;
            }
            lat += 3.13;
        }
    }

    /// Near every band's and zone's edge, where a square holds a sliver of
    /// the part its letters name, the reference still reads back to it.
    #[test]
    fn a_sliver_of_a_square_reads_back_to_itself() {
        let mut lats = vec![-80.0, 84.0];
        for k in 0..20 {
            let edge = -80.0 + 8.0 * f64::from(k);
            lats.extend([edge - 0.001, edge, edge + 0.001]);
        }
        let mut lons = vec![3.0, 9.0, 12.0, 21.0, 33.0, 42.0];
        for k in 0..60 {
            lons.push(-180.0 + 6.0 * f64::from(k));
        }
        let lons: Vec<f64> = (lons.iter())
            .flat_map(|&l| [l - 0.01, l - 0.0001, l, l + 0.0001, l + 0.01])
            .collect();
        let mut checked = 0;
        for &lat in lats.iter().filter(|l| (-80.0..=84.0).contains(*l)) {
            for &lon in &lons {
                let Some(full) = mgrs(lat, lon) else { continue };
                for d in 1..=5 {
                    let typed = truncate_mgrs(&full, d);
                    let sq =
                        parse_mgrs(&typed).unwrap_or_else(|e| panic!("{lat} {lon} {typed}: {e}"));
                    let back = truncate_mgrs(&mgrs(sq.lat, sq.lon).unwrap(), d);
                    assert_eq!(back, typed, "{lat} {lon}");
                    checked += 1;
                }
            }
        }
        assert!(checked > 30_000, "{checked}");
        // The cases a review found refused.
        for (lat, lon) in [
            (-71.999, 0.0),
            (-71.999, 180.0),
            (-72.0, 6.0),
            (83.09, 21.01),
        ] {
            let typed = truncate_mgrs(&mgrs(lat, lon).unwrap(), 1);
            let sq = parse_mgrs(&typed).unwrap();
            assert_eq!(truncate_mgrs(&mgrs(sq.lat, sq.lon).unwrap(), 1), typed);
        }
    }

    #[test]
    fn a_character_outside_ascii_is_named_as_typed() {
        assert!(parse_mgrs("38T KM ９１").unwrap_err().ends_with("not `９`"));
        let spaced = parse_mgrs("38T\u{a0}KM 9198").unwrap();
        assert_eq!(spaced, parse_mgrs("38T KM 9198").unwrap());
    }

    #[test]
    fn a_ten_digit_reference_is_within_a_meter() {
        let (lat, lon) = (42.177_615_809, 42.481_292_209);
        let sq = parse_mgrs(&mgrs(lat, lon).unwrap()).unwrap();
        let (e0, n0) = utm(lat, lon, 38);
        let (e1, n1) = utm(sq.lat, sq.lon, 38);
        assert!((e1 - e0).abs() < 1.0 && (n1 - n0).abs() < 1.0);
        assert_eq!(sq.side, 1.0);
    }

    #[test]
    fn the_spellings_read_alike() {
        let a = parse_mgrs("38T KM 91980 72567").unwrap();
        for s in [
            "38TKM9198072567",
            "38t km 91980 72567",
            " 38T KM9198072567 ",
        ] {
            assert_eq!(parse_mgrs(s).unwrap(), a, "{s}");
        }
        let alaska = mgrs(60.0, -135.0).unwrap();
        assert!(alaska.starts_with("8V"), "{alaska}");
        assert_eq!(parse_mgrs(&format!("0{alaska}")), parse_mgrs(&alaska));
        let b = parse_mgrs("31N AA 66021 00000").unwrap();
        assert_eq!(parse_mgrs("31naa6602100000").unwrap(), b);
    }

    #[test]
    fn a_square_cut_by_a_zone_or_band_edge_reads_to_its_own_part() {
        // 42° E parts zones 37 and 38, and 48° N bands T and U. Find a
        // 10 km square of zone 38 and band U whose plain center lies past
        // one of them.
        let cut = (0..2000)
            .map(|j| (48.0001 + f64::from(j) * 1e-4, 42.0001))
            .find(|&(lat, lon)| {
                let (e, n) = utm(lat, lon, 38);
                e.rem_euclid(10_000.0) > 6_000.0 || n.rem_euclid(10_000.0) > 6_000.0
            })
            .expect("a cut square near the corner");
        let full = mgrs(cut.0, cut.1).unwrap();
        assert!(full.starts_with("38U"), "{full}");
        let typed = truncate_mgrs(&full, 1);
        let sq = parse_mgrs(&typed).unwrap();
        assert!(sq.lon >= 42.0 && sq.lat >= 48.0, "{} {}", sq.lat, sq.lon);
        assert_eq!(truncate_mgrs(&mgrs(sq.lat, sq.lon).unwrap(), 1), typed);
    }

    #[test]
    fn a_malformed_reference_says_what_is_wrong() {
        let e = |s: &str| parse_mgrs(s).unwrap_err();
        assert!(e("38T KM 9198 725").ends_with("this one has 7"));
        assert!(e("38T KM").ends_with("this one has 0"));
        assert!(e("38T KM 919807256712").ends_with("this one has 12"));
        assert!(e("61T KM 11").contains("1 to 60"));
        assert!(e("T KM 11").contains("starts with its zone"));
        assert!(e("38KM 11").contains("three letters"));
        assert!(e("38I KM 11").contains("not a latitude band"));
        assert!(e("38T AM 11").contains("no column `A`"));
        assert!(e("38T KW 11").contains("not a square's row"));
        assert!(e("38T KM 9x").contains("not `X`"));
        // Kutaisi's square in a southern band.
        assert!(e("38C KM 11").contains("band C"), "{}", e("38C KM 11"));
    }
}
