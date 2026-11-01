//! Draws the samples and records what the tool answers for each.
//!
//! The terrain is stratified twice. Windows on a jittered grid spread the
//! samples over the terrain, and each sample then falls in a class of ground,
//! so a few steep or wet points are not swamped by the plains around them.
//! The draw is fixed by the seed, so a run can be repeated after an update.

use std::collections::{BTreeMap, HashMap};

use dcsterrain_core::query::{Terrain, answer};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// A window's side at most, in meters, and the windows along each axis.
const WINDOW_M: f64 = 20_000.0;
const WINDOWS: usize = 12;
/// The lattice points along a window's side.
const LATTICE: f64 = 100.0;
/// Off the lattice by less than half a cell, so a point stays in the class
/// its cell was given but falls anywhere on the mesh's triangles.
const JITTER_M: f64 = 24.0;
/// The farthest from a network a route's end may lie, and the farthest from
/// water a point counts as shore, in meters.
const NEAR_M: f64 = 100.0;
/// A line this close to grazing the ground, in meters, is a hard case. An
/// agreement figure over random lines is mostly easy ones.
const MARGINAL_M: f64 = 1.0;

const SIGHT_BANDS: [(&str, f64, f64); 4] = [
    ("0.5-2km", 500.0, 2_000.0),
    ("2-8km", 2_000.0, 8_000.0),
    ("8-25km", 8_000.0, 25_000.0),
    ("25-60km", 25_000.0, 60_000.0),
];
/// Heights above the ground of the two ends: troops to troops, a mast, an
/// aircraft.
const SIGHT_AGL: [(f64, f64); 3] = [(2.0, 2.0), (2.0, 100.0), (2.0, 500.0)];
/// A detour no road takes: the game bounds none.
const ANY_DETOUR: f64 = 1e6;
const ROUTE_BANDS: [(&str, f64, f64); 4] = [
    ("2-10km", 2_000.0, 10_000.0),
    ("10-40km", 10_000.0, 40_000.0),
    ("40-120km", 40_000.0, 120_000.0),
    ("120-400km", 120_000.0, 400_000.0),
];

/// How many samples to draw.
#[derive(Debug, Clone, Copy)]
pub struct Sizes {
    pub heights_per_stratum: usize,
    pub points_per_runway: usize,
    pub sights_per_stratum: usize,
    pub marginal_sights: usize,
    pub roads_per_band: usize,
    pub rails_per_band: usize,
    pub mgrs: usize,
}

