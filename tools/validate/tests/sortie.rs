//! The chunks run in a Lua 5.1 state with a stand-in for DCS that answers from
//! the plan, so the plan, the Lua and the report are held together with no
//! game running.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use dcsterrain_core::query::Terrain;
use dcsterrain_core::synth::{self, Synth};
use dcsterrain_validate::plan::{Plan, Sizes, plan};
use dcsterrain_validate::{report, sortie};
use mlua::{Function, Lua, Table, Value};

fn sizes() -> Sizes {
    Sizes {
        heights_per_stratum: 8,
        points_per_runway: 2,
        sights_per_stratum: 4,
        marginal_sights: 8,
        roads_per_band: 2,
        rails_per_band: 1,
        mgrs: 20,
    }
}

fn the_plan() -> &'static Plan {
    static PLAN: OnceLock<Plan> = OnceLock::new();
    PLAN.get_or_init(|| {
        let s = Synth::new(
            synth::DEFAULT_SEED,
            synth::DEFAULT_SIZE_KM,
            synth::CELL_SIZE_M,
        )
        .unwrap();
        plan(&Terrain::Synth(Box::new(s)), 1, &sizes()).unwrap()
    })
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir()
        .join("dcsterrain-validate-tests")
        .join(format!("{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn key(a: f64, b: f64) -> (u64, u64) {
    (a.to_bits(), b.to_bits())
}

/// A DCS that gives the plan's own answers, the height off by `offset`, on
/// `loaded` and `build`.
fn stand_in(plan: &Plan, loaded: Option<&str>, build: &str, offset: f64) -> Lua {
    let lua = Lua::new();
    let g = lua.globals();
    g.set("_APP_VERSION", build).unwrap();
    let t: Table = lua.create_table().unwrap();
    let name = loaded.map(str::to_string);
    t.set(
        "GetTerrainConfig",
        lua.create_function(move |_, _: String| Ok(name.clone()))
            .unwrap(),
    )
    .unwrap();
    let heights: HashMap<_, _> = plan
        .heights
        .iter()
        .map(|c| (key(c.x, c.z), (c.height + offset, c.water)))
        .collect();
    let surface = heights.clone();
    t.set(
        "GetHeight",
        lua.create_function(move |_, (x, z): (f64, f64)| Ok(heights[&key(x, z)].0))
            .unwrap(),
    )
    .unwrap();
    t.set(
        "GetSurfaceType",
        lua.create_function(move |_, (x, z): (f64, f64)| {
            Ok(if surface[&key(x, z)].1 != 0.0 {
                "sea"
            } else {
                "land"
            })
        })
        .unwrap(),
    )
    .unwrap();
    let sights: HashMap<_, _> = plan
        .sights
        .iter()
        .map(|c| (key(c.a[0], c.b[2]), c.visible))
        .collect();
    t.set(
        "isVisible",
        lua.create_function(move |_, a: (f64, f64, f64, f64, f64, f64)| Ok(sights[&key(a.0, a.5)]))
            .unwrap(),
    )
    .unwrap();
    let routes: HashMap<_, _> = plan
        .routes
        .iter()
        .map(|c| (key(c.from[0], c.to[1]), c.length))
        .collect();
    t.set(
        "findPathOnRoads",
        lua.create_function(move |lua, a: (String, f64, f64, f64, f64)| {
            let path = lua.create_table()?;
            for (i, y) in [0.0, routes[&key(a.1, a.4)]].into_iter().enumerate() {
                let p = lua.create_table()?;
                p.set("x", 0.0)?;
                p.set("y", y)?;
                path.set(i + 1, p)?;
            }
            Ok(path)
        })
        .unwrap(),
    )
    .unwrap();
    g.set("terrain", t).unwrap();

    let refs: HashMap<_, _> = plan
        .mgrs
        .iter()
        .map(|c| (key(c.x, c.z), (c.lat, c.lon)))
        .collect();
    let grids: HashMap<_, _> = plan
        .mgrs
        .iter()
        .map(|c| (key(c.lat, c.lon), c.mgrs.clone()))
        .collect();
    let coord = lua.create_table().unwrap();
    coord
        .set(
            "LOtoLL",
            lua.create_function(move |_, p: Table| {
                let (x, z): (f64, f64) = (p.get("x")?, p.get("z")?);
                Ok(refs[&key(x, z)])
            })
            .unwrap(),
        )
        .unwrap();
    coord
        .set(
            "LLtoMGRS",
            lua.create_function(move |lua, (lat, lon): (f64, f64)| {
                let text = &grids[&key(lat, lon)];
                let p: Vec<&str> = text.split(' ').collect();
                let m = lua.create_table()?;
                m.set("UTMZone", p[0])?;
                m.set("MGRSDigraph", p[1])?;
                m.set("Easting", p[2].parse::<f64>().unwrap() + 0.5)?;
                m.set("Northing", p[3].parse::<f64>().unwrap() + 0.5)?;
                Ok(m)
            })
            .unwrap(),
        )
        .unwrap();
    g.set("coord", coord).unwrap();
    let net = lua.create_table().unwrap();
    net.set(
        "dostring_in",
        lua.create_function(|lua, (state, code): (String, String)| {
            assert_eq!(state, "server");
            lua.load(code).eval::<Value>()
        })
        .unwrap(),
    )
    .unwrap();
    g.set("net", net).unwrap();
    lua
}

fn run(lua: &Lua, dir: &Path, name: &str) -> mlua::Result<String> {
    let path = dir.join("chunks").join(format!("{name}.lua"));
    let code = fs::read_to_string(path).unwrap();
    let chunk: Function = lua.load(code).into_function()?;
    chunk.call(())
}

#[test]
fn a_faithful_dcs_passes_every_target() {
    let plan = the_plan();
    assert!(!plan.mgrs.is_empty() && !plan.routes.is_empty() && !plan.sights.is_empty());
    let dir = scratch("faithful");
    let lua = stand_in(plan, Some(&plan.terrain), &plan.dcs_build, 0.25);
    for name in sortie::write(plan, &dir).unwrap() {
        run(&lua, &dir, &name).unwrap();
    }
    let figures = report::report(plan, &dir).unwrap();
    assert_eq!(figures["pass"], true, "{figures:#}");
    assert_eq!(figures["height"]["all"]["max"], 0.25);
    assert_eq!(figures["los_agreement"], 1.0);
    assert_eq!(figures["dcs_build"], plan.dcs_build);
    assert_eq!(figures["mgrs"]["end_to_end_m"]["max"], 0.0);
}

#[test]
fn a_height_past_the_target_fails_the_sortie() {
    let plan = the_plan();
    let dir = scratch("offset");
    let lua = stand_in(plan, Some(&plan.terrain), &plan.dcs_build, 0.75);
    for name in sortie::write(plan, &dir).unwrap() {
        run(&lua, &dir, &name).unwrap();
    }
    let figures = report::report(plan, &dir).unwrap();
    assert_eq!(figures["pass"], false);
    assert_eq!(figures["height"]["pass"], false);
    assert_eq!(figures["los_agreement"], 1.0);
}

/// The figures of a sortie by a DCS on the plan's terrain and build, once
/// `patch` has changed it.
fn sortie_with(name: &str, offset: f64, patch: &str) -> serde_json::Value {
    let plan = the_plan();
    let dir = scratch(name);
    let lua = stand_in(plan, Some(&plan.terrain), &plan.dcs_build, offset);
    lua.load(patch).exec().unwrap();
    for name in sortie::write(plan, &dir).unwrap() {
        run(&lua, &dir, &name).unwrap();
    }
    report::report(plan, &dir).unwrap()
}

#[test]
fn a_height_too_low_fails_as_one_too_high_does() {
    assert_eq!(sortie_with("low", -0.75, "")["height"]["pass"], false);
}

#[test]
fn another_surface_fails_the_water_target() {
    let f = sortie_with(
        "surface",
        0.0,
        "terrain.GetSurfaceType = function() return 'land' end",
    );
    assert_eq!(f["water"]["pass"], false);
    assert_eq!(f["pass"], false);
}

#[test]
fn an_opposite_sightline_fails_the_target() {
    let f = sortie_with(
        "sight",
        0.0,
        "local f = terrain.isVisible
         terrain.isVisible = function(...) return not f(...) end",
    );
    assert_eq!(f["los_agreement"], 0.0);
    assert_eq!(f["pass"], false);
}

#[test]
fn a_route_the_game_refuses_fails_the_target() {
    let f = sortie_with(
        "refused",
        0.0,
        "terrain.findPathOnRoads = function() return nil end",
    );
    assert_eq!(f["route"]["pass"], false);
    assert_eq!(f["route"]["no_path"], the_plan().routes.len());
}

#[test]
fn another_square_with_the_same_digits_fails_the_target() {
    let f = sortie_with(
        "square",
        0.0,
        "local f = coord.LLtoMGRS
         coord.LLtoMGRS = function(...)
           local m = f(...)
           m.MGRSDigraph = 'ZZ'
           return m
         end",
    );
    assert_eq!(f["mgrs"]["end_to_end_m"]["max"], 0.0);
    assert_eq!(f["mgrs"]["pass"], false);
}

#[test]
fn another_terrain_is_refused_in_the_report() {
    let plan = the_plan();
    let dir = scratch("terrain");
    let names = sortie::write(plan, &dir).unwrap();
    let lua = stand_in(plan, Some(&plan.terrain), &plan.dcs_build, 0.0);
    run(&lua, &dir, &names[0]).unwrap();
    let mut other = plan.clone();
    other.terrain = "Nevada".to_string();
    assert!(
        report::report(&other, &dir)
            .unwrap_err()
            .contains("the plan is")
    );
}

#[test]
fn planning_again_leaves_no_reply_of_the_last_plan() {
    let plan = the_plan();
    let dir = scratch("again");
    let names = sortie::write(plan, &dir).unwrap();
    let lua = stand_in(plan, Some(&plan.terrain), &plan.dcs_build, 0.0);
    run(&lua, &dir, &names[0]).unwrap();
    sortie::write(plan, &dir).unwrap();
    assert!(fs::read_dir(dir.join("replies")).unwrap().next().is_none());
}

#[test]
fn no_call_runs_until_the_planned_terrain_is_loaded() {
    let plan = the_plan();
    let dir = scratch("menu");
    let names = sortie::write(plan, &dir).unwrap();
    let mgrs = names.iter().find(|n| n.starts_with("mgrs")).unwrap();
    for loaded in [None, Some("Nevada")] {
        let lua = stand_in(plan, loaded, &plan.dcs_build, 0.0);
        lua.globals().set("coord", Value::Nil).unwrap();
        let e = run(&lua, &dir, mgrs).unwrap_err().to_string();
        assert!(e.contains("is loaded, not"), "{e}");
    }
}

#[test]
fn another_build_is_refused_in_the_game_and_in_the_report() {
    let plan = the_plan();
    let dir = scratch("build");
    let names = sortie::write(plan, &dir).unwrap();
    let lua = stand_in(plan, Some(&plan.terrain), "9.9.9.9", 0.0);
    let e = run(&lua, &dir, &names[0]).unwrap_err().to_string();
    assert!(e.contains("the plan is"), "{e}");

    let mut older = plan.clone();
    older.dcs_build = "9.9.9.9".to_string();
    let lua = stand_in(plan, Some(&plan.terrain), &plan.dcs_build, 0.0);
    run(&lua, &dir, &names[0]).unwrap();
    let e = report::report(&older, &dir).unwrap_err();
    assert!(e.contains("the plan is"), "{e}");
}

#[test]
fn a_missing_reply_is_refused() {
    let plan = the_plan();
    let dir = scratch("missing");
    sortie::write(plan, &dir).unwrap();
    let e = report::report(plan, &dir).unwrap_err();
    assert!(e.contains("heights-000"), "{e}");
}

#[test]
fn the_same_seed_draws_the_same_plan() {
    let s = Synth::new(
        synth::DEFAULT_SEED,
        synth::DEFAULT_SIZE_KM,
        synth::CELL_SIZE_M,
    )
    .unwrap();
    assert_eq!(
        plan(&Terrain::Synth(Box::new(s)), 1, &sizes()).unwrap(),
        *the_plan()
    );
}
