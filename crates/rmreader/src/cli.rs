//! rmreader CLI: `init` wizard, or regenerate from a config path.
use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::{config, deploy, generate, readback, readwise, wizard};

#[derive(Parser)]
#[command(
    name = "rmreader",
    version,
    about = "Readwise Reader -> reMarkable reader PDFs",
    args_conflicts_with_subcommands = true
)]
struct Cli {
    /// Path to an existing rmreader.toml to regenerate.
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Create config interactively and generate.
    Init,
}

pub fn run(args: Vec<String>) -> anyhow::Result<()> {
    let cli = Cli::try_parse_from(args).unwrap_or_else(|e| e.exit());
    let transport = readwise::http::UreqTransport;
    match (cli.command, cli.config) {
        (Some(Command::Init), _) => {
            let (cfg, out_dir, cfg_path) = wizard::run_wizard(&transport)?;
            cfg.validate()?;
            std::fs::create_dir_all(&out_dir)?;
            config::dump(&cfg, &cfg_path)?;
            let fetcher = generate::UreqImageFetcher {
                timeout_secs: cfg.images.timeout_secs,
                concurrency: cfg.images.concurrency,
            };
            let targets = generate::generate(&cfg, &transport, &fetcher)?;
            deploy::get_deployer(&cfg)?.deploy(&targets)?;
            println!("Wrote {} PDF(s) to {}", targets.len(), out_dir.display());
            Ok(())
        }
        (None, Some(path)) => {
            let cfg = config::load(&path)?;
            cfg.validate()?;
            let fetcher = generate::UreqImageFetcher {
                timeout_secs: cfg.images.timeout_secs,
                concurrency: cfg.images.concurrency,
            };
            let deployer = deploy::get_deployer(&cfg)?;
            // Read-back on-device annotations and apply them, BEFORE regenerating.
            if let Err(e) = readback::sync_collection(
                &*deployer,
                &transport,
                &cfg.readwise.token,
                &cfg.deploy.library_folder,
                "Library",
            ) {
                eprintln!("[rmreader] Library read-back failed (continuing): {e:#}");
            }
            if cfg.feed.enabled {
                if let Err(e) = readback::sync_collection(
                    &*deployer,
                    &transport,
                    &cfg.readwise.token,
                    &cfg.deploy.feed_folder,
                    "Feed",
                ) {
                    eprintln!("[rmreader] Feed read-back failed (continuing): {e:#}");
                }
            }
            let targets = generate::generate(&cfg, &transport, &fetcher)?;
            for (pdf, folder) in &targets {
                deployer.replace(pdf, folder)?;
            }
            println!("Regenerated {} PDF(s)", targets.len());
            Ok(())
        }
        (None, None) => {
            use clap::CommandFactory;
            Cli::command().print_help()?;
            println!();
            Ok(())
        }
    }
}

pub fn main() -> anyhow::Result<()> {
    run(std::env::args().collect())
}
