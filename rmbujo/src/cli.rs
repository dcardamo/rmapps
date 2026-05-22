//! rmbujo CLI: `new` wizard, or regenerate from a config path.

use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

use crate::{config, deploy, generate, wizard};

#[derive(Parser)]
#[command(
    name = "rmbujo",
    version,
    about = "Dot-grid bullet journal PDF generator for reMarkable",
    args_conflicts_with_subcommands = true
)]
struct Cli {
    /// Path to an existing rmbujo.toml to regenerate.
    config: Option<PathBuf>,
    /// Re-fetch ICS feeds (otherwise cached feeds are reused on regenerate).
    #[arg(long = "refresh-feeds")]
    refresh_feeds: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Create a new year interactively.
    New,
}

pub fn run(args: Vec<String>) -> anyhow::Result<()> {
    // try_parse_from returns Err for --help/--version (DisplayHelp/DisplayVersion);
    // e.exit() handles those correctly (print + exit 0) instead of propagating as anyhow::Error.
    let cli = Cli::try_parse_from(args).unwrap_or_else(|e| e.exit());
    match (cli.command, cli.config, cli.refresh_feeds) {
        (Some(Command::New), _, _) => {
            let (config, out_dir, config_path) = wizard::run_wizard()?;
            config.validate()?;
            std::fs::create_dir_all(&out_dir)?;
            config::dump(&config, &config_path)?;
            // First run: always fetch feeds fresh.
            let paths = generate::generate_year(&config, &out_dir, true)?;
            deploy::get_deployer(&config)?.deploy(&paths)?;
            println!("Wrote {} PDFs to {}", paths.len(), out_dir.display());
            Ok(())
        }
        (None, Some(path), refresh_feeds) => {
            let config = config::load(&path)?;
            config.validate()?;
            let out_dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
            // Reuse cached feeds unless --refresh-feeds was passed.
            let paths = generate::generate_year(&config, &out_dir, refresh_feeds)?;
            deploy::get_deployer(&config)?.refresh(&paths)?;
            println!("Regenerated {} PDFs in {}", paths.len(), out_dir.display());
            Ok(())
        }
        (None, None, _) => {
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
