//! The packed file's `meta` keys, and their values from an extract.
//!
//! Every value is text, or NULL where the extract does not know it: without
//! the hook a run has no bounds, bullseyes or sea flag, and the validation
//! figures stay NULL until the validation sortie stamps them.

use serde_json::Value;

use super::{FORMAT_VERSION, TILE_SIZE};
use crate::extract::{CELL_SIZE_M, Extract};

/// Every key a packed file's `meta` carries, in the order `check` reports
/// them. ADR 0023 removed `terrain_digest`; ADR 0039 removed the fill triple
/// and replaced `has_mission_pass` with `has_hook`; ADR 0008 added the
/// authored rectangle.
pub const KEYS: [&str; 36] = [
    "format_version",
    "theatre_id",
    "dcs_build",
    "dcs_build_timestamp",
    "terrain_fingerprint",
    "extracted_at",
    "packed_at",
    "dcsterrain_version",
    "crs_proj4",
    "bounds_sw_x",
    "bounds_sw_z",
    "bounds_ne_x",
    "bounds_ne_z",
    "grid_origin_x",
    "grid_origin_z",
    "grid_width",
    "grid_height",
    "cell_size",
    "tile_size",
    "sea_enabled",
    "bullseye_blue_x",
    "bullseye_blue_z",
    "bullseye_red_x",
    "bullseye_red_z",
    "has_hook",
    "layers_dropped",
    "omit_sea_tiles",
    "authored_bounds_min_x",
    "authored_bounds_min_z",
    "authored_bounds_max_x",
    "authored_bounds_max_z",
    "authored_bounds_source",
    "validation_height_rmse",
    "validation_los_agreement",
    "validation_route_ratio",
    "validation_at",
];

/// The keys that may be NULL; every other key always has a value. An
/// extract this build reads always has an authored rectangle, but ADR 0009's
/// rule that a packed file may carry none, all five NULL, still stands.
pub const NULLABLE: [&str; 19] = [
    "crs_proj4",
    "bounds_sw_x",
    "bounds_sw_z",
    "bounds_ne_x",
    "bounds_ne_z",
    "sea_enabled",
    "bullseye_blue_x",
    "bullseye_blue_z",
    "bullseye_red_x",
    "bullseye_red_z",
    "validation_height_rmse",
    "validation_los_agreement",
    "validation_route_ratio",
    "validation_at",
    "authored_bounds_min_x",
    "authored_bounds_min_z",
    "authored_bounds_max_x",
    "authored_bounds_max_z",
    "authored_bounds_source",
];

/// The fingerprint files of ADR 0023, by their manifest role.
const FINGERPRINT_ROLES: [&str; 3] = ["surface", "roads", "scene"];

