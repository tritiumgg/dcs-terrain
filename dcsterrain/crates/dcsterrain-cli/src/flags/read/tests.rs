use super::*;

fn spec(op: &str) -> Spec {
    Spec::of(op).expect("a listed operation")
}

/// The occurrences a command line gives, each flag with its values.
fn given(line: &[&[&str]]) -> Vec<(String, Vec<String>)> {
    line.iter()
        .map(|o| {
            let (flag, values) = o.split_first().unwrap();
            let values = values.iter().map(|v| v.to_string()).collect();
            (flag.to_string(), values)
        })
        .collect()
}

fn read(op: &str, line: &[&[&str]]) -> Draft {
    spec(op).read(&given(line)).unwrap()
}

fn refused(op: &str, line: &[&[&str]]) -> String {
    spec(op).read(&given(line)).unwrap_err().0
}

fn places(d: &Draft) -> Vec<(&str, &str, &str)> {
    d.pending
        .iter()
        .map(|p| (p.pointer.as_str(), p.flag.as_str(), p.text.as_str()))
        .collect()
}

#[test]
fn a_flag_is_its_field_and_x_and_z_are_one_place() {
    let s = spec("score_site");
    let names: Vec<&str> = s.fields.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["at", "footprint", "criteria"]);
    assert!(s.geojson);
    let d = read(
        "score_site",
        &[
            &["at", "airport:Kutaisi"],
            &["footprint", "120", "80"],
            &["criteria", "slope-max", "3"],
        ],
    );
    assert_eq!(
        d.request,
        json!({"footprint": {"width": 120.0, "depth": 80.0},
               "criteria": [{"name": "slope_max", "params": {"deg": 3.0}}]})
    );
    assert_eq!(places(&d), [("", "--at", "airport:Kutaisi")]);
    assert_eq!(
        d.pending[0].kind,
        PendingKind::Place(PlaceAs::Split(String::new()))
    );
}

#[test]
fn the_farp_question_reads_as_the_guide_writes_it() {
    let d = read(
        "find_sites",
        &[
            &["region", "airdrome-band", "Kutaisi", "3000", "15000"],
            &["footprint", "width=120", "depth=80"],
            &["criteria", "surface-in", "land"],
            &["criteria", "slope_max", "3"],
            &["criteria", "dist-road-band", "100", "1000"],
            &["criteria", "clearing-max", "0"],
            &["spacing", "2000"],
            &["limit", "3"],
        ],
    );
    assert_eq!(
        d.request,
        json!({
            "region": {"AirdromeBand": {"airdrome": null, "min_dist": 3000.0, "max_dist": 15000.0}},
            "footprint": {"width": 120.0, "depth": 80.0},
            "criteria": [
                {"name": "surface_in", "params": {"classes": ["land"]}},
                {"name": "slope_max", "params": {"deg": 3.0}},
                {"name": "dist_road_band", "params": {"min": 100.0, "max": 1000.0}},
                {"name": "clearing_max", "params": {"max": 0}},
            ],
            "spacing": 2000.0,
            "limit": 3,
        })
    );
    assert_eq!(d.pending.len(), 1);
    assert_eq!(d.pending[0].kind, PendingKind::Airdrome);
    assert_eq!(d.pending[0].pointer, "/region/AirdromeBand/airdrome");
}

#[test]
fn a_height_follows_the_place_after_an_at_sign() {
    let d = read(
        "visible",
        &[&["a", "town:Zugdidi@2"], &["b", "38TKM9198072567@1200msl"]],
    );
    assert_eq!(
        d.request,
        json!({"a": {"alt_agl": 2.0}, "b": {"alt": 1200.0}})
    );
    assert_eq!(
        places(&d),
        [
            ("/a", "--a", "town:Zugdidi"),
            ("/b", "--b", "38TKM9198072567")
        ]
    );
    let e = refused("viewshed", &[&["observer", "town:Zugdidi@10msl"]]);
    assert!(e.contains("above the ground"), "{e}");
    let e = refused("visible", &[&["a", "town:Zugdidi"], &["b", "1,2@3"]]);
    assert!(
        e.contains("needs a height") && e.contains("town:Zugdidi@10"),
        "{e}"
    );
}

#[test]
fn a_list_takes_values_or_the_flag_again_and_numbers_split_at_commas() {
    let d = read(
        "radar_floor",
        &[
            &["site", "-284887,683859@10"],
            &["radius", "20000"],
            &["bands-agl", "30,100", "300"],
            &["bands-agl", "1000"],
            &["points", "1000,2000", "3000,4000"],
        ],
    );
    assert_eq!(d.request["bands_agl"], json!([30.0, 100.0, 300.0, 1000.0]));
    assert_eq!(d.request["points"], json!([null, null]));
    assert_eq!(
        places(&d)[1..],
        [
            ("/points/0", "--points", "1000,2000"),
            ("/points/1", "--points", "3000,4000")
        ]
    );
    assert_eq!(places(&d)[0], ("/site", "--site", "-284887,683859"));
}

