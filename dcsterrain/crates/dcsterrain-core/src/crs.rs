//! The map projection: DCS `x` (north) and `z` (east) are a WGS84 transverse
//! Mercator with `k_0 = 0.9996`, a whole-degree central meridian and a false
//! origin.
//!
//! The series is Krüger's to sixth order in the third flattening, which is
//! good to well under a millimetre within a few thousand kilometres of the
//! central meridian.

const A: f64 = 6_378_137.0;
const F: f64 = 1.0 / 298.257_223_563;
pub const K_0: f64 = 0.9996;

/// One map's projection. The offsets are what the projected northing and
/// easting exceed DCS `x` and `z` by.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tmerc {
    pub lon_0: i32,
    pub easting: f64,
    pub northing: f64,
}

struct Series {
    /// The rectifying radius scaled by `k_0`.
    k0a: f64,
    e: f64,
    alpha: [f64; 6],
    beta: [f64; 6],
    delta: [f64; 6],
}

fn series() -> Series {
    let n = F / (2.0 - F);
    let (n2, n3, n4, n5, n6) = (n * n, n.powi(3), n.powi(4), n.powi(5), n.powi(6));
    let rect = A / (1.0 + n) * (1.0 + n2 / 4.0 + n4 / 64.0 + n6 / 256.0);
    Series {
        k0a: K_0 * rect,
        e: (F * (2.0 - F)).sqrt(),
        alpha: [
            n / 2.0 - 2.0 * n2 / 3.0 + 5.0 * n3 / 16.0 + 41.0 * n4 / 180.0 - 127.0 * n5 / 288.0
                + 7891.0 * n6 / 37800.0,
            13.0 * n2 / 48.0 - 3.0 * n3 / 5.0 + 557.0 * n4 / 1440.0 + 281.0 * n5 / 630.0
                - 1983433.0 * n6 / 1935360.0,
            61.0 * n3 / 240.0 - 103.0 * n4 / 140.0
                + 15061.0 * n5 / 26880.0
                + 167603.0 * n6 / 181440.0,
            49561.0 * n4 / 161280.0 - 179.0 * n5 / 168.0 + 6601661.0 * n6 / 7257600.0,
            34729.0 * n5 / 80640.0 - 3418889.0 * n6 / 1995840.0,
            212378941.0 * n6 / 319334400.0,
        ],
        beta: [
            n / 2.0 - 2.0 * n2 / 3.0 + 37.0 * n3 / 96.0 - n4 / 360.0 - 81.0 * n5 / 512.0
                + 96199.0 * n6 / 604800.0,
            n2 / 48.0 + n3 / 15.0 - 437.0 * n4 / 1440.0 + 46.0 * n5 / 105.0
                - 1118711.0 * n6 / 3870720.0,
            17.0 * n3 / 480.0 - 37.0 * n4 / 840.0 - 209.0 * n5 / 4480.0 + 5569.0 * n6 / 90720.0,
            4397.0 * n4 / 161280.0 - 11.0 * n5 / 504.0 - 830251.0 * n6 / 7257600.0,
            4583.0 * n5 / 161280.0 - 108847.0 * n6 / 3991680.0,
            20648693.0 * n6 / 638668800.0,
        ],
        delta: [
            2.0 * n - 2.0 * n2 / 3.0 - 2.0 * n3 + 116.0 * n4 / 45.0 + 26.0 * n5 / 45.0
                - 2854.0 * n6 / 675.0,
            7.0 * n2 / 3.0 - 8.0 * n3 / 5.0 - 227.0 * n4 / 45.0
                + 2704.0 * n5 / 315.0
                + 2323.0 * n6 / 945.0,
            56.0 * n3 / 15.0 - 136.0 * n4 / 35.0 - 1262.0 * n5 / 105.0 + 73814.0 * n6 / 2835.0,
            4279.0 * n4 / 630.0 - 332.0 * n5 / 35.0 - 399572.0 * n6 / 14175.0,
            4174.0 * n5 / 315.0 - 144838.0 * n6 / 6237.0,
            601676.0 * n6 / 22275.0,
        ],
    }
}

