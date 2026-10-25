//! The printed schema is the contract the Kotlin client generates from, so it
//! is held to a committed copy: any change to a request or response type
//! fails here until the copy is regenerated, and the regenerated copy is the
//! reviewable diff.
//!
//! To accept a change, run the test with `DCSTERRAIN_UPDATE_SNAPSHOTS=1`
//! (that value only, so a stray `=0` cannot rewrite it) and commit the
//! rewritten file.

use std::path::Path;
use std::process::Command;

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_dcsterrain"))
        .args(args)
        .output()
        .expect("the binary runs")
}

#[test]
fn the_schema_matches_its_snapshot() {
    let out = run(&["schema"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let printed = String::from_utf8(out.stdout).unwrap();

    let doc: serde_json::Value = serde_json::from_str(&printed).expect("stdout is one JSON value");
    assert_eq!(
        doc["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );

    let snapshot = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots/schema.json");
    if std::env::var_os("DCSTERRAIN_UPDATE_SNAPSHOTS").is_some_and(|v| v == "1") {
        std::fs::write(&snapshot, &printed).unwrap();
        return;
    }
    let committed = std::fs::read_to_string(&snapshot).unwrap_or_default();
    assert!(
        printed == committed,
        "`dcsterrain schema` differs from {}; rerun with DCSTERRAIN_UPDATE_SNAPSHOTS=1 \
         to accept the change, and review the diff",
        snapshot.display()
    );
}

#[test]
fn an_unknown_operation_is_refused() {
    let out = run(&["schema", "--op", "nope"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("describe"));
}

#[test]
fn a_stub_says_so_on_stderr() {
    let out = run(&["stamp", "anywhere", "k=v"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("not built yet"));
}
