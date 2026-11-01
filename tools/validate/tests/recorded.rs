//! Opt-in: the routes of a recorded sortie against the tool as it is now. Set
//! `DCSTERRAIN_INSTALL` to the install root and `DCSTERRAIN_SORTIE` to a
//! sortie's `--out` directory, whose `plan.json` and `replies` are read; without
//! them this passes reading nothing, which is what CI does. Run it with
//! `--release`. The install is only read; the cache is a scratch directory.

use std::path::PathBuf;

use dcsterrain_core::cache::Cache;
use dcsterrain_core::query::{Terrain, answer};
use dcsterrain_validate::plan::Plan;
use serde_json::{Value, json};

/// How much longer than the game's a route may be, twice the most measured.
const LONGER_AT_MOST: f64 = 0.005;

fn recorded(dir: &std::path::Path) -> Vec<Value> {
    let mut out = Vec::new();
    for k in 0.. {
        let path = dir.join("replies").join(format!("routes-{k:03}.json"));
        let Ok(text) = std::fs::read_to_string(path) else {
            return out;
        };
        let v: Value = serde_json::from_str(&text).unwrap();
        out.extend(v["values"].as_array().unwrap().iter().cloned());
    }
    out
}

#[test]
fn every_refused_route_is_refused_and_none_is_longer_than_the_game() {
    let (Some(root), Some(dir)) = (
        std::env::var_os("DCSTERRAIN_INSTALL").map(PathBuf::from),
        std::env::var_os("DCSTERRAIN_SORTIE").map(PathBuf::from),
    ) else {
        eprintln!("DCSTERRAIN_INSTALL or DCSTERRAIN_SORTIE is not set; skipped");
        return;
    };
    let plan: Plan =
        serde_json::from_str(&std::fs::read_to_string(dir.join("plan.json")).unwrap()).unwrap();
    let replies = recorded(&dir);
    assert_eq!(replies.len(), plan.routes.len());
    let scratch = std::env::temp_dir().join(format!("dcsterrain-recorded-{}", std::process::id()));
    let cache = Cache::new(&scratch);
    let t = Terrain::open(&root, &plan.terrain, &cache, true).unwrap();
    let mut refused = 0;
    let mut errors = Vec::new();
    let mut wrong = Vec::new();
    for (i, (c, game)) in plan.routes.iter().zip(&replies).enumerate() {
        // How far the game's path starts and stops from the ends asked.
        let off = |k: usize, [x, z]: [f64; 2]| {
            let (a, b) = (game[k].as_f64(), game[k + 1].as_f64());
            a.zip(b).map_or(0.0, |(a, b)| (a - x).hypot(b - z))
        };
        let past = off(2, c.from) + off(4, c.to);
        let game = game[0].as_f64().filter(|l| *l > 0.0);
        let [fx, fz] = c.from;
        let [tx, tz] = c.to;
        let tool = answer(
            &t,
            "route",
            &json!({"network": c.network,
                    "from": {"x": fx, "z": fz},
                    "to": {"x": tx, "z": tz},
                    "max_detour_factor": 1e6}),
        );
        let tool = match tool {
            Ok(v) => v["length"].as_f64(),
            Err(e) if e[0].code == "no_route" => None,
            Err(e) => panic!("route {i}: {e:?}"),
        };
        match (tool, game) {
            (Some(tool), Some(game)) => {
                errors.push((tool / game - 1.0, i, tool, game, past));
            }
            (None, None) => refused += 1,
            _ => wrong.push((i, c.network.clone(), tool, game)),
        }
    }
    errors.sort_by(|a, b| a.0.total_cmp(&b.0));
    let shorter = errors.iter().filter(|e| e.0 < 0.0).count();
    // A route shorter than the game's by more than the game's path runs past
    // the ends asked took a way the game did not.
    let unexplained: Vec<_> = errors.iter().filter(|e| e.3 - e.2 > e.4 + 0.5).collect();
    eprintln!("shorter beyond the game's own ends: {unexplained:?}");
    eprintln!(
        "{} compared, {shorter} shorter than the game's, p50 {:+.4}, least {:?}, most {:?}; {refused} refused by both; {} disagree",
        errors.len(),
        errors[errors.len() / 2].0,
        errors[0],
        errors[errors.len() - 1],
        wrong.len()
    );
    for e in errors.iter().take(5).chain(errors.iter().rev().take(5)) {
        eprintln!("{e:?}");
    }
    drop(t);
    let _ = std::fs::remove_dir_all(&scratch);
    assert!(wrong.is_empty(), "{wrong:?}");
    let most = errors[errors.len() - 1].0;
    assert!(most <= LONGER_AT_MOST, "{most}");
}
