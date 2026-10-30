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
pub(crate) fn forward(lat_deg: f64, dlon_deg: f64) -> (f64, f64) {
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
            self.lon_0,
            negated(self.easting),
            negated(self.northing)
        )
    }

    /// The bearing from `a` to `b`, each DCS `[x, z]`, in degrees clockwise
    /// from true north at `a`, 0 when they coincide: the azimuth of the
    /// normal section, the plane holding the ellipsoid's normal at `a` and
    /// the point `b`, taken in `a`'s local east-north plane. It departs
    /// from the geodesic's initial azimuth as the square of the distance:
    /// under 1e-5° within 100 km, under 2.5e-4° within 500 km.
    pub fn true_bearing(&self, a: [f64; 2], b: [f64; 2]) -> f64 {
        let (lat, lon) = self.to_latlon(a[0], a[1]);
        let (lat_b, lon_b) = self.to_latlon(b[0], b[1]);
        let (pa, pb) = (ecef(lat, lon), ecef(lat_b, lon_b));
        let d = [pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]];
        let (sp, cp) = lat.to_radians().sin_cos();
        let (sl, cl) = lon.to_radians().sin_cos();
        let east = -sl * d[0] + cl * d[1];
        let north = -sp * cl * d[0] - sp * sl * d[1] + cp * d[2];
        bearing(north, east)
    }
}

/// The bearing from `a` to `b`, each DCS `[x, z]`, in degrees clockwise
/// from grid north, DCS `x`, 0 when they coincide. It differs from the true
/// bearing by the meridian convergence, which grows with the distance from
/// the central meridian and the latitude: about 8° at Caucasus's east edge.
pub fn grid_bearing(a: [f64; 2], b: [f64; 2]) -> f64 {
    bearing(b[0] - a[0], b[1] - a[1])
}

/// Degrees clockwise from the north axis, in [0, 360).
fn bearing(north: f64, east: f64) -> f64 {
    degrees(east.atan2(north))
}

/// An angle in radians clockwise from north as degrees in [0, 360).
pub fn degrees(rad: f64) -> f64 {
    // Adding zero turns a negative zero into a positive one.
    rad.to_degrees().rem_euclid(360.0) % 360.0 + 0.0
}

/// Earth-centered coordinates of a position on the ellipsoid.
fn ecef(lat: f64, lon: f64) -> [f64; 3] {
    let e2 = F * (2.0 - F);
    let (sp, cp) = lat.to_radians().sin_cos();
    let (sl, cl) = lon.to_radians().sin_cos();
    let n = A / (1.0 - e2 * sp * sp).sqrt();
    [n * cp * cl, n * cp * sl, n * (1.0 - e2) * sp]
}

/// `-v`, without the sign a zero would print with.
fn negated(v: f64) -> f64 {
    if v == 0.0 { 0.0 } else { -v }
}

/// Each measured map's projection, by theatre id, for a map whose files
/// carry too few positioned beacons to fit one. Nevada's row was fitted
/// from its beacons; the rest were measured in a running sim.
pub const TABLE: [(&str, Tmerc); 9] = [
    ("Afghanistan", tm(63, 300150.0, 3759657.0)),
    ("Caucasus", tm(33, 99517.0, 4998115.0)),
    ("GermanyCW", tm(21, -35427.62, 6061633.128)),
    ("MarianaIslands", tm(147, -238418.0, 1491840.0)),
    ("MarianaIslandsWWII", tm(147, -238418.0, 1491840.0)),
    ("Nevada", tm(-117, 193996.81, 4410028.06)),
    ("PersianGulf", tm(57, -75756.0, 2894933.0)),
    ("SinaiMap", tm(33, -169222.0, 3325313.0)),
    ("Syria", tm(39, -282801.0, 3879866.0)),
];

const fn tm(lon_0: i32, easting: f64, northing: f64) -> Tmerc {
    Tmerc {
        lon_0,
        easting,
        northing,
    }
}

pub fn table(theatre: &str) -> Option<Tmerc> {
    TABLE.iter().find(|(id, _)| *id == theatre).map(|(_, t)| *t)
}

/// One point given both ways: DCS metres and degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pair {
    pub x: f64,
    pub z: f64,
    pub lat: f64,
    pub lon: f64,
}

