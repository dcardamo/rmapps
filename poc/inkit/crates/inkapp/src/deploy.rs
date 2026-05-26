//! Device-agnostic on-device deployment facade. Apps resolve the `[device]`
//! backend (from `config.toml`) plus their own target folder, build a transport
//! via [`resolve_transport`], and pass it to [`publish`] / [`sync_once`]. This is
//! the only place a concrete device backend is named, so `inkapp-config` never
//! needs to depend on a `*-device` crate.

use std::future::Future;
use std::time::Duration;

use inkapp_core::connector::ConnectorSet;
use inkapp_core::error::{Error, Result};
use inkapp_core::runtime::{App, Cycle, DocSet};
use inkapp_core::secrets::SecretStore;
use inkapp_core::sync::{self, DeviceTransport};

use rm_device::CloudTransport;

/// Resolve a backend identifier + device folder + secret store into a concrete
/// transport. The single place backends are named; a new device family adds
/// one arm and one `*-device` crate. Errors on an unknown backend.
///
/// The reMarkable transport prefers a stored device token (paired via
/// [`crate::pair`]); it falls back to `RM_CLOUD_*` env vars for CI / one-shot use.
pub fn resolve_transport(
    backend: &str,
    folder: String,
    secrets: &SecretStore,
) -> Result<Box<dyn DeviceTransport>> {
    match backend {
        "remarkable" => Ok(Box::new(CloudTransport::from_secrets(secrets, folder)?)),
        other => Err(Error::Config(format!("unknown deploy backend {other:?}"))),
    }
}

/// Render the app's document set and push every document over the given transport.
pub async fn publish<M, Msg, Cx: ConnectorSet>(
    app: &mut App<M, Msg, Cx>,
    transport: &dyn DeviceTransport,
) -> Result<()> {
    let mut set = DocSet::default();
    sync::publish(app, &mut set, transport).await
}

/// Pull device ink, fold one cycle, and apply the resulting ops over the transport.
pub async fn sync_once<M, Msg: Clone, Cx: ConnectorSet>(
    app: &mut App<M, Msg, Cx>,
    transport: &dyn DeviceTransport,
) -> Result<Cycle<Msg>> {
    let mut set = DocSet::default();
    sync::sync_once(app, &mut set, transport).await
}

/// Publish the document set, then loop: every `interval`, run one `sync_once`
/// cycle (pull ink → fold → push/delete). Returns when `shutdown` resolves.
/// Apps usually pass `tokio::signal::ctrl_c()` as the shutdown future.
pub async fn serve<M, Msg, Cx: ConnectorSet>(
    app: &mut App<M, Msg, Cx>,
    transport: &dyn DeviceTransport,
    interval: Duration,
    shutdown: impl Future<Output = ()>,
) -> Result<()>
where
    Msg: Clone + std::fmt::Debug,
{
    let mut set = DocSet::default();
    sync::serve(app, &mut set, transport, interval, shutdown).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_store() -> (tempfile::TempDir, SecretStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = SecretStore::open(dir.path().join("secrets.json")).unwrap();
        (dir, store)
    }

    #[test]
    fn resolve_routes_known_and_rejects_unknown_backends() {
        let (_d, secrets) = empty_store();

        // Unknown backend → clear config error.
        match resolve_transport("supernote", "/X".into(), &secrets) {
            Err(e) => assert!(
                e.to_string().contains("unknown deploy backend"),
                "unexpected error: {e}"
            ),
            Ok(_) => panic!("an unknown backend must not resolve"),
        }

        // Known "remarkable": with no credentials in store OR env, this MUST fail —
        // but with a credential error, NOT the unknown-backend error.
        // SAFETY: single-threaded test; env vars cleared.
        std::env::remove_var("RM_CLOUD_DEVICE_TOKEN");
        std::env::remove_var("RM_CLOUD_USER_TOKEN");
        if let Err(e) = resolve_transport("remarkable", "/X".into(), &secrets) {
            assert!(
                !e.to_string().contains("unknown deploy backend"),
                "remarkable should be a known backend, got: {e}"
            );
        }
    }
}
