//! The requests the binary's tests send, shared by every surface that
//! answers them, so each is checked against the same ones.

// Each test crate that includes this uses only some of it.
#![allow(dead_code)]

use serde_json::{Value, json};

pub mod install;

/// Where the requests of one batch look: a center on the ground, an
/// airdrome, and two ends of a road.
pub struct Around {
    pub x: f64,
    pub z: f64,
    pub airdrome: Value,
    pub from: Value,
    pub to: Value,
}

/// One request per operation, small enough for a test.
pub fn every_operation(at: &Around) -> Vec<(&'static str, Value)> {
    let (x, z) = (at.x, at.z);
    let observer = json!({"x": x, "z": z, "alt_agl": 10});
    let point = json!({"x": x, "z": z});
    let circle = json!({"Circle": {"x": x, "z": z, "radius": 1500}});
    let box_ = json!({"Box": {
        "min_x": x - 1000.0, "min_z": z - 1000.0, "max_x": x + 1000.0, "max_z": z + 1000.0,
    }});
    let wide = json!({"Circle": {"x": x, "z": z, "radius": 20000}});
    let east = json!({"x": x, "z": z + 3000.0, "alt_agl": 100});
    let north = json!({"x": x + 3000.0, "z": z, "alt_agl": 100});
    let footprint = json!({"width": 60, "depth": 40});
    let criteria = json!([
        {"name": "slope_max", "params": {"deg": 30}},
        {"name": "dist_road_band", "params": {"min": 0, "max": 50000}},
    ]);
    let placement =
        |id: &str, shape: Value| json!({"id": id, "x": x, "z": z, "heading": 0, "shape": shape});
    vec![
        ("describe", json!({})),
        (
            "sample",
            json!({"points": [point], "layers": ["height", "slope", "dist_road"]}),
        ),
        ("geo", json!({"to_latlon": [point]})),
        (
            "score_site",
            json!({"x": x, "z": z, "footprint": footprint, "criteria": criteria}),
        ),
        (
            "find_sites",
            json!({"region": circle, "footprint": footprint, "criteria": criteria,
                   "spacing": 200, "limit": 3, "geojson": true}),
        ),
        (
            "visible",
            json!({"a": {"x": x, "z": z, "alt_agl": 10}, "b": east, "geojson": true}),
        ),
        (
            "viewshed",
            json!({"observer": observer, "radius": 2000, "target_alt_agl": 2, "geojson": true}),
        ),
        (
            "radar_floor",
            json!({"site": observer, "radius": 2000, "geojson": true}),
        ),
        (
            "coverage",
            json!({"site": observer, "envelope": {"radius": 2000, "alt_bands_agl": [30, 100]},
                   "asset": circle, "geojson": true}),
        ),
        (
            "exposure",
            json!({"sites": [{"observer": observer, "radius": 2000}], "target_alt_agl": 30,
                   "geojson": true}),
        ),
        (
            "approach_spectrum",
            json!({"site": observer, "radius": 2000, "alt_bands_agl": [30], "geojson": true}),
        ),
        (
            "unmask_profile",
            json!({"site": observer, "route": [east, north], "geojson": true}),
        ),
        (
            "route",
            json!({"network": "roads", "from": at.from, "to": at.to, "geojson": true}),
        ),
        (
            "route_alternatives",
            json!({"network": "roads", "from": at.from, "to": at.to, "k": 2, "geojson": true}),
        ),
        (
            "nearest",
            json!({"kind": "road", "x": x, "z": z, "n": 2, "geojson": true}),
        ),
        (
            "scenery_in",
            json!({"region": box_, "max_rows": 1000000, "geojson": true}),
        ),
        (
            "chokepoints",
            json!({"region": wide, "network": "roads", "limit": 5, "geojson": true}),
        ),
        (
            "trafficability",
            json!({"x": x, "z": z, "max_slope_deg": 30, "network": "roads", "geojson": true}),
        ),
        ("point_info", json!({"points": [point], "geojson": true})),
        (
            "airfield_info",
            json!({"airdrome": at.airdrome, "geojson": true}),
        ),
        (
            "validate_placements",
            json!({"placements": [
                placement("point", json!("Point")),
                placement("rect", json!({"Rect": {"width": 60, "depth": 40}})),
                placement("circle", json!({"Circle": {"radius": 30}})),
            ], "geojson": true}),
        ),
        ("towns", json!({"limit": 3, "geojson": true})),
    ]
}

/// The synthetic terrain's own center, airdrome and road.
pub fn synth() -> Around {
    Around {
        x: 12000.0,
        z: 15000.0,
        airdrome: json!(1),
        from: json!({"x": 10000, "z": -12000}),
        to: json!({"x": 20000, "z": 10000}),
    }
}

/// A batch whose lines are mostly bad: a byte-order mark, blank lines, lines
/// that are not JSON or not requests, requests the operations refuse, and a
/// last line with no newline.
pub fn bad_lines() -> Vec<u8> {
    let mut text = Vec::new();
    text.extend_from_slice(b"\xEF\xBB\xBF{\"op\": \"describe\"}\r\n");
    text.extend_from_slice(b"\n   \n");
    text.extend_from_slice(b"not json\n");
    text.extend_from_slice(b"{\"id\": 1, \"op\": \"nope\"}\n");
    text.extend_from_slice(b"{\"id\": 2}\n");
    text.extend_from_slice(b"{\"id\": 3, \"op\": \"visible\", \"x\": 1}\n");
    text.extend_from_slice(b"{\"id\": 4, \"op\": 5}\n");
    text.extend_from_slice(b"[1, 2]\n");
    text.extend_from_slice(b"\"\xFF\"\n");
    let bad = r#"{"id": 5, "op": "sample", "request": {"points": [{"x": "north", "z": 0}], "layers": []}}"#;
    text.extend_from_slice(bad.as_bytes());
    text.push(b'\n');
    text.extend_from_slice(b"{\"id\": 6, \"op\": \"visible\", \"request\": {\"a\": {\"x\": 0, \"z\": 0}, \"b\": {\"x\": 1, \"z\": 1, \"alt\": 5}}}\n");
    text.extend_from_slice(
        b"{\"id\": 7, \"op\": \"sample\", \"request\": {\"points\": [], \"layers\": [\"nope\"]}}\n",
    );
    // The last line needs no newline.
    text.extend_from_slice(b"{\"id\": [\"last\"], \"op\": \"describe\"}");
    text
}
