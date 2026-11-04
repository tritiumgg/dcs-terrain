//! The readable answers of the main operations: what was asked and how
//! many passed, a table with units in its header and MGRS beside each x and
//! z, and what the table left out. The rest print the plain list.

use serde_json::Value;

use super::table::{Align, KEEP, Page, Table, count, fixed};

/// What a readable answer needs beside the answer itself.
pub struct Asked<'a> {
    pub request: &'a Value,
    /// The MGRS of a point, as `geo` gives it.
    pub grid: &'a dyn Fn(f64, f64) -> String,
    pub color: bool,
    /// Where the GeoJSON was written, when it was.
    pub geojson_out: Option<&'a std::path::Path>,
}

/// The answer to `op` as a page of text, or none when `op` has no readable
/// form.
pub fn readable(op: &str, asked: &Asked, answer: &Value) -> Option<String> {
    let mut page = Page::new(asked.color);
    match op {
        "find_sites" => find_sites(&mut page, asked, answer),
        "nearest" => nearest(&mut page, asked, answer),
        "route" => route(&mut page, asked, answer),
        _ => return None,
    }
    if let Some(path) = asked.geojson_out {
        page.blank();
        page.note(&format!("The GeoJSON is written to {}.", path.display()));
    }
    Some(page.finish())
}

fn num(v: &Value) -> Option<f64> {
    v.as_f64()
}

fn whole(v: &Value) -> String {
    fixed(num(v), 0)
}

fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "—".into(),
        v => v.to_string(),
    }
}

/// The x, z and MGRS columns of a point.
fn place(asked: &Asked, at: &Value) -> [String; 3] {
    match (num(&at["x"]), num(&at["z"])) {
        (Some(x), Some(z)) => [fixed(Some(x), 0), fixed(Some(z), 0), (asked.grid)(x, z)],
        _ => ["—".into(), "—".into(), "—".into()],
    }
}

/// A point in words: `12000, 15000 (36T XR 14517 10114)`.
fn place_words(asked: &Asked, at: &Value) -> String {
    let [x, z, grid] = place(asked, at);
    format!("{x}, {z} ({grid})")
}

/// The place columns, appended to a table.
fn with_place(t: Table) -> Table {
    t.col("x", Align::Right)
        .col("z", Align::Right)
        .col("MGRS", Align::Left)
}

/// How a criterion's measured value reads: its unit, and the cell.
fn term_value(name: &str, v: &Value) -> (&'static str, String) {
    let v = num(v);
    match name {
        "slope_max" | "enclosure_min" | "masked_from" => ("°", fixed(v, 1)),
        "span_max" => ("m", fixed(v, 1)),
        n if n.starts_with("dist_") => ("m", fixed(v, 0)),
        "surface_in" | "surface_enum_in" | "cover_in" | "cover_fraction_min"
        | "not_visible_from" | "visible_to" | "valid" => ("%", fixed(v.map(|s| s * 100.0), 0)),
        "scenery_max" | "scenery_in_footprint_max" | "clearing_max" => ("count", fixed(v, 0)),
        // The landform's code, in the order the criterion's hard test names
        // them.
        "tpi_class_in" => (
            "class",
            v.and_then(|c| {
                ["ridge", "valley", "upper", "lower", "mid", "flat"]
                    .get(c as usize)
                    .map(|s| s.to_string())
            })
            .unwrap_or_else(|| "—".into()),
        ),
        _ => ("", fixed(v, 2)),
    }
}

