//! The `dcsterrain` binary.
//!
//! stdout carries only a subcommand's report or its JSON document, so it can
//! be piped; warnings and progress go to stderr, more of them with
//! `--verbose`. A subcommand the build has not reached says so and exits 2.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use dcsterrain_core::synth;
use tracing::Level;

#[derive(Parser)]
#[command(
    name = "dcsterrain",
    version,
    about = "DCS World terrain, packed and queried"
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
    /// Decode a map's terrain files from a DCS install into an extract
    /// directory.
    Extract {
        /// The DCS install root. It is only read.
        #[arg(long)]
        install: PathBuf,
        /// The map's directory name under `Mods/terrains`.
        #[arg(long)]
        map: String,
        /// An empty or absent directory to write the extract to.
        #[arg(long)]
        out: PathBuf,
        /// The hook's tables-pass output, from the same build, to merge in.
        #[arg(long)]
        hook: Option<PathBuf>,
    },
    /// Validate an extract directory; exit 1 with a report when it fails.
    CheckExtract { dir: PathBuf },
    /// Build a packed file from an extract directory.
    Pack {
        dir: PathBuf,
        /// Named from the theatre, build and cell size when omitted.
        out: Option<PathBuf>,
        #[arg(long)]
        config: Option<PathBuf>,
        /// Grids to leave out, comma-separated.
        #[arg(long, value_delimiter = ',')]
        drop: Vec<String>,
        /// The packed base cell size in metres.
        #[arg(long, default_value_t = 50)]
        cell_size: u32,
        #[arg(long)]
        allow_partial: bool,
        #[arg(long)]
        allow_crs_residual: bool,
        #[arg(long)]
        betweenness_samples: Option<u32>,
        #[arg(long)]
        threads: Option<usize>,
        /// Overwrite an existing file whose `meta` differs.
        #[arg(long)]
        force: bool,
        /// Print the report as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Check a packed file's invariants; exit 1 with a report when it fails.
    Check {
        file: PathBuf,
        /// Also compare the file's terrain fingerprint with this install's.
        #[arg(long)]
        install: Option<PathBuf>,
    },
    /// Write `meta` keys into a packed file.
    Stamp {
        file: PathBuf,
        /// `key=value` pairs.
        #[arg(required = true)]
        pairs: Vec<String>,
    },
    /// Run one operation on a packed file, JSON in and out.
    Query {
        file: PathBuf,
        op: String,
        /// The request as JSON.
        request: String,
        #[arg(long)]
        max_rows: Option<u32>,
    },
    /// Serve the operations over MCP on stdio.
    Serve {
        file: PathBuf,
        /// Further packed files, chosen per call by theatre.
        #[arg(long = "file")]
        files: Vec<PathBuf>,
        #[arg(long)]
        max_rows: Option<u32>,
        /// Log here rather than to stderr.
        #[arg(long)]
        log: Option<PathBuf>,
    },
    /// Print the JSON schema of every operation's request and response.
    Schema {
        /// Only this operation.
        #[arg(long)]
        op: Option<String>,
    },
    /// Write the synthetic theatre as an extract directory.
    Synth {
        /// An empty or absent directory to write the extract to.
        dir: PathBuf,
        /// Places the town's buildings; everything else ignores it.
        #[arg(long, default_value_t = synth::DEFAULT_SEED)]
        seed: u32,
        /// The side of the square grid.
        #[arg(long, default_value_t = synth::DEFAULT_SIZE_KM)]
        size_km: u32,
        /// The cell size in metres; the extract is always 50.
        #[arg(long, default_value_t = synth::CELL_SIZE_M)]
        cell: u32,
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
        Command::Extract { .. } => not_yet("extract"),
        Command::CheckExtract { dir } => run_check_extract(&dir),
        Command::Pack { .. } => not_yet("pack"),
        Command::Check { .. } => not_yet("check"),
        Command::Stamp { .. } => not_yet("stamp"),
        Command::Query { .. } => not_yet("query"),
        Command::Serve { .. } => not_yet("serve"),
        Command::Synth {
            dir,
            seed,
            size_km,
            cell,
        } => run_synth(&dir, seed, size_km, cell),
    }
}

fn run_synth(dir: &Path, seed: u32, size_km: u32, cell: u32) -> ExitCode {
    let theatre = match synth::Theatre::new(seed, size_km, cell) {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("{e}");
            return ExitCode::from(2);
        }
    };
    match synth::write(&theatre, dir) {
        Ok(w) => {
            tracing::info!(
                "wrote the synthetic theatre to {}: {} files, {} of them tiles",
                dir.display(),
                w.files,
                w.tiles
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            tracing::error!("cannot write {}: {e}", dir.display());
            ExitCode::FAILURE
        }
    }
}

fn run_check_extract(dir: &Path) -> ExitCode {
    match dcsterrain_core::extract::check(dir) {
        Ok(s) => {
            println!(
                "{}: passes: {} on DCS {}, {} tiles, {} tables",
                dir.display(),
                s.theatre,
                s.dcs_build,
                s.tiles,
                s.tables
            );
            ExitCode::SUCCESS
        }
        Err(failures) => {
            for f in &failures {
                println!("{f}");
            }
            tracing::error!(
                "{} fails: {} problems, each listed with its path",
                dir.display(),
                failures.len()
            );
            ExitCode::FAILURE
        }
    }
}

fn not_yet(name: &str) -> ExitCode {
    tracing::error!("`{name}` is not built yet");
    ExitCode::from(2)
}
