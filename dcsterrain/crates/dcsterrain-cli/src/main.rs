//! The `dcsterrain` binary.
//!
//! stdout carries only a subcommand's report or its JSON document, so it can
//! be piped; warnings and progress go to stderr, more of them with
//! `--verbose`.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use dcsterrain_core::cache::{self, Cache, Stored};
use dcsterrain_core::decode::install;
use dcsterrain_core::query::{self, Terrain};
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
    /// List every terrain and build the cache holds, with its entries.
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
    /// Delete every build but the one the install has, for every terrain.
    Prune {
        /// The DCS install root. On Windows it defaults to the one the
        /// registry names.
        #[arg(long)]
        install: Option<PathBuf>,
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
        } => run_query(install, &terrain, (seed, size_km, cell), &op, request),
        Command::ListTerrains {
            install,
            json,
            cache,
        } => list_terrains(install, json, cache),
        Command::Index {
            install,
            terrain,
            cache,
        } => index(install, &terrain, cache),
        Command::Cache { action } => match action {
            CacheAction::List {
                install,
                json,
                cache,
            } => cache_list(install, json, cache),
            CacheAction::Prune { install, cache } => cache_prune(install, cache),
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
        match Terrain::open(&root, id) {
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
        .map(|s| match (&cache, &s.id, &build) {
            (Some(c), Some(id), Some(b)) => c.stored(id, &b.version),
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
/// the cache holds for it at the installed build.
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
                        "version": f.version, "readable": f.readable,
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

fn index(install: Option<PathBuf>, id: &str, cache: CacheDir) -> ExitCode {
    report((|| {
        let cache = cache.open()?;
        let root = install_root(install)?;
        let map = install::open(&root, id).map_err(|e| e.0)?;
        let terrain = cache.terrain(&root, &map);
        let built = terrain.index(&cache::entries()).map_err(|e| e.0)?;
        println!(
            "{} {} at {}",
            map.theatre,
            map.build.version,
            terrain.dir().display()
        );
        if built.is_empty() {
            println!("no entry kinds exist yet: nothing to build");
        }
        for (name, fresh) in built {
            println!("{name}: {}", if fresh { "built" } else { "current" });
        }
        Ok(())
    })())
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
        .map(|b| cache::dir_name(&b.version))
        .ok();
    let is_installed = |s: &Stored| installed.as_deref().map(|b| b == s.build);
    if as_json {
        let rows: Vec<Value> = all
            .iter()
            .map(|s| {
                let mut v = stored_json(s);
                v["terrain"] = s.terrain.clone().into();
                v["dcs_build"] = s.build.clone().into();
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
            Some(true) => " (installed)",
            Some(false) => " (not installed: prune deletes it)",
            None => "",
        };
        println!("{} {}{mark}: {}", s.terrain, s.build, cache_cell(Some(s)));
        for (name, size) in &s.entries {
            println!("    {name}  {}", bytes(*size));
        }
    }
    ExitCode::SUCCESS
}

fn cache_prune(install: Option<PathBuf>, cache: CacheDir) -> ExitCode {
    report((|| {
        let cache = cache.open()?;
        let build = install::read_build(&install_root(install)?).map_err(|e| e.0)?;
        let gone = cache.prune(&build.version).map_err(|e| e.0)?;
        if gone.is_empty() {
            println!("nothing to prune");
        }
        for s in gone {
            println!("deleted {} {} ({})", s.terrain, s.build, bytes(s.size()));
        }
        Ok(())
    })())
}

fn cache_clear(terrain: Option<&str>, yes: bool, cache: CacheDir) -> ExitCode {
    report((|| {
        let cache = cache.open()?;
        let figures = cache.validation_files(terrain).map_err(|e| e.0)?;
        let validation = figures.is_empty() || yes || confirm(&figures);
        let cleared = cache.clear(terrain, validation).map_err(|e| e.0)?;
        if cleared.is_empty() {
            println!("nothing to clear");
        }
        for s in cleared {
            println!("cleared {} {}", s.terrain, s.build);
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