/// The `meta` rows for a pack of `x` at `cell_size`, in `KEYS` order. Fails
/// on a `config.json` value of the wrong shape, naming it.
pub fn from_extract(
    x: &Extract,
    config: &Value,
    cell_size: u32,
    packed_at: &str,
) -> Result<Vec<(&'static str, Option<String>)>, String> {
    let m = &x.manifest;
    let fingerprint: Vec<_> = m
        .files
        .iter()
        .filter(|f| FINGERPRINT_ROLES.contains(&f.role.as_str()))
        .collect();
    let fingerprint = serde_json::to_string(&fingerprint).map_err(|e| e.to_string())?;
    let crs_proj4 = match &config["crs"]["proj4"] {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        v => return Err(format!("crs.proj4 is {v}, not a string")),
    };
    let [sw_x, sw_z, ne_x, ne_z] = bounds_m(&config["bounds_km"])?;
    let sea = match &config["sea_enabled"] {
        Value::Null => None,
        Value::Bool(b) => Some(flag(*b)),
        v => return Err(format!("sea_enabled is {v}, not true or false")),
    };
    let bullseye = &config["default_bullseye"];
    let [blue_x, blue_z] = point(bullseye, "blue")?;
    let [red_x, red_z] = point(bullseye, "red")?;
    // The extract is always 50 m; a coarser base keeps the origin and covers
    // the same ground, its last cell overhanging where the count is odd.
    let cells = |n: u32| (n * CELL_SIZE_M).div_ceil(cell_size);
    let a = &m.authored_bounds_m;

    let some = |s: String| Some(s);
    let rows = [
        ("format_version", some(FORMAT_VERSION.to_string())),
        ("theatre_id", some(m.theatre.clone())),
        ("dcs_build", some(m.dcs_build.clone())),
        ("dcs_build_timestamp", some(m.dcs_build_timestamp.clone())),
        ("terrain_fingerprint", some(fingerprint)),
        ("extracted_at", some(m.extracted_at.clone())),
        ("packed_at", some(packed_at.to_string())),
        ("dcsterrain_version", some(env!("CARGO_PKG_VERSION").into())),
        ("crs_proj4", crs_proj4),
        ("bounds_sw_x", sw_x),
        ("bounds_sw_z", sw_z),
        ("bounds_ne_x", ne_x),
        ("bounds_ne_z", ne_z),
        ("grid_origin_x", some(m.grid.origin_x.to_string())),
        ("grid_origin_z", some(m.grid.origin_z.to_string())),
        ("grid_width", some(cells(m.grid.width).to_string())),
        ("grid_height", some(cells(m.grid.height).to_string())),
        ("cell_size", some(cell_size.to_string())),
        ("tile_size", some(TILE_SIZE.to_string())),
        ("sea_enabled", sea),
        ("bullseye_blue_x", blue_x),
        ("bullseye_blue_z", blue_z),
        ("bullseye_red_x", red_x),
        ("bullseye_red_z", red_z),
        ("has_hook", some(flag(m.hook.is_some()))),
        ("layers_dropped", some("[]".into())),
        ("omit_sea_tiles", some(flag(m.omit_sea_tiles))),
        ("authored_bounds_min_x", some(a.min_x.to_string())),
        ("authored_bounds_min_z", some(a.min_z.to_string())),
        ("authored_bounds_max_x", some(a.max_x.to_string())),
        ("authored_bounds_max_z", some(a.max_z.to_string())),
        (
            "authored_bounds_source",
            some(m.authored_bounds_source.clone()),
        ),
        ("validation_height_rmse", None),
        ("validation_los_agreement", None),
        ("validation_route_ratio", None),
        ("validation_at", None),
    ];
    debug_assert!(rows.iter().map(|r| r.0).eq(KEYS));
    Ok(rows.to_vec())
}

fn flag(b: bool) -> String {
    if b { "1" } else { "0" }.into()
}

/// `bounds_km` as `[sw_x, sw_z, ne_x, ne_z]` in metres (ADR 0008), each NULL
/// when the extract has no hook.
fn bounds_m(v: &Value) -> Result<[Option<String>; 4], String> {
    if v.is_null() {
        return Ok([None, None, None, None]);
    }
    let corner = |name: &str| -> Result<[f64; 2], String> {
        let bad = || format!("bounds_km.{name} is not an [x, z] pair of numbers");
        let pair = v[name]
            .as_array()
            .filter(|a| a.len() == 2)
            .ok_or_else(bad)?;
        let n = |i: usize| pair[i].as_f64().ok_or_else(bad);
        Ok([n(0)?, n(1)?])
    };
    // Rounded to the millimetre, so a kilometre figure with three decimals
    // does not come back with a binary tail.
    let m = |km: f64| Some(((km * 1e6).round() / 1e3).to_string());
    let [sw_x, sw_z] = corner("sw")?;
    let [ne_x, ne_z] = corner("ne")?;
    Ok([m(sw_x), m(sw_z), m(ne_x), m(ne_z)])
}

