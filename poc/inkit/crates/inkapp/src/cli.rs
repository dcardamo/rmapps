//! Clap argument groups and subcommands exposed by the facade so app binaries
//! can mount framework subcommands directly.
//!
//! - **Config subcommand:** re-exports `inkapp_config::cli::{run, ConfigCmd}`
//!   so callers only need to import `inkapp::cli`.
//! - **Preview subcommand args:** [`PreviewArgs`] used by `app preview ...`.
//! - **Operator subcommand:** [`OpCmd`] (`pair`, `secret set|list`) — device
//!   pairing + secret store management. Dispatched via [`run_op`] (async).
//!   Named `run_op` to avoid collision with the re-exported `run` (config).

use std::path::PathBuf;

use clap::{Subcommand, ValueEnum};

use inkapp_core::secrets::{Scope, SecretStore};
use rm_cloud::Config;

// Re-export config subcommand support from inkapp-config.
pub use inkapp_config::cli::{run, ConfigCmd};

/// Args for the `preview` subcommand (render docs locally, optionally serve).
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

/// Operator-setup commands: pair the device, manage secrets.
///
/// Mounted by app binaries under a top-level `Op` subcommand. Device-neutral
/// by design *except* the `Pair` arm, which calls reMarkable-specific pairing
/// via [`rm_device::pair`] (re-exported as [`crate::pair`]).
#[derive(Subcommand, Debug)]
pub enum OpCmd {
    /// Pair this machine with a reMarkable using an 8-char code from
    /// https://my.remarkable.com/device/desktop/connect.
    Pair {
        /// The 8-character one-time code shown in the browser.
        code: String,
    },
    /// Manage the per-user secret store (`secrets.json`).
    #[command(subcommand)]
    Secret(SecretCmd),
}

/// Subcommands of `secret`.
#[derive(Subcommand, Debug)]
pub enum SecretCmd {
    /// Store a secret. Scope is `connector` or `device-auth`.
    Set {
        #[arg(value_enum)]
        scope: ScopeArg,
        name: String,
        value: String,
    },
    /// List `(scope, name)` pairs in the store. Values are never printed.
    List,
}

/// CLI-facing subset of [`Scope`] — `UserKey` is deliberately not exposed.
#[derive(ValueEnum, Debug, Clone, Copy)]
pub enum ScopeArg {
    Connector,
    DeviceAuth,
}

impl From<ScopeArg> for Scope {
    fn from(s: ScopeArg) -> Self {
        match s {
            ScopeArg::Connector => Scope::ConnectorCred,
            ScopeArg::DeviceAuth => Scope::DeviceAuth,
        }
    }
}

/// Dispatch an operator command against the secret store at `secrets_path`.
/// Returns a process exit code (0 = success). Named `run_op` to avoid colliding
/// with the re-exported config-CLI [`run`].
pub async fn run_op(cmd: OpCmd, secrets_path: PathBuf) -> std::io::Result<i32> {
    match cmd {
        OpCmd::Pair { code } => {
            let mut secrets = SecretStore::open(&secrets_path)
                .map_err(|e| std::io::Error::other(format!("open secrets: {e}")))?;
            let config = Config::from_env();
            match crate::pair(&mut secrets, &config, &code).await {
                Ok(()) => {
                    println!("paired: device token stored at {}", secrets_path.display());
                    Ok(0)
                }
                Err(e) => {
                    eprintln!("pair failed: {e}");
                    Ok(1)
                }
            }
        }
        OpCmd::Secret(SecretCmd::Set { scope, name, value }) => {
            let mut secrets = SecretStore::open(&secrets_path)
                .map_err(|e| std::io::Error::other(format!("open secrets: {e}")))?;
            secrets
                .set(scope.into(), &name, value.as_bytes())
                .map_err(|e| std::io::Error::other(format!("write secret: {e}")))?;
            println!("stored {scope:?} / {name}");
            Ok(0)
        }
        OpCmd::Secret(SecretCmd::List) => {
            // Re-opening to avoid a long-lived store handle; reads are cheap.
            let secrets = SecretStore::open(&secrets_path)
                .map_err(|e| std::io::Error::other(format!("open secrets: {e}")))?;
            // Iterate by trying each CLI-visible scope.
            for (scope_arg, label) in [
                (ScopeArg::Connector, "connector"),
                (ScopeArg::DeviceAuth, "device-auth"),
            ] {
                for name in secrets.names(scope_arg.into()) {
                    println!("{label}  {name}");
                }
            }
            Ok(0)
        }
    }
}
