//! Opt-in: the airdrome ids against the missions the install ships. Set
//! `DCSTERRAIN_INSTALL` to the DCS install root; without it this passes
//! without reading anything, which is what CI does. The install is only
//! read, and no `.crypt` file is opened.
//!
//! The missions are an oracle, never a source: each route point a mission
//! ties to an airdrome by `airdromeId` names the airdrome nearest it, and
//! the ids the tool joins by position must be the ones the missions give.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use dcsterrain_core::cache::{Cache, TerrainCache};
use dcsterrain_core::decode::install::{self, TERRAINS_DIR};
use dcsterrain_core::decode::lua::{self, Table, Value};
use dcsterrain_core::decode::scn5::Index;
use dcsterrain_core::source::Refusal;
use dcsterrain_core::source::install::{IdBy, SCENE_NODES, SceneIndex, Tables};

fn install_root() -> Option<PathBuf> {
    match std::env::var_os("DCSTERRAIN_INSTALL") {
        Some(p) => Some(PathBuf::from(p)),
        None => {
            eprintln!("DCSTERRAIN_INSTALL is not set; skipped");
            None
        }
    }
}

/// A fresh cache directory, removed when dropped.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The scene's index, as `tables.json` reads it: from the cache.
fn scene_index(terrain: &TerrainCache) -> Result<SceneIndex, Refusal> {
    let Some(scene) = terrain.map().files.iter().find(|f| f.role == "scene") else {
        return Ok(None);
    };
    let opened = terrain.get(&SCENE_NODES)?;
    let index = Index::decode(&opened.payload().unwrap()).unwrap();
    Ok(Some((index, scene.path.clone())))
}

/// A route point further than this from every airdrome names none: a
/// take-off or landing point lies on the airdrome.
const POINT_REACH_M: f64 = 5_000.0;

fn u16_at(b: &[u8], at: usize) -> Option<usize> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?).into())
}

fn u32_at(b: &[u8], at: usize) -> Option<usize> {
    usize::try_from(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?)).ok()
}

/// The entry `name` of the zip archive `b`, stored or deflated; `None` when
/// the archive holds none it can read.
fn zip_entry(b: &[u8], name: &str) -> Option<Vec<u8>> {
    let end = (0..b.len().saturating_sub(21))
        .rev()
        .find(|&i| b[i..].starts_with(&[0x50, 0x4b, 0x05, 0x06]))?;
    let (count, mut at) = (u16_at(b, end + 10)?, u32_at(b, end + 16)?);
    for _ in 0..count {
        if !b.get(at..)?.starts_with(&[0x50, 0x4b, 0x01, 0x02]) {
            return None;
        }
        let (method, size) = (u16_at(b, at + 10)?, u32_at(b, at + 20)?);
        let (n, extra, comment) = (
            u16_at(b, at + 28)?,
            u16_at(b, at + 30)?,
            u16_at(b, at + 32)?,
        );
        let local = u32_at(b, at + 42)?;
        if b.get(at + 46..at + 46 + n)? == name.as_bytes() {
            let data = local + 30 + u16_at(b, local + 26)? + u16_at(b, local + 28)?;
            let raw = b.get(data..data + size)?;
            return match method {
                0 => Some(raw.to_vec()),
                8 => miniz_oxide::inflate::decompress_to_vec(raw).ok(),
                _ => None,
            };
        }
        at += 46 + n + extra + comment;
    }
    None
}

/// Every `.miz` under `dir`.
fn missions(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            missions(&p, out);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("miz")) {
            out.push(p);
        }
    }
}

/// Each `(airdromeId, x, z)` a table in `t` gives, at any depth.
fn points(t: &Table, out: &mut Points) {
    if let (Some(id), Some(x), Some(y)) = (t.num("airdromeId"), t.num("x"), t.num("y")) {
        // A mission's `y` is DCS z.
        out.push((id as i64, [x, y]));
    }
    for (_, v) in &t.entries {
        if let Value::Table(t) = v {
            points(t, out);
        }
    }
}

/// Each route point a mission ties to an airdrome: its `airdromeId` and
/// where it lies, DCS x and z.
type Points = Vec<(i64, [f64; 2])>;

