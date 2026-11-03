use super::*;
use dcsterrain_core::query::truncate_mgrs;
use dcsterrain_core::synth::{self, Synth};

fn synth() -> Terrain {
    let s = Synth::new(
        synth::DEFAULT_SEED,
        synth::DEFAULT_SIZE_KM,
        synth::CELL_SIZE_M,
    )
    .unwrap();
    Terrain::Synth(Box::new(s))
}

/// The refusal as a person reads it, after the flag.
pub(super) fn refusal(terrain: &Terrain, text: &str) -> (String, String) {
    let e = resolve(terrain, "--at", text).unwrap_err();
    assert_eq!(e.len(), 1);
    assert_eq!(e[0].path, "--at");
    (e[0].code.clone(), format!("--at: {}", e[0].message))
}

pub(super) fn at(terrain: &Terrain, text: &str) -> (f64, f64) {
    let p = resolve(terrain, "--at", text).unwrap_or_else(|e| panic!("{text}: {e:?}"));
    (p.x, p.z)
}

pub(super) fn geo_of(terrain: &Terrain, x: f64, z: f64) -> (f64, f64, String) {
    let q = GeoRequest {
        to_latlon: vec![Point { x, z }],
        to_xz: Vec::new(),
    };
    let g = query::geo(terrain, q).unwrap().to_latlon.remove(0);
    (g.lat, g.lon, g.mgrs.unwrap())
}

/// Degrees, minutes and seconds, as a person writes them.
fn dms(v: f64, pos: char, neg: char) -> String {
    let letter = if v < 0.0 { neg } else { pos };
    let v = v.abs();
    let d = v.floor();
    let m = ((v - d) * 60.0).floor();
    let s = ((v - d) * 60.0 - m) * 60.0;
    format!("{d}°{m}'{s:.4}\"{letter}")
}

/// Each form names `(x, z)`, as `geo` converts it: x,z itself, latitude and
/// longitude in each spelling, and MGRS at every precision, whose square
/// holds the point and which `geo` writes back the same.
pub(super) fn every_form_round_trips(terrain: &Terrain, x: f64, z: f64) {
    let near = |(a, b): (f64, f64), tol: f64| {
        assert!((a - x).hypot(b - z) <= tol, "({a}, {b}) is not ({x}, {z})");
    };
    near(at(terrain, &format!("{x},{z}")), 0.0);
    near(at(terrain, &format!("{x}m, {z}m")), 0.0);
    let (lat, lon, grid) = geo_of(terrain, x, z);
    let ns = if lat < 0.0 { 'S' } else { 'N' };
    let ew = if lon < 0.0 { 'W' } else { 'E' };
    let (alat, alon) = (lat.abs(), lon.abs());
    for text in [
        format!("{alat:.10}{ns} {alon:.10}{ew}"),
        format!("{ns}{alat:.10}, {ew}{alon:.10}"),
        format!("{alon:.10}{ew} {alat:.10}{ns}"),
        format!("{} {}", dms(lat, 'N', 'S'), dms(lon, 'E', 'W')),
    ] {
        near(at(terrain, &text), 0.01);
    }
    for d in 1..=5 {
        let typed = truncate_mgrs(&grid, d);
        let p = at(terrain, &typed);
        let (_, _, back) = geo_of(terrain, p.0, p.1);
        assert_eq!(truncate_mgrs(&back, d), typed);
        let side = 10f64.powi(5 - d as i32);
        near(p, side * std::f64::consts::SQRT_2);
        assert_eq!(at(terrain, &typed.replace(' ', "").to_lowercase()), p);
    }
}

#[test]
fn on_synth_each_form_round_trips_against_geo() {
    let t = synth();
    let tables = t.source().tables().unwrap();
    let a = &tables.airdromes[0];
    let (ax, az) = (a.x.unwrap(), a.z.unwrap());
    for text in [
        "airport:Synth Field",
        "airport:SYNTH",
        "AIRPORT: synth-field",
    ] {
        assert_eq!(at(&t, text), (ax, az), "{text}");
    }
    let (lat, lon, _) = geo_of(&t, ax, az);
    assert!((lat - a.lat.unwrap()).abs() < 1e-9 && (lon - a.lon.unwrap()).abs() < 1e-9);
    every_form_round_trips(&t, ax, az);
    for (k, town) in tables.towns.iter().enumerate() {
        let name = synth::town_name(k as u32 + 1);
        assert_eq!(at(&t, &format!("town:{name}")), (town.x, town.z));
        let upper = format!("town:{}", name.to_uppercase());
        assert_eq!(at(&t, &upper), (town.x, town.z));
        every_form_round_trips(&t, town.x, town.z);
    }
    let e = query::describe(&t, DescribeRequest {})
        .unwrap()
        .extent
        .unwrap();
    for (x, z) in [
        (e.min_x + 10_000.0, e.min_z + 10_000.0),
        ((e.min_x + e.max_x) / 2.0, (e.min_z + e.max_z) / 2.0),
        (e.max_x - 10_000.0, e.max_z - 10_000.0),
    ] {
        every_form_round_trips(&t, x, z);
    }
}