/// A projection fitted to pairs, and how well it fits the ones it kept.
#[derive(Debug, Clone, PartialEq)]
pub struct Fit {
    pub tmerc: Tmerc,
    /// The pairs the fit kept, distinct and in the order given.
    pub kept: Vec<Pair>,
    pub rms_m: f64,
    pub max_m: f64,
}

/// The fewest distinct pairs a fit is made from.
pub const MIN_PAIRS: usize = 3;

/// A pair this far from the best meridian's offsets is not used: a wrong
/// meridian misses by kilometres, and six-decimal degrees by centimetres.
const OUTLIER_M: f64 = 5.0;

/// The longitudes either side of the pairs' span that are tried as the
/// central meridian. A map sits within a few degrees of its own.
const MERIDIAN_REACH_DEG: i32 = 12;

/// The widest span of longitude a fit is tried over. Every map spans a few
/// degrees; much wider is not one map, or not degrees.
const MAX_SPAN_DEG: f64 = 30.0;

/// The median of the values, the mean of the middle two of an even count;
/// `None` of none.
pub fn median(mut v: Vec<f64>) -> Option<f64> {
    v.sort_by(f64::total_cmp);
    let m = v.len() / 2;
    match v.len() {
        0 => None,
        n if n % 2 == 1 => Some(v[m]),
        _ => Some((v[m - 1] + v[m]) / 2.0),
    }
}

