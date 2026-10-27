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
    // Nevada's scene file is the one version 8 on the install, so it is the
    // check that the version is read rather than assumed. Its directory has
    // shipped without an `entry.lua` before; then it is not a map, and the
    // check cannot run, which is said rather than passed.
    let nevada = maps.iter().find(|d| d.eq_ignore_ascii_case("Nevada"));
    if nevada.is_none() {
        eprintln!("no Nevada map on this install: its scene version 8 is NOT checked");
    }
    let listed = install::terrains(&root).unwrap();
    assert_eq!(
        listed.iter().map(|t| t.dir.as_str()).collect::<Vec<_>>(),
        maps.iter().map(String::as_str).collect::<Vec<_>>()
    );
    for (dir, t) in maps.iter().zip(&listed) {
        let id =
            t.id.as_deref()
                .unwrap_or_else(|| panic!("{dir} declares no id"));
        let map = install::open(&root, id).unwrap_or_else(|e| panic!("{dir}: {e}"));
        assert_eq!(&map.dir, dir);
        let files = install::classify(&root, &map.files).unwrap_or_else(|e| panic!("{dir}: {e}"));
        let scene = files.iter().find(|f| f.role == "scene").unwrap();
        eprintln!(
            "{dir}: theatre {}, {} files, scene version {}",
            map.theatre,
            map.files.len(),
            scene.version.unwrap()
        );
        if Some(dir) == nevada {
            assert_eq!(scene.version, Some(8), "Nevada's scene file");
        }
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
            let h = match container::read(&path) {
                Ok(Ok(h)) => h,
                Ok(Err(_)) => {
                    plain += 1;
                    continue;
                }
                Err(e) => panic!("{name} cannot be read: {e}"),
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
