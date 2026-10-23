//! `dcsterrain synth` writes an extract directory a user can pack without DCS.

use std::path::PathBuf;
use std::process::Command;

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_dcsterrain"))
        .args(args)
        .output()
        .expect("the binary runs")
}

/// A fresh directory under the system temp, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let name = format!("dcsterrain-cli-{tag}-{}", std::process::id());
        let dir = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&dir);
        Scratch(dir)
    }

    fn arg(&self) -> &str {
        self.0.to_str().unwrap()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn writes_an_extract_then_refuses_to_overwrite_it() {
    let s = Scratch::new("synth");
    let out = run(&["synth", s.arg()]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stdout.is_empty());

    let manifest = std::fs::read_to_string(s.0.join("manifest.json")).unwrap();
    let m: serde_json::Value = serde_json::from_str(&manifest).unwrap();
    assert_eq!(m["format_version"], 2);
    assert_eq!(m["theatre"], "Synth");

    let again = run(&["synth", s.arg()]);
    assert_eq!(again.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&again.stderr).contains("not empty"));
    assert_eq!(
        std::fs::read_to_string(s.0.join("manifest.json")).unwrap(),
        manifest
    );
}

#[test]
fn refuses_a_cell_other_than_fifty() {
    let s = Scratch::new("cell");
    let out = run(&["synth", s.arg(), "--cell", "100"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(!s.0.exists());
}

#[test]
fn check_extract_passes_the_synthetic_extract_and_names_a_broken_file() {
    let s = Scratch::new("check");
    assert!(run(&["synth", s.arg()]).status.success());

    let ok = run(&["check-extract", s.arg()]);
    assert!(
        ok.status.success(),
        "{}",
        String::from_utf8_lossy(&ok.stdout)
    );
    assert!(String::from_utf8_lossy(&ok.stdout).contains("passes: Synth"));

    std::fs::write(s.0.join("tiles/water/0_0.bin"), b"short").unwrap();
    let bad = run(&["check-extract", s.arg()]);
    assert_eq!(bad.status.code(), Some(1));
    let report = String::from_utf8_lossy(&bad.stdout);
    assert!(report.contains("tiles/water/0_0.bin: 5 bytes"), "{report}");
}

#[test]
fn check_extract_refuses_a_directory_with_no_manifest() {
    let s = Scratch::new("nomanifest");
    std::fs::create_dir_all(&s.0).unwrap();
    let out = run(&["check-extract", s.arg()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stdout).contains("manifest.json: missing"));
}
