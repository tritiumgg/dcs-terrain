//! Opt-in: the places of every installed terrain, built into a scratch
//! cache, with each terrain's size classes, forms and build cost printed as
//! a table; and on Caucasus, places ranked against real towns' census
//! populations. Set `DCSTERRAIN_INSTALL` to the DCS install root; without
//! it this passes without reading anything, which is what CI does. Run it
//! with `--release` for the times; `DCSTERRAIN_TERRAIN` names one terrain to
//! run alone. The install is only read.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use dcsterrain_core::cache::Cache;
use dcsterrain_core::decode::install::{read_build, terrains};
use dcsterrain_core::places::{Building, Form, SizeClass, buildings_in, link};
use dcsterrain_core::query::{Terrain, answer};
use dcsterrain_core::source::Source;
use dcsterrain_core::source::install::{Install, PLACES};
use dcsterrain_core::threads::cpu_cycles;
use serde_json::json;

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Census populations of towns on the Caucasus terrain, each under the
/// name its `towns.lua` point carries: Georgia's 2014 census (Geostat) and
/// Russia's of 1 October 2021 (Rosstat). Abkhazia and South Ossetia have
/// no census either country's statistics office stands behind, and a name
/// the towns list holds twice, or a village sharing a town's name, is left
/// out, since which point is meant cannot be told.
const CENSUS: &[(&str, f64)] = &[
    // Georgia, 2014.
    ("TBILISI", 1_108_717.0),
    ("BATUMI", 152_839.0),
    ("KUTAISI", 147_635.0),
    ("RUSTAVI", 125_103.0),
    ("GORI", 48_143.0),
    ("ZUGDIDI", 42_998.0),
    ("POTI", 41_465.0),
    ("KOBULETI", 27_546.0),
    ("KHASHURI", 26_135.0),
    ("SAMTREDIA", 25_318.0),
    ("SENAKI", 21_596.0),
    ("ZESTAFONI", 20_814.0),
    ("Marneuli", 20_211.0),
    ("OZURGETI", 14_785.0),
    ("KASPI", 13_423.0),
    ("CHIATURA", 12_803.0),
    ("TSHALTUBO", 11_281.0),
    ("Sagarejo", 10_871.0),
    ("Gardabani", 10_753.0),
    ("TKIBULI", 9_770.0),
    ("HONI", 8_987.0),
    ("MTSKHETA", 7_940.0),
    ("KARELI", 6_654.0),
    ("LANCHHUTI", 6_395.0),
    ("Dusheti", 6_167.0),
    ("SACHHERE", 6_140.0),
    ("ABASHA", 4_941.0),
    ("MARTVILI", 4_425.0),
    ("DZHVARI", 4_361.0),
    ("ONI", 2_656.0),
    ("AMBROLAURI", 2_047.0),
    // Russia, 2021.
    ("KRASNODAR", 1_099_344.0),
    ("SOCHI", 448_250.0),
    ("VLADIKAVKAZ", 295_830.0),
    ("NOVOROSSIYSK", 262_293.0),
    ("NAL'CHIK", 247_054.0),
    ("PYATIGORSK", 146_473.0),
    ("MAYKOP", 143_385.0),
    ("KISLOVODSK", 127_521.0),
    ("Nazran", 122_350.0),
    ("ESSENTUKI", 119_658.0),
    ("CHERKESSK", 113_226.0),
    ("ANAPA", 81_863.0),
    ("GELENDZHIK", 80_204.0),
    ("MINERAL'NYE VODY", 70_485.0),
    ("GEORGIEVSK", 63_221.0),
    ("SLAVYANSK-NA-KUBANI", 62_985.0),
    ("Sunzha", 62_078.0),
    ("TUAPSE", 61_571.0),
    ("PROHLADNIY", 59_938.0),
    ("BELORECHENSK", 55_870.0),
    ("KRYMSK", 54_597.0),
    ("KARABULAK", 43_037.0),
    ("TEMRYUK", 41_608.0),
    ("Goryachiy Klyuch", 40_903.0),
    ("APSHERONSK", 40_289.0),
    ("UST'-LABINSK", 40_158.0),
    ("BAKSAN", 39_593.0),
    ("ABINSK", 39_511.0),
    ("MOZDOK", 36_784.0),
    ("MALGOBEK", 36_480.0),
    ("GORYACHEVODSKIY", 36_032.0),
    ("BESLAN", 35_929.0),
    ("NARTKALA", 33_203.0),
    ("UST'-DZHEGUTA", 31_137.0),
    ("INOZEMTSEVO", 27_500.0),
    ("ZHELEZNOVODSK", 22_863.0),
    ("LERMONTOV", 22_444.0),
    ("TYRNYAUZ", 22_056.0),
    ("ALAGIR", 21_550.0),
    ("NOVOPAVLOVSK", 20_490.0),
    ("TEREK", 19_948.0),
];

