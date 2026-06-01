//! `rmapps` — the single CLI for the reMarkable toolset.
//!
//! Subcommands:
//! - `auth`  — native pairing + credential status (no rmapi).
//! - `bujo`  — generate/deploy bullet-journal PDFs.
//! - `reader`— pull Readwise Reader collections, read-back, deploy.
//! - `digest`— summarize reMarkable docs into per-source digests.
//! - `sync`  — run the config-driven `[[sync]]` tasks once.
//!
//! All deploy goes through the native `rm-cloud` client (see `cloud.rs`); rmapi
//! is gone. Each subcommand constructs one [`cloud::Cloud`] and reuses it.

mod auth;
mod bujo;
mod cloud;
mod config;
mod digest;
mod ls;
mod reader;
mod rm;
mod sync;

use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "rmapps", version, about)]
struct Cli {
    /// Path to the rmapps config (default: <config_dir>/rmapps/config.toml).
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Pair with the reMarkable cloud and inspect credential status (native; no rmapi).
    Auth(auth::AuthArgs),
    /// Generate and deploy bullet-journal PDFs.
    Bujo(bujo::BujoArgs),
    /// Pull Readwise Reader collections, run read-back, and deploy reader PDFs.
    Reader,
    /// Summarize reMarkable docs into per-source digests.
    Digest(digest::DigestArgs),
    /// Run the configured `[[sync]]` tasks once.
    Sync,
    /// List the entries directly under a cloud folder (default: root).
    Ls(ls::LsArgs),
    /// Delete a document or folder in the cloud (`--recursive` for folders).
    Rm(rm::RmArgs),
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let cfg_path = cli.config.as_deref();
    match cli.command {
        Command::Auth(args) => auth::run(args),
        Command::Bujo(args) => {
            let cfg = config::load(cfg_path)?;
            bujo::run(args, &cfg)
        }
        Command::Reader => {
            let cfg = config::load(cfg_path)?;
            reader::run(&cfg)
        }
        Command::Digest(args) => {
            let cfg = config::load(cfg_path)?;
            digest::run(args, &cfg)
        }
        Command::Sync => {
            let cfg = config::load(cfg_path)?;
            sync::run(&cfg)
        }
        Command::Ls(args) => ls::run(args),
        Command::Rm(args) => rm::run(args),
    }
}
