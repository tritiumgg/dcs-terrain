//! MGRS references on WGS84, through UTM: the zone and latitude band, the
//! 100 km square's two letters, and the easting and northing within it to
//! the meter, each truncated as MGRS writes them.

use crate::crs::forward;

const BANDS: &[u8] = b"CDEFGHJKLMNPQRSTUVWX";
const COLUMNS: [&[u8]; 3] = [b"ABCDEFGH", b"JKLMNPQR", b"STUVWXYZ"];
const ROWS: &[u8] = b"ABCDEFGHJKLMNPQRSTUV";
const FALSE_EASTING: f64 = 500_000.0;
const FALSE_NORTHING_SOUTH: f64 = 10_000_000.0;

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

/// The reference of `lat`, `lon` in degrees, such as `31N AA 66021 00000`;
/// `None` outside 80° S to 84° N, where MGRS uses polar grids, or for a
/// position that is not finite.
pub fn mgrs(lat: f64, lon: f64) -> Option<String> {
    if !(lat.is_finite() && lon.is_finite()) || !(-80.0..=84.0).contains(&lat) {
        return None;
    }
    let z = zone(lat, lon);
    let meridian = f64::from(z * 6 - 183);
    let mut dlon = lon - meridian;
    dlon = (dlon + 180.0).rem_euclid(360.0) - 180.0;
    let (n, e) = forward(lat, dlon);
    let easting = FALSE_EASTING + e;
    let northing = if lat < 0.0 {
        FALSE_NORTHING_SOUTH + n
    } else {
        n
    };
    let band = BANDS[(((lat + 80.0) / 8.0).floor() as usize).min(BANDS.len() - 1)];
    let col = (easting / 100_000.0).floor() as usize;
    let column = *COLUMNS[((z - 1) % 3) as usize].get(col.checked_sub(1)?)?;
    let shift = if z % 2 == 0 { 5 } else { 0 };
    let row = ROWS[((northing / 100_000.0).floor() as usize + shift) % ROWS.len()];
    let within = |v: f64| (v.rem_euclid(100_000.0)).floor() as u32;
    Some(format!(
        "{z}{} {}{} {:05} {:05}",
        band as char,
        column as char,
        row as char,
        within(easting),
        within(northing)
    ))
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
}