/// What the missions say, by terrain: each route point's airdrome id and
/// position. Also how many missions were read and how many could not be.
fn mission_points(root: &Path) -> (BTreeMap<String, Points>, usize, usize) {
    let mut files = Vec::new();
    missions(root, &mut files);
    files.sort();
    let (mut read, mut unread) = (0, 0);
    let mut out: BTreeMap<String, Points> = BTreeMap::new();
    for f in files {
        let rel = f
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let text = fs::read(&f)
            .ok()
            .and_then(|b| zip_entry(&b, "mission"))
            .and_then(|m| lua::run_text(root, &format!("{rel}/mission"), &m).ok());
        let Some(mission) = text.as_ref().and_then(|c| c.table("mission")) else {
            unread += 1;
            continue;
        };
        read += 1;
        let Some(theatre) = mission.str("theatre") else {
            continue;
        };
        points(mission, out.entry(theatre.to_string()).or_default());
    }
    (out, read, unread)
}

fn maps(root: &Path) -> Vec<String> {
    let mut out: Vec<String> = fs::read_dir(root.join(TERRAINS_DIR))
        .unwrap()
        .flatten()
        .filter(|e| e.path().join(install::ENTRY_FILE).is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    out.sort();
    out
}

#[test]
fn ids_joined_by_position_are_the_ones_the_missions_give() {
    let Some(root) = install_root() else { return };
    let build = install::read_build(&root).unwrap();
    eprintln!("build {} {}", build.version, build.timestamp);
    let (said, read, unread) = mission_points(&root);
    eprintln!("{read} missions read, {unread} not");
    // A reader that failed on every archive would leave nothing to check.
    assert!(
        read > 0 && unread == 0,
        "{unread} of the missions could not be read"
    );

    eprintln!(
        "| Terrain | Airdromes | With a runway | Label | Beacons | Tower | None | None with a runway | Named by missions | Agree | Disagree |"
    );
    eprintln!("|---|---|---|---|---|---|---|---|---|---|---|");
    let mut wrong = Vec::new();
    // Each terrain's airdromes with an id, and the joined ids the missions
    // confirm.
    let mut joined: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let (mut hold_out, mut held_wrong) = (Vec::new(), Vec::new());
    let (mut slowest, mut placing) = (Duration::ZERO, Duration::ZERO);
    for dir in maps(&root) {
        let map = install::read_map(&root, &dir).unwrap_or_else(|e| panic!("{dir}: {e}"));
        let files = install::classify(&root, &map.files).unwrap();
        let cache = Scratch(
            std::env::temp_dir().join(format!("dcsterrain-ids-{dir}-{}", std::process::id())),
        );
        let terrain = Cache::new(&cache.0).terrain(&root, &map);
        let index = || scene_index(&terrain);
        let t =
            Tables::decode(&root, &map, &files, &index).unwrap_or_else(|e| panic!("{dir}: {e}"));
        let sites: Vec<[f64; 2]> = t
            .airdromes
            .iter()
            .map(|a| [a.x.unwrap_or(f64::NAN), a.z.unwrap_or(f64::NAN)])
            .collect();
        // The ids the missions give each airdrome, with how often.
        let mut given: Vec<BTreeMap<i64, usize>> = vec![BTreeMap::new(); sites.len()];
        for &(id, p) in said.get(&map.theatre).map_or(&[][..], |v| v) {
            let d = |s: &[f64; 2]| (s[0] - p[0]).hypot(s[1] - p[1]);
            let near = (0..sites.len())
                .filter(|&i| d(&sites[i]) <= POINT_REACH_M)
                .min_by(|&i, &j| d(&sites[i]).total_cmp(&d(&sites[j])));
            if let Some(i) = near {
                *given[i].entry(id).or_default() += 1;
            }
        }
        let by = |k: IdBy| t.airdromes.iter().filter(|a| a.id_by == Some(k)).count();
        let (mut named, mut agree, mut disagree, mut confirmed) = (0, 0, 0, 0);
        for (a, g) in t.airdromes.iter().zip(&given) {
            let Some((&most, _)) = g.iter().max_by_key(|(_, n)| **n) else {
                continue;
            };
            named += 1;
            match a.id {
                Some(id) if g.contains_key(&id) && id == most => {
                    agree += 1;
                    confirmed += usize::from(a.id_by != Some(IdBy::Label));
                }
                Some(id) => {
                    disagree += 1;
                    eprintln!(
                        "  {} {}: {id} by {:?}, missions {g:?}",
                        map.theatre, a.name_id, a.id_by
                    );
                    if a.id_by != Some(IdBy::Label) {
                        wrong.push(format!(
                            "{} {}: {id}, missions {g:?}",
                            map.theatre, a.name_id
                        ));
                    }
                }
                None => eprintln!("  {} {}: none, missions {g:?}", map.theatre, a.name_id),
            }
        }
        let ids: BTreeSet<i64> = t.airdromes.iter().filter_map(|a| a.id).collect();
        let with = t.airdromes.iter().filter(|a| a.id.is_some()).count();
        assert_eq!(ids.len(), with, "{}: an id is given twice", map.theatre);
        eprintln!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {named} | {agree} | {disagree} |",
            map.theatre,
            t.airdromes.len(),
            t.airdromes.iter().filter(|a| a.x.is_some()).count(),
            by(IdBy::Label),
            by(IdBy::Beacons),
            by(IdBy::Tower),
            t.airdromes.len() - with,
            t.airdromes
                .iter()
                .filter(|a| a.id.is_none() && a.x.is_some())
                .count(),
        );
        for (a, g) in t.airdromes.iter().zip(&given) {
            if matches!(a.id_by, Some(IdBy::Beacons | IdBy::Tower)) {
                eprintln!(
                    "  {} {} = {:?} by {:?}, missions {g:?}",
                    map.theatre, a.name_id, a.id, a.id_by
                );
            }
        }
        joined.insert(map.theatre.clone(), (with, confirmed));

        // Held out: the joins run on the airdromes `Radio.lua` labels too, as
        // if it labeled none, and each id they give one is held to its label.
        let start = Instant::now();
        let towers = Tables::towers(&root, &t.radio, &index).unwrap();
        placing = placing.max(start.elapsed());
        let start = Instant::now();
        let blind = t.joined(&towers, &BTreeSet::new());
        slowest = slowest.max(start.elapsed());
        let mut held: BTreeMap<IdBy, [usize; 2]> = BTreeMap::new();
        for (a, j) in t.airdromes.iter().zip(&blind) {
            let (Some(IdBy::Label), Some((id, by))) = (a.id_by, *j) else {
                continue;
            };
            let c = held.entry(by).or_default();
            if Some(id) == a.id {
                c[0] += 1;
            } else {
                c[1] += 1;
                held_wrong.push(format!(
                    "{} {}: {id} by {by:?}, label {:?}",
                    map.theatre, a.name_id, a.id
                ));
            }
        }
        let labeled = by(IdBy::Label);
        for (by, [right, wrong]) in held.iter() {
            hold_out.push(format!(
                "| {} | {labeled} | {by:?} | {right} | {wrong} |",
                map.theatre
            ));
        }
    }
    eprintln!("\nthe joins take at most {slowest:?} on a terrain");
    eprintln!("placing the towers takes at most {placing:?} on a terrain");
    eprintln!("\nHeld out, over the airdromes Radio.lua labels:");
    eprintln!("| Terrain | Labeled | Join | Equal to the label | Not |");
    eprintln!("|---|---|---|---|---|");
    for row in &hold_out {
        eprintln!("{row}");
    }
    assert!(
        held_wrong.is_empty(),
        "joins that contradict a label: {held_wrong:#?}"
    );
    assert!(
        wrong.is_empty(),
        "joined ids the missions contradict: {wrong:#?}"
    );
    // Where no label gives one: Nevada at least 6 ids and PersianGulf 27,
    // and at least as many as the beacons alone gave on 2.9.30.28738 that
    // the missions confirm.
    for (terrain, ids, confirmed) in [("Nevada", 6, 6), ("PersianGulf", 27, 25)] {
        if let Some(&(with, sure)) = joined.get(terrain) {
            assert!(with >= ids && sure >= confirmed, "{terrain}: {joined:?}");
        }
    }
}