impl Default for Sizes {
    fn default() -> Self {
        Sizes {
            heights_per_stratum: 200,
            points_per_runway: 8,
            sights_per_stratum: 50,
            marginal_sights: 300,
            roads_per_band: 40,
            rails_per_band: 15,
            mgrs: 400,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Plan {
    pub terrain: String,
    pub dcs_build: String,
    pub terrain_fingerprint: String,
    pub seed: u64,
    pub heights: Vec<HeightCase>,
    pub sights: Vec<SightCase>,
    pub routes: Vec<RouteCase>,
    pub mgrs: Vec<MgrsCase>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HeightCase {
    pub stratum: String,
    pub x: f64,
    pub z: f64,
    pub height: f64,
    pub water: f64,
    pub surface: f64,
}

/// A sightline, its ends as `[x, alt, z]`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SightCase {
    /// `random`, which the target is judged on, or `marginal`.
    pub set: String,
    pub stratum: String,
    pub a: [f64; 3],
    pub b: [f64; 3],
    pub visible: bool,
    pub clearance_m: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RouteCase {
    pub network: String,
    pub stratum: String,
    /// The tool's snapped ends, so both answers run between the same points,
    /// or the points asked where it refuses the route and snaps none.
    pub from: [f64; 2],
    pub to: [f64; 2],
    /// The tool's length; none where it refuses the route.
    pub length: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MgrsCase {
    pub stratum: String,
    pub x: f64,
    pub z: f64,
    pub lat: f64,
    pub lon: f64,
    pub mgrs: String,
}

/// SplitMix64: enough for a sample, and the same on every platform.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }

    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }

    fn index(&mut self, n: usize) -> usize {
        (self.unit() * n as f64) as usize
    }

    fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            items.swap(i, self.index(i + 1));
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Row {
    window: usize,
    x: f64,
    z: f64,
    slope: f64,
    water: f64,
    surface: f64,
    cover: f64,
    road: f64,
    rail: f64,
    shore: f64,
}

impl Row {
    /// The class of ground. Where a decoder could go wrong differently comes
    /// first, so a road through a forest counts as a road.
    fn stratum(&self) -> &'static str {
        match () {
            _ if self.water == 2.0 => "sea",
            _ if self.water == 1.0 => "lake",
            _ if self.water == 3.0 => "river",
            _ if self.surface == 5.0 => "runway",
            _ if self.surface == 4.0 => "road",
            _ if self.shore <= NEAR_M => "shore",
            _ if self.cover == 4.0 => "town",
            _ if self.cover == 3.0 => "forest",
            _ if self.slope >= 20.0 => "steep",
            _ if self.slope >= 5.0 => "hilly",
            _ => "flat",
        }
    }
}

fn ask(terrain: &Terrain, op: &str, request: &Value) -> Result<Value, String> {
    answer(terrain, op, request).map_err(|issues| format!("{op}: {issues:?}"))
}

/// Items in an order that spreads over their groups: a shuffle, then each
/// group's first, then each group's second, and so on.
fn spread<T, K: std::hash::Hash + Eq>(
    rng: &mut Rng,
    mut items: Vec<T>,
    group: impl Fn(&T) -> K,
) -> Vec<T> {
    rng.shuffle(&mut items);
    let mut seen: HashMap<K, usize> = HashMap::new();
    let mut ranked: Vec<(usize, T)> = items
        .into_iter()
        .map(|item| {
            let n = seen.entry(group(&item)).or_insert(0);
            *n += 1;
            (*n, item)
        })
        .collect();
    ranked.sort_by_key(|(rank, _)| *rank);
    ranked.into_iter().map(|(_, item)| item).collect()
}

fn rows(terrain: &Terrain, rng: &mut Rng) -> Result<Vec<Row>, String> {
    let d = ask(terrain, "describe", &json!({}))?;
    let e = &d["extent"];
    let num = |k: &str| e[k].as_f64().ok_or(format!("describe gave no {k}"));
    let (min_x, min_z) = (num("min_x")?, num("min_z")?);
    let (max_x, max_z) = (num("max_x")?, num("max_z")?);
    let (cell_x, cell_z) = (
        (max_x - min_x) / WINDOWS as f64,
        (max_z - min_z) / WINDOWS as f64,
    );
    let side = WINDOW_M.min(cell_x).min(cell_z);
    let step = (side / LATTICE).max(50.0);
    let mut out = Vec::new();
    for i in 0..WINDOWS * WINDOWS {
        let x = min_x + cell_x * (i % WINDOWS) as f64 + rng.range(0.0, cell_x - side);
        let z = min_z + cell_z * (i / WINDOWS) as f64 + rng.range(0.0, cell_z - side);
        let reply = ask(
            terrain,
            "sample",
            &json!({
                "region": {"Box": {"min_x": x, "min_z": z, "max_x": x + side, "max_z": z + side}},
                "step": step,
                "max_rows": 20_000,
                "layers": ["height", "slope", "water", "surface", "cover",
                           "dist_road", "dist_rail", "dist_water"],
            }),
        )?;
        for r in reply["rows"].as_array().ok_or("sample gave no rows")? {
            let v = &r["values"];
            let get = |k: &str| v[k].as_f64();
            if get("height").is_none() {
                continue;
            }
            out.push(Row {
                window: i,
                x: r["x"].as_f64().ok_or("a row has no x")?,
                z: r["z"].as_f64().ok_or("a row has no z")?,
                slope: get("slope").unwrap_or(0.0),
                water: get("water").unwrap_or(0.0),
                surface: get("surface").unwrap_or(1.0),
                cover: get("cover").unwrap_or(1.0),
                road: get("dist_road").unwrap_or(f64::MAX),
                rail: get("dist_rail").unwrap_or(f64::MAX),
                shore: get("dist_water").unwrap_or(f64::MAX),
            });
        }
    }
    Ok(out)
}

/// The tool's height, water and surface at points, in order; `None` where
/// it has no height.
fn at_points(terrain: &Terrain, points: &[[f64; 2]]) -> Result<Vec<Option<[f64; 3]>>, String> {
    let mut out = Vec::with_capacity(points.len());
    for chunk in points.chunks(5_000) {
        let request: Vec<Value> = chunk
            .iter()
            .map(|p| json!({"x": p[0], "z": p[1]}))
            .collect();
        let reply = ask(
            terrain,
            "sample",
            &json!({"points": request, "layers": ["height", "water", "surface"]}),
        )?;
        let rows = reply["rows"].as_array().ok_or("sample gave no rows")?;
        // The cases pair with these rows by position.
        if rows.len() != chunk.len() {
            return Err(format!(
                "sample gave {} rows of {}",
                rows.len(),
                chunk.len()
            ));
        }
        for r in rows {
            let v = &r["values"];
            out.push(
                match (
                    v["height"].as_f64(),
                    v["water"].as_f64(),
                    v["surface"].as_f64(),
                ) {
                    (Some(h), Some(w), Some(s)) => Some([h, w, s]),
                    _ => None,
                },
            );
        }
    }
    Ok(out)
}

/// Points along each runway, where the terrain's mesh overlaps the airfield's
/// own surface, and which a lattice mostly misses.
fn runway_points(
    terrain: &Terrain,
    rows: &[Row],
    per_runway: usize,
) -> Result<Vec<[f64; 2]>, String> {
    let Some(r) = rows.first() else {
        return Ok(Vec::new());
    };
    let near = ask(
        terrain,
        "nearest",
        &json!({"kind": "airdrome", "x": r.x, "z": r.z, "n": 50}),
    )?;
    let mut out = Vec::new();
    for a in near["results"]
        .as_array()
        .ok_or("nearest gave no results")?
    {
        let id = a["id"].as_str().ok_or("an airdrome has no id")?;
        let id: u32 = id.parse().map_err(|_| format!("airdrome id {id}"))?;
        let info = ask(terrain, "airfield_info", &json!({"airdrome": id}))?;
        for runway in info["runways"].as_array().ok_or("no runways")? {
            let end = |i: usize| -> Result<[f64; 2], String> {
                let t = &runway["thresholds"][i];
                match (t["x"].as_f64(), t["z"].as_f64()) {
                    (Some(x), Some(z)) => Ok([x, z]),
                    _ => Err(format!("a runway has no threshold {i}: {runway}")),
                }
            };
            let (p, q) = (end(0)?, end(1)?);
            for i in 0..per_runway {
                let t = (i as f64 + 0.5) / per_runway as f64;
                out.push([p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]);
            }
        }
    }
    Ok(out)
}

fn heights(
    terrain: &Terrain,
    rows: &[Row],
    rng: &mut Rng,
    sizes: &Sizes,
) -> Result<Vec<HeightCase>, String> {
    let mut strata: BTreeMap<&str, Vec<Row>> = BTreeMap::new();
    for r in rows {
        strata.entry(r.stratum()).or_default().push(*r);
    }
    let mut points = Vec::new();
    let mut labels = Vec::new();
    for (name, rows) in strata {
        for r in spread(rng, rows, |r| r.window)
            .iter()
            .take(sizes.heights_per_stratum)
        {
            let (dx, dz) = (
                rng.range(-JITTER_M, JITTER_M),
                rng.range(-JITTER_M, JITTER_M),
            );
            points.push([r.x + dx, r.z + dz]);
            labels.push(name.to_string());
        }
    }
    for p in runway_points(terrain, rows, sizes.points_per_runway)? {
        points.push(p);
        labels.push("runway".to_string());
    }
    let answers = at_points(terrain, &points)?;
    Ok(points
        .iter()
        .zip(labels)
        .zip(answers)
        .filter_map(|((p, stratum), a)| {
            a.map(|[height, water, surface]| HeightCase {
                stratum,
                x: p[0],
                z: p[1],
                height,
                water,
                surface,
            })
        })
        .collect())
}

fn sights(
    terrain: &Terrain,
    rows: &[Row],
    rng: &mut Rng,
    sizes: &Sizes,
) -> Result<Vec<SightCase>, String> {
    let land: Vec<&Row> = rows.iter().filter(|r| r.water == 0.0).collect();
    if land.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    let mut hard: Vec<SightCase> = Vec::new();
    // Enough draws that the rare grazing lines turn up.
    let tries = sizes.sights_per_stratum * 16;
    for (band, near, far) in SIGHT_BANDS {
        for (agl_a, agl_b) in SIGHT_AGL {
            let stratum = format!("{band}/{agl_a}-{agl_b}m");
            let mut ends = Vec::with_capacity(tries * 2);
            for _ in 0..tries {
                let a = land[rng.index(land.len())];
                let range = rng.range(near, far);
                let bearing = rng.range(0.0, std::f64::consts::TAU);
                ends.push([a.x, a.z]);
                ends.push([a.x + range * bearing.cos(), a.z + range * bearing.sin()]);
            }
            let ground = at_points(terrain, &ends)?;
            let mut random = 0;
            for (e, g) in ends.chunks(2).zip(ground.chunks(2)) {
                let (Some(ga), Some(gb)) = (g[0], g[1]) else {
                    continue;
                };
                let a = [e[0][0], ga[0] + agl_a, e[0][1]];
                let b = [e[1][0], gb[0] + agl_b, e[1][1]];
                let reply = answer(
                    terrain,
                    "visible",
                    &json!({"a": {"x": a[0], "alt": a[1], "z": a[2]},
                            "b": {"x": b[0], "alt": b[1], "z": b[2]}}),
                );
                let Ok(reply) = reply else { continue };
                let case = SightCase {
                    set: "random".to_string(),
                    stratum: stratum.clone(),
                    a,
                    b,
                    visible: reply["visible"].as_bool().ok_or("visible gave no answer")?,
                    clearance_m: reply["clearance_m"].as_f64().ok_or("no clearance")?,
                };
                if random < sizes.sights_per_stratum {
                    random += 1;
                    out.push(case);
                } else if case.clearance_m.abs() <= MARGINAL_M {
                    hard.push(SightCase {
                        set: "marginal".to_string(),
                        ..case
                    });
                }
            }
        }
    }
    out.extend(
        spread(rng, hard, |c| c.stratum.clone())
            .into_iter()
            .take(sizes.marginal_sights),
    );
    Ok(out)
}

fn routes(terrain: &Terrain, rows: &[Row], rng: &mut Rng, sizes: &Sizes) -> Vec<RouteCase> {
    let mut out = Vec::new();
    for (network, quota) in [
        ("roads", sizes.roads_per_band),
        ("railroads", sizes.rails_per_band),
    ] {
        let ends: Vec<&Row> = rows
            .iter()
            .filter(|r| {
                let d = if network == "roads" { r.road } else { r.rail };
                d <= NEAR_M
            })
            .collect();
        if ends.is_empty() {
            continue;
        }
        for (band, near, far) in ROUTE_BANDS {
            let mut found = 0;
            for _ in 0..quota * 100 {
                if found == quota {
                    break;
                }
                let (a, b) = (ends[rng.index(ends.len())], ends[rng.index(ends.len())]);
                let straight = (a.x - b.x).hypot(a.z - b.z);
                if !(near..far).contains(&straight) {
                    continue;
                }
                // Far past any detour, so a refusal is the network's own.
                let reply = answer(
                    terrain,
                    "route",
                    &json!({"network": network,
                            "from": {"x": a.x, "z": a.z},
                            "to": {"x": b.x, "z": b.z},
                            "max_detour_factor": ANY_DETOUR}),
                );
                let reply = match reply {
                    Ok(reply) => Some(reply),
                    Err(e) if e.iter().any(|i| i.code == "no_route") => None,
                    Err(_) => continue,
                };
                let point = |k: &str, or: [f64; 2]| {
                    let Some(r) = &reply else { return Some(or) };
                    let p = &r[k];
                    Some([p["x"].as_f64()?, p["z"].as_f64()?])
                };
                let (Some(from), Some(to)) = (
                    point("snapped_from", [a.x, a.z]),
                    point("snapped_to", [b.x, b.z]),
                ) else {
                    continue;
                };
                let length = reply.as_ref().and_then(|r| r["length"].as_f64());
                found += 1;
                out.push(RouteCase {
                    network: network.to_string(),
                    stratum: band.to_string(),
                    from,
                    to,
                    length,
                });
            }
        }
    }
    out
}

/// Points just either side of each UTM zone edge and latitude band edge on
/// the terrain, where a reference changes letters and an off-by-one shows.
fn seams(terrain: &Terrain, cases: &[HeightCase]) -> Result<Vec<[f64; 2]>, String> {
    const SIDE_DEG: f64 = 0.0005;
    const ALONG: usize = 10;
    let all: Vec<Value> = cases.iter().map(|c| json!({"x": c.x, "z": c.z})).collect();
    let reply = ask(terrain, "geo", &json!({"to_latlon": all}))?;
    let list = reply["to_latlon"].as_array().ok_or("geo gave no list")?;
    let span = |k: &str| {
        list.iter()
            .filter_map(|p| p[k].as_f64())
            .fold((f64::MAX, f64::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)))
    };
    let ((lat0, lat1), (lon0, lon1)) = (span("lat"), span("lon"));
    let edges = |lo: f64, hi: f64, step: f64| -> Vec<f64> {
        let first = (lo / step).ceil() as i64;
        let last = (hi / step).floor() as i64;
        (first..=last).map(|k| k as f64 * step).collect()
    };
    let mut wanted = Vec::new();
    for lon in edges(lon0, lon1, 6.0) {
        for i in 0..ALONG {
            let lat = lat0 + (lat1 - lat0) * (i as f64 + 0.5) / ALONG as f64;
            wanted.push(json!({"lat": lat, "lon": lon - SIDE_DEG}));
            wanted.push(json!({"lat": lat, "lon": lon + SIDE_DEG}));
        }
    }
    for lat in edges(lat0, lat1, 8.0) {
        for i in 0..ALONG {
            let lon = lon0 + (lon1 - lon0) * (i as f64 + 0.5) / ALONG as f64;
            wanted.push(json!({"lat": lat - SIDE_DEG, "lon": lon}));
            wanted.push(json!({"lat": lat + SIDE_DEG, "lon": lon}));
        }
    }
    let reply = ask(terrain, "geo", &json!({"to_xz": wanted}))?;
    Ok(reply["to_xz"]
        .as_array()
        .ok_or("geo gave no list")?
        .iter()
        .filter_map(|p| Some([p["x"].as_f64()?, p["z"].as_f64()?]))
        .collect())
}

fn mgrs(
    terrain: &Terrain,
    heights: &[HeightCase],
    rng: &mut Rng,
    sizes: &Sizes,
) -> Result<Vec<MgrsCase>, String> {
    let mut points: Vec<(String, [f64; 2])> =
        spread(rng, heights.iter().collect(), |c| c.stratum.clone())
            .into_iter()
            .take(sizes.mgrs)
            .map(|c| (c.stratum.clone(), [c.x, c.z]))
            .collect();
    points.extend(
        seams(terrain, heights)?
            .into_iter()
            .map(|p| ("seam".to_string(), p)),
    );
    let all: Vec<Value> = points
        .iter()
        .map(|(_, p)| json!({"x": p[0], "z": p[1]}))
        .collect();
    let reply = ask(terrain, "geo", &json!({"to_latlon": all}))?;
    Ok(points
        .iter()
        .zip(reply["to_latlon"].as_array().ok_or("geo gave no list")?)
        .filter_map(|((stratum, p), g)| {
            Some(MgrsCase {
                stratum: stratum.clone(),
                x: p[0],
                z: p[1],
                lat: g["lat"].as_f64()?,
                lon: g["lon"].as_f64()?,
                mgrs: g["mgrs"].as_str()?.to_string(),
            })
        })
        .collect())
}

/// Draws every sample and records the tool's answer to it.
pub fn plan(terrain: &Terrain, seed: u64, sizes: &Sizes) -> Result<Plan, String> {
    let id = terrain.provenance();
    let mut rng = Rng::new(seed);
    let rows = rows(terrain, &mut rng)?;
    let heights = heights(terrain, &rows, &mut rng, sizes)?;
    let sights = sights(terrain, &rows, &mut rng, sizes)?;
    let routes = routes(terrain, &rows, &mut rng, sizes);
    let mgrs = mgrs(terrain, &heights, &mut rng, sizes)?;
    Ok(Plan {
        terrain: id.terrain,
        dcs_build: id.dcs_build,
        terrain_fingerprint: id.terrain_fingerprint,
        seed,
        heights,
        sights,
        routes,
        mgrs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_generator_is_fixed() {
        let mut r = Rng::new(1);
        assert_eq!(r.next(), 0x910A_2DEC_8902_5CC1);
    }

    #[test]
    fn spread_takes_each_group_in_turn() {
        let mut rng = Rng::new(7);
        let items = vec![(0, 1), (0, 2), (0, 3), (1, 4), (2, 5)];
        let out = spread(&mut rng, items, |i| i.0);
        let mut first: Vec<_> = out[..3].iter().map(|i| i.0).collect();
        first.sort_unstable();
        assert_eq!(first, [0, 1, 2]);
    }
}
