use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "rmdigest", about = "Annotation digests for reMarkable PDFs")]
struct Cli {
    /// Path to rmdigest.toml
    config: PathBuf,
    #[arg(long)]
    dry_run: bool,
    #[arg(long)]
    local: bool,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let cfg = rmdigest::config::Config::load(&cli.config)?;
    println!("rmdigest: {} watched path(s)", cfg.watched_paths.len());
    let _ = (cli.dry_run, cli.local); // wired in a later task
    Ok(())
}
