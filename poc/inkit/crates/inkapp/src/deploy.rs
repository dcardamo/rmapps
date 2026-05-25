//! Device-agnostic on-device deployment facade. Apps resolve the `[device]`
//! backend (from `config.toml`) plus their own target folder, build a transport
//! via [`resolve_transport`], and pass it to [`publish`] / [`sync_once`]. This is
//! the only place a concrete device backend is named, so `inkapp-config` never
//! needs to depend on a `*-device` crate.

use inkapp_core::connector::ConnectorSet;
use inkapp_core::error::{Error, Result};
use inkapp_core::runtime::{App, Cycle, DocSet};
use inkapp_core::sync::{self, DeviceTransport};

use rm_device::RmTransport;

/// Resolve a backend identifier + device folder into a concrete transport. The
/// single place backends are named; a new device family adds one arm and one
/// `*-device` crate. Errors on an unknown backend.
pub fn resolve_transport(backend: &str, folder: String) -> Result<Box<dyn DeviceTransport>> {
    match backend {
        "remarkable" => Ok(Box::new(RmTransport::new(folder))),
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
    fn resolve_known_and_unknown_backends() {
        assert!(resolve_transport("remarkable", "/X".into()).is_ok());
        assert!(resolve_transport("supernote", "/X".into()).is_err());
    }
}
