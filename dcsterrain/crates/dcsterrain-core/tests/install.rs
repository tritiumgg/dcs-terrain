//! Opt-in: classifies every map file of a real install. Set
//! `DCSTERRAIN_INSTALL` to the DCS install root; without it these pass
//! without reading anything, which is what CI does. The install is only read,
//! and only a file's header: never a `.crypt` file.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use dcsterrain_core::decode::container;
use dcsterrain_core::decode::install::{self, TERRAINS_DIR};
use dcsterrain_core::decode::{Decoder, dispatch};

fn install_root() -> Option<PathBuf> {
    match std::env::var_os("DCSTERRAIN_INSTALL") {
        Some(p) => Some(PathBuf::from(p)),
        None => {
            eprintln!("DCSTERRAIN_INSTALL is not set; skipped");
            None
        }
    }
}

/// Each directory under `Mods/terrains` that holds an `entry.lua`. The
/// install also ships directories without one, which are not maps.
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

fn files_under(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in fs::read_dir(dir).unwrap().flatten() {
        let t = e.file_type().unwrap();
        if t.is_dir() {
            files_under(&e.path(), out);
        } else if t.is_file() {
            out.push(e.path());
        }
    }
}

#[test]
fn every_map_reads_with_a_decoder_for_each_file() {
    let Some(root) = install_root() else { return };
    let build = install::read_build(&root).unwrap();
    eprintln!("DCS {} ({})", build.version, build.timestamp);
    let maps = maps(&root);
    assert!(!maps.is_empty(), "no maps under {}", root.display());
    let mut scenes = BTreeMap::new();
    for dir in &maps {
        let map = install::read_map(&root, dir).unwrap_or_else(|e| panic!("{dir}: {e}"));
        let scene = map.files.iter().find(|f| f.role == "scene").unwrap();
        eprintln!(
            "{dir}: theatre {}, {} files, scene version {}",
            map.theatre,
            map.files.len(),
            scene.version.unwrap()
        );
        scenes.insert(map.theatre.clone(), scene.version.unwrap());
    }
    if let Some(&v) = scenes.get("Nevada") {
        assert_eq!(v, 8, "Nevada's scene file");
    }
}

#[test]
fn every_container_of_a_class_a_decoder_reads_dispatches() {
    let Some(root) = install_root() else { return };
    let mut seen: BTreeMap<(String, u32), usize> = BTreeMap::new();
    let mut plain = 0usize;
    let mut refused = Vec::new();
    for dir in maps(&root) {
        let mut files = Vec::new();
        files_under(&root.join(TERRAINS_DIR).join(&dir), &mut files);
        for path in files {
            let name = path.to_string_lossy();
            if name.to_ascii_lowercase().ends_with(".crypt") {
                continue;
            }
            let Ok(Ok(h)) = container::read(&path) else {
                plain += 1;
                continue;
            };
            *seen.entry((h.class.clone(), h.version)).or_default() += 1;
            let read = Decoder::ALL.iter().any(|d| d.class() == h.class);
            if read && dispatch(&name, &h).is_err() {
                refused.push(format!("{name}: {} {}", h.class, h.version));
            }
        }
    }
    for ((class, version), n) in &seen {
        eprintln!("{n:6}  {class} {version}");
    }
    eprintln!("{plain:6}  not a container");
    assert!(refused.is_empty(), "{refused:#?}");
}
