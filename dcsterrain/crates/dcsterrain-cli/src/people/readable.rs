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
        "route_alternatives" => route_alternatives(&mut page, asked, answer),
        "point_info" => point_info(&mut page, asked, answer),
        "airfield_info" => airfield_info(&mut page, asked, answer),
        "towns" => towns(&mut page, asked, answer),
        "visible" => visible(&mut page, asked, answer),
        "trafficability" => trafficability(&mut page, asked, answer),
        "geo" => geo(&mut page, asked, answer),
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

/// A direction in degrees, where 359.96 rounds to 0 and not to 360.
fn bearing(v: &Value, places: usize) -> String {
    let scale = 10f64.powi(places as i32);
    fixed(
        num(v).map(|d| ((d * scale).round() / scale).rem_euclid(360.0)),
        places,
    )
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
    if candidates.len() >= limit && limit < 200 {
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
        row.push(bearing(&c["orientation_deg"], 0));
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
    let of_class = match asked.request["class"].as_str() {
        Some(c) if kind == "scenery" => format!(" of class {c}"),
        _ => String::new(),
    };
    if results.is_empty() {
        page.line(&format!(
            "No {many}{of_class} near {at}: the terrain has none."
        ));
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
            "Only {} of the {asked_for} asked for: the terrain has no more{of_class}.",
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
        // A place no town names has no name; every other result shows a
        // dash for a missing one.
        let name = match (&r["name"], kind.as_str()) {
            (Value::Null, "place") => "(unnamed)".into(),
            (v, _) => text(v),
        };
        let mut row = vec![(n + 1).to_string(), whole(&r["distance"]), name];
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
    let straight = num(&a["straight_line"]).unwrap_or(0.0);
    page.line(&if straight > 0.0 {
        format!(
            "The route by {network}: {}, {} the {} straight line.",
            distance(&a["length"]),
            stretch(a),
            distance(&a["straight_line"])
        )
    } else {
        format!(
            "The route by {network}: {}, from and to the same place.",
            distance(&a["length"])
        )
    });
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

/// A length in words: meters under a kilometer, else kilometers.
fn distance(v: &Value) -> String {
    match num(v) {
        Some(m) if m < 1000.0 => format!("{} m", fixed(Some(m), 0)),
        _ => format!("{} km", km(v)),
    }
}

/// A list of numbers as one cell: `1, 2, 4`, or `none`.
fn listed(v: &Value, each: impl Fn(&Value) -> String) -> String {
    match v.as_array() {
        Some(a) if a.is_empty() => "none".into(),
        Some(a) => a.iter().map(each).collect::<Vec<_>>().join(", "),
        None => "—".into(),
    }
}

fn route_alternatives(page: &mut Page, asked: &Asked, a: &Value) {
    let empty = Vec::new();
    let routes = a["routes"].as_array().unwrap_or(&empty);
    let network = text(&asked.request["network"]);
    let k = num(&asked.request["k"]).unwrap_or(3.0) as usize;
    page.line(&format!(
        "{} by {network}, the least costly first.",
        count(routes.len(), "route", "routes")
    ));
    if routes.len() < k {
        let stretch = num(&asked.request["max_stretch"]).unwrap_or(1.5);
        let overlap = num(&asked.request["max_overlap"]).unwrap_or(0.5);
        page.line(&format!(
            "{k} were asked for. A route is kept only when it is new, at most {} × the \
             first's length, and shares at most {} % of its length with each route kept; \
             the search, at most {} tries, found no more.",
            fixed(Some(stretch), 2),
            fixed(Some(overlap * 100.0), 0),
            // The penalty method routes again at most four times per route
            // asked for.
            4 * k
        ));
    }
    let Some(first) = routes.first() else {
        return;
    };
    route_ends(page, asked, first);

    page.heading("Routes");
    let mut t = Table::new()
        .col("#", Align::Right)
        .col("Length (km)", Align::Right)
        .col("Stretch", Align::Right)
        .col("Up (m)", Align::Right)
        .col("Down (m)", Align::Right)
        .col("Grade (%)", Align::Right)
        .col("Legs", Align::Right)
        // Left out whole rather than cut, so no list ends mid-number.
        .optional("Overlap (%)", Align::Left, 2)
        .optional("Chokepoint edges", Align::Left, 1);
    for (n, r) in routes.iter().enumerate() {
        let chokepoints = if r["chokepoints"].is_null() {
            "not built yet".into()
        } else {
            listed(&r["chokepoints"], text)
        };
        t.row(vec![
            (n + 1).to_string(),
            km(&r["length"]),
            format!("{} ×", fixed(num(&r["stretch"]), 2)),
            whole(&r["ascent"]),
            whole(&r["descent"]),
            fixed(num(&r["max_grade_pct"]), 1),
            r["legs"].as_array().map_or(0, Vec::len).to_string(),
            listed(&r["overlap_with"], |s| fixed(num(s).map(|s| s * 100.0), 0)),
            chokepoints,
        ]);
    }
    page.table(&t);
    page.blank();
    page.note(
        "Stretch is each route's length over the first's. Overlap is the share of its length \
         each other route runs along, in order. Chokepoint edges are its edges in the top tenth \
         of the network's betweenness.",
    );
    page.line(&format!(
        "Edges every route runs along: {}.",
        listed(&first["shared_edges_with_all"], text)
    ));
    let all: Vec<&Value> = routes.iter().collect();
    for (n, r) in routes.iter().enumerate() {
        route_legs(page, asked, r, &format!("Route {} legs", n + 1));
    }
    waypoints_left_out(page, &all);
}

fn point_info(page: &mut Page, asked: &Asked, a: &Value) {
    let empty = Vec::new();
    let points = a["points"].as_array().unwrap_or(&empty);
    if points.is_empty() {
        page.line("No points were asked about.");
        return;
    }
    page.line(&format!(
        "{}, in the order asked.",
        count(points.len(), "point", "points")
    ));

    page.heading("Ground");
    let mut t = with_place(Table::new().col("#", Align::Right))
        .col("Height (m)", Align::Right)
        .col("Slope (°)", Align::Right)
        .col("Aspect (°)", Align::Right)
        .col("Water", Align::Left)
        .optional("Depth (m)", Align::Right, 1)
        .col("Cover", Align::Left);
    for (n, p) in points.iter().enumerate() {
        let mut row = vec![(n + 1).to_string()];
        row.extend(place(asked, p));
        row.extend([
            fixed(num(&p["height"]), 1),
            fixed(num(&p["slope"]), 1),
            bearing(&p["aspect"], 0),
            text(&p["water"]),
            fixed(num(&p["depth"]), 1),
            text(&p["cover"]),
        ]);
        t.row(row);
    }
    page.table(&t);

    page.heading("Nearby");
    let mut t = Table::new()
        .col("#", Align::Right)
        .cut("Town", 12)
        .cut("Place", 8)
        .col("Road (m)", Align::Right)
        .col("Road", Align::Left)
        .cut("Airfield", 8)
        .col("Airfield (km)", Align::Right);
    for (n, p) in points.iter().enumerate() {
        let place = &p["place"];
        let place = if place.is_null() {
            "—".into()
        } else {
            let name = place["name"].as_str().unwrap_or("unnamed");
            format!("{name} ({})", text(&place["size"]))
        };
        t.row(vec![
            (n + 1).to_string(),
            p["town"]["description"].as_str().unwrap_or("—").into(),
            place,
            whole(&p["road"]["distance"]),
            text(&p["road"]["class"]),
            p["airfield"]["name"].as_str().unwrap_or("—").into(),
            km(&p["airfield"]["distance"]),
        ]);
    }
    page.table(&t);
    page.blank();
    if a["places_built"] == false {
        page.note(
            "The places are not built yet, so none is shown: `dcsterrain index` builds them.",
        );
    }
    page.note(
        "Aspect is the downslope direction from grid north. Latitude and longitude, the \
         bearings from each town, and where each road and airfield lie: --json.",
    );
}

fn yes_no(v: &Value) -> String {
    match v.as_bool() {
        Some(true) => "yes".into(),
        Some(false) => "no".into(),
        None => "—".into(),
    }
}

/// A frequency in the unit a pilot reads it in: kHz under 1 MHz, else MHz.
fn frequency(v: &Value) -> String {
    match num(v) {
        Some(hz) if hz < 1e6 => format!("{} kHz", fixed(Some(hz / 1e3), 0)),
        Some(hz) => format!("{} MHz", fixed(Some(hz / 1e6), 3)),
        None => "—".into(),
    }
}

fn mhz(v: &Value) -> String {
    fixed(num(v).map(|hz| hz / 1e6), 3)
}

fn airfield_info(page: &mut Page, asked: &Asked, a: &Value) {
    let empty = Vec::new();
    let id = if a["id"].is_null() {
        "no id".into()
    } else {
        format!("id {}", text(&a["id"]))
    };
    page.line(&format!(
        "{} ({}, {id}): elevation {} m, at {}.",
        text(&a["name"]),
        text(&a["name_id"]),
        whole(&a["elevation"]),
        place_words(asked, a)
    ));
    page.line("Courses are true: there is no magnetic model.");

    let runways = a["runways"].as_array().unwrap_or(&empty);
    if runways.is_empty() {
        page.blank();
        page.line("No runways: the elevation is the highest ground at its stands.");
    } else {
        page.heading("Runways, a row per end");
        let mut t = with_place(
            Table::new()
                .col("Runway", Align::Left)
                .col("End", Align::Left)
                .col("Course (°)", Align::Right)
                .col("Length (m)", Align::Right)
                .col("Width (m)", Align::Right)
                .col("Height (m)", Align::Right),
        );
        for r in runways {
            let ends = r["thresholds"].as_array().unwrap_or(&empty);
            for (i, e) in ends.iter().enumerate() {
                let (runway, length, width) = if i == 0 {
                    (
                        text(&r["designator"]),
                        whole(&r["length"]),
                        whole(&r["width"]),
                    )
                } else {
                    Default::default()
                };
                let mut row = vec![
                    runway,
                    text(&e["designator"]),
                    bearing(&e["course_true_deg"], 1),
                    length,
                    width,
                    fixed(num(&e["height"]), 1),
                ];
                row.extend(place(asked, e));
                t.row(row);
            }
        }
        page.table(&t);
    }

    let navaids = a["navaids"].as_array().unwrap_or(&empty);
    if navaids.is_empty() {
        page.blank();
        page.line("No navaids.");
    } else {
        page.heading("Navaids");
        let mut t = with_place(
            Table::new()
                .col("Callsign", Align::Left)
                .cut("Kind", 6)
                .col("Channel", Align::Right)
                .col("Frequency", Align::Right),
        );
        for n in navaids {
            let mut row = vec![
                text(&n["callsign"]),
                text(&n["kind"]),
                text(&n["channel"]),
                frequency(&n["frequency_hz"]),
            ];
            row.extend(place(asked, n));
            t.row(row);
        }
        page.table(&t);
    }

    let radio = a["radio"].as_array().unwrap_or(&empty);
    if radio.is_empty() {
        page.blank();
        page.line("No radio.");
    } else {
        page.heading("Radio");
        let mut t = Table::new()
            .cut("Callsign", 10)
            .col("UHF (MHz)", Align::Right)
            .col("VHF (MHz)", Align::Right)
            .col("HF (MHz)", Align::Right)
            .col("FM (MHz)", Align::Right);
        for r in radio {
            t.row(vec![
                text(&r["callsign"]),
                mhz(&r["uhf_hz"]),
                mhz(&r["vhf_hz"]),
                mhz(&r["hf_hz"]),
                mhz(&r["fm_hz"]),
            ]);
        }
        page.table(&t);
    }

    let stands = a["stands"].as_array().unwrap_or(&empty);
    let fits = !asked.request["fits"].is_null();
    if stands.is_empty() {
        page.blank();
        page.line("No stands.");
    } else {
        let sizes: Vec<String> = a["stand_sizes"]
            .as_array()
            .unwrap_or(&empty)
            .iter()
            .map(|s| {
                format!(
                    "{} of {} × {} m",
                    whole(&s["count"]),
                    whole(&s["width"]),
                    whole(&s["length"])
                )
            })
            .collect();
        page.heading(&format!("Stands: {}", sizes.join(", ")));
        if fits {
            page.line(&format!(
                "{} the airframe for certain. A dash under Fits: no known size fails it, but \
                 one is unknown.",
                count(
                    num(&a["stands_fitting"]).unwrap_or(0.0) as usize,
                    "stand fits",
                    "stands fit"
                )
            ));
        }
        let mut t = Table::new()
            .cut("Stand", 5)
            .col("Width (m)", Align::Right)
            .col("Length (m)", Align::Right)
            .optional("Height (m)", Align::Right, 1)
            .col("Heli", Align::Left)
            .col("Planes", Align::Left);
        if fits {
            t = t.col("Fits", Align::Left);
        }
        let mut t = with_place(t);
        for s in stands {
            let mut row = vec![
                text(&s["name"]),
                whole(&s["width"]),
                whole(&s["length"]),
                whole(&s["height"]),
                yes_no(&s["helicopters"]),
                yes_no(&s["airplanes"]),
            ];
            if fits {
                row.push(yes_no(&s["fits"]));
            }
            row.extend(place(asked, s));
            t.row(row);
        }
        page.table(&t);
    }
    page.blank();
    page.note(
        "Grid courses, each end's latitude and longitude, the navaids' heights and the stands' \
         classes: --json.",
    );
}

fn towns(page: &mut Page, asked: &Asked, a: &Value) {
    let empty = Vec::new();
    let places = a["places"].as_array().unwrap_or(&empty);
    let matched = num(&a["matched"]).unwrap_or(0.0) as usize;
    if places.is_empty() {
        let r = asked.request;
        let given = |k: &str| r[k].as_array().is_some_and(|a| !a.is_empty());
        let mut loosen = Vec::new();
        if !r["region"].is_null() {
            loosen.push("widen the region");
        }
        if given("size") {
            loosen.push("loosen --size");
        }
        if given("form") {
            loosen.push("loosen --form");
        }
        if num(&r["min_buildings"]).is_some_and(|n| n > 0.0) {
            loosen.push("lower --min-buildings");
        }
        page.line(&match loosen.len() {
            0 => "No place: the terrain has none.".to_string(),
            _ => format!("No place matches. To find some, {}.", loosen.join(", or ")),
        });
        return;
    }
    let by = if asked.request["sort"] == "floor" {
        "the most floor area"
    } else {
        "the most buildings"
    };
    page.line(&format!(
        "{} of the {matched} that match, {by} first.",
        count(places.len(), "place", "places")
    ));
    // Outlines hold a place's whole ground, so fewer are allowed.
    let most = if asked.request["outlines"] == true {
        100
    } else {
        1000
    };
    if matched > places.len() && places.len() < most {
        page.line(&format!(
            "--limit shows more, up to {}.",
            if most == 1000 { "1 000" } else { "100" }
        ));
    }
    let mut t = Table::new()
        .col("#", Align::Right)
        .cut("Name", 10)
        .col("Size", Align::Left)
        .col("Form", Align::Left)
        .col("Buildings", Align::Right)
        .optional("Floor (m²)", Align::Right, 2)
        .optional("Area (ha)", Align::Right, 1);
    t = with_place(t);
    for (n, p) in places.iter().enumerate() {
        let mut row = vec![
            (n + 1).to_string(),
            p["name"].as_str().unwrap_or("(unnamed)").into(),
            text(&p["size"]),
            text(&p["form"]),
            whole(&p["buildings"]),
            whole(&p["floor"]),
            whole(&p["hectares"]),
        ];
        row.extend(place(asked, p));
        t.row(row);
    }
    page.table(&t);
    page.blank();
    let outlines = if asked.request["outlines"] == true {
        ", and the outlines, which --geojson-out draws"
    } else {
        ""
    };
    page.note(&format!(
        "Each place's other names, id and the box of its group{outlines}: --json."
    ));
}

/// A point's height as a request gives it.
fn height_words(at: &Value) -> String {
    match (num(&at["alt_agl"]), num(&at["alt"])) {
        (Some(agl), _) => format!("{} m above the ground", fixed(Some(agl), 1)),
        (None, Some(msl)) => format!("{} m above sea level", fixed(Some(msl), 1)),
        _ => "—".into(),
    }
}

/// A latitude or longitude as a place flag reads it back: unsigned, to six
/// decimals, about a decimeter, with its hemisphere letter.
fn hemisphere(v: &Value, positive: char, negative: char) -> String {
    match num(v) {
        Some(d) if d.is_finite() => {
            let letter = if d < 0.0 { negative } else { positive };
            format!("{}{letter}", fixed(Some(d.abs()), 6))
        }
        _ => "—".into(),
    }
}

/// Every form of each point, whichever way it was converted, so one row
/// answers a person who has any of them.
fn geo(page: &mut Page, asked: &Asked, a: &Value) {
    let empty = Vec::new();
    let list = |v: &Value| v.as_array().unwrap_or(&empty).clone();
    let (latlon_asked, latlon_got) = (list(&asked.request["to_latlon"]), list(&a["to_latlon"]));
    let (xz_asked, xz_got) = (list(&asked.request["to_xz"]), list(&a["to_xz"]));
    let n = latlon_got.len() + xz_got.len();
    if n == 0 {
        page.line("No points were given. Give one to --to-latlon or --to-xz.");
        return;
    }
    let order = if !latlon_got.is_empty() && !xz_got.is_empty() {
        " --to-latlon's first, then --to-xz's."
    } else {
        ""
    };
    page.line(&format!(
        "{}, in the order given.{order}",
        count(n, "point", "points")
    ));
    let mut t = with_place(Table::new().col("#", Align::Right))
        .col("Latitude (°)", Align::Right)
        .col("Longitude (°)", Align::Right);
    let mut shown = 0;
    let mut row = |xz: &Value, latlon: &Value, grid: Option<String>| {
        shown += 1;
        let [x, z, from_xz] = place(asked, xz);
        let mut cells = vec![shown.to_string(), x, z, grid.unwrap_or(from_xz)];
        cells.push(hemisphere(&latlon["lat"], 'N', 'S'));
        cells.push(hemisphere(&latlon["lon"], 'E', 'W'));
        t.row(cells);
    };
    for (at, got) in latlon_asked.iter().zip(&latlon_got) {
        row(at, got, got["mgrs"].as_str().map(String::from));
    }
    for (at, got) in xz_asked.iter().zip(&xz_got) {
        row(got, at, None);
    }
    page.table(&t);
    page.blank();
    page.note(
        "Each row is one point in every form: copy any of them into an option that takes a \
         place. Latitude and longitude are WGS84.",
    );
}

fn visible(page: &mut Page, asked: &Asked, a: &Value) {
    let (from, to) = (&asked.request["a"], &asked.request["b"]);
    let clearance = fixed(num(&a["clearance_m"]).map(f64::abs), 1);
    if a["visible"] == true {
        page.line(&format!(
            "Yes: A and B see each other. The line clears the ground by {clearance} m at its lowest."
        ));
    } else {
        page.line(&format!(
            "No: the ground blocks the line, rising {clearance} m above it at its highest."
        ));
    }
    if let (Some(ax), Some(az), Some(bx), Some(bz)) = (
        num(&from["x"]),
        num(&from["z"]),
        num(&to["x"]),
        num(&to["z"]),
    ) {
        page.line(&format!(
            "The ends are {} km apart.",
            fixed(Some((ax - bx).hypot(az - bz) / 1000.0), 2)
        ));
    }
    let mut t = with_place(Table::new().col("Point", Align::Left)).col("Height", Align::Left);
    for (name, at) in [("A", from), ("B", to)] {
        let mut row = vec![name.to_string()];
        row.extend(place(asked, at));
        row.push(height_words(at));
        t.row(row);
    }
    if !a["blocking"].is_null() {
        let mut row = vec!["Blocked".to_string()];
        row.extend(place(asked, &a["blocking"]));
        row.push(height_words(&a["blocking"]));
        t.row(row);
    }
    page.table(&t);
    if !a["blocking"].is_null() {
        page.blank();
        page.note("Blocked is where the ground first rises above the line from A.");
    }
}

/// A reason as a sentence, after `prefix` and with a full stop.
fn sentence(prefix: &str, reason: &Value) -> String {
    format!("{prefix}{}.", text(reason))
}

fn trafficability(page: &mut Page, asked: &Asked, a: &Value) {
    let way = &a["way"];
    let reachable = a["reachable"] == true;
    page.line(&sentence(
        if reachable { "Yes: " } else { "No: " },
        &a["reason"],
    ));
    if reachable {
        page.line(&format!(
            "The way is {}, as hard to cross as {} of open flat ground; it climbs {} m \
             and descends {} m.",
            distance(&way["length"]),
            distance(&way["cost"]),
            whole(&way["ascent"]),
            whole(&way["descent"]),
        ));
    }
    // The way may be the straight line itself, which the first line said.
    let turns = way["path"].as_array().map_or(0, Vec::len);
    let is_straight = turns == 2 && way["meets"] == a["nearest_road"];
    if !is_straight {
        page.line(&format!(
            "The straight line to the nearest road is {}: {}.",
            distance(&a["distance"]),
            text(&a["straight"]["reason"])
        ));
    }
    let mut t = with_place(Table::new().col("Point", Align::Left).optional(
        "Edge",
        Align::Right,
        1,
    ));
    let mut row = |name: &str, edge: String, at: &Value| {
        let mut r = vec![name.to_string(), edge];
        r.extend(place(asked, at));
        t.row(r);
    };
    row("From", String::new(), asked.request);
    if reachable {
        row("Meets the road", text(&way["edge_id"]), &way["meets"]);
    }
    row("Nearest road", String::new(), &a["nearest_road"]);
    if !a["straight"]["blocking"].is_null() {
        row(
            "Straight blocked",
            String::new(),
            &a["straight"]["blocking"],
        );
    }
    page.table(&t);
    if reachable && turns > 2 {
        page.blank();
        page.note(&format!(
            "The way's {} between its ends {} left out: --json lists them, --geojson-out \
             draws the way.",
            count(turns - 2, "turn", "turns"),
            if turns == 3 { "is" } else { "are" }
        ));
    }
}
