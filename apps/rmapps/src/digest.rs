//! `rmapps digest` — text-extract + ink-summarize reMarkable docs into per-source
//! digests, deployed back into each source doc's own folder via the native client.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Args;

use rmdigest::deploy::LocalBackend;
use rmdigest::generate;
use rmdigest::state;

use crate::cloud::Cloud;
use crate::cloud_adapters::CloudBackend;
use crate::config::Config;

#[derive(Args, Default)]
pub struct DigestArgs {
    /// Generate PDFs to disk via the local backend instead of the cloud.
    #[arg(long)]
    local: bool,
    /// Print the plan without writing or deploying.
    #[arg(long = "dry-run")]
    dry_run: bool,
    /// Output directory for generated digests (used with --local).
    #[arg(long)]
    out: Option<PathBuf>,
}

pub fn run(args: DigestArgs, cfg: &Config) -> Result<()> {
    let digest = cfg
        .digest
        .as_ref()
        .context("no [digest] section in rmapps config")?;
    digest.validate()?;

    let state_path = std::env::var("RMDIGEST_STATE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| state::State::default_path());

    let opts = generate::Opts {
        dry_run: args.dry_run,
        local_output: args.out.clone(),
    };

    if args.local {
        let backend = LocalBackend;
        generate::run(digest, &backend, &state_path, &opts)
    } else {
        let cl = Cloud::from_stored()?;
        let backend = CloudBackend { cloud: &cl };
        generate::run(digest, &backend, &state_path, &opts)
    }
}
