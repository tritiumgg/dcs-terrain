//! The `dcsterrain` binary.
//!
//! stdout carries only a subcommand's report or its JSON document, so it can
//! be piped; warnings and progress go to stderr, more of them with
//! `--verbose`.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
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
    },
    /// Print the JSON schema of every operation's request and response.
    Schema {
        /// Only this operation.
        #[arg(long)]
        op: Option<String>,
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
        Command::ListTerrains { install, json } => list_terrains(install, json),
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

fn list_terrains(install: Option<PathBuf>, as_json: bool) -> ExitCode {
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
    let build = install::read_build(&root).ok();
    let surveys: Vec<install::Survey> = listed.iter().map(|t| install::survey(&root, t)).collect();
    if as_json {
        println!("{}", listing_json(&root, build.as_ref(), &surveys));
    } else {
        print!("{}", listing_text(&root, build.as_ref(), &surveys));
    }
    ExitCode::SUCCESS
}

fn listing_json(root: &Path, build: Option<&install::Build>, surveys: &[install::Survey]) -> Value {
    let terrains: Vec<Value> = surveys
        .iter()
        .map(|s| {
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
            })
        })
        .collect();
    json!({
        "install": root.display().to_string(),
        "dcs_build": build.map(|b| b.version.clone()),
        "terrains": terrains,
    })
}

/// One line per terrain, and under one that cannot open, each file no
/// decoder reads and the reason.
fn listing_text(
    root: &Path,
    build: Option<&install::Build>,
    surveys: &[install::Survey],
) -> String {
    let build = build.map_or("an unknown build", |b| b.version.as_str());
    let mut out = format!("DCS {build} at {}\n\n", root.display());
    let id = |s: &install::Survey| s.id.clone().unwrap_or_else(|| "(no id)".into());
    let wid = surveys
        .iter()
        .map(|s| id(s).len())
        .max()
        .unwrap_or(0)
        .max(2);
    let wdir = surveys
        .iter()
        .map(|s| s.dir.len())
        .max()
        .unwrap_or(0)
        .max(6);
    out.push_str(&format!("{:wid$}  {:wdir$}  files\n", "id", "folder"));
    for s in surveys {
        let read = s.files.iter().filter(|f| f.readable).count();
        let state = match &s.refusal {
            None => format!("{read} of {} readable", s.files.len()),
            Some(_) => format!("{read} of {} readable, cannot open", s.files.len()),
        };
        out.push_str(&format!("{:wid$}  {:wdir$}  {state}\n", id(s), s.dir));
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
