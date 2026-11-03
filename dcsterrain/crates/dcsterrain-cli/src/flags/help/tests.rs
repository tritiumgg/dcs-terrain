use super::*;
use dcsterrain_core::types::OPERATIONS;

fn spec(op: &str) -> Spec {
    Spec::of(op).expect("a listed operation")
}

/// The occurrences a command line gives, each flag with its values.

#[test]
fn the_help_lists_every_field_its_default_and_each_shape() {
    for o in OPERATIONS {
        let s = spec(o.name);
        let help = s.help(None);
        for f in &s.fields {
            assert!(
                help.contains(&format!("--{} ", kebab(&f.name)))
                    || help.contains(&format!("--{}\n", kebab(&f.name))),
                "{}: {}",
                o.name,
                f.name
            );
        }
        assert_eq!(help.contains("--geojson-out"), s.geojson, "{}", o.name);
        for line in help.lines() {
            assert!(line.chars().count() <= 100, "{}: {line}", o.name);
        }
    }
    let help = spec("find_sites").help(None);
    assert!(help.contains("[default: 20]"), "{help}");
    assert!(
        help.contains("airdrome-band <airdrome> <min-dist> <max-dist>"),
        "{help}"
    );
    assert!(help.contains("slope-max <deg>"), "{help}");
    assert!(help.contains("clearing-max <max> radius="), "{help}");
}