/// One bullseye's `[x, z]`, NULL when the extract has no hook.
fn point(bullseye: &Value, side: &str) -> Result<[Option<String>; 2], String> {
    let p = &bullseye[side];
    if p.is_null() {
        return Ok([None, None]);
    }
    let n = |k: &str| {
        p[k].as_f64()
            .map(|v| Some(v.to_string()))
            .ok_or_else(|| format!("default_bullseye.{side}.{k} is not a number"))
    };
    Ok([n("x")?, n("z")?])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratch::Scratch;
    use crate::synth;
    use serde_json::json;

    fn synth_extract(s: &Scratch) -> (Extract, Value) {
        let t = synth::Theatre::new(synth::DEFAULT_SEED, synth::DEFAULT_SIZE_KM, 50).unwrap();
        synth::write(&t, &s.0).unwrap();
        let x = Extract::open(&s.0).unwrap();
        let config =
            serde_json::from_str(&std::fs::read_to_string(s.0.join("config.json")).unwrap())
                .unwrap();
        (x, config)
    }

    fn get<'a>(rows: &'a [(&str, Option<String>)], key: &str) -> Option<&'a str> {
        rows.iter().find(|r| r.0 == key).unwrap().1.as_deref()
    }

    #[test]
    fn fills_every_key_from_the_synthetic_extract() {
        let s = Scratch::new();
        let (x, config) = synth_extract(&s);
        let rows = from_extract(&x, &config, 50, "2026-01-01T00:00:00Z").unwrap();
        assert!(rows.iter().map(|r| r.0).eq(KEYS));
        for (k, v) in &rows {
            assert_eq!(v.is_none(), k.starts_with("validation_"), "{k}");
        }
        assert_eq!(get(&rows, "theatre_id"), Some(synth::THEATRE));
        assert_eq!(get(&rows, "has_hook"), Some("1"));
        assert_eq!(get(&rows, "authored_bounds_source"), Some("surface5"));
        assert_eq!(get(&rows, "grid_width"), Some("1400"));

        // Metres, not the kilometres config.json carries.
        let km = config["bounds_km"]["sw"][0].as_f64().unwrap();
        let sw_x: f64 = get(&rows, "bounds_sw_x").unwrap().parse().unwrap();
        assert!((sw_x - km * 1000.0).abs() < 1e-3, "{sw_x} from {km} km");

        let fp: Value = serde_json::from_str(get(&rows, "terrain_fingerprint").unwrap()).unwrap();
        let roles: Vec<&str> = fp
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["role"].as_str().unwrap())
            .collect();
        assert_eq!(roles, FINGERPRINT_ROLES);
    }

    #[test]
    fn a_coarser_base_covers_the_same_ground() {
        let s = Scratch::new();
        let (mut x, config) = synth_extract(&s);
        x.manifest.grid.width = 1401;
        let rows = from_extract(&x, &config, 100, "t").unwrap();
        assert_eq!(get(&rows, "cell_size"), Some("100"));
        assert_eq!(get(&rows, "grid_width"), Some("701"));
        assert_eq!(get(&rows, "grid_height"), Some("700"));
    }

    #[test]
    fn without_the_hook_bounds_bullseyes_and_sea_are_null() {
        let s = Scratch::new();
        let (mut x, mut config) = synth_extract(&s);
        x.manifest.hook = None;
        for k in ["bounds_km", "default_bullseye", "sea_enabled"] {
            config[k] = Value::Null;
        }
        let rows = from_extract(&x, &config, 50, "t").unwrap();
        assert_eq!(get(&rows, "has_hook"), Some("0"));
        for (k, v) in &rows {
            assert_eq!(
                v.is_none(),
                NULLABLE.contains(k) && *k != "crs_proj4" && !k.starts_with("authored_"),
                "{k}"
            );
        }
    }

    #[test]
    fn names_a_config_value_of_the_wrong_shape() {
        let s = Scratch::new();
        let (x, mut config) = synth_extract(&s);
        config["bounds_km"]["ne"] = json!([1]);
        let e = from_extract(&x, &config, 50, "t").unwrap_err();
        assert!(e.contains("bounds_km.ne"), "{e}");
    }
}