/// Northing and easting from the central meridian and the equator, with no
/// false offsets.
fn forward(lat_deg: f64, dlon_deg: f64) -> (f64, f64) {
    let s = series();
    let (phi, lam) = (lat_deg.to_radians(), dlon_deg.to_radians());
    let t = (phi.sin().atanh() - s.e * (s.e * phi.sin()).atanh()).sinh();
    let xi_p = t.atan2(lam.cos());
    let eta_p = (lam.sin() / (1.0 + t * t).sqrt()).atanh();
    let (mut xi, mut eta) = (xi_p, eta_p);
    for (j, a) in s.alpha.iter().enumerate() {
        let k = 2.0 * (j + 1) as f64;
        xi += a * (k * xi_p).sin() * (k * eta_p).cosh();
        eta += a * (k * xi_p).cos() * (k * eta_p).sinh();
    }
    (s.k0a * xi, s.k0a * eta)
}

/// Latitude and longitude from the central meridian, in degrees.
fn inverse(northing: f64, easting: f64) -> (f64, f64) {
    let s = series();
    let (xi, eta) = (northing / s.k0a, easting / s.k0a);
    let (mut xi_p, mut eta_p) = (xi, eta);
    for (j, b) in s.beta.iter().enumerate() {
        let k = 2.0 * (j + 1) as f64;
        xi_p -= b * (k * xi).sin() * (k * eta).cosh();
        eta_p -= b * (k * xi).cos() * (k * eta).sinh();
    }
    let chi = (xi_p.sin() / eta_p.cosh()).asin();
    let mut phi = chi;
    for (j, d) in s.delta.iter().enumerate() {
        phi += d * (2.0 * (j + 1) as f64 * chi).sin();
    }
    let lam = eta_p.sinh().atan2(xi_p.cos());
    (phi.to_degrees(), lam.to_degrees())
}

impl Tmerc {
    /// DCS `x` and `z` of a latitude and longitude.
    pub fn to_xz(&self, lat: f64, lon: f64) -> (f64, f64) {
        let (n, e) = forward(lat, lon - f64::from(self.lon_0));
        (n - self.northing, e - self.easting)
    }

    /// Latitude and longitude of DCS `x` and `z`.
    pub fn to_latlon(&self, x: f64, z: f64) -> (f64, f64) {
        let (lat, dlon) = inverse(x + self.northing, z + self.easting);
        (lat, dlon + f64::from(self.lon_0))
    }

    /// The PROJ string whose forward direction gives `(z, x)`: PROJ adds its
    /// false origin, so the offsets are negated into it.
    pub fn proj4(&self) -> String {
        format!(
            "+proj=tmerc +lat_0=0 +lon_0={} +k_0={K_0} +x_0={} +y_0={} +datum=WGS84 +units=m",
            self.lon_0, -self.easting, -self.northing
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAUCASUS: Tmerc = Tmerc {
        lon_0: 33,
        easting: 99517.0,
        northing: 4998115.0,
    };

    // Published UTM figures, which share k_0 = 0.9996: 3 degrees east of the
    // central meridian on the equator is 333 978.556 m east of it, and 45
    // degrees north on the meridian is 4 982 950.400 m north.
    #[test]
    fn matches_published_utm_figures() {
        let (n, e) = forward(0.0, 3.0);
        assert!(n.abs() < 1e-6 && (e - 333_978.556).abs() < 0.005, "{n} {e}");
        let (n, e) = forward(45.0, 0.0);
        assert!(
            (n - 4_982_950.400).abs() < 0.005 && e.abs() < 1e-6,
            "{n} {e}"
        );
    }

    #[test]
    fn round_trips_to_a_millimetre() {
        for (x, z) in [
            (0.0, 0.0),
            (-30_000.0, -45_000.0),
            (45_000.0, 30_000.0),
            (-300_000.0, 600_000.0),
        ] {
            let (lat, lon) = CAUCASUS.to_latlon(x, z);
            let (x2, z2) = CAUCASUS.to_xz(lat, lon);
            assert!(
                (x2 - x).abs() < 1e-3 && (z2 - z).abs() < 1e-3,
                "{x} {z} -> {x2} {z2}"
            );
        }
    }

    #[test]
    fn the_proj_string_negates_the_offsets() {
        assert_eq!(
            CAUCASUS.proj4(),
            "+proj=tmerc +lat_0=0 +lon_0=33 +k_0=0.9996 +x_0=-99517 +y_0=-4998115 +datum=WGS84 +units=m"
        );
    }
}
