//! Clap argument groups and subcommands exposed by the facade so app binaries
//! can mount framework subcommands directly.
//!
//! Re-exports `inkapp_config::cli` items (`ConfigCmd`, `run`) so that callers
//! only need to import `inkapp::cli`.

use std::path::PathBuf;

// Re-export config subcommand support from inkapp-config.
pub use inkapp_config::cli::{run, ConfigCmd};

#[derive(clap::Args, Debug, Clone)]
pub struct PreviewArgs {
    /// Directory to write `<key>.pdf` files into.
    #[arg(long, default_value = "./preview")]
    pub out: PathBuf,
    /// Also bind an HTTP server on 0.0.0.0 and serve the rendered PDFs.
    #[arg(long, default_value_t = false)]
    pub serve: bool,
    /// Port for --serve (default 4747).
    #[arg(long, default_value_t = 4747)]
    pub port: u16,
}
