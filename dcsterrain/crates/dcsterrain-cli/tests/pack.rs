//! `dcsterrain pack` turns the synthetic extract into a packed file that
//! `dcsterrain check` passes, reporting every `meta` key.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use dcsterrain_core::pack::meta::KEYS;

fn run(cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_dcsterrain"))
        .current_dir(cwd)
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
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn packs_the_synthetic_extract_under_its_default_name_and_checks_it() {
    let s = Scratch::new("pack");
    let synth = run(&s.0, &["synth", "extract"]);
    assert!(synth.status.success(), "{}", stderr(&synth));

    let pack = run(&s.0, &["pack", "extract"]);
    assert!(pack.status.success(), "{}", stderr(&pack));
    let name = "Synth-0.0.0.0-50m.sqlite";
    assert!(s.0.join(name).is_file());
    assert!(String::from_utf8_lossy(&pack.stdout).starts_with(name));

    let check = run(&s.0, &["check", name]);
    assert!(check.status.success(), "{}", stderr(&check));
    let stdout = String::from_utf8_lossy(&check.stdout);
    let keys: Vec<&str> = stdout
        .lines()
        .map(|l| l.split(" = ").next().unwrap())
        .collect();
    assert_eq!(keys, KEYS);
    assert!(stdout.contains("validation_at = NULL\n"), "{stdout}");
    assert!(stdout.contains("theatre_id = Synth\n"), "{stdout}");
}

#[test]
fn packs_at_100_m_when_asked() {
    let s = Scratch::new("pack100");
    assert!(run(&s.0, &["synth", "extract"]).status.success());
    let pack = run(&s.0, &["pack", "extract", "--cell-size", "100"]);
    assert!(pack.status.success(), "{}", stderr(&pack));
    let check = run(&s.0, &["check", "Synth-0.0.0.0-100m.sqlite"]);
    assert!(check.status.success(), "{}", stderr(&check));
    assert!(String::from_utf8_lossy(&check.stdout).contains("cell_size = 100\n"));
}

#[test]
fn check_fails_on_a_file_that_is_not_packed() {
    let s = Scratch::new("checkbad");
    std::fs::write(s.0.join("notes.txt"), "not a database").unwrap();
    let check = run(&s.0, &["check", "notes.txt"]);
    assert_eq!(check.status.code(), Some(1));
    assert!(check.stdout.is_empty());
    assert!(stderr(&check).contains("not a packed file"));
}

#[test]
fn a_flag_not_built_yet_says_so() {
    let s = Scratch::new("packflag");
    let out = run(&s.0, &["pack", "extract", "--drop", "horizon"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("--drop"));
}