/// Ranks from 1, ties sharing the mean of the ranks they span.
fn ranks(v: &[f64]) -> Vec<f64> {
    let mut ix: Vec<usize> = (0..v.len()).collect();
    ix.sort_by(|&i, &j| v[i].total_cmp(&v[j]));
    let mut r = vec![0.0; v.len()];
    let mut k = 0;
    while k < ix.len() {
        let mut e = k;
        while e + 1 < ix.len() && v[ix[e + 1]] == v[ix[k]] {
            e += 1;
        }
        let mean = (k + e) as f64 / 2.0 + 1.0;
        for &i in &ix[k..=e] {
            r[i] = mean;
        }
        k = e + 1;
    }
    r
}

/// Spearman's rank correlation: Pearson's on the ranks, which stays right
/// where values tie.
fn spearman(a: &[f64], b: &[f64]) -> f64 {
    let (ra, rb) = (ranks(a), ranks(b));
    let n = a.len() as f64;
    let (ma, mb) = (ra.iter().sum::<f64>() / n, rb.iter().sum::<f64>() / n);
    let (mut c, mut va, mut vb) = (0.0, 0.0, 0.0);
    for i in 0..a.len() {
        c += (ra[i] - ma) * (rb[i] - mb);
        va += (ra[i] - ma).powi(2);
        vb += (rb[i] - mb).powi(2);
    }
    c / (va * vb).sqrt()
}

#[test]
fn spearman_is_one_for_any_increasing_map_and_handles_ties() {
    let a = [1.0, 5.0, 2.0, 9.0];
    let b: Vec<f64> = a.iter().map(|v: &f64| v.powi(3) + 7.0).collect();
    assert!((spearman(&a, &b) - 1.0).abs() < 1e-12);
    let rev: Vec<f64> = a.iter().map(|v| -v).collect();
    assert!((spearman(&a, &rev) + 1.0).abs() < 1e-12);
    assert_eq!(ranks(&[3.0, 1.0, 3.0, 2.0]), [3.5, 1.0, 3.5, 2.0]);
}