#[test]
fn a_name_two_towns_share_is_refused_with_each_one() {
    let t = synth();
    let tables = t.source().tables().unwrap();
    let (one, two) = (&tables.towns[0], &tables.towns[1]);
    // A second town of the first's name, where the second town stands.
    let shared = || {
        let mut all = names(&t)?;
        all.push(Named {
            kind: Kind::Town,
            shown: synth::town_name(1),
            keys: vec![fold(&synth::town_name(1))],
            at: Some((two.x, two.z)),
        });
        Ok(all)
    };
    let e = resolve_among(&t, shared, "--at", "town:synth town 1").unwrap_err();
    let (g1, g2) = (geo_of(&t, one.x, one.z).2, geo_of(&t, two.x, two.z).2);
    assert_eq!(e[0].code, "ambiguous_place");
    assert_eq!(
        e[0].message,
        format!(
            "\"synth town 1\" names 2 towns on synth. Type the position of the one you mean:\n  \
             {g1}   x {:.0} z {:.0}\n  {g2}   x {:.0} z {:.0}",
            one.x, one.z, two.x, two.z
        )
    );
    // Each choice, typed as shown, is a place.
    for line in e[0].message.lines().skip(1) {
        let grid = line.trim().split("   ").next().unwrap();
        resolve_among(&t, shared, "--at", grid).unwrap();
    }
}

#[test]
fn a_name_needs_its_kind() {
    let t = synth();
    assert_eq!(
        refusal(&t, "Synth Field"),
        (
            "invalid_place".into(),
            "--at: \"Synth Field\" needs a kind. Type airport:Synth Field.".into()
        )
    );
    assert_eq!(
        refusal(&t, "synth town 3").1,
        "--at: \"synth town 3\" needs a kind. Type town:Synth Town 3."
    );
    let (code, m) = refusal(&t, "Nowhere");
    assert_eq!(code, "invalid_place");
    assert!(
        m.starts_with(
            "--at: \"Nowhere\" is not a place: a name needs its kind, and no airport or town \
             on synth has it.\nThe closest names: "
        ),
        "{m}"
    );
    assert!(m.ends_with(FORMS), "{m}");
}

#[test]
fn an_unknown_name_is_answered_with_the_closest() {
    let t = synth();
    assert_eq!(
        refusal(&t, "town:Synth Twn 2"),
        (
            "unknown_place".into(),
            "--at: no town on synth is named \"Synth Twn 2\".\nThe closest names: town:Synth \
             Town 2, town:Synth Town 1, town:Synth Town 3."
                .into()
        )
    );
    assert_eq!(
        refusal(&t, "airport:Synth Town 1").1,
        "--at: no airport on synth is named \"Synth Town 1\".\nThe closest names: \
         airport:Synth Field."
    );
    // A part of a name is not the name.
    assert_eq!(refusal(&t, "town:Synth").0, "unknown_place");
    let kind = refusal(&t, "airfield:Synth Field").1;
    assert!(
        kind.contains("it has the kind `airfield:`, and the kinds are airport: and town:"),
        "{kind}"
    );
    assert!(refusal(&t, "town:").1.contains("it names no town."));
}

#[test]
fn two_small_numbers_are_refused_with_both_readings() {
    let t = synth();
    assert_eq!(
        refusal(&t, "42.177616, 42.481292"),
        (
            "invalid_place".into(),
            "--at: \"42.177616, 42.481292\" could be latitude and longitude or DCS meters near \
             the terrain's origin.\nFor latitude and longitude, type: 42.177616N \
             42.481292E\nFor DCS meters, type: 42.177616m,42.481292m"
                .into()
        )
    );
    assert_eq!(
        refusal(&t, "-33.85,-151.2").1,
        "--at: \"-33.85,-151.2\" could be latitude and longitude or DCS meters near the \
         terrain's origin.\nFor latitude and longitude, type: 33.85S 151.2W\nFor DCS meters, \
         type: -33.85m,-151.2m"
    );
    assert!(
        !refusal(&t, "120,170")
            .1
            .contains("latitude and longitude, type")
    );
}

