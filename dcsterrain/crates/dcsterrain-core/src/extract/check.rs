//! Validates an extract directory.
//!
//! Every failure is collected rather than the first returned, so a user fixing
//! an extract sees the whole list at once. A manifest that cannot be read is
//! the one exception: nothing else can be checked without it.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use super::{CELL_SIZE_M, Extract, Failure, LAYERS, Samples, TILE_SIZE, TileEntry, layer};

const WATER_SEA: i32 = 2;

/// What a passing extract holds, for the report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub theatre: String,
    pub dcs_build: String,
    pub tiles: usize,
    pub tables: usize,
}

/// Validates the extract at `dir`.
pub fn check(dir: &Path) -> Result<Summary, Vec<Failure>> {
    let x = Extract::open(dir).map_err(|f| vec![f])?;
    let mut out = Vec::new();
    check_grid(&x, &mut out);
    check_layers(&x, &mut out);
    if out.is_empty() {
        // Tile sizes and positions mean nothing on a grid or layer set that
        // is itself wrong.
        check_tiles(&x, &mut out);
    }
    super::tables::check_tables(&x, &mut out);
    if out.is_empty() {
        Ok(Summary {
            theatre: x.manifest.theatre.clone(),
            dcs_build: x.manifest.dcs_build.clone(),
            tiles: x.manifest.tiles.len(),
            tables: x.manifest.tables.len(),
        })
    } else {
        Err(out)
    }
}

fn check_grid(x: &Extract, out: &mut Vec<Failure>) {
    let g = &x.manifest.grid;
    let mut bad = |what: String| out.push(Failure::new(super::MANIFEST, what));
    if g.cell_size != CELL_SIZE_M {
        bad(format!(
            "grid.cell_size {} where an extract is {CELL_SIZE_M}",
            g.cell_size
        ));
    }
    if g.tile_size != TILE_SIZE {
        bad(format!(
            "grid.tile_size {} where an extract is {TILE_SIZE}",
            g.tile_size
        ));
    }
    if g.height == 0 || g.width == 0 {
        bad(format!("grid is {} by {} cells", g.height, g.width));
    }
}

fn check_layers(x: &Extract, out: &mut Vec<Failure>) {
    let listed = &x.manifest.layers;
    for l in LAYERS {
        let Some(e) = listed.get(l.name) else {
            out.push(Failure::new(
                super::MANIFEST,
                format!("layers lacks {}", l.name),
            ));
            continue;
        };
        if e.dtype != l.dtype.name() || e.nodata != i64::from(l.nodata) {
            out.push(Failure::new(
                super::MANIFEST,
                format!(
                    "layers.{} is {} with nodata {}, where the format has {} with nodata {}",
                    l.name,
                    e.dtype,
                    e.nodata,
                    l.dtype.name(),
                    l.nodata
                ),
            ));
        }
    }
    for name in listed.keys() {
        if layer(name).is_none() {
            out.push(Failure::new(
                super::MANIFEST,
                format!("layers names {name}, which the format does not have"),
            ));
        }
    }
}

/// What a listed tile turned out to hold, for the rules that relate one
/// layer's tile to another's.
struct Read {
    all_sea: bool,
    all_nodata: bool,
}

fn check_tiles(x: &Extract, out: &mut Vec<Failure>) {
    let (nx, nz) = x.manifest.grid.tiles();
    let mut seen: BTreeMap<(&str, u32, u32), Option<Read>> = BTreeMap::new();
    for t in &x.manifest.tiles {
        let path = t.expected_path();
        if layer(&t.layer).is_none() {
            out.push(Failure::new(&t.path, format!("unknown layer {}", t.layer)));
            continue;
        }
        if t.path != path {
            out.push(Failure::new(
                &t.path,
                format!("listed where it must be {path}"),
            ));
            continue;
        }
        if t.tx >= nx || t.tz >= nz {
            out.push(Failure::new(
                &t.path,
                format!("outside the grid's {nx} by {nz} tiles"),
            ));
            continue;
        }
        if seen.contains_key(&(t.layer.as_str(), t.tx, t.tz)) {
            out.push(Failure::new(&t.path, "listed twice"));
            continue;
        }
        let read = match x.read_tile(t) {
            Ok(s) => check_samples(t, &s, out),
            Err(f) => {
                out.push(f);
                None
            }
        };
        seen.insert((t.layer.as_str(), t.tx, t.tz), read);
    }
    check_absent(x, &seen, out);
    check_unlisted(x, out);
}

