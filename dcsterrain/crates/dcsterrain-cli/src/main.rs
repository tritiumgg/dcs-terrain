//! The `dcsterrain` binary.
//!
//! stdout carries only a subcommand's report or its JSON document, so it can
//! be piped; warnings and progress go to stderr, more of them with
//! `--verbose`.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use dcsterrain_core::cache::{self, Cache, Entry, Stored};
use dcsterrain_core::check;
use dcsterrain_core::classes;
use dcsterrain_core::decode::install;
use dcsterrain_core::query::{self, Terrain};
use dcsterrain_core::source::install::{Tables, TablesJson};
use dcsterrain_core::synth::{self, Synth};
use dcsterrain_core::types::Issue;
use serde_json::{Value, json};
use tracing::Level;

#[derive(Parser)]
#[command(
    name = "dcsterrain",
    version,
    about = "Terrain questions answered from a DCS World install"
)]
struct Cli {
    /// More progress and diagnostics on stderr.
    #[arg(long, global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Answer one request about a terrain, as JSON on stdout.
    Query {
        /// The DCS install root. On Windows it defaults to the one the
        /// registry names.
        #[arg(long)]
        install: Option<PathBuf>,
        /// The terrain's id, as `list-terrains` shows it, or `synth` for the
        /// synthetic terrain, which needs no install.
        #[arg(long)]
        terrain: String,
        /// The synthetic terrain's seed.
        #[arg(long, default_value_t = synth::DEFAULT_SEED)]
        seed: u32,
        /// The synthetic terrain's side, in kilometers.
        #[arg(long, default_value_t = synth::DEFAULT_SIZE_KM)]
        size_km: u32,
        /// The synthetic terrain's cell, in meters.
        #[arg(long, default_value_t = synth::CELL_SIZE_M)]
        cell: u32,
        /// The operation, such as `describe`.
        op: String,
        /// The request as JSON; `{}` when omitted.
        request: Option<String>,
        #[command(flatten)]
        cache: CacheDir,
    },
    /// List the terrains an install holds: each id, its folder, and whether
    /// a decoder reads each of its terrain files.
    ListTerrains {
        /// The DCS install root. On Windows it defaults to the one the
        /// registry names.
        #[arg(long)]
        install: Option<PathBuf>,
        /// Print one JSON document rather than a table.
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        cache: CacheDir,
    },
    /// Build a terrain's cache entries ahead of the first query that needs
    /// them. Optional: a query builds what it needs.
    Index {
        /// The DCS install root. On Windows it defaults to the one the
        /// registry names.
        #[arg(long)]
        install: Option<PathBuf>,
        /// The terrain's id, as `list-terrains` shows it.
        #[arg(long)]
        terrain: String,
        /// A directory of the optional hook's tables, from the same terrain and
        /// build, whose airdrome fields only a running DCS knows are merged in.
        #[arg(long, value_name = "DIR")]
        hook: Option<PathBuf>,
        /// Accept a projection fitted to the terrain's beacons that misses one
        /// by more than 2 m.
        #[arg(long)]
        allow_crs_residual: bool,
        #[command(flatten)]
        cache: CacheDir,
    },
    /// Report a terrain's files and which decoder reads each, what the cache
    /// holds, its tables, its scenery classes and its last validation
    /// figures. Fails naming each file no decoder reads.
    Check {
        /// The DCS install root. On Windows it defaults to the one the
        /// registry names.
        #[arg(long)]
        install: Option<PathBuf>,
        /// The terrain's id, as `list-terrains` shows it.
        #[arg(long)]
        terrain: String,
        /// How many of the models placed most to list.
        #[arg(long, default_value_t = 25)]
        top: usize,
        /// Print one JSON document rather than a report.
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        cache: CacheDir,
    },
    /// Show or delete what the cache holds.
    Cache {
        #[command(subcommand)]
        action: CacheAction,
    },
    /// Print the JSON schema of every operation's request and response.
    Schema {
        /// Only this operation.
        #[arg(long)]
        op: Option<String>,
    },
}

