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
