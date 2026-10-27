//! The `dcsterrain` binary.
//!
//! stdout carries only a subcommand's report or its JSON document, so it can
//! be piped; warnings and progress go to stderr, more of them with
//! `--verbose`.

use std::io::IsTerminal;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
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
