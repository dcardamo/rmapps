//! rmbujo CLI: `new` wizard, or regenerate from a config path.

use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

use crate::{config, deploy, generate, wizard};

#[derive(Parser)]
#[command(name = "rmbujo", version, about = "Dot-grid bullet journal PDF generator for reMarkable", args_conflicts_with_subcommands = true)]
struct Cli {
    /// Path to an existing rmbujo.toml to regenerate.
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Create a new year interactively.
    New,
}

pub fn run(args: Vec<String>) -> anyhow::Result<()> {
    let cli = Cli::try_parse_from(args)?;
    match (cli.command, cli.config) {
        (Some(Command::New), _) => {
            let (config, out_dir, config_path) = wizard::run_wizard()?;
            config::dump(&config, &config_path)?;
            let paths = generate::generate_year(&config, &out_dir)?;
            deploy::get_deployer(&config)?.deploy(&paths)?;
            println!("Wrote {} PDFs to {}", paths.len(), out_dir.display());
            Ok(())
        }
        (None, Some(path)) => {
            let config = config::load(&path)?;
            let out_dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
            let paths = generate::generate_year(&config, &out_dir)?;
            deploy::get_deployer(&config)?.refresh(&paths)?;
            println!("Regenerated {} PDFs in {}", paths.len(), out_dir.display());
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