#[derive(clap::Args)]
struct CacheDir {
    /// The cache directory. It defaults to the user's cache location:
    /// `%LOCALAPPDATA%\dcs-terrain\cache` on Windows,
    /// `~/Library/Caches/dcs-terrain` on macOS, and
    /// `$XDG_CACHE_HOME/dcs-terrain` or `~/.cache/dcs-terrain` elsewhere.
    #[arg(long = "cache", value_name = "DIR")]
    dir: Option<PathBuf>,
}

impl CacheDir {
    fn open(self) -> Result<Cache, String> {
        match self.dir {
            Some(d) => Ok(Cache::new(d)),
            None => cache::default_root().map(Cache::new).map_err(|e| e.0),
        }
    }
}

#[derive(Subcommand)]
enum CacheAction {
    /// List every terrain the cache holds, with its entries and the build
    /// they were made on.
    List {
        /// The DCS install root, to mark the build it has. On Windows it
        /// defaults to the one the registry names; without one no build is
        /// marked.
        #[arg(long)]
        install: Option<PathBuf>,
        /// Print one JSON document rather than a table.
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        cache: CacheDir,
    },
    /// Delete everything the cache holds, or one terrain's. Asks before
    /// deleting the live validation figures, which cannot be rebuilt.
    Clear {
        /// Only this terrain's id.
        #[arg(long)]
        terrain: Option<String>,
        /// Delete the validation figures too, without asking.
        #[arg(long)]
        yes: bool,
        #[command(flatten)]
        cache: CacheDir,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal())
        .with_max_level(if cli.verbose {
            Level::DEBUG
        } else {
            Level::INFO
        })
        .with_target(false)
        .without_time()
        .init();

    match cli.command {
        Command::Query {
            install,
            terrain,
            seed,
            size_km,
            cell,
            op,
            request,
            cache,
        } => run_query(
            install,
            &terrain,
            (seed, size_km, cell),
            &op,
            request,
            cache,
        ),
        Command::ListTerrains {
            install,
            json,
            cache,
        } => list_terrains(install, json, cache),
        Command::Index {
            install,
            terrain,
            hook,
            allow_crs_residual,
            cache,
        } => index(install, &terrain, hook, allow_crs_residual, cache),
        Command::Check {
            install,
            terrain,
            top,
            json,
            cache,
        } => check(install, &terrain, top, json, cache),
        Command::Cache { action } => match action {
            CacheAction::List {
                install,
                json,
                cache,
            } => cache_list(install, json, cache),
            CacheAction::Clear {
                terrain,
                yes,
                cache,
            } => cache_clear(terrain.as_deref(), yes, cache),
        },
        Command::Schema { op } => match dcsterrain_core::types::schema(op.as_deref()) {
            Ok(doc) => {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&doc).expect("a schema serializes")
                );
                ExitCode::SUCCESS
            }
            Err(e) => {
                tracing::error!("{e}");
                ExitCode::from(2)
            }
        },
    }
}

/// The install `--install` names, or on Windows the one the registry names.
fn install_root(given: Option<PathBuf>) -> Result<PathBuf, String> {
    if let Some(p) = given {
        return Ok(p);
    }
    registry_install()
}

#[cfg(windows)]
fn registry_install() -> Result<PathBuf, String> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;
    const KEY: &str = r"Software\Eagle Dynamics\DCS World";
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(KEY)
        .and_then(|k| k.get_value::<String, _>("Path"))
        .map(PathBuf::from)
        .map_err(|e| {
            format!("no DCS install in the registry (HKCU\\{KEY}, Path: {e}); pass --install <dir>")
        })
}

#[cfg(not(windows))]
fn registry_install() -> Result<PathBuf, String> {
    Err("no install given: pass --install <dir>".into())
}

/// Prints issues as the one JSON document on stdout, and fails.
fn refuse(issues: Vec<Issue>) -> ExitCode {
    for i in &issues {
        tracing::error!("{}: {}", i.code, i.message);
    }
    println!("{}", json!({ "issues": issues }));
    ExitCode::FAILURE
}

