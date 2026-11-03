use super::*;
use dcsterrain_core::types::OPERATIONS;

fn spec(op: &str) -> Spec {
    Spec::of(op).expect("a listed operation")
}

/// The occurrences a command line gives, each flag with its values.

#[test]
fn every_operation_has_a_summary_and_its_fields_have_help() {
    for o in OPERATIONS {
        let s = spec(o.name);
        assert!(!s.summary.is_empty(), "{}", o.name);
        for f in &s.fields {
            assert!(!f.help.is_empty(), "{}: {}", o.name, f.name);
            assert_ne!(
                f.shape,
                Shape::Json,
                "{}: {} has no typed form",
                o.name,
                f.name
            );
        }
    }
}

#[test]
fn a_misspelling_is_matched_to_the_closest_name() {
    let names = ["spacing", "step", "limit"];
    assert_eq!(closest("spacng", names.iter().copied()), Some("spacing"));
    assert_eq!(closest("zzzzzz", names.iter().copied()), None);
}