fn find_sites(page: &mut Page, asked: &Asked, a: &Value) {
    let empty = Vec::new();
    let candidates = a["candidates"].as_array().unwrap_or(&empty);
    let stats = &a["stats"];
    let (considered, passed) = (
        whole(&stats["cells_considered"]),
        whole(&stats["cells_passed"]),
    );
    let skipped = num(&stats["tiles_skipped"]).unwrap_or(0.0);
    let skipped = if skipped > 0.0 {
        format!(
            ", and {} skipped whole",
            count(skipped as usize, "tile", "tiles")
        )
    } else {
        String::new()
    };
    if candidates.is_empty() {
        page.line(&format!(
            "No site passes every test: of {considered} lattice points in the region, \
             {passed} passed{skipped}. Loosen a criterion, or widen the region."
        ));
        return;
    }
    page.line(&format!(
        "{}, best first, of {considered} lattice points in the region; {passed} passed \
         every test{skipped}.",
        count(candidates.len(), "site", "sites")
    ));
    let limit = num(&asked.request["limit"]).unwrap_or(20.0) as usize;
    if candidates.len() >= limit {
        page.line("--limit asks for more sites, up to 200.");
    }

    // Every candidate passes every test, so each term shows only what it
    // measured; `valid` is all of the footprint on each.
    let names: Vec<String> = candidates[0]["terms"]
        .as_array()
        .unwrap_or(&empty)
        .iter()
        .map(|t| text(&t["name"]))
        .collect();
    let shown: Vec<usize> = (0..names.len()).filter(|&i| names[i] != "valid").collect();
    let mut t = with_place(
        Table::new()
            .col("#", Align::Right)
            .col("Score", Align::Right),
    )
    .col("Hdg (°)", Align::Right);
    for (rank, &i) in shown.iter().enumerate() {
        let (unit, _) = term_value(&names[i], &Value::Null);
        let header = if unit.is_empty() {
            names[i].clone()
        } else {
            format!("{} ({unit})", names[i])
        };
        // The first criteria asked for are kept longest.
        let keep = (KEEP - 1).saturating_sub(rank as u8);
        let align = if unit == "class" {
            Align::Left
        } else {
            Align::Right
        };
        t = t.optional(&header, align, keep);
    }
    for (n, c) in candidates.iter().enumerate() {
        let mut row = vec![(n + 1).to_string(), fixed(num(&c["score"]), 3)];
        row.extend(place(asked, c));
        row.push(fixed(num(&c["orientation_deg"]), 0));
        let terms = c["terms"].as_array().unwrap_or(&empty);
        for &i in &shown {
            row.push(term_value(&names[i], &terms.get(i).unwrap_or(&Value::Null)["value"]).1);
        }
        t.row(row);
    }
    page.table(&t);
    page.blank();
    page.note(
        "Each criterion's score, 0 to 1, is in --json; the site's score is their weighted sum.",
    );
}

fn nearest(page: &mut Page, asked: &Asked, a: &Value) {
    let empty = Vec::new();
    let results = a["results"].as_array().unwrap_or(&empty);
    let kind = text(&asked.request["kind"]);
    let (one, many) = match kind.as_str() {
        "rail" => ("railway", "railways"),
        "scenery" => ("scenery object", "scenery objects"),
        "node" => ("node side", "node sides"),
        "place" => ("place", "places"),
        "airdrome" => ("airdrome", "airdromes"),
        "town" => ("town", "towns"),
        "beacon" => ("beacon", "beacons"),
        _ => ("road", "roads"),
    };
    let at = place_words(asked, asked.request);
    if results.is_empty() {
        page.line(&format!("No {many} near {at}: the terrain has none."));
        return;
    }
    page.line(&match results.len() {
        1 => format!("The nearest {one} to {at}."),
        n => format!("The {n} nearest {many} to {at}, nearest first."),
    });
    if matches!(kind.as_str(), "road" | "rail") {
        page.line("Each row is the nearest point of one edge of the network.");
    }
    let asked_for = num(&asked.request["n"]).unwrap_or(1.0) as usize;
    if kind != "node" && results.len() < asked_for {
        page.line(&format!(
            "Only {} of the {asked_for} asked for: the terrain has no more.",
            results.len()
        ));
    }
    let mut t = Table::new()
        .col("#", Align::Right)
        .col("Distance (m)", Align::Right)
        .cut("Name", 12);
    if kind == "node" {
        t = t.col("Side", Align::Left);
    }
    let mut t = with_place(
        t.optional("Class", Align::Left, 2)
            .optional("Id", Align::Right, 1),
    );
    for (n, r) in results.iter().enumerate() {
        let mut row = vec![(n + 1).to_string(), whole(&r["distance"]), text(&r["name"])];
        if kind == "node" {
            let side = text(&r["kind"]);
            row.push(side.strip_prefix("node_").unwrap_or(&side).to_string());
        }
        row.extend([text(&r["class"]), text(&r["id"])]);
        row.extend(place(asked, r));
        t.row(row);
    }
    page.table(&t);
}