fn run_query(
    install: Option<PathBuf>,
    id: &str,
    (seed, size_km, cell): (u32, u32, u32),
    op: &str,
    request: Option<String>,
    cache: CacheDir,
) -> ExitCode {
    let refused = |code: &str, message: String| refuse(vec![Issue::new(code, &message, "")]);
    let request: Value = match request.as_deref().map(serde_json::from_str).transpose() {
        Ok(v) => v.unwrap_or_else(|| json!({})),
        Err(e) => return refused("invalid_request", format!("the request is not JSON: {e}")),
    };
    let terrain = if id == synth::TERRAIN_ID {
        match Synth::new(seed, size_km, cell) {
            Ok(s) => Terrain::Synth(s),
            Err(e) => return refused("terrain_refused", e.0),
        }
    } else {
        let defaults = (
            synth::DEFAULT_SEED,
            synth::DEFAULT_SIZE_KM,
            synth::CELL_SIZE_M,
        );
        if (seed, size_km, cell) != defaults {
            return refused(
                "invalid_flag",
                "--seed, --size-km and --cell set the synthetic terrain only".into(),
            );
        }
        let root = match install_root(install) {
            Ok(r) => r,
            Err(e) => return refused("no_install", e),
        };
        let cache = match cache.open() {
            Ok(c) => c,
            Err(e) => return refused("no_cache", e),
        };
        match Terrain::open(&root, id, &cache) {
            Ok(t) => t,
            Err(e) => return refused("terrain_refused", e.0),
        }
    };
    match query::answer(&terrain, op, &request) {
        Ok(v) => {
            println!("{v}");
            ExitCode::SUCCESS
        }
        Err(issues) => refuse(issues),
    }
}

fn list_terrains(install: Option<PathBuf>, as_json: bool, cache: CacheDir) -> ExitCode {
    let listed = install_root(install).and_then(|root| {
        let listed = install::terrains(&root).map_err(|e| e.0)?;
        Ok((root, listed))
    });
    let (root, listed) = match listed {
        Ok(l) => l,
        // A table reader gets the reason on stderr; a JSON reader also gets
        // it as the one document on stdout.
        Err(e) if as_json => return refuse(vec![Issue::new("no_install", &e, "")]),
        Err(e) => {
            tracing::error!("{e}");
            return ExitCode::FAILURE;
        }
    };
    // Without a cache directory the listing still answers, its cache column
    // unknown.
    let cache = cache.open().inspect_err(|e| tracing::warn!("{e}")).ok();
    let build = install::read_build(&root).ok();
    let surveys: Vec<install::Survey> = listed.iter().map(|t| install::survey(&root, t)).collect();
    let stored: Vec<Option<Stored>> = surveys
        .iter()
        .map(|s| match (&cache, &s.id) {
            (Some(c), Some(id)) => c.stored(id),
            _ => None,
        })
        .collect();
    let listing = Listing {
        root: &root,
        build: build.as_ref(),
        cache: cache.as_ref(),
        surveys: &surveys,
        stored: &stored,
    };
    if as_json {
        println!("{}", listing_json(&listing));
    } else {
        print!("{}", listing_text(&listing));
    }
    ExitCode::SUCCESS
}

/// What `list-terrains` reports: each terrain surveyed, and beside it what
/// the cache holds for it.
struct Listing<'a> {
    root: &'a Path,
    build: Option<&'a install::Build>,
    cache: Option<&'a Cache>,
    surveys: &'a [install::Survey],
    stored: &'a [Option<Stored>],
}

fn stored_json(s: &Stored) -> Value {
    let entries: Vec<Value> = s
        .entries
        .iter()
        .map(|(name, size)| json!({ "name": name, "size": size }))
        .collect();
    json!({
        "path": s.path.display().to_string(), "size": s.size(), "entries": entries,
        "dcs_builds": s.builds,
    })
}

fn listing_json(l: &Listing) -> Value {
    let terrains: Vec<Value> = l
        .surveys
        .iter()
        .zip(l.stored)
        .map(|(s, stored)| {
            let files: Vec<Value> = s
                .files
                .iter()
                .map(|f| {
                    json!({
                        "role": f.role, "path": f.path, "class": f.class,
                        "version": f.version, "readable": f.readable, "why": f.why,
                    })
                })
                .collect();
            json!({
                "id": s.id, "folder": s.dir, "readable": s.refusal.is_none(),
                "refusal": s.refusal, "files": files,
                "cache": stored.as_ref().map(stored_json),
            })
        })
        .collect();
    json!({
        "install": l.root.display().to_string(),
        "dcs_build": l.build.map(|b| b.version.clone()),
        "cache": l.cache.map(|c| c.root().display().to_string()),
        "terrains": terrains,
    })
}

