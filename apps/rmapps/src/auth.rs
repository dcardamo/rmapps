//! Native reMarkable pairing + credential storage — no rmapi.
//!
//! Pairing exchanges an 8-char one-time code (from
//! <https://my.remarkable.com/device/desktop/connect>) for a long-lived device
//! token via `rm_cloud::register_device`, then stores it at
//! `~/.config/rmapps/auth.json` with mode 0600. The short-lived user token is
//! always refreshed lazily from the device token, so only the device token is
//! persisted.

use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};
use rm_cloud::{register_device, Client, Config};
use serde::{Deserialize, Serialize};

#[derive(Args)]
pub struct AuthArgs {
    #[command(subcommand)]
    cmd: AuthCmd,
}

#[derive(Subcommand)]
enum AuthCmd {
    /// Pair this machine: enter an 8-char code from
    /// https://my.remarkable.com/device/desktop/connect.
    Login {
        /// The one-time code. If omitted, you'll be prompted.
        code: Option<String>,
    },
    /// Show whether a device token is stored and verify it against the cloud.
    Status,
    /// Remove stored credentials.
    Logout,
}

/// Persisted credentials. Only the long-lived device token is stored; the user
/// token is derived from it on demand.
#[derive(Serialize, Deserialize, Default)]
struct StoredAuth {
    device_token: String,
}

/// Path to the credential file (`~/.config/rmapps/auth.json`).
pub fn auth_path() -> Result<PathBuf> {
    let base = dirs::config_dir().context("could not resolve a config directory")?;
    Ok(base.join("rmapps").join("auth.json"))
}

/// Load the stored device token, or return a helpful error if unpaired.
pub fn load_device_token() -> Result<String> {
    let auth = load().with_context(|| {
        format!(
            "not paired — run `rmapps auth login` (looked at {})",
            auth_path().map(|p| p.display().to_string()).unwrap_or_default()
        )
    })?;
    if auth.device_token.is_empty() {
        bail!("stored device token is empty; re-run `rmapps auth login`");
    }
    Ok(auth.device_token)
}

pub fn run(args: AuthArgs) -> Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(async {
        match args.cmd {
            AuthCmd::Login { code } => login(code).await,
            AuthCmd::Status => status().await,
            AuthCmd::Logout => logout(),
        }
    })
}

async fn login(code: Option<String>) -> Result<()> {
    let code = match code {
        Some(c) => c,
        None => prompt(
            "Enter the 8-char code from https://my.remarkable.com/device/desktop/connect: ",
        )?,
    };
    let code = code.trim().to_string();
    if code.is_empty() {
        bail!("no code provided");
    }

    let http = reqwest::Client::new();
    let token = register_device(&http, &Config::from_env(), &code)
        .await
        .context("device pairing failed (is the code valid and unused?)")?;
    save(&StoredAuth {
        device_token: token.clone(),
    })?;
    println!("Paired. Stored device token at {}", auth_path()?.display());

    // Prove the token end-to-end: refresh a user token and list the cloud root.
    let client = Client::from_device_token(Config::from_env(), token);
    let entries = client
        .ls("")
        .await
        .context("paired, but listing the cloud root failed")?;
    println!("Verified against the live cloud — root has {} item(s).", entries.len());
    Ok(())
}

async fn status() -> Result<()> {
    let path = auth_path()?;
    if !path.exists() {
        println!("Not paired (no {}). Run `rmapps auth login`.", path.display());
        return Ok(());
    }
    let token = load_device_token()?;
    let client = Client::from_device_token(Config::from_env(), token);
    let entries = client
        .ls("")
        .await
        .context("device token was rejected by the cloud; re-run `rmapps auth login`")?;
    println!(
        "Paired and verified ({}). Root has {} item(s).",
        path.display(),
        entries.len()
    );
    Ok(())
}

fn logout() -> Result<()> {
    let path = auth_path()?;
    if path.exists() {
        std::fs::remove_file(&path)?;
        println!("Removed {}", path.display());
    } else {
        println!("Nothing to remove ({} does not exist).", path.display());
    }
    Ok(())
}

/// Atomically write `auth` with 0600 perms (dir 0700), so the token is never
/// world/group readable.
fn save(auth: &StoredAuth) -> Result<()> {
    let path = auth_path()?;
    let dir = path.parent().expect("auth path always has a parent");
    std::fs::create_dir_all(dir)?;
    let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));

    let bytes = serde_json::to_vec_pretty(auth)?;
    let tmp = dir.join("auth.json.tmp");
    {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(&bytes)?;
        f.flush()?;
    }
    std::fs::rename(&tmp, &path)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}

fn load() -> Result<StoredAuth> {
    let path = auth_path()?;
    let bytes = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn prompt(msg: &str) -> Result<String> {
    print!("{msg}");
    std::io::stdout().flush()?;
    let mut s = String::new();
    std::io::stdin().read_line(&mut s)?;
    Ok(s)
}