/// Kilometers to two places: ten meters.
fn km(v: &Value) -> String {
    fixed(num(v).map(|m| m / 1000.0), 2)
}

/// Where each end of a route was given and where it joined the network.
fn route_ends(page: &mut Page, asked: &Asked, route: &Value) {
    page.heading("Ends, each where it joins the network");
    let mut t = with_place(Table::new().col("End", Align::Left)).col("Moved (m)", Align::Right);
    for (end, given, snapped) in [
        ("From", &asked.request["from"], &route["snapped_from"]),
        ("To", &asked.request["to"], &route["snapped_to"]),
    ] {
        let moved = match (
            num(&given["x"]),
            num(&given["z"]),
            num(&snapped["x"]),
            num(&snapped["z"]),
        ) {
            (Some(gx), Some(gz), Some(sx), Some(sz)) => Some((gx - sx).hypot(gz - sz)),
            _ => None,
        };
        let mut row = vec![end.to_string()];
        row.extend(place(asked, snapped));
        row.push(fixed(moved, 0));
        t.row(row);
    }
    page.table(&t);
}

/// A route's legs, each with where it ends.
fn route_legs(page: &mut Page, asked: &Asked, route: &Value, heading: &str) {
    let empty = Vec::new();
    let legs = route["legs"].as_array().unwrap_or(&empty);
    let exposure = asked.request["exposure_observers"]
        .as_array()
        .is_some_and(|o| !o.is_empty());
    page.heading(heading);
    let mut t = Table::new()
        .col("#", Align::Right)
        .col("Class", Align::Left)
        .optional("Edge", Align::Right, 1)
        .col("Length (km)", Align::Right)
        .col("Up (m)", Align::Right)
        .col("Down (m)", Align::Right)
        .col("Grade (%)", Align::Right);
    if exposure {
        t = t.optional("Seen (%)", Align::Right, 2);
    }
    let mut t = with_place(t);
    for (n, l) in legs.iter().enumerate() {
        let mut row = vec![
            (n + 1).to_string(),
            text(&l["class"]),
            text(&l["edge_id"]),
            km(&l["length"]),
            whole(&l["ascent"]),
            whole(&l["descent"]),
            fixed(num(&l["max_grade_pct"]), 1),
        ];
        if exposure {
            row.push(fixed(num(&l["exposure"]).map(|s| s * 100.0), 0));
        }
        row.extend(place(asked, &l["to"]));
        t.row(row);
    }
    page.table(&t);
}

/// How much longer than the straight line a route is: `1.15 ×`.
fn stretch(route: &Value) -> String {
    match (num(&route["length"]), num(&route["straight_line"])) {
        (Some(l), Some(s)) if s > 0.0 => format!("{} ×", fixed(Some(l / s), 2)),
        _ => "—".into(),
    }
}

/// What a route's table leaves out.
fn waypoints_left_out(page: &mut Page, routes: &[&Value]) {
    let n: usize = routes
        .iter()
        .map(|r| r["waypoints"].as_array().map_or(0, Vec::len))
        .sum();
    page.blank();
    page.note(&format!(
        "Each leg's x, z and MGRS are where it ends. {} left out: --json lists them, \
         --geojson-out draws the way.",
        count(n, "waypoint is", "waypoints are")
    ));
}

fn route(page: &mut Page, asked: &Asked, a: &Value) {
    let network = text(&asked.request["network"]);
    page.line(&format!(
        "The route by {network}: {} km, {} the {} km straight line.",
        km(&a["length"]),
        stretch(a),
        km(&a["straight_line"])
    ));
    page.line(&format!(
        "It climbs {} m and descends {} m; its steepest 100 m is {} %.",
        whole(&a["ascent"]),
        whole(&a["descent"]),
        fixed(num(&a["max_grade_pct"]), 1),
    ));
    route_ends(page, asked, a);
    route_legs(page, asked, a, "Legs");
    waypoints_left_out(page, &[a]);
}