/// Min and max against the contents, and each value against its layer's
/// domain.
fn check_samples(t: &TileEntry, s: &Samples, out: &mut Vec<Failure>) -> Option<Read> {
    let l = layer(&t.layer)?;
    let present = || s.iter().filter(|&v| v != l.nodata).map(i64::from);
    let (min, max) = (present().min(), present().max());
    if (t.min, t.max) != (min, max) {
        let show = |v: Option<i64>| v.map_or("null".to_string(), |v| v.to_string());
        out.push(Failure::new(
            &t.path,
            format!(
                "manifest says min {} max {}, the tile holds min {} max {}",
                show(t.min),
                show(t.max),
                show(min),
                show(max)
            ),
        ));
    }
    let allowed = |v: i32| match l.name {
        "water" => matches!(v, 0..=3) || v == l.nodata,
        "surface" => matches!(v, 0..=5),
        // A count is clamped one short of nodata, so any u16 is valid; and
        // any i16 but nodata is a height.
        _ => true,
    };
    if let Some((i, v)) = s.iter().enumerate().find(|&(_, v)| !allowed(v)) {
        out.push(Failure::new(
            &t.path,
            format!("sample {i} is {v}, not a {} value", l.name),
        ));
    }
    Some(Read {
        all_sea: s.iter().all(|v| v == WATER_SEA),
        all_nodata: s.iter().all(|v| v == l.nodata),
    })
}

/// A reader rebuilds an absent tile from the `water` tile beside it, so the
/// tiles that are present must be ones that rule can stand on.
fn check_absent(
    x: &Extract,
    seen: &BTreeMap<(&str, u32, u32), Option<Read>>,
    out: &mut Vec<Failure>,
) {
    for (&(name, tx, tz), _) in seen.iter().filter(|(k, _)| k.0 != "water") {
        if !seen.contains_key(&("water", tx, tz)) {
            out.push(Failure::new(
                format!("tiles/{name}/{tx}_{tz}.bin"),
                format!("present, but no water tile {tx}_{tz} is, which marks every cell outside the mesh"),
            ));
        }
    }
    for (&(_, tx, tz), read) in seen.iter().filter(|(k, _)| k.0 == "water") {
        // A water tile that could not be read is already reported.
        let Some(read) = read else { continue };
        // Its absence is what says every cell is outside the mesh; a reader
        // that finds it present takes the cells beside it as terrain.
        if read.all_nodata {
            out.push(Failure::new(
                format!("tiles/water/{tx}_{tz}.bin"),
                "every cell is outside the mesh, so the tile is omitted, not written",
            ));
        }
        let sea = read.all_sea && x.manifest.omit_sea_tiles;
        for name in ["height", "surface"] {
            if !sea && !seen.contains_key(&(name, tx, tz)) {
                out.push(Failure::new(
                    format!("tiles/{name}/{tx}_{tz}.bin"),
                    "not listed, though its water tile is and is not all sea",
                ));
            }
        }
    }
}

