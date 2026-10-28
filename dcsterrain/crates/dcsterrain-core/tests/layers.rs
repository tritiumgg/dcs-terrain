//! Opt-in: derives every layer over a region of each real terrain. Set
//! `DCSTERRAIN_INSTALL` to the DCS install root; without it these pass
//! without reading anything, which is what CI does. Run them optimized,
//! `cargo test --release --test layers -- --nocapture`, to see the times.
//! The install is only read; the cache they build is a scratch directory.

use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use dcsterrain_core::cache::Cache;
use dcsterrain_core::classes::Class;
use dcsterrain_core::decode::install;
use dcsterrain_core::decode::scn5::Pool;
use dcsterrain_core::layers::{Cells, DISTANCE_CAP_M, Derived, TPI_300, TPI_2000, Target, derive};
use dcsterrain_core::source::install::Install;
use dcsterrain_core::source::{Cover, Source};

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
            std::env::temp_dir().join(format!("dcsterrain-layers-{name}-{}", std::process::id()));
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

fn layers() -> Vec<(&'static str, Derived)> {
    let dist = |to| Derived::Distance {
        to,
        cap: DISTANCE_CAP_M,
    };
    vec![
        ("valid", Derived::Valid),
        ("slope", Derived::Slope),
        ("aspect", Derived::Aspect),
        ("roughness", Derived::Roughness),
        ("tpi_300", TPI_300),
        ("tpi_2000", TPI_2000),
        ("dist_road", dist(Target::Road)),
        ("dist_rail", dist(Target::Rail)),
        ("dist_water", dist(Target::Water)),
        ("dist_builtup", dist(Target::Builtup)),
        ("dist_forest", dist(Target::Cover(Cover::Forest))),
        (
            "count_building 1 km",
            Derived::Count {
                class: Class::Building,
                radius: 1000.0,
            },
        ),
        (
            "count_industrial 1 km",
            Derived::Count {
                class: Class::Industrial,
                radius: 1000.0,
            },
        ),
        (
            "tree_density 300 m",
            Derived::Density {
                pool: Pool::Trees,
                radius: 300.0,
            },
        ),
        (
            "line_density 300 m",
            Derived::Density {
                pool: Pool::Lines,
                radius: 300.0,
            },
        ),
        (
            "forest share 300 m",
            Derived::CoverFraction {
                cover: Cover::Forest,
                radius: 300.0,
            },
        ),
    ]
}

/// A 20 by 20 km region around each terrain's first airdrome, each layer
/// derived twice; the first layer reads the region's tiles, which the rest find
/// held.
/// Every layer answers, a region around an airfield holds valid ground, and
/// a building stands within a kilometer of somewhere in it.
#[test]
fn every_layer_derives_over_a_region_of_every_terrain() {
    let Some(root) = install_root() else { return };
    let build = install::read_build(&root).unwrap();
    eprintln!("DCS {} ({})", build.version, build.timestamp);
    for listed in install::terrains(&root).unwrap() {
        let Some(id) = listed.id else { continue };
        let cache = TempDir::new(&id);
        let t = Install::open(&root, &id, &Cache::new(&cache.0)).unwrap();
        let tables = t.tables().unwrap();
        let Some(a) = tables.airdromes.iter().find(|a| a.x.is_some()) else {
            continue;
        };
        let (x, z) = (a.x.unwrap(), a.z.unwrap());
        // The scenery index and the road graph are cache entries; build
        // them first, so the times below are the layers'.
        let started = Instant::now();
        t.scenery().unwrap().classes();
        t.roads().unwrap();
        eprintln!(
            "{id}: around {} at ({x:.0}, {z:.0}); scenery and roads ready in {:.1} s",
            a.name_id,
            started.elapsed().as_secs_f64()
        );
        let (x0, z0) = (x - 10_000.0, z - 10_000.0);
        let at = Cells {
            rows: 400,
            cols: 400,
            ..Cells::covering(&t.grid(), [x0, z0, x0, z0])
        };
        for (name, d) in layers() {
            let started = Instant::now();
            let r = derive(&t, d, at).unwrap();
            let first = started.elapsed().as_secs_f64() * 1e3;
            let started = Instant::now();
            let bits = |v: &[f32]| v.iter().map(|x| x.to_bits()).collect::<Vec<_>>();
            let twice = derive(&t, d, at).unwrap();
            assert!(bits(&twice.values) == bits(&r.values), "{id} {name}");
            let again = started.elapsed().as_secs_f64() * 1e3;
            let known: Vec<f32> = r.values.iter().copied().filter(|v| !v.is_nan()).collect();
            let mean = known.iter().map(|&v| f64::from(v)).sum::<f64>() / known.len().max(1) as f64;
            eprintln!(
                "  {name:<22} {first:>7.1} ms, again {again:>6.1} ms; {} of 160000 defined, \
                 mean {mean:.3}{}",
                known.len(),
                if r.capped { ", capped" } else { "" }
            );
            match name {
                "valid" => assert!(mean > 0.1, "{id}: {mean} valid"),
                "count_building 1 km" => assert!(mean > 0.0, "{id}: no buildings"),
                _ => {}
            }
        }
    }
}