/// Fits the central meridian and the offsets to `pairs`. Each whole-degree
/// meridian near them is scored by the RMS of the residuals about the median
/// offsets; the best one's offsets are then the means over the pairs within
/// 5 m, rounded to the millimetre. `None` with fewer than three distinct
/// pairs, before or after that cut, with a latitude past a pole, or with
/// longitudes spanning more than 30 degrees.
pub fn fit(pairs: &[Pair]) -> Option<Fit> {
    let mut unique: Vec<Pair> = Vec::new();
    for p in pairs {
        if !unique.contains(p) {
            unique.push(*p);
        }
    }
    if unique.len() < MIN_PAIRS {
        return None;
    }
    let (lo, hi) = unique.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| {
        (lo.min(p.lon), hi.max(p.lon))
    });
    // NaN fails both comparisons, so it is off the earth too.
    let on_earth = |p: &Pair| p.lat.abs() <= 90.0 && p.lon.abs() <= 180.0;
    if !unique.iter().all(on_earth) || hi - lo > MAX_SPAN_DEG {
        return None;
    }
    // How far each pair's projection sits from its DCS position, per axis.
    let offsets = |lon_0: i32| -> Vec<(f64, f64)> {
        unique
            .iter()
            .map(|p| {
                let (n, e) = forward(p.lat, p.lon - f64::from(lon_0));
                (e - p.z, n - p.x)
            })
            .collect()
    };
    let residuals = |d: &[(f64, f64)], e: f64, n: f64| -> Vec<f64> {
        d.iter().map(|(de, dn)| (de - e).hypot(dn - n)).collect()
    };
    let rms = |r: &[f64]| (r.iter().map(|r| r * r).sum::<f64>() / r.len() as f64).sqrt();

    let first = lo.floor() as i32 - MERIDIAN_REACH_DEG;
    let last = hi.ceil() as i32 + MERIDIAN_REACH_DEG;
    let (lon_0, d, e, n) = (first..=last)
        .map(|l| {
            let d = offsets(l);
            let e = median(d.iter().map(|o| o.0).collect()).expect("a fit has pairs");
            let n = median(d.iter().map(|o| o.1).collect()).expect("a fit has pairs");
            let score = rms(&residuals(&d, e, n));
            (score, l, d, e, n)
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, l, d, e, n)| (l, d, e, n))?;

    let near: Vec<usize> = residuals(&d, e, n)
        .iter()
        .enumerate()
        .filter(|(_, r)| **r < OUTLIER_M)
        .map(|(i, _)| i)
        .collect();
    if near.len() < MIN_PAIRS {
        return None;
    }
    let mean = |f: fn(&(f64, f64)) -> f64| {
        let s: f64 = near.iter().map(|&i| f(&d[i])).sum();
        (s / near.len() as f64 * 1000.0).round() / 1000.0
    };
    let tmerc = Tmerc {
        lon_0,
        easting: mean(|o| o.0),
        northing: mean(|o| o.1),
    };
    let kept_d: Vec<(f64, f64)> = near.iter().map(|&i| d[i]).collect();
    let r = residuals(&kept_d, tmerc.easting, tmerc.northing);
    Some(Fit {
        tmerc,
        kept: near.iter().map(|&i| unique[i]).collect(),
        rms_m: rms(&r),
        max_m: r.iter().copied().fold(0.0, f64::max),
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    const CAUCASUS: Tmerc = tm(33, 99517.0, 4998115.0);

    /// Pairs as `Beacons.lua` gives them: degrees to six decimals.
    fn pairs(t: &Tmerc, at: &[(f64, f64)]) -> Vec<Pair> {
        let six = |v: f64| (v * 1e6).round() / 1e6;
        at.iter()
            .map(|&(x, z)| {
                let (lat, lon) = t.to_latlon(x, z);
                Pair {
                    x,
                    z,
                    lat: six(lat),
                    lon: six(lon),
                }
            })
            .collect()
    }

    const SPREAD: [(f64, f64); 6] = [
        (-1321.8, 246748.5),
        (-356584.8, 618472.4),
        (-300000.0, 900000.0),
        (-50000.0, 400000.0),
        (-200000.0, 700000.0),
        (-120000.0, 300000.0),
    ];

    #[test]
    fn recovers_the_meridian_and_offsets_from_rounded_degrees() {
        for (id, t) in TABLE {
            // Each map's own spread, in its own frame.
            let at: Vec<(f64, f64)> = SPREAD
                .iter()
                .map(|&(x, z)| (x / 4.0, (z - 500000.0) / 4.0))
                .collect();
            let f = fit(&pairs(&t, &at)).unwrap();
            assert_eq!(f.tmerc.lon_0, t.lon_0, "{id}");
            assert!(
                (f.tmerc.easting - t.easting).abs() < 0.05
                    && (f.tmerc.northing - t.northing).abs() < 0.05,
                "{id}: {f:?}"
            );
            assert!(f.max_m < 0.1 && f.rms_m <= f.max_m, "{id}: {f:?}");
            assert_eq!(f.kept.len(), at.len());
        }
    }

    #[test]
    fn drops_a_pair_far_from_the_rest_and_a_repeated_one() {
        let mut p = pairs(&CAUCASUS, &SPREAD);
        p.push(p[0]);
        p[1].x += 40.0;
        let f = fit(&p).unwrap();
        assert_eq!(f.tmerc.lon_0, 33);
        assert_eq!(f.kept.len(), SPREAD.len() - 1);
        assert!(!f.kept.contains(&p[1]));
        assert!((f.tmerc.northing - CAUCASUS.northing).abs() < 0.05, "{f:?}");
    }

    #[test]
    fn fewer_than_three_distinct_pairs_fit_nothing() {
        let p = pairs(&CAUCASUS, &SPREAD[..2]);
        assert_eq!(fit(&p), None);
        assert_eq!(fit(&[p[0], p[0], p[1]]), None);
        assert_eq!(fit(&[]), None);
    }

    #[test]
    fn degrees_no_map_could_hold_fit_nothing() {
        let mut p = pairs(&CAUCASUS, &SPREAD);
        p[0].lon = 1e10;
        assert_eq!(fit(&p), None);
        p[0].lon = f64::INFINITY;
        assert_eq!(fit(&p), None);
        p[0].lon = f64::NAN;
        assert_eq!(fit(&p), None);
        let mut p = pairs(&CAUCASUS, &SPREAD);
        p[0].lat = 91.0;
        assert_eq!(fit(&p), None);
    }

    #[test]
    fn a_zero_offset_prints_without_a_sign() {
        assert!(tm(0, 0.0, 0.0).proj4().contains("+x_0=0 +y_0=0 "));
    }

    #[test]
    fn looks_up_a_theatre_in_the_table() {
        assert_eq!(table("Caucasus"), Some(CAUCASUS));
        assert_eq!(table("MarianaIslandsWWII").map(|t| t.lon_0), Some(147));
        assert_eq!(table("Kola"), None);
    }

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

    /// The geodesic's initial azimuth in degrees, by Vincenty's inverse
    /// iteration on WGS84: the reference a bearing is checked against.
    pub(crate) fn geodesic_azimuth(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
        let reduced = |lat: f64| ((1.0 - F) * lat.to_radians().tan()).atan();
        let (su1, cu1) = reduced(lat1).sin_cos();
        let (su2, cu2) = reduced(lat2).sin_cos();
        let l = (lon2 - lon1).to_radians();
        let mut lam = l;
        for _ in 0..200 {
            let (sl, cl) = lam.sin_cos();
            let ss = ((cu2 * sl).powi(2) + (cu1 * su2 - su1 * cu2 * cl).powi(2)).sqrt();
            let cs = su1 * su2 + cu1 * cu2 * cl;
            let sigma = ss.atan2(cs);
            let sa = cu1 * cu2 * sl / ss;
            let c2a = 1.0 - sa * sa;
            let c2sm = if c2a == 0.0 {
                0.0
            } else {
                cs - 2.0 * su1 * su2 / c2a
            };
            let c = F / 16.0 * c2a * (4.0 + F * (4.0 - 3.0 * c2a));
            let next = l
                + (1.0 - c)
                    * F
                    * sa
                    * (sigma + c * ss * (c2sm + c * cs * (-1.0 + 2.0 * c2sm * c2sm)));
            let done = (next - lam).abs() < 1e-14;
            lam = next;
            if done {
                break;
            }
        }
        let (sl, cl) = lam.sin_cos();
        (cu2 * sl)
            .atan2(cu1 * su2 - su1 * cu2 * cl)
            .to_degrees()
            .rem_euclid(360.0)
    }

    pub(crate) fn turn(a: f64, b: f64) -> f64 {
        ((a - b + 540.0).rem_euclid(360.0) - 180.0).abs()
    }

    #[test]
    fn the_true_bearing_is_the_geodesic_azimuth_within_its_bound() {
        let mut u = crate::roads::tests::Unit(0x9e37_79b9_7f4a_7c15);
        // The worst within 100 km, and within 500 km.
        let mut worst = [0.0f64; 2];
        for (_, t) in TABLE {
            for _ in 0..2000 {
                let a = [(u.next() - 0.5) * 1.2e6, (u.next() - 0.5) * 1.2e6];
                let (r, th) = (
                    u.next().powi(2) * 500e3 + 1.0,
                    u.next() * std::f64::consts::TAU,
                );
                let b = [a[0] + r * th.cos(), a[1] + r * th.sin()];
                let (lat1, lon1) = t.to_latlon(a[0], a[1]);
                let (lat2, lon2) = t.to_latlon(b[0], b[1]);
                let want = geodesic_azimuth(lat1, lon1, lat2, lon2);
                let off = turn(t.true_bearing(a, b), want);
                if r <= 100e3 {
                    worst[0] = worst[0].max(off);
                }
                worst[1] = worst[1].max(off);
            }
        }
        assert!(worst[0] < 1e-5 && worst[1] < 2.5e-4, "{worst:?}°");
    }

    #[test]
    fn north_along_a_meridian_is_true_north_and_grid_north_is_x() {
        // 30 km east of the central meridian a meridian leans west of grid
        // north, so the two bearings differ by the convergence.
        let a = CAUCASUS.to_xz(43.0, 33.4);
        let b = CAUCASUS.to_xz(43.2, 33.4);
        let (a, b) = ([a.0, a.1], [b.0, b.1]);
        assert!(turn(CAUCASUS.true_bearing(a, b), 0.0) < 1e-6);
        let grid = grid_bearing(a, b);
        // Convergence is about the longitude off the meridian times the
        // sine of the latitude.
        let want = 0.4 * 43.1f64.to_radians().sin();
        assert!((grid - (360.0 - want)).abs() < 2e-3, "{grid}");
        assert_eq!(grid_bearing([0.0, 0.0], [-1.0, 0.0]), 180.0);
        assert_eq!(grid_bearing([0.0, 0.0], [0.0, 1.0]), 90.0);
        assert_eq!(grid_bearing([0.0, 0.0], [0.0, 0.0]), 0.0);
        assert_eq!(CAUCASUS.true_bearing(a, a), 0.0);
    }
}