/// `1 entry, 12 B` or `3 entries, 1.2 MiB`, or `none`.
fn cache_cell(stored: Option<&Stored>) -> String {
    let Some(s) = stored else {
        return "none".into();
    };
    let n = s.entries.len();
    let plural = if n == 1 { "entry" } else { "entries" };
    format!("{n} {plural}, {}", bytes(s.size()))
}

fn bytes(n: u64) -> String {
    const UNITS: [&str; 4] = ["KiB", "MiB", "GiB", "TiB"];
    if n < 1024 {
        return format!("{n} B");
    }
    let mut v = n as f64 / 1024.0;
    let mut unit = 0;
    while v >= 1024.0 && unit < UNITS.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    format!("{v:.1} {}", UNITS[unit])
}

/// One line per terrain, and under one that cannot open, each file no
/// decoder reads and the reason.
fn listing_text(l: &Listing) -> String {
    let build = l.build.map_or("an unknown build", |b| b.version.as_str());
    let mut out = format!("DCS {build} at {}\n", l.root.display());
    match l.cache {
        Some(c) => out.push_str(&format!("cache at {}\n\n", c.root().display())),
        None => out.push_str("cache unknown\n\n"),
    }
    let id = |s: &install::Survey| s.id.clone().unwrap_or_else(|| "(no id)".into());
    let cells: Vec<String> = l.stored.iter().map(|s| cache_cell(s.as_ref())).collect();
    let widest = |w: &mut dyn Iterator<Item = usize>, min: usize| w.max().unwrap_or(0).max(min);
    let wid = widest(&mut l.surveys.iter().map(|s| id(s).len()), 2);
    let wdir = widest(&mut l.surveys.iter().map(|s| s.dir.len()), 6);
    let wcache = widest(&mut cells.iter().map(String::len), 5);
    out.push_str(&format!(
        "{:wid$}  {:wdir$}  {:wcache$}  files\n",
        "id", "folder", "cache"
    ));
    for (s, cell) in l.surveys.iter().zip(&cells) {
        let read = s.files.iter().filter(|f| f.readable).count();
        let state = match &s.refusal {
            None => format!("{read} of {} readable", s.files.len()),
            Some(_) => format!("{read} of {} readable, cannot open", s.files.len()),
        };
        out.push_str(&format!(
            "{:wid$}  {:wdir$}  {cell:wcache$}  {state}\n",
            id(s),
            s.dir
        ));
        for f in s.files.iter().filter(|f| !f.readable) {
            let what = match (&f.class, f.version) {
                (Some(c), Some(v)) => format!("{c} version {v}"),
                _ => "not a terrain file".into(),
            };
            out.push_str(&format!("    unreadable: {} ({what})\n", f.path));
        }
        if let Some(why) = &s.refusal {
            out.push_str(&format!("    {why}\n"));
        }
    }
    out
}

