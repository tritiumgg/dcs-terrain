use super::*;
use clap::CommandFactory;
use dcsterrain_core::types::OPERATIONS;

fn spec(op: &str) -> Spec {
    Spec::of(op).unwrap()
}

fn args(line: &str) -> Vec<String> {
    line.split_whitespace().map(String::from).collect()
}

/// Splits a command line as a shell would, keeping quoted words whole.
fn shell_words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    for c in line.chars() {
        match c {
            '"' => quoted = !quoted,
            ' ' if !quoted => {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            }
            c => word.push(c),
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

#[test]
fn values_follow_their_flag_and_a_leading_hyphen_is_a_value() {
    let s = spec("find_sites");
    let line = tokens(
        &s,
        &args(
            "--terrain synth --region circle -284887,683859 1500 --footprint 60 40 \
             --criteria slope-max 3 --spacing=200 --json --limit 3",
        ),
    )
    .unwrap();
    assert_eq!(
        line.given,
        [
            ("region".into(), args("circle -284887,683859 1500")),
            ("footprint".into(), args("60 40")),
            ("criteria".into(), args("slope-max 3")),
            ("spacing".into(), args("200")),
            ("limit".into(), args("3")),
        ]
    );
    assert_eq!(line.get("terrain"), Some("synth"));
    assert!(line.has("json"));
}

#[test]
fn a_single_value_flag_closes_after_its_value() {
    let s = spec("nearest");
    let e = tokens(&s, &args("--kind road --at 1,2 3")).unwrap_err();
    assert!(e.0.contains("\"3\" follows no flag"), "{}", e.0);
    let line = tokens(&s, &args("--n -1")).unwrap();
    assert_eq!(line.given, [("n".into(), args("-1"))]);
}

#[test]
fn an_unknown_flag_names_the_closest_real_one() {
    let s = spec("find_sites");
    let e = tokens(&s, &args("--spacng 200")).unwrap_err();
    assert_eq!(
        e.0,
        "--spacng is not a flag of find-sites. Did you mean `--spacing`?"
    );
    let e = tokens(&s, &args("--terain synth")).unwrap_err();
    assert!(e.0.ends_with("Did you mean `--terrain`?"), "{}", e.0);
    let e = tokens(&s, &args("-x")).unwrap_err();
    assert!(e.0.contains("-x is not a flag"), "{}", e.0);
    let e = tokens(&spec("describe"), &args("--geojson-out a.json")).unwrap_err();
    assert!(e.0.contains("returns no GeoJSON"), "{}", e.0);
    let e = tokens(&s, &args("--terrain")).unwrap_err();
    assert_eq!(e.0, "--terrain needs a value");
}

#[test]
fn an_issue_is_named_by_its_flag() {
    assert_eq!(flag_of("criteria.2.params.deg"), "--criteria #3 deg");
    assert_eq!(flag_of("x"), "--at");
    assert_eq!(flag_of("max_detour_factor"), "--max-detour-factor");
    assert_eq!(flag_of("--sites #1 observer"), "--sites #1 observer");
    assert_eq!(flag_of(""), "");
}

#[test]
fn the_plain_list_names_every_field() {
    let answer = json!({
        "candidates": [{"x": 1, "z": 2}, {"x": 3, "z": 4}],
        "stats": {"evaluated": 10},
        "empty": [],
        "geojson": {"type": "FeatureCollection", "features": [{}, {}]},
        "terrain": "synth",
        "visible": true,
        "first_seen": null,
    });
    let text = plain(&answer, Some(std::path::Path::new("out.geojson")));
    assert_eq!(
        text,
        "candidates: 2 items\n  {\"x\":1,\"z\":2}\n  {\"x\":3,\"z\":4}\nstats:\n  \
         evaluated: 10\nempty: none\ngeojson: FeatureCollection, 2 features, written to \
         out.geojson\nterrain: synth\nvisible: true\nfirst_seen: none\n"
    );
}

#[test]
fn two_corners_in_either_order_make_a_box() {
    let mut v =
        json!({"region": {"Box": {"min_x": 5.0, "min_z": 1.0, "max_x": 2.0, "max_z": 4.0}}});
    corners(&mut v);
    assert_eq!(
        v,
        json!({"region": {"Box": {"min_x": 2.0, "min_z": 1.0, "max_x": 5.0, "max_z": 4.0}}})
    );
}

#[test]
fn every_operation_has_an_example_that_reads() {
    for o in OPERATIONS {
        let e = example(o.name).unwrap_or_else(|| panic!("{} has no example", o.name));
        let words = shell_words(e);
        assert_eq!(words[0], "dcsterrain");
        assert_eq!(words[1], kebab(o.name), "{e}");
        let s = spec(o.name);
        let line = tokens(&s, &words[2..]).unwrap_or_else(|u| panic!("{e}: {}", u.0));
        s.read(&line.given)
            .unwrap_or_else(|u| panic!("{e}: {}", u.0));
    }
    assert_eq!(EXAMPLES.len(), OPERATIONS.len());
}

#[test]
fn no_command_or_flag_clashes_with_another() {
    let cli = crate::Cli::command();
    for o in OPERATIONS {
        let name = kebab(o.name);
        assert!(
            cli.find_subcommand(&name).is_none(),
            "{name} is already a command"
        );
        for f in spec(o.name).fields {
            let flag = kebab(&f.name);
            assert!(
                !SHARED.iter().any(|(n, _)| *n == flag),
                "{}: --{flag} is a shared flag",
                o.name
            );
        }
    }
    for name in [
        "query",
        "mcp",
        "list-terrains",
        "index",
        "check",
        "cache",
        "schema",
        "help",
    ] {
        assert!(!OPERATIONS.iter().any(|o| kebab(o.name) == name), "{name}");
    }
}

#[test]
fn after_two_hyphens_every_word_is_a_value() {
    let s = spec("nearest");
    let line = tokens(&s, &args("--kind scenery --class -- -x")).unwrap();
    assert_eq!(line.given[1], ("class".into(), args("-x")));
}

#[test]
fn a_shared_flag_is_given_once_and_geojson_points_to_its_flag() {
    let s = spec("nearest");
    let e = tokens(&s, &args("--terrain synth --terrain Caucasus")).unwrap_err();
    assert_eq!(e.0, "--terrain is given more than once");
    let e = tokens(&s, &args("--geojson")).unwrap_err();
    assert!(
        e.0.ends_with("Did you mean `--geojson-out <file>`?"),
        "{}",
        e.0
    );
    let e = tokens(&s, &args("--j")).unwrap_err();
    assert!(!e.0.contains("Did you mean"), "{}", e.0);
}
