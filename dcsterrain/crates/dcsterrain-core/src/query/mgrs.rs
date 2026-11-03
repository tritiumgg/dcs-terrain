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
    let s: Vec<u8> = text
        .bytes()
        .filter(|b| !b.is_ascii_whitespace())
        .map(|b| b.to_ascii_uppercase())
        .collect();
    let zone_len = s.iter().take_while(|b| b.is_ascii_digit()).count();
    if !(1..=2).contains(&zone_len) {
        return Err("an MGRS grid starts with its zone, 1 to 60".into());
    }
    let zone: i32 = std::str::from_utf8(&s[..zone_len])
        .expect("digits are ASCII")
        .parse()
        .expect("one or two digits parse");
    if !(1..=60).contains(&zone) {
        return Err(format!("the zone must be 1 to 60, not {zone}"));
    }
    let rest = &s[zone_len..];
    let letters = rest.iter().take_while(|b| b.is_ascii_alphabetic()).count();
    if letters != 3 {
        return Err(
            "an MGRS grid needs three letters after its zone: the band, then the square".into(),
        );
    }
    let (b, c, r) = (rest[0], rest[1], rest[2]);
    let digits = &rest[3..];
    if let Some(&bad) = digits.iter().find(|b| !b.is_ascii_digit()) {
        return Err(format!(
            "only digits may follow the square's letters, not `{}`",
            bad as char
        ));
    }
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
    // other letters; the point is the center of the part these name.
    let inside = |e: f64, n: f64| {
        let (lat, lon) = at(e, n);
        covered(lat, lon) && self::zone(lat, lon) == zone && band(lat) == b
    };
    const STEPS: usize = 64;
    let middle_of_inside = |from: f64, f: &dyn Fn(f64) -> bool| -> Option<f64> {
        let mut found: Option<(f64, f64)> = None;
        for j in 0..STEPS {
            let v = from + side * (j as f64 + 0.5) / STEPS as f64;
            if f(v) {
                found = Some(found.map_or((v, v), |(a, _)| (a, v)));
            }
        }
        found.map(|(first, last)| (first + last) / 2.0)
    };
    let (mut e, mut n) = (center.0, cycle + side / 2.0);
    if !inside(e, n) {
        if let Some(v) = middle_of_inside(cycle, &|v| inside(e, v)) {
            n = v;
        }
        if !inside(e, n)
            && let Some(v) = middle_of_inside(east0, &|v| inside(v, n))
        {
            e = v;
        }
        if !inside(e, n) {
            return Err(format!(
                "square {}{} of zone {zone} lies outside band {}",
                c as char, r as char, b as char
            ));
        }
    }
    let (lat, lon) = at(e, n);
    Ok(MgrsSquare {
        lat,
        lon,
        digits: k as u32,
        side,
    })
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