#[test]
fn every_installed_terrain_records_its_places() {
    let Some(root) = std::env::var_os("DCSTERRAIN_INSTALL").map(PathBuf::from) else {
        eprintln!("DCSTERRAIN_INSTALL is not set; skipped");
        return;
    };
    let build = read_build(&root).unwrap();
    println!("build {} ({})", build.version, build.timestamp);
    println!(
        "| terrain | buildings placed | places | {} | {} | build s / Gcycles | read ms |",
        SizeClass::ALL.map(|s| s.name()).join(" | "),
        Form::ALL.map(|f| f.name()).join(" | "),
    );
    let (mut queries, mut census_rho) = (vec![], None);
    for listed in terrains(&root).unwrap().into_iter().filter(|l| {
        std::env::var("DCSTERRAIN_TERRAIN").map_or(true, |o| l.id.as_deref() == Some(&o))
    }) {
        let Some(id) = listed.id else {
            println!("| {} | no entry.lua | | | | | | | | | | | |", listed.dir);
            continue;
        };
        let dir = Scratch(
            std::env::temp_dir().join(format!("dcsterrain-places-{}-{id}", std::process::id())),
        );
        let t = Install::open(&root, &id, &Cache::new(&dir.0)).unwrap();
        // The tables and the scenery index first, so the time is the walk
        // and the grouping.
        t.tables().unwrap();
        t.scenery().unwrap();
        assert!(t.places().unwrap().is_none(), "{id}: places before a build");
        let (c, s) = (cpu_cycles(), Instant::now());
        t.cache().get(&PLACES).unwrap();
        let (build_s, gcycles) = (s.elapsed().as_secs_f64(), (cpu_cycles() - c) as f64 / 1e9);
        let fresh = Install::open(&root, &id, &Cache::new(&dir.0)).unwrap();
        let s = Instant::now();
        let p = fresh.places().unwrap().expect("built");
        let read_ms = s.elapsed().as_secs_f64() * 1e3;
        let placed: u64 = p.places.iter().map(|q| u64::from(q.buildings)).sum();
        let by_size: Vec<String> = SizeClass::ALL
            .iter()
            .map(|&k| p.places.iter().filter(|q| q.size == k).count().to_string())
            .collect();
        let by_form: Vec<String> = Form::ALL
            .iter()
            .map(|&k| p.places.iter().filter(|q| q.form == k).count().to_string())
            .collect();
        println!(
            "| {id} | {placed} | {} | {} | {} | {build_s:.1} / {gcycles:.1} | {read_ms:.1} |",
            p.places.len(),
            by_size.join(" | "),
            by_form.join(" | "),
        );
        assert!(p.places.windows(2).all(|w| w[0].id < w[1].id), "{id}");

        // Queries on the built places: the 20 largest outlined, then 1 000
        // points over their boxes, each asking which place it stands in.
        let tq = Terrain::open(&root, &id, &Cache::new(&dir.0), true).unwrap();
        let (c, s) = (cpu_cycles(), Instant::now());
        let v = answer(&tq, "towns", &json!({"limit": 20, "outlines": true})).unwrap();
        let (towns_ms, towns_mc) = (
            s.elapsed().as_secs_f64() * 1e3,
            (cpu_cycles() - c) as f64 / 1e6,
        );
        let rings: usize = v["places"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|p| p["outline"].as_array().unwrap().iter())
            .map(|poly| {
                poly.as_array()
                    .unwrap()
                    .iter()
                    .map(|r| r.as_array().unwrap().len())
                    .sum::<usize>()
            })
            .sum();
        let boxes: Vec<[f64; 4]> = v["places"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| {
                let b = &p["bbox"];
                ["min_x", "min_z", "max_x", "max_z"].map(|k| b[k].as_f64().unwrap())
            })
            .collect();
        let mut state = 0x9e37_79b9_7f4a_7c15u64;
        let mut unit = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 11) as f64 / (1u64 << 53) as f64
        };
        let pts: Vec<_> = (0..1000)
            .map(|k| {
                let b = boxes[k % boxes.len().max(1)];
                json!({"x": b[0] + unit() * (b[2] - b[0]), "z": b[1] + unit() * (b[3] - b[1])})
            })
            .collect();
        let (c, s) = (cpu_cycles(), Instant::now());
        let v = answer(&tq, "point_info", &json!({"points": pts})).unwrap();
        let (pi_ms, pi_mc) = (
            s.elapsed().as_secs_f64() * 1e3,
            (cpu_cycles() - c) as f64 / 1e6,
        );
        let inside = v["points"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|p| !p["place"].is_null())
            .count();
        // Each against every place near it, its buildings read and linked
        // in full: the cell holding the point is a place's when a member's
        // circle, grown by 100 m, holds the cell's center.
        let mut groups: HashMap<u64, Vec<Building>> = HashMap::new();
        for (pt, got) in pts.iter().zip(v["points"].as_array().unwrap()) {
            let (x, z) = (pt["x"].as_f64().unwrap(), pt["z"].as_f64().unwrap());
            let center = |v: f64| ((v / 50.0).floor() + 0.5) * 50.0;
            let (cx, cz) = (center(x), center(z));
            let want = p.places.iter().find(|q| {
                let [x0, z0, x1, z1] = q.bbox;
                let e = 160.0;
                if !((x0 - e..=x1 + e).contains(&cx) && (z0 - e..=z1 + e).contains(&cz)) {
                    return false;
                }
                let m = groups.entry(q.id).or_insert_with(|| {
                    let mut b = buildings_in(tq.source(), q.bbox).unwrap();
                    b.retain(|b| (x0..=x1).contains(&b.x) && (z0..=z1).contains(&b.z));
                    let l = link(&b);
                    let first = b.iter().position(|b| b.id == q.id).unwrap();
                    (0..b.len())
                        .filter(|&i| l[i] == l[first])
                        .map(|i| b[i])
                        .collect()
                });
                m.iter()
                    .any(|b| (b.x - cx).hypot(b.z - cz) <= 100.0 + f64::from(b.r))
            });
            assert_eq!(
                got["place"]["id"].as_u64(),
                want.map(|q| q.id),
                "{id} {x} {z}"
            );
        }
        queries.push(format!("| {id} | {towns_ms:.0} / {towns_mc:.0} | {rings} | {pi_ms:.0} / {pi_mc:.0} | {inside} |"));

        if id == "Caucasus" {
            let towns = &fresh.tables().unwrap().towns;
            let census: HashMap<&str, f64> = CENSUS.iter().copied().collect();
            let mut named: HashMap<u32, usize> = HashMap::new();
            for (k, q) in p.places.iter().enumerate() {
                for &n in &q.names {
                    named.insert(n, k);
                }
            }
            let (mut floor, mut count, mut real) = (vec![], vec![], vec![]);
            let (mut unplaced, mut rows) = (vec![], vec![]);
            for (n, t) in towns.iter().enumerate() {
                let Some(&pop) = census.get(t.name.as_str()) else {
                    continue;
                };
                match named.get(&(n as u32)) {
                    Some(&k) => {
                        floor.push(p.places[k].residential_floor);
                        count.push(f64::from(p.places[k].buildings));
                        real.push(pop);
                        let q = &p.places[k];
                        rows.push(format!(
                            "| {} | {pop:.0} | {:.0} | {} | {} | {} | {} |",
                            t.name,
                            q.residential_floor,
                            q.buildings,
                            q.names.len(),
                            q.size.name(),
                            q.form.name()
                        ));
                    }
                    None => unplaced.push(t.name.as_str()),
                }
            }
            let listed = CENSUS
                .iter()
                .filter(|(n, _)| towns.iter().filter(|t| t.name == *n).count() == 1)
                .count();
            println!("| town | census | residential floor m² | buildings | names | size | form |");
            rows.iter().for_each(|r| println!("{r}"));
            let (rho, rho_n) = (spearman(&floor, &real), spearman(&count, &real));
            println!(
                "census: {} towns listed, {listed} found once in the towns, {} naming a \
                 place ({} name none: {}); Spearman by residential floor {rho:.3}, by \
                 building count {rho_n:.3}",
                CENSUS.len(),
                real.len(),
                unplaced.len(),
                unplaced.join(", ")
            );
            assert_eq!(
                listed,
                CENSUS.len(),
                "a census name is not in the towns once"
            );
            census_rho = Some(rho);
        }
    }
    println!(
        "| terrain | towns, 20 largest outlined, ms / Mcycles | outline vertices | point_info, 1 000 points in their boxes, ms / Mcycles | in a place |"
    );
    queries.iter().for_each(|r| println!("{r}"));
    if let Some(rho) = census_rho {
        assert!(rho >= 0.6, "rank correlation {rho:.3}, under 0.6");
    }
}
