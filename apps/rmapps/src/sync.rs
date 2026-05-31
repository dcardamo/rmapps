//! `rmapps sync` — run each configured `[[sync]]` task once.
//!
//! For now this simply iterates `config.sync` and runs each task's app with
//! default flags, reusing the same code paths as the standalone subcommands.
//
// TODO: triggers + shared snapshot — schedule (`every`), on-change (`watch`),
// generation-poll (`trigger`), and `month_window` are parsed but not yet acted
// on; a later task adds generation-poll triggers and a single shared cloud
// snapshot across all tasks in one run.

use anyhow::Result;

use crate::bujo::{self, BujoArgs};
use crate::config::Config;
use crate::digest::{self, DigestArgs};
use crate::reader;

pub fn run(cfg: &Config) -> Result<()> {
    if cfg.sync.is_empty() {
        println!("No [[sync]] tasks configured.");
        return Ok(());
    }
    for task in &cfg.sync {
        println!("[rmapps] sync: running {}", task.app);
        match task.app.as_str() {
            "bujo" => bujo::run(default_bujo_args(), cfg)?,
            "reader" => reader::run(cfg)?,
            "digest" => digest::run(default_digest_args(), cfg)?,
            other => anyhow::bail!("unknown sync app {other:?} (expected bujo|reader|digest)"),
        }
    }
    Ok(())
}

fn default_bujo_args() -> BujoArgs {
    // clap's `Args` derive doesn't give us a Default; build via parse of an empty
    // arg list so every optional flag is None/false.
    use clap::Parser;
    #[derive(Parser)]
    struct Wrap {
        #[command(flatten)]
        inner: BujoArgs,
    }
    Wrap::parse_from(["bujo"]).inner
}

fn default_digest_args() -> DigestArgs {
    use clap::Parser;
    #[derive(Parser)]
    struct Wrap {
        #[command(flatten)]
        inner: DigestArgs,
    }
    Wrap::parse_from(["digest"]).inner
}
