//! `dcsterrain-validate plan` then, after the chunks have run in DCS,
//! `dcsterrain-validate report`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use dcsterrain_core::cache::{self, Cache};
use dcsterrain_core::query::Terrain;
use dcsterrain_core::synth::{self, Synth};
use dcsterrain_validate::plan::{Plan, Sizes, plan};
use dcsterrain_validate::{report, sortie};

const PLAN_FILE: &str = "plan.json";

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Draw the samples, record the tool's answers and write the chunks.
    Plan {
        /// The DCS install root; the terrain's files are read from it.
        #[arg(long)]
        install: Option<PathBuf>,
        /// The terrain's id, or `synth`.
        #[arg(long)]
        terrain: String,
        /// Where the plan, the chunks and DCS's replies go.
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value_t = 1)]
        seed: u64,
        /// The cache directory, when not the default.
        #[arg(long)]
        cache: Option<PathBuf>,
    },
    /// Score the replies and write `validation.json`, here and to the cache.
    Report {
        /// The directory `plan` wrote.
        #[arg(long)]
        out: PathBuf,
        /// The cache directory, when not the default.
        #[arg(long)]
        cache: Option<PathBuf>,
    },
}

fn open_cache(dir: Option<PathBuf>) -> Result<Cache, String> {
    match dir {
        Some(d) => Ok(Cache::new(d)),
        None => cache::default_root().map(Cache::new).map_err(|e| e.0),
    }
}

fn run_plan(
    install: Option<PathBuf>,
    id: &str,
    out: &Path,
    seed: u64,
    dir: Option<PathBuf>,
) -> Result<(), String> {
    let terrain = if id == "synth" {
        let s = Synth::new(
            synth::DEFAULT_SEED,
            synth::DEFAULT_SIZE_KM,
            synth::CELL_SIZE_M,
        )
        .map_err(|e| format!("{e:?}"))?;
        Terrain::Synth(Box::new(s))
    } else {
        let install = install.ok_or("--install names the DCS install")?;
        Terrain::open(&install, id, &open_cache(dir)?, false).map_err(|e| e.0)?
    };
    let plan = plan(&terrain, seed, &Sizes::default())?;
    fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let text = serde_json::to_string(&plan).map_err(|e| e.to_string())?;
    fs::write(out.join(PLAN_FILE), text).map_err(|e| e.to_string())?;
    let chunks = sortie::write(&plan, out)?;
    println!(
        "{} on {}: {} heights, {} sightlines, {} routes, {} references in {} chunks",
        plan.terrain,
        plan.dcs_build,
        plan.heights.len(),
        plan.sights.len(),
        plan.routes.len(),
        plan.mgrs.len(),
        chunks.len()
    );
    Ok(())
}

fn run_report(out: &Path, dir: Option<PathBuf>) -> Result<bool, String> {
    let path = out.join(PLAN_FILE);
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let plan: Plan = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let figures = report::report(&plan, out)?;
    let text = serde_json::to_string_pretty(&figures).map_err(|e| e.to_string())?;
    fs::write(out.join(cache::VALIDATION_FILE), &text).map_err(|e| e.to_string())?;
    let terrain_dir = open_cache(dir)?.dir(&plan.terrain);
    fs::create_dir_all(&terrain_dir).map_err(|e| e.to_string())?;
    fs::write(terrain_dir.join(cache::VALIDATION_FILE), &text).map_err(|e| e.to_string())?;
    println!("{text}");
    Ok(figures["pass"] == true)
}

fn main() -> ExitCode {
    let done = match Cli::parse().command {
        Command::Plan {
            install,
            terrain,
            out,
            seed,
            cache,
        } => run_plan(install, &terrain, &out, seed, cache).map(|()| true),
        Command::Report { out, cache } => run_report(&out, cache),
    };
    match done {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => {
            eprintln!("a target was missed");
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