/// A file under `tiles/` the manifest does not list, or a `.tmp` left by a
/// write that never renamed it.
fn check_unlisted(x: &Extract, out: &mut Vec<Failure>) {
    let listed: BTreeSet<&str> = x.manifest.tiles.iter().map(|t| t.path.as_str()).collect();
    let mut stack = vec![(x.dir.join("tiles"), "tiles".to_string())];
    while let Some((dir, rel)) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let name = format!("{rel}/{}", e.file_name().to_string_lossy());
            // The entry's own type, not its target's: a link back to an
            // ancestor would otherwise be walked until the OS refused.
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                stack.push((e.path(), name));
            } else if !listed.contains(name.as_str()) {
                out.push(Failure::new(name, "on disk but not in the manifest"));
            }
        }
    }
    if let Ok(entries) = fs::read_dir(&x.dir) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.ends_with(".tmp") {
                out.push(Failure::new(name, "a write that never finished"));
            }
        }
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::extract::MANIFEST;
    use crate::scratch::Scratch;
    use crate::synth;
    use serde_json::Value;

    pub(crate) fn synth_extract() -> Scratch {
        let s = Scratch::new();
        let t =
            synth::Theatre::new(synth::DEFAULT_SEED, synth::DEFAULT_SIZE_KM, CELL_SIZE_M).unwrap();
        synth::write(&t, &s.0).unwrap();
        s
    }

    pub(crate) fn edit_manifest(dir: &Path, f: impl FnOnce(&mut Value)) {
        let path = dir.join(MANIFEST);
        let mut m: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        f(&mut m);
        fs::write(&path, serde_json::to_string_pretty(&m).unwrap()).unwrap();
    }

    pub(crate) fn failures(dir: &Path) -> Vec<Failure> {
        check(dir).expect_err("the extract should fail")
    }

    pub(crate) fn has(list: &[Failure], path: &str, words: &str) -> bool {
        list.iter()
            .any(|f| f.path == path && f.message.contains(words))
    }

    #[test]
    fn accepts_the_synthetic_extract() {
        let s = synth_extract();
        let sum = check(&s.0).unwrap();
        assert_eq!(sum.theatre, synth::THEATRE);
        assert!(sum.tiles > 0);
    }

    #[test]
    fn refuses_a_tile_of_the_wrong_size() {
        let s = synth_extract();
        let p = "tiles/water/0_0.bin";
        let bytes = fs::read(s.0.join(p)).unwrap();
        fs::write(s.0.join(p), &bytes[..bytes.len() - 1]).unwrap();
        let f = failures(&s.0);
        assert!(has(&f, p, "65535 bytes"), "{f:?}");
    }

    #[test]
    fn refuses_a_missing_tile() {
        let s = synth_extract();
        let p = "tiles/height/0_0.bin";
        fs::remove_file(s.0.join(p)).unwrap();
        let f = failures(&s.0);
        assert!(has(&f, p, "not on disk"), "{f:?}");
    }

    #[test]
    fn refuses_an_extra_file() {
        let s = synth_extract();
        // (1, 1) is all sea, so its height tile is omitted; one put back by
        // hand is a file the manifest does not list.
        let p = "tiles/height/1_1.bin";
        fs::write(s.0.join(p), vec![0u8; 256 * 256 * 2]).unwrap();
        let f = failures(&s.0);
        assert!(has(&f, p, "not in the manifest"), "{f:?}");
    }

    #[test]
    fn refuses_a_bad_min_or_max() {
        let s = synth_extract();
        edit_manifest(&s.0, |m| {
            let t = m["tiles"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|t| t["layer"] == "trees")
                .unwrap();
            t["max"] = 4.into();
        });
        let f = failures(&s.0);
        assert!(has(&f, "tiles/trees/3_4.bin", "min 0 max 4"), "{f:?}");
    }

    #[test]
    fn refuses_an_unknown_version() {
        let s = synth_extract();
        edit_manifest(&s.0, |m| m["format_version"] = 1.into());
        let f = failures(&s.0);
        assert!(has(&f, MANIFEST, "format_version 1"), "{f:?}");
    }

    #[test]
    fn refuses_a_directory_with_no_manifest() {
        let s = synth_extract();
        fs::remove_file(s.0.join(MANIFEST)).unwrap();
        let f = failures(&s.0);
        assert!(has(&f, MANIFEST, "did not finish"), "{f:?}");
    }

    #[test]
    fn refuses_an_unlisted_tile_that_a_reader_would_need() {
        let s = synth_extract();
        edit_manifest(&s.0, |m| {
            m["tiles"]
                .as_array_mut()
                .unwrap()
                .retain(|t| !(t["layer"] == "surface" && t["tx"] == 0 && t["tz"] == 0));
        });
        let f = failures(&s.0);
        // Not listed where the rule needs it, and on disk where nothing lists it.
        let p = "tiles/surface/0_0.bin";
        assert!(has(&f, p, "not all sea"), "{f:?}");
        assert!(has(&f, p, "not in the manifest"), "{f:?}");
    }

    #[test]
    fn refuses_a_tile_whose_water_tile_is_absent() {
        let s = synth_extract();
        fs::remove_file(s.0.join("tiles/water/0_0.bin")).unwrap();
        edit_manifest(&s.0, |m| {
            m["tiles"]
                .as_array_mut()
                .unwrap()
                .retain(|t| !(t["layer"] == "water" && t["tx"] == 0 && t["tz"] == 0));
        });
        let f = failures(&s.0);
        assert!(has(&f, "tiles/height/0_0.bin", "outside the mesh"), "{f:?}");
    }

    #[test]
    fn refuses_a_path_outside_the_tiles() {
        let s = synth_extract();
        edit_manifest(&s.0, |m| m["tiles"][0]["path"] = "../elsewhere.bin".into());
        let f = failures(&s.0);
        assert!(has(&f, "../elsewhere.bin", "must be tiles/"), "{f:?}");
    }

    #[test]
    fn refuses_a_water_value_outside_the_codes() {
        let s = synth_extract();
        let p = "tiles/water/0_0.bin";
        let mut bytes = fs::read(s.0.join(p)).unwrap();
        bytes[7] = 9;
        fs::write(s.0.join(p), &bytes).unwrap();
        let f = failures(&s.0);
        assert!(has(&f, p, "sample 7 is 9"), "{f:?}");
    }

    #[test]
    fn refuses_a_layer_whose_dtype_differs() {
        let s = synth_extract();
        edit_manifest(&s.0, |m| m["layers"]["trees"]["dtype"] = "u8".into());
        let f = failures(&s.0);
        assert!(has(&f, MANIFEST, "layers.trees is u8"), "{f:?}");
    }

    #[test]
    fn refuses_a_water_tile_with_no_cell_in_the_mesh() {
        let s = synth_extract();
        let p = "tiles/water/1_1.bin";
        fs::write(s.0.join(p), vec![255u8; 256 * 256]).unwrap();
        edit_manifest(&s.0, |m| {
            let t = m["tiles"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|t| t["path"] == p)
                .unwrap();
            t["min"] = Value::Null;
            t["max"] = Value::Null;
        });
        let f = failures(&s.0);
        assert!(has(&f, p, "omitted, not written"), "{f:?}");
    }

    #[test]
    fn refuses_a_tile_listed_twice() {
        let s = synth_extract();
        edit_manifest(&s.0, |m| {
            let a = m["tiles"].as_array_mut().unwrap();
            a.push(a[0].clone());
        });
        let f = failures(&s.0);
        assert!(has(&f, "tiles/height/0_0.bin", "listed twice"), "{f:?}");
    }

    #[test]
    fn refuses_a_tile_outside_the_grid() {
        let s = synth_extract();
        let p = "tiles/height/6_0.bin";
        fs::write(s.0.join(p), vec![0u8; 256 * 256 * 2]).unwrap();
        edit_manifest(&s.0, |m| {
            let a = m["tiles"].as_array_mut().unwrap();
            let mut t = a[0].clone();
            t["tx"] = 6.into();
            t["path"] = p.into();
            a.push(t);
        });
        let f = failures(&s.0);
        assert!(has(&f, p, "outside the grid's 6 by 6"), "{f:?}");
    }

    #[test]
    fn refuses_a_tile_of_an_unknown_layer() {
        let s = synth_extract();
        edit_manifest(&s.0, |m| {
            let a = m["tiles"].as_array_mut().unwrap();
            let mut t = a[0].clone();
            t["layer"] = "depth".into();
            t["path"] = "tiles/depth/0_0.bin".into();
            a.push(t);
        });
        let f = failures(&s.0);
        assert!(has(&f, "tiles/depth/0_0.bin", "unknown layer"), "{f:?}");
    }

    #[test]
    fn refuses_a_grid_of_another_cell_or_tile_size() {
        let s = synth_extract();
        edit_manifest(&s.0, |m| {
            m["grid"]["cell_size"] = 100.into();
            m["grid"]["tile_size"] = 128.into();
        });
        let f = failures(&s.0);
        assert!(has(&f, MANIFEST, "grid.cell_size 100"), "{f:?}");
        assert!(has(&f, MANIFEST, "grid.tile_size 128"), "{f:?}");
    }

    #[test]
    fn refuses_layers_the_format_does_not_have() {
        let s = synth_extract();
        edit_manifest(&s.0, |m| {
            let layers = m["layers"].as_object_mut().unwrap();
            let lines = layers.remove("lines").unwrap();
            layers.insert("depth".into(), lines);
            layers["height"]["nodata"] = 0.into();
        });
        let f = failures(&s.0);
        assert!(has(&f, MANIFEST, "layers lacks lines"), "{f:?}");
        assert!(has(&f, MANIFEST, "layers names depth"), "{f:?}");
        assert!(
            has(&f, MANIFEST, "layers.height is i16 with nodata 0"),
            "{f:?}"
        );
    }

    #[test]
    fn refuses_a_surface_value_above_runway() {
        let s = synth_extract();
        let p = "tiles/surface/0_0.bin";
        let mut bytes = fs::read(s.0.join(p)).unwrap();
        bytes[0] = 6;
        fs::write(s.0.join(p), &bytes).unwrap();
        let f = failures(&s.0);
        assert!(has(&f, p, "sample 0 is 6"), "{f:?}");
    }

    #[test]
    fn an_all_sea_tile_needs_its_height_when_sea_tiles_are_kept() {
        let s = synth_extract();
        edit_manifest(&s.0, |m| m["omit_sea_tiles"] = false.into());
        let f = failures(&s.0);
        assert!(has(&f, "tiles/height/1_1.bin", "not all sea"), "{f:?}");
    }

    #[test]
    fn refuses_a_stray_file_anywhere_under_the_tiles() {
        let s = synth_extract();
        fs::create_dir_all(s.0.join("tiles/bogus")).unwrap();
        fs::write(s.0.join("tiles/bogus/0_0.bin"), b"x").unwrap();
        fs::write(s.0.join("tiles/water/notes.txt"), b"x").unwrap();
        fs::write(s.0.join("tiles/water/0_0.bin.tmp"), b"x").unwrap();
        let f = failures(&s.0);
        for p in [
            "tiles/bogus/0_0.bin",
            "tiles/water/notes.txt",
            "tiles/water/0_0.bin.tmp",
        ] {
            assert!(has(&f, p, "not in the manifest"), "{p}: {f:?}");
        }
    }

    #[test]
    fn refuses_a_leftover_temporary_file() {
        let s = synth_extract();
        fs::write(s.0.join("towns.json.tmp"), "[]").unwrap();
        let f = failures(&s.0);
        assert!(has(&f, "towns.json.tmp", "never finished"), "{f:?}");
    }
}