#[test]
fn a_malformed_value_says_what_is_wrong_and_lists_the_forms() {
    let t = synth();
    assert_eq!(
        refusal(&t, "38T KM 9198 725"),
        (
            "invalid_place".into(),
            format!(
                "--at: \"38T KM 9198 725\" is not a place: an MGRS grid needs an even number of \
                 digits, 2 to 10, and this one has 7.\n{FORMS}"
            )
        )
    );
    let why = |s: &str| {
        let (code, m) = refusal(&t, s);
        assert_eq!(code, "invalid_place", "{s}");
        assert!(m.ends_with(FORMS), "{m}");
        m.lines().next().unwrap().to_string()
    };
    let has = |s: &str, part: &str| {
        let m = why(s);
        assert!(m.contains(part), "{s}: {m}");
    };
    has("-42.1N 42.4E", "sign on its N half");
    has("N42.1 42.4E", "one hemisphere letter before");
    has("42.0N 42.0E 5", "numbers both before and after");
    has("N 42 E", "one hemisphere letter before");
    has("42.1N 42.4N", "one must be N or S");
    has("42.1N", "1 hemisphere letters");
    has("42 61N 42E", "minutes 61");
    has("42 10 60N 42E", "seconds 60");
    has("42.5 10N 42E", "only the last number");
    has("42°10\"N 42E", "minutes as seconds");
    has("91N 42E", "over 90");
    has("42N 181E", "over 180");
    has("42,5N 42E", "a decimal is written with a point");
    // An exponent reads as a hemisphere letter, which is refused all the same.
    has("1e5,2e5", "hemisphere letters");
    has("1,2,3", "more than one comma");
    has("1000m,2000", "`m` on one number only");
    has("10@20", "has `@`, which no place takes");
    has("airport:Synth Field@100", "has `@`");
    has("5mm,6m", "is not two numbers");
    has("town:-", "it names no town");
    has("   ", "it is empty");
}

#[test]
fn a_place_off_the_terrain_is_refused() {
    let t = synth();
    let e = query::describe(&t, DescribeRequest {})
        .unwrap()
        .extent
        .unwrap();
    let (code, m) = refusal(&t, "1000000,1000000");
    assert_eq!(code, "off_terrain");
    assert_eq!(
        m,
        format!(
            "--at: 1000000,1000000 lies off synth, which covers x {:.0} to {:.0} and z {:.0} to \
             {:.0}.",
            e.min_x, e.max_x, e.min_z, e.max_z
        )
    );
    assert_eq!(refusal(&t, "0.5N 0.5E").0, "off_terrain");
}

#[test]
fn names_fold_case_accents_and_punctuation() {
    assert_eq!(fold("Mineral'nye Vody"), fold("MineralnyeVody"));
    assert_eq!(fold("NAL'CHIK"), fold("Nalchik"));
    assert_eq!(fold("Senaki-Kolkhi"), fold("senaki kolkhi"));
    assert_eq!(fold("Krasnodar_P"), fold("krasnodar p"));
    assert_eq!(fold("Kur.Sairme"), fold("Kur Sairme"));
    assert_eq!(fold("Fürstenfeldbruck"), fold("Furstenfeldbruck"));
    assert_eq!(fold("Şanlıurfa"), "sanliurfa");
    assert_ne!(fold("Gomi"), fold("Gori"));
    assert_eq!(fold("Cafe\u{301}"), fold("Café"));
    assert_eq!(fold("İzmir"), "izmir");
}

#[test]
fn the_forms_are_told_apart() {
    let r = |s: &str| read(s).unwrap();
    assert!(matches!(r("38TKM9198072567"), Written::Mgrs(_)));
    assert!(matches!(r("31N EA 1 2"), Written::Mgrs(_)));
    assert!(matches!(r("42N 42E"), Written::LatLon { .. }));
    assert!(matches!(r("-284887, 683859"), Written::Xz { .. }));
    assert!(matches!(r("Zugdidi"), Written::Bare(_)));
    assert!(matches!(r("town:42"), Written::Name { .. }));
    assert!(matches!(r("airport :Kutaisi"), Written::Name { .. }));
    // The quotes a word processor writes are minutes and seconds.
    assert_eq!(
        r("42°10’39.4”N 42°28’52.7”E"),
        r("42°10'39.4\"N 42°28'52.7\"E")
    );
    assert_eq!(
        r("S33 51 W151 12"),
        Written::LatLon {
            lat: -(33.0 + 51.0 / 60.0),
            lon: -(151.0 + 12.0 / 60.0)
        }
    );
}

