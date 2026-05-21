//! rmreader CLI: `init` wizard, or regenerate from a config path.
use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::{config, deploy, generate, readwise, wizard};

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
    let fetcher = generate::UreqImageFetcher;
    match (cli.command, cli.config) {
        (Some(Command::Init), _) => {
            let (cfg, out_dir, cfg_path) = wizard::run_wizard(&transport)?;
            cfg.validate()?;
            std::fs::create_dir_all(&out_dir)?;
            config::dump(&cfg, &cfg_path)?;
            let targets = generate::generate(&cfg, &transport, &fetcher)?;
            deploy::get_deployer(&cfg)?.deploy(&targets)?;
            println!("Wrote {} PDF(s) to {}", targets.len(), out_dir.display());
            Ok(())
        }
        (None, Some(path)) => {
            let cfg = config::load(&path)?;
            cfg.validate()?;
            let targets = generate::generate(&cfg, &transport, &fetcher)?;
            deploy::get_deployer(&cfg)?.refresh(&targets)?;
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