/// Runs a subcommand whose failure is one message on stderr.
fn report(done: Result<(), String>) -> ExitCode {
    match done {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn index(
    install: Option<PathBuf>,
    id: &str,
    hook: Option<PathBuf>,
    allow_crs_residual: bool,
    cache: CacheDir,
) -> ExitCode {
    report((|| {
        let cache = cache.open()?;
        let root = install_root(install)?;
        let map = install::open(&root, id).map_err(|e| e.0)?;
        let terrain = cache.terrain(&root, &map);
        // The hook's tables are no input the key covers, so importing them
        // rebuilds the tables whether or not they are current; a directory
        // from another build is refused before anything is built.
        let hooked = hook.as_deref().map(TablesJson::with_hook);
        if let Some(t) = &hooked {
            t.check_hook(&map).map_err(|e| e.0)?;
        }
        println!(
            "{} {} at {}",
            map.theatre,
            map.build.version,
            terrain.dir().display()
        );
        // The hook's tables go first, so an airdrome they give another id
        // than `Radio.lua` does refuses before the slow entries are built.
        let mut entries = cache::entries();
        if let Some(t) = &hooked {
            terrain.rebuild(t).map_err(|e| e.0)?;
            println!("{}: built", t.name());
            entries.retain(|e| e.name() != t.name());
        }
        for entry in entries {
            let fresh = terrain.get(entry).map_err(|e| e.0)?.built;
            println!(
                "{}: {}",
                entry.name(),
                if fresh { "built" } else { "current" }
            );
        }
        let tables = Tables::open(&terrain).map_err(|e| e.0)?;
        if let Some(h) = &tables.hook {
            match &h.dropped {
                None => println!(
                    "the hook's tables from {}: {} airdromes, {} it names that no taxiway file does",
                    h.dir,
                    h.airdromes,
                    h.unmatched.len()
                ),
                Some(why) => println!("the hook's tables from {} were dropped: {why}", h.dir),
            }
        }
        tables
            .check_crs(&map.theatre, allow_crs_residual)
            .map_err(|e| e.0)
    })())
}

/// How many of the tallest models `check` lists.
const TALLEST: usize = 10;

fn check(
    install: Option<PathBuf>,
    id: &str,
    top: usize,
    as_json: bool,
    cache: CacheDir,
) -> ExitCode {
    let report = cache.open().and_then(|cache| {
        let root = install_root(install)?;
        check::check(&root, id, &cache, top, TALLEST).map_err(|e| e.0)
    });
    let report = match report {
        Ok(r) => r,
        Err(e) if as_json => return refuse(vec![Issue::new("terrain_refused", &e, "")]),
        Err(e) => {
            tracing::error!("{e}");
            return ExitCode::FAILURE;
        }
    };
    if as_json {
        println!(
            "{}",
            serde_json::to_string(&report).expect("a report serializes")
        );
    } else {
        print!("{}", check_text(&report));
    }
    // The report names each problem; a JSON reader gets them on stderr too.
    if as_json {
        for p in &report.problems {
            tracing::error!("{p}");
        }
    }
    if report.problems.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// A family as its JSON spells it.
fn spelled(f: classes::Family) -> String {
    match serde_json::to_value(f) {
        Ok(Value::String(s)) => s,
        Ok(v) => v.to_string(),
        Err(_) => "?".into(),
    }
}

fn decided_by(s: &classes::Source) -> String {
    match s {
        classes::Source::Override { file, reason } => format!("{file} override: {reason}"),
        classes::Source::Rule(r) => r.clone(),
    }
}

/// One catalog row: count, model, class, family, box and what decided it.
fn model_line(r: &classes::Row) -> String {
    let a = &r.assignment;
    let family = a.family.map_or_else(|| "-".into(), spelled);
    let mut by = format!("class: {}", decided_by(&a.class_by));
    if a.family.is_some() {
        by.push_str(&format!("; family: {}", decided_by(&a.family_by)));
    }
    format!(
        "  {:>9}  {:<34} {:<10} {:<11} {:>6.1} x {:>6.1} x {:>6.1} m  {by}\n",
        r.count,
        r.model,
        a.class.name(),
        family,
        r.obb_w,
        r.obb_d,
        r.height,
    )
}

fn list_or_none(items: &[String]) -> String {
    if items.is_empty() {
        "none".into()
    } else {
        items.join(", ")
    }
}

fn check_text(r: &check::Report) -> String {
    let build = match (&r.dcs_build, &r.dcs_build_timestamp) {
        (Some(v), Some(t)) => format!("DCS {v} ({t})"),
        _ => "an unknown DCS build".into(),
    };
    let mut out = format!(
        "{} in {} on {build} at {}\n\nfiles\n",
        r.terrain, r.folder, r.install
    );
    // An airfield role holds a file per airfield, 450 on GermanyCW: those a
    // decoder reads are one line per role, class and version, and each file
    // none reads has its own.
    // (role, path, what it is, files grouped into the row)
    let mut rows: Vec<(&str, String, String, usize)> = Vec::new();
    for f in &r.files {
        let what = match (&f.class, f.version) {
            (Some(c), Some(v)) => format!("{c} version {v}"),
            _ => "read as text".into(),
        };
        if f.why.is_none() && f.role.starts_with("airfield_") {
            let dir = f.path.rsplit_once('/').map_or("", |(d, _)| d);
            let same = |row: &&mut (&str, String, String, usize)| {
                row.3 > 0 && row.0 == f.role && row.1 == dir && row.2 == what
            };
            match rows.iter_mut().find(same) {
                Some(row) => row.3 += 1,
                None => rows.push((&f.role, dir.to_string(), what, 1)),
            }
            continue;
        }
        let what = match &f.why {
            None => what,
            Some(_) => format!("{what}, NO DECODER READS IT"),
        };
        rows.push((&f.role, f.path.clone(), what, 0));
    }
    let rows: Vec<(&str, String, String)> = rows
        .into_iter()
        .map(|(role, path, what, n)| match n {
            0 => (role, path, what),
            1 => (role, format!("{path}/ (1 file)"), what),
            n => (role, format!("{path}/ ({n} files)"), what),
        })
        .collect();
    let wrole = rows.iter().map(|r| r.0.len()).max().unwrap_or(0);
    let wpath = rows.iter().map(|r| r.1.len()).max().unwrap_or(0);
    for (role, path, what) in &rows {
        out.push_str(&format!("  {role:wrole$}  {path:wpath$}  {what}\n"));
    }

    if let Some(c) = &r.cache {
        out.push_str(&format!("\ncache at {}\n", c.dir));
        for e in &c.entries {
            let size = e
                .size
                .map(|s| format!(", {}", bytes(s)))
                .unwrap_or_default();
            let why = e.why.as_ref().map(|w| format!(": {w}")).unwrap_or_default();
            out.push_str(&format!("  {:<17} {}{size}{why}\n", e.name, e.state));
        }
        out.push_str(&format!(
            "  {:<17} {} tiles, {}\n",
            "horizon",
            c.horizon_tiles,
            bytes(c.horizon_size)
        ));
    }

    if let Some(t) = &r.tables {
        out.push_str(&format!(
            "\ntables\n  {} airdromes, {} runways, {} stands, {} beacons, {} radio, {} towns, \
             {} nodes\n",
            t.airdromes, t.runways, t.stands, t.beacons, t.radio, t.towns, t.nodes
        ));
        let fit = match (t.crs.n, t.crs.rms_m, t.crs.max_m) {
            (Some(n), Some(rms), Some(max)) => {
                format!("fitted to {n} beacons, rms {rms:.2} m, worst {max:.2} m")
            }
            _ => format!("from the table ({})", t.crs.source),
        };
        out.push_str(&format!("  projection {}, {fit}\n", t.crs.proj4));
        out.push_str(&format!(
            "  Lua included: {}\n",
            list_or_none(&t.lua_included)
        ));
        out.push_str(&format!(
            "  Lua unresolved: {}\n",
            list_or_none(&t.lua_unresolved)
        ));
        let hook = match &t.hook {
            None => "none imported".into(),
            Some(h) => match &h.dropped {
                Some(why) => format!("from {} dropped: {why}", h.dir),
                None => format!(
                    "from {}: {} airdromes, {} it names that no taxiway file does",
                    h.dir,
                    h.airdromes,
                    h.unmatched.len()
                ),
            },
        };
        out.push_str(&format!("  the hook's tables: {hook}\n"));
    }

    if let Some(c) = &r.catalog {
        out.push_str(&format!(
            "\nscenery: {} models, {} objects\n",
            c.models, c.objects
        ));
        let by_class: Vec<String> = c
            .counts_by_class
            .iter()
            .map(|(class, n)| format!("{} {n}", class.name()))
            .collect();
        out.push_str(&format!("  objects by class: {}\n", by_class.join(", ")));
        let unmatched: Vec<String> = c
            .unmatched
            .iter()
            .map(|(file, key)| format!("{key} ({file})"))
            .collect();
        out.push_str(&format!(
            "  overrides naming no model: {}\n",
            list_or_none(&unmatched)
        ));
        out.push_str(&format!("  the {} placed most:\n", c.most_placed.len()));
        for row in &c.most_placed {
            out.push_str(&model_line(row));
        }
        out.push_str(&format!("  the {} tallest:\n", c.tallest.len()));
        for row in &c.tallest {
            out.push_str(&model_line(row));
        }
    }

    let validation = match &r.validation {
        None => "none".into(),
        Some(v) => match v.get("dcs_build").and_then(Value::as_str) {
            Some(b) => format!("measured on {b}"),
            None => "present, its build unrecorded".into(),
        },
    };
    out.push_str(&format!("\nvalidation figures: {validation}\n"));

    if r.problems.is_empty() {
        out.push_str("\nno problems\n");
    } else {
        out.push_str("\nproblems\n");
        for p in &r.problems {
            out.push_str(&format!("  {p}\n"));
        }
    }
    out
}

fn cache_list(install: Option<PathBuf>, as_json: bool, cache: CacheDir) -> ExitCode {
    let listed = cache.open().and_then(|c| {
        let all = c.list().map_err(|e| e.0)?;
        Ok((c, all))
    });
    let (cache, all) = match listed {
        Ok(l) => l,
        Err(e) if as_json => return refuse(vec![Issue::new("no_cache", &e, "")]),
        Err(e) => {
            tracing::error!("{e}");
            return ExitCode::FAILURE;
        }
    };
    // The installed build is only a mark here; without an install the list
    // still answers.
    let installed = install_root(install)
        .and_then(|r| install::read_build(&r).map_err(|e| e.0))
        .map(|b| b.version)
        .ok();
    // Entries made on another build are re-stamped or rebuilt when next used.
    let is_installed = |s: &Stored| installed.as_ref().map(|b| s.builds == [b.clone()]);
    if as_json {
        let rows: Vec<Value> = all
            .iter()
            .map(|s| {
                let mut v = stored_json(s);
                v["terrain"] = s.terrain.clone().into();
                v["installed"] = is_installed(s).into();
                v
            })
            .collect();
        let doc = json!({ "cache": cache.root().display().to_string(), "stored": rows });
        println!("{doc}");
        return ExitCode::SUCCESS;
    }
    println!("cache at {}", cache.root().display());
    if all.is_empty() {
        println!("empty");
    }
    for s in &all {
        let mark = match is_installed(s) {
            Some(true) => " (the installed build)",
            Some(false) => " (checked again when next used)",
            None => "",
        };
        let builds = if s.builds.is_empty() {
            String::new()
        } else {
            format!(" on {}", s.builds.join(", "))
        };
        println!("{}{builds}{mark}: {}", s.terrain, cache_cell(Some(s)));
        for (name, size) in &s.entries {
            println!("    {name}  {}", bytes(*size));
        }
    }
    ExitCode::SUCCESS
}

fn cache_clear(terrain: Option<&str>, yes: bool, cache: CacheDir) -> ExitCode {
    report((|| {
        let cache = cache.open()?;
        // Asking about a directory that will be refused anyway would mislead.
        cache.check_tag().map_err(|e| e.0)?;
        let figures = cache.validation_files(terrain).map_err(|e| e.0)?;
        let validation = figures.is_empty() || yes || confirm(&figures);
        let cleared = cache.clear(terrain, validation).map_err(|e| e.0)?;
        if cleared.is_empty() {
            println!("nothing to clear");
        }
        for s in cleared {
            println!("cleared {}", s.terrain);
        }
        if !validation {
            println!("kept {} {}", figures.len(), cache::VALIDATION_FILE);
        }
        Ok(())
    })())
}

/// Asks on stderr whether to delete the validation figures, reading the
/// answer from stdin. Anything but yes, and no answer, keeps them.
fn confirm(figures: &[PathBuf]) -> bool {
    eprintln!("These hold live validation figures, which no command can rebuild:");
    for f in figures {
        eprintln!("    {}", f.display());
    }
    eprint!("Delete them too? [y/N] ");
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer).is_ok()
        && matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}
