//! `rmapps` — the single CLI for the reMarkable toolset.
//!
//! Subcommands are added incrementally during the monorepo migration:
//! `auth` (native pairing, no rmapi), then `bujo`/`reader`/`digest` generation,
//! then `sync` (config-driven orchestration). See the workspace README.

mod auth;
mod cloud;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "rmapps", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Pair with the reMarkable cloud and inspect credential status (native; no rmapi).
    Auth(auth::AuthArgs),
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Auth(args) => auth::run(args),
    }
}
