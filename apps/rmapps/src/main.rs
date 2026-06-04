//! `rmapps` — the single CLI for the reMarkable toolset.
//!
//! Subcommands:
//! - `auth`  — native pairing + credential status (no rmapi).
//! - `bujo`  — generate/deploy bullet-journal PDFs.
//! - `reader`— pull Readwise Reader collections, read-back, deploy.
//! - `digest`— summarize reMarkable docs into per-source digests.
//! - `sync`  — run the config-driven `[[sync]]` scheduled tasks once.
//! - `watch` — run the resident daemon: scheduled tasks + push-driven reactions.
//! - `push`  — upload a single PDF to a cloud folder (replace or content-only).
//!
//! All deploy goes through the native `rm-cloud` client (see `cloud.rs`); rmapi
//! is gone. Each subcommand constructs one [`cloud::Cloud`] and reuses it.

mod auth;
mod bujo;
mod cache_cmd;
mod cloud;
mod cloud_adapters;
mod config;
mod digest;
mod get;
mod lock;
mod ls;
mod push;
mod reader;
mod rm;
mod sync;
mod timing;
mod watch;

use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "rmapps", version, about)]
struct Cli {
    /// Path to the rmapps config (default: <config_dir>/rmapps/config.toml).
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    /// Print per-stage timing for the run (also enabled by `RMAPPS_TIMINGS=1`).
    #[arg(long, global = true)]
    timings: bool,
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
    /// Run the resident daemon: scheduled tasks + push-driven reactions.
    Watch(watch::WatchArgs),
    /// Upload a PDF to a cloud folder (`--content-only` to preserve on-device ink).
    Push(push::PushArgs),
    /// Download a document's original source file (PDF/EPUB) to disk.
    Get(get::GetArgs),
    /// List the entries directly under a cloud folder (default: root).
    Ls(ls::LsArgs),
    /// Delete a document or folder in the cloud (`--recursive` for folders).
    Rm(rm::RmArgs),
    /// Inspect and prune the local blob cache.
    Cache(cache_cmd::CacheArgs),
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let timings = timing::timings_enabled(cli.timings, std::env::var("RMAPPS_TIMINGS").ok().as_deref());
    timing::init(timings);
    let cfg_path = cli.config.as_deref();
    match cli.command {
        Command::Auth(args) => auth::run(args),
        Command::Bujo(args) => {
            let cfg = config::load(cfg_path)?;
            let _lock = lock::acquire("bujo", lock::Wait::Fail)?;
            bujo::run(args, &cfg)
        }
        Command::Reader => {
            let cfg = config::load(cfg_path)?;
            let _lock = lock::acquire("reader", lock::Wait::Fail)?;
            reader::run(&cfg)
        }
        Command::Digest(args) => {
            let cfg = config::load(cfg_path)?;
            let _lock = lock::acquire("digest", lock::Wait::Fail)?;
            digest::run(args, &cfg)
        }
        Command::Sync => {
            let cfg = config::load(cfg_path)?;
            let _lock = lock::acquire("sync", lock::Wait::Fail)?;
            sync::run(&cfg)
        }
        Command::Watch(args) => {
            let cfg = config::load(cfg_path)?;
            watch::run(args, &cfg)
        }
        Command::Push(args) => {
            let _lock = lock::acquire("push", lock::Wait::Fail)?;
            push::run(args)
        }
        Command::Get(args) => get::run(args),
        Command::Ls(args) => ls::run(args),
        Command::Rm(args) => {
            let _lock = lock::acquire("rm", lock::Wait::Fail)?;
            rm::run(args)
        }
        // `cache` only inspects/prunes the local blob cache — no cloud mutation, so no lock.
        Command::Cache(args) => cache_cmd::run(args),
    }
}