#[test]
fn a_list_of_objects_takes_one_each_time() {
    let d = read(
        "exposure",
        &[
            &["sites", "town:A@10", "20000"],
            &["sites", "radius=15000", "observer=town:B@5"],
            &["target-alt-agl", "30"],
        ],
    );
    assert_eq!(
        d.request["sites"],
        json!([
            {"observer": {"alt_agl": 10.0}, "radius": 20000.0},
            {"observer": {"alt_agl": 5.0}, "radius": 15000.0},
        ])
    );
    assert_eq!(
        places(&d),
        [
            ("/sites/0/observer", "--sites #1 observer", "town:A"),
            ("/sites/1/observer", "--sites #2 observer", "town:B"),
        ]
    );
}

#[test]
fn a_list_inside_a_criterion_repeats_its_key() {
    let d = read(
        "find_sites",
        &[
            &["region", "circle", "town:Zugdidi", "10000"],
            &["footprint", "60", "40"],
            &[
                "criteria",
                "not-visible-from",
                "observers=town:A@10",
                "observers=town:B@12",
                "max-fraction=0",
            ],
            &["spacing", "1000"],
        ],
    );
    assert_eq!(
        d.request["criteria"],
        json!([{"name": "not_visible_from", "params": {
            "observers": [{"alt_agl": 10.0}, {"alt_agl": 12.0}],
            "max_fraction": 0.0,
        }}])
    );
    assert_eq!(d.request["region"], json!({"Circle": {"radius": 10000.0}}));
    assert_eq!(
        places(&d)[0],
        ("/region/Circle", "--region circle at", "town:Zugdidi")
    );
}

#[test]
fn a_box_takes_two_corners_and_a_polygon_its_corners() {
    let d = read("towns", &[&["region", "box", "1,2m", "3m,4m"]]);
    assert_eq!(d.request["region"], json!({"Box": {}}));
    let kinds: Vec<&PendingKind> = d.pending.iter().map(|p| &p.kind).collect();
    assert_eq!(
        kinds,
        [
            &PendingKind::Place(PlaceAs::Split("min_".into())),
            &PendingKind::Place(PlaceAs::Split("max_".into())),
        ]
    );
    let d = read("towns", &[&["region", "polygon", "a", "b", "c"]]);
    assert_eq!(
        d.request["region"],
        json!({"Polygon": {"points": [null, null, null]}})
    );
    assert_eq!(d.pending[2].pointer, "/region/Polygon/points/2");
    assert_eq!(d.pending[2].kind, PendingKind::Place(PlaceAs::Pair));
}

#[test]
fn words_lists_maps_and_switches() {
    let d = read(
        "towns",
        &[
            &["size", "town,large-village"],
            &["form", "Compact"],
            &["sort", "floor"],
            &["outlines"],
        ],
    );
    assert_eq!(
        d.request,
        json!({"size": ["town", "large_village"], "form": ["compact"], "sort": "floor",
               "outlines": true})
    );
    let d = read(
        "find_sites",
        &[
            &[
                "region",
                "{\"Circle\": {\"x\": 1, \"z\": 2, \"radius\": 3}}",
            ],
            &["footprint", "1", "1"],
            &["criteria", "valid"],
            &["spacing", "1"],
            &["weights", "slope-max=2", "dist_road_band=0.5"],
        ],
    );
    assert_eq!(
        d.request["weights"],
        json!({"slope_max": 2.0, "dist_road_band": 0.5})
    );
    assert_eq!(d.request["criteria"], json!([{"name": "valid"}]));
    assert_eq!(d.request["region"]["Circle"]["radius"], 3);
}

#[test]
fn optional_values_of_an_object_are_keyed() {
    let d = read(
        "airfield_info",
        &[
            &["airdrome", "airport:Batumi"],
            &["fits", "width=16", "length=17", "height=5", "helicopter"],
        ],
    );
    assert_eq!(
        d.request["fits"],
        json!({"width": 16.0, "length": 17.0, "height": 5.0, "helicopter": true})
    );
    let d = read(
        "find_sites",
        &[
            &["region", "circle", "1000,2000", "5"],
            &["footprint", "120", "80", "orientations-deg=0,45,90"],
            &["criteria", "clearing-max", "0", "radius=200"],
            &["spacing", "1"],
        ],
    );
    assert_eq!(
        d.request["footprint"]["orientations_deg"],
        json!([0.0, 45.0, 90.0])
    );
    assert_eq!(
        d.request["criteria"][0]["params"],
        json!({"max": 0, "radius": 200.0})
    );
}

