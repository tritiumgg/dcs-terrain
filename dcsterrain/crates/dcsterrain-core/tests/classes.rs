//! Opt-in: classifies the scenery models of real terrains. Set
//! `DCSTERRAIN_INSTALL` to the DCS install root; without it these pass
//! without reading anything, which is what CI does. Run them with
//! `--nocapture` to see each catalog's head. The install is only read.

use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use dcsterrain_core::cache::Cache;
use dcsterrain_core::classes::{Catalog, Class, Source};
use dcsterrain_core::decode::install;
use dcsterrain_core::source::install::Install;

fn install_root() -> Option<PathBuf> {
    match std::env::var_os("DCSTERRAIN_INSTALL") {
        Some(p) => Some(PathBuf::from(p)),
        None => {
            eprintln!("DCSTERRAIN_INSTALL is not set; skipped");
            None
        }
    }
}

/// A fresh directory under the system temp, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir =
            std::env::temp_dir().join(format!("dcsterrain-classes-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn catalog(id: &str) -> Option<Catalog> {
    let root = install_root()?;
    let build = install::read_build(&root).unwrap();
    let cache = TempDir::new(id);
    let t = Install::open(&root, id, &Cache::new(&cache.0)).unwrap();
    let s = t.scenery().unwrap();
    let at = Instant::now();
    let classes = s.classes();
    let classified = at.elapsed().as_secs_f64() * 1000.0;
    let at = Instant::now();
    let cat = s.catalog().unwrap();
    let counted = at.elapsed().as_secs_f64() * 1000.0;
    eprintln!(
        "{id} on {}: {} models classified in {classified:.1} ms, counted in {counted:.0} ms",
        build.version,
        classes.models.len()
    );
    eprintln!("{id}: objects by class {:?}", cat.counts_by_class);
    // Every object is counted once: the pool's entries and the group's.
    let total: u64 = cat.rows.iter().map(|r| r.count).sum();
    let index = s.index();
    assert_eq!(
        total,
        index.pools[0].count + index.group.len() as u64,
        "{id}"
    );
    for r in cat.rows.iter().take(25) {
        eprintln!(
            "  {:>8}  {:<34} {:<10} {:<12} {:>6.1} x {:>6.1} x {:>6.1}  {:?}",
            r.count,
            r.model,
            r.assignment.class.name(),
            r.assignment
                .family
                .map_or_else(|| "-".to_string(), |f| format!("{f:?}")),
            r.obb_w,
            r.obb_d,
            r.height,
            r.assignment.class_by
        );
    }
    for r in cat.tallest(10) {
        eprintln!(
            "  tall {:>6.1} m  {:<34} {:<10} {:>8}",
            r.height,
            r.model,
            r.assignment.class.name(),
            r.count
        );
    }
    Some(cat)
}

#[test]
fn every_terrain_classifies_and_counts_its_objects() {
    let Some(root) = install_root() else {
        return;
    };
    for t in install::terrains(&root).unwrap() {
        let Some(id) = t.id else { continue };
        let cat = catalog(&id).unwrap();
        assert!(cat.unmatched.is_empty(), "{id}: {:?}", cat.unmatched);
    }
}

#[test]
fn every_caucasus_override_names_a_model_and_decides_it() {
    let Some(cat) = catalog("Caucasus") else {
        return;
    };
    assert!(cat.unmatched.is_empty(), "{:?}", cat.unmatched);
    let overridden: Vec<_> = cat
        .rows
        .iter()
        .filter(|r| matches!(&r.assignment.class_by, Source::Override { file, .. } if file == "Caucasus"))
        .collect();
    eprintln!("Caucasus: {} models overridden", overridden.len());
    assert!(overridden.len() > 80, "{}", overridden.len());
}

#[test]
fn no_caucasus_smoke_volume_is_a_structure() {
    let Some(cat) = catalog("Caucasus") else {
        return;
    };
    let smoke: Vec<_> = cat
        .rows
        .iter()
        .filter(|r| r.model.starts_with("smoke#"))
        .collect();
    let placed: u64 = smoke.iter().map(|r| r.count).sum();
    eprintln!("Caucasus: {} smoke models, {placed} objects", smoke.len());
    assert!(placed > 0);
    for r in smoke {
        assert_eq!(r.assignment.class, Class::Effect, "{}", r.model);
        assert!(!r.assignment.settles(), "{}", r.model);
    }
    assert_eq!(cat.counts_by_class[&Class::Effect], placed);
}