/// Opt-in: Caucasus from the install `DCSTERRAIN_INSTALL` names, its cache
/// in the user's cache directory. Without the variable it passes having read
/// nothing, which is what CI does.
#[test]
fn on_caucasus_each_form_round_trips_and_a_shared_name_is_refused() {
    use dcsterrain_core::cache::{self, Cache};
    use dcsterrain_core::decode::install;
    let Some(root) = std::env::var_os("DCSTERRAIN_INSTALL") else {
        eprintln!("DCSTERRAIN_INSTALL is not set; skipped");
        return;
    };
    let root = std::path::PathBuf::from(root);
    let build = install::read_build(&root).unwrap();
    eprintln!("DCS {} ({})", build.version, build.timestamp);
    let cache = Cache::new(cache::default_root().unwrap());
    let t = Terrain::open(&root, "Caucasus", &cache, false).unwrap();
    let tables = t.source().tables().unwrap();

    let kutaisi = tables
        .airdromes
        .iter()
        .find(|a| a.name_id == "KUTAISI")
        .unwrap();
    let (kx, kz) = (kutaisi.x.unwrap(), kutaisi.z.unwrap());
    assert_eq!(geo_of(&t, kx, kz).2, "38T KM 91980 72567");
    let (x, z) = at(&t, "38T KM 91980 72567");
    assert!((x - kx).abs() < 1.0 && (z - kz).abs() < 1.0);

    // Every airport by each of its names, and every form of its point.
    let mut airports = 0;
    for a in &tables.airdromes {
        let Some(want) = a.x.zip(a.z) else { continue };
        for name in std::iter::once(&a.name_id).chain(&a.display_name) {
            assert_eq!(at(&t, &format!("airport:{name}")), want, "{name}");
            let loose = name.to_lowercase().replace('-', " ");
            assert_eq!(at(&t, &format!("airport:{loose}")), want, "{loose}");
        }
        every_form_round_trips(&t, want.0, want.1);
        airports += 1;
    }
    assert!(airports >= 20, "{airports}");
    for (typed, name_id) in [
        ("airport:Senaki Kolkhi", "Senaki"),
        ("airport:tbilisi lochini", "Lochini"),
        ("airport:mineralnye vody", "MinVody"),
        ("airport:krasnodar p", "Krasnodar_P"),
    ] {
        let a = tables
            .airdromes
            .iter()
            .find(|a| a.name_id == name_id)
            .unwrap();
        assert_eq!(at(&t, typed), (a.x.unwrap(), a.z.unwrap()), "{typed}");
    }

    // Every 25th town whose name no other town holds.
    let key = |n: &str| fold(n);
    let mut towns = 0;
    for town in tables.towns.iter().step_by(25) {
        let k = key(&town.name);
        let shared = tables.towns.iter().filter(|o| key(&o.name) == k).count();
        if shared > 1 {
            continue;
        }
        assert_eq!(at(&t, &format!("town:{}", town.name)), (town.x, town.z));
        every_form_round_trips(&t, town.x, town.z);
        towns += 1;
    }
    assert!(towns >= 50, "{towns}");
    let nalchik = tables.towns.iter().find(|o| o.name == "NAL'CHIK").unwrap();
    assert_eq!(at(&t, "town:nalchik"), (nalchik.x, nalchik.z));

    assert_eq!(
        refusal(&t, "Kutaisi"),
        (
            "invalid_place".into(),
            "--at: \"Kutaisi\" needs a kind. Type airport:Kutaisi or town:KUTAISI.".into()
        )
    );
    // Two towns of the list whose names differ only in case: the list repeats
    // other names under one key, which keeps only its last town.
    let shared: Vec<_> = (tables.towns.iter())
        .filter(|o| fold(&o.name) == "mayskiy")
        .collect();
    assert_eq!(shared.len(), 2);
    let (code, m) = refusal(&t, "town:Mayskiy");
    assert_eq!(code, "ambiguous_place");
    let mut lines = m.lines();
    assert_eq!(
        lines.next().unwrap(),
        "--at: \"Mayskiy\" names 2 towns on Caucasus. Type the position of the one you mean:"
    );
    for (line, town) in lines.zip(&shared) {
        let grid = geo_of(&t, town.x, town.z).2;
        assert_eq!(line, format!("  {grid}   {}", shown_xz(town.x, town.z)));
        let (x, z) = at(&t, &grid);
        assert!((x - town.x).abs() < 1.0 && (z - town.z).abs() < 1.0);
    }
    eprintln!(
        "{airports} airports and {towns} towns round-trip on DCS {}",
        build.version
    );
}

#[test]
fn rows_repeated_at_one_position_are_one_place() {
    let t = synth();
    let tables = t.source().tables().unwrap();
    let one = &tables.towns[0];
    let repeated = || {
        let mut all = names(&t)?;
        all.push(Named {
            kind: Kind::Town,
            shown: synth::town_name(1),
            keys: vec![fold(&synth::town_name(1))],
            at: Some((one.x, one.z)),
        });
        Ok(all)
    };
    let p = resolve_among(&t, repeated, "--at", "town:Synth Town 1").unwrap();
    assert_eq!((p.x, p.z), (one.x, one.z));
}
