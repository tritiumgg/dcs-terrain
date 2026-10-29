//! Opt-in: `check` on a real install. Set `DCSTERRAIN_INSTALL` to the DCS
//! install root; without it this passes without reading anything, which is
//! what CI does. The install is only read; the cache is a fresh directory.

use std::fs;
use std::process::Command;
use std::time::Instant;

use serde_json::Value;

#[test]
fn check_passes_on_caucasus() {
    let Some(root) = std::env::var_os("DCSTERRAIN_INSTALL") else {
        eprintln!("DCSTERRAIN_INSTALL is not set; skipped");
        return;
    };
    let cache = std::env::temp_dir().join(format!("dcsterrain-check-{}", std::process::id()));
    let _ = fs::remove_dir_all(&cache);
    let at = Instant::now();
    let out = Command::new(env!("CARGO_BIN_EXE_dcsterrain"))
        .args(["check", "--terrain", "Caucasus", "--json", "--install"])
        .arg(&root)
        .arg("--cache")
        .arg(&cache)
        .output()
        .expect("the binary runs");
    let took = at.elapsed().as_secs_f64();
    let _ = fs::remove_dir_all(&cache);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    eprintln!(
        "Caucasus on {}: checked in {took:.1} s with an empty cache",
        v["dcs_build"]
    );
    assert!(
        v["files"]
            .as_array()
            .unwrap()
            .iter()
            .all(|f| f["readable"] == true)
    );
    assert!(v["tables"]["airdromes"].as_u64().unwrap() > 0);
    assert!(v["tables"]["crs"]["max_m"].as_f64().unwrap() < 2.0);
    assert!(v["catalog"]["objects"].as_u64().unwrap() > 0);
    assert_eq!(v["catalog"]["most_placed"].as_array().unwrap().len(), 25);
    assert_eq!(v["catalog"]["tallest"].as_array().unwrap().len(), 10);
    assert_eq!(v["catalog"]["unmatched"], Value::Array(Vec::new()));
}