#[test]
fn a_nested_object_takes_dotted_keys_or_commas() {
    let d = read(
        "scenery_in",
        &[&[
            "footprint-at",
            "town:A",
            "30",
            "footprint.width=60",
            "footprint.depth=40",
        ]],
    );
    assert_eq!(
        d.request["footprint_at"],
        json!({"orientation_deg": 30.0, "footprint": {"width": 60.0, "depth": 40.0}})
    );
    let d = read(
        "scenery_in",
        &[&["footprint-at", "town:A", "30", "footprint=60,40"]],
    );
    assert_eq!(
        d.request["footprint_at"]["footprint"],
        json!({"width": 60.0, "depth": 40.0})
    );
}

#[test]
fn a_placement_takes_keys_and_its_shape_by_name_or_json() {
    let d = read(
        "validate_placements",
        &[
            &["placements", "id=sam", "at=town:A", "max-slope-deg=5"],
            &[
                "placements",
                "id=r",
                "at=town:B",
                "shape=rect",
                "shape.width=60",
                "shape.depth=40",
            ],
            &[
                "placements",
                "id=c",
                "at=1000,2000",
                "shape={\"Circle\":{\"radius\":30}}",
            ],
        ],
    );
    assert_eq!(
        d.request["placements"],
        json!([
            {"id": "sam", "max_slope_deg": 5.0},
            {"id": "r", "shape": {"Rect": {"width": 60.0, "depth": 40.0}}},
            {"id": "c", "shape": {"Circle": {"radius": 30}}},
        ])
    );
    assert_eq!(d.pending[1].pointer, "/placements/1");
}

#[test]
fn geo_converts_latitude_and_longitude_written_either_way() {
    let d = read(
        "geo",
        &[
            &["to-xz", "42.18,42.48", "42.177616N 42.481292E"],
            &["to-latlon", "1000,2000"],
        ],
    );
    assert_eq!(
        d.request["to_xz"],
        json!([{"lat": 42.18, "lon": 42.48}, {"lat": 42.177616, "lon": 42.481292}])
    );
}

#[test]
fn a_mistake_is_named_with_what_would_do() {
    let e = refused("find_sites", &[&["region", "circel", "a", "1"]]);
    assert!(e.contains("Did you mean `circle`?"), "{e}");
    let e = refused(
        "find_sites",
        &[
            &["region", "circle", "a", "1"],
            &["footprint", "1", "1"],
            &["criteria", "slop-max", "3"],
            &["spacing", "1"],
        ],
    );
    assert!(e.contains("Did you mean `slope-max`?"), "{e}");
    let e = refused("find_sites", &[&["region", "circle", "a"]]);
    assert!(e.contains("needs `radius`"), "{e}");
    let e = refused("find_sites", &[&["region", "circle", "a", "1", "2"]]);
    assert!(e.contains("more values than it takes: 2"), "{e}");
    let e = refused(
        "find_sites",
        &[
            &["region", "circle", "a", "1"],
            &["footprint", "1", "1", "widht=3"],
        ],
    );
    assert!(e.contains("Did you mean `width`?"), "{e}");
    let e = refused("towns", &[&["sort", "flor"]]);
    assert!(e.contains("Did you mean `floor`?"), "{e}");
    let e = refused("towns", &[&["limit", "ten"]]);
    assert!(e.contains("whole number"), "{e}");
    let e = refused("find_sites", &[]);
    assert!(e.contains("--region is required"), "{e}");
    let e = refused("towns", &[&["limit", "1"], &["limit", "2"]]);
    assert!(e.contains("more than once"), "{e}");
}

#[test]
fn a_list_inside_an_object_takes_several_values_in_order_or_after_its_key() {
    let d = read(
        "score_site",
        &[
            &["at", "1000,2000"],
            &["footprint", "20", "30", "orientations-deg=0", "90", "45"],
            &[
                "criteria",
                "not-visible-from",
                "town:A@1",
                "town:B@2",
                "0.5",
            ],
            &[
                "criteria",
                "visible-to",
                "targets=town:C@3",
                "town:D@4",
                "min-fraction=1",
            ],
        ],
    );
    assert_eq!(
        d.request["footprint"]["orientations_deg"],
        json!([0.0, 90.0, 45.0])
    );
    assert_eq!(
        d.request["criteria"][0]["params"],
        json!({"observers": [{"alt_agl": 1.0}, {"alt_agl": 2.0}], "max_fraction": 0.5})
    );
    assert_eq!(
        d.request["criteria"][1]["params"],
        json!({"targets": [{"alt_agl": 3.0}, {"alt_agl": 4.0}], "min_fraction": 1.0})
    );
}

#[test]
fn a_list_of_text_splits_at_commas_and_a_map_key_is_given_once() {
    let d = read(
        "sample",
        &[
            &["points", "1000,2000"],
            &["layers", "height,slope", "cover"],
        ],
    );
    assert_eq!(d.request["layers"], json!(["height", "slope", "cover"]));
    let e = refused(
        "find_sites",
        &[
            &["region", "circle", "a", "1"],
            &["footprint", "1", "1"],
            &["criteria", "valid"],
            &["spacing", "1"],
            &["weights", "slope-max=1", "slope_max=2"],
        ],
    );
    assert!(e.contains("given more than once"), "{e}");
}
