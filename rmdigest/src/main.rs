use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "rmdigest", about = "Annotation digests for reMarkable PDFs")]
struct Cli {
    /// Path to rmdigest.toml
    config: PathBuf,

    /// Generate locally without uploading (writes PDFs to ./rmdigest-out or --out).
    /// Uses RmapiBackend for discovery/fetch but writes outputs locally.
    #[arg(long)]
    local: bool,

    /// Don't upload (still fetches + generates).
    #[arg(long)]
    dry_run: bool,

    /// Output dir for --local.
    #[arg(long)]
    out: Option<PathBuf>,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let cfg = rmdigest::config::Config::load(&cli.config)?;
    let state_path = std::env::var_os("RMDIGEST_STATE")
        .map(PathBuf::from)
        .unwrap_or_else(rmdigest::state::State::default_path);

    let backend: Box<dyn rmdigest::deploy::Backend> = if cli.local {
        // Local mode: use LocalBackend which does not upload.
        // list() returns empty — use --local primarily with tests or inject a backend.
        // For discovery against the real cloud with local output, pair with --dry-run=false
        // and the backend still writes locally via LocalBackend::put.
        Box::new(rmdigest::deploy::LocalBackend::new(
            cli.out
                .clone()
                .unwrap_or_else(|| PathBuf::from("rmdigest-out")),
        ))
    } else {
        Box::new(rmdigest::deploy::RmapiBackend::new(
            rmdigest::deploy::ProcessRmapi::new()?,
        ))
    };

    let opts = rmdigest::generate::Opts {
        dry_run: cli.dry_run,
        local_output: cli.out,
    };

    rmdigest::generate::run(&cfg, backend.as_ref(), &state_path, &opts)
}
