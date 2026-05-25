//! Device-agnostic on-device deployment facade. Apps resolve the `[device]`
//! backend (from `config.toml`) plus their own target folder, build a transport
//! via [`resolve_transport`], and pass it to [`publish`] / [`sync_once`]. This is
//! the only place a concrete device backend is named, so `inkapp-config` never
//! needs to depend on a `*-device` crate.

use inkapp_core::connector::ConnectorSet;
use inkapp_core::error::{Error, Result};
use inkapp_core::runtime::{App, Cycle, DocSet};
use inkapp_core::sync::{self, DeviceTransport};

use rm_device::CloudTransport;

/// Resolve a backend identifier + device folder into a concrete transport. The
/// single place backends are named; a new device family adds one arm and one
/// `*-device` crate. Errors on an unknown backend.
pub fn resolve_transport(backend: &str, folder: String) -> Result<Box<dyn DeviceTransport>> {
    match backend {
        // The reMarkable transport talks to the cloud natively via `rm-cloud`
        // (credentials from `RM_CLOUD_DEVICE_TOKEN` / `RM_CLOUD_USER_TOKEN`).
        "remarkable" => Ok(Box::new(CloudTransport::from_env(folder)?)),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_routes_known_and_rejects_unknown_backends() {
        // An unknown backend is rejected with a clear config error.
        match resolve_transport("supernote", "/X".into()) {
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
        if let Err(e) = resolve_transport("remarkable", "/X".into()) {
            assert!(
                !e.to_string().contains("unknown deploy backend"),
                "remarkable should be a known backend"
            );
        }
    }
}
