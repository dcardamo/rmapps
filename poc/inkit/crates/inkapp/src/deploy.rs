//! Config-driven, device-agnostic on-device deployment. Apps call
//! `inkapp::publish` / `inkapp::sync_once`; the backend and target folder come
//! from a `deploy.toml` located via the `INKAPP_DEPLOY_CONFIG` env var. This is
//! the only place a concrete device backend is named.

use std::path::Path;

use inkapp_core::connector::ConnectorSet;
use inkapp_core::error::{Error, Result};
use inkapp_core::runtime::{App, Cycle, DocSet};
use inkapp_core::sync::{self, DeviceTransport};

use rm_device::CloudTransport;

/// Env var naming the path to the deploy TOML.
const CONFIG_ENV: &str = "INKAPP_DEPLOY_CONFIG";

fn default_backend() -> String {
    "remarkable".to_string()
}

/// Deployment configuration: which device backend, and the device folder this
/// app's documents live under.
#[derive(Debug, serde::Deserialize)]
pub struct DeployConfig {
    #[serde(default = "default_backend")]
    pub backend: String,
    pub folder: String,
}

impl DeployConfig {
    /// Parse a `DeployConfig` from TOML text.
    pub fn from_toml(text: &str) -> Result<Self> {
        toml::from_str(text).map_err(|e| Error::Config(format!("parse deploy config: {e}")))
    }

    /// Load from the file named by `INKAPP_DEPLOY_CONFIG`.
    pub fn from_env() -> Result<Self> {
        let path = std::env::var(CONFIG_ENV)
            .map_err(|_| Error::Config(format!("{CONFIG_ENV} is not set")))?;
        Self::from_path(path)
    }

    fn from_path(path: impl AsRef<Path>) -> Result<Self> {
        let text = std::fs::read_to_string(path.as_ref())
            .map_err(|e| Error::Config(format!("read deploy config {:?}: {e}", path.as_ref())))?;
        Self::from_toml(&text)
    }
}

/// Resolve a config into a concrete transport. The single place backends are
/// named; a new device family adds one arm and one `*-device` crate.
fn resolve(cfg: &DeployConfig) -> Result<Box<dyn DeviceTransport>> {
    match cfg.backend.as_str() {
        "remarkable" => Ok(Box::new(CloudTransport::from_env(cfg.folder.clone())?)),
        other => Err(Error::Config(format!("unknown deploy backend {other:?}"))),
    }
}

/// Render the app's document set and push every document to the configured device.
pub async fn publish<M, Msg, Cx: ConnectorSet>(app: &mut App<M, Msg, Cx>) -> Result<()> {
    let transport = resolve(&DeployConfig::from_env()?)?;
    let mut set = DocSet::default();
    sync::publish(app, &mut set, transport.as_ref()).await
}

/// Pull device ink, fold one cycle, and apply the resulting ops to the device.
pub async fn sync_once<M, Msg: Clone, Cx: ConnectorSet>(
    app: &mut App<M, Msg, Cx>,
) -> Result<Cycle<Msg>> {
    let transport = resolve(&DeployConfig::from_env()?)?;
    let mut set = DocSet::default();
    sync::sync_once(app, &mut set, transport.as_ref()).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_routes_known_and_rejects_unknown_backends() {
        // An unknown backend is rejected with a clear config error.
        let bad = DeployConfig {
            backend: "supernote".into(),
            folder: "/X".into(),
        };
        match resolve(&bad) {
            Err(e) => assert!(
                e.to_string().contains("unknown deploy backend"),
                "unexpected error: {e}"
            ),
            Ok(_) => panic!("an unknown backend must not resolve"),
        }

        // The known "remarkable" backend routes to the cloud transport. We assert
        // routing (not a live connection): resolving may still fail downstream if
        // no cloud credentials are present, but it must NOT be the unknown-backend
        // error above.
        let ok = DeployConfig {
            backend: "remarkable".into(),
            folder: "/X".into(),
        };
        if let Err(e) = resolve(&ok) {
            assert!(
                !e.to_string().contains("unknown deploy backend"),
                "remarkable should be a known backend"
            );
        }
    }
}
