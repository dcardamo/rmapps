//! inkapp — the app-authoring facade. Re-exports the framework surface from
//! `inkapp-core` plus the default reMarkable device, so apps read as in the docs.

pub use inkapp_core::component::Component;
pub use inkapp_core::connector::{Connector, ConnectorError, ConnectorSet};
pub use inkapp_core::crypto::Key;
pub use inkapp_core::device::Device;
pub use inkapp_core::document::{DocKey, Document, Documents};
pub use inkapp_core::manifest::{Manifest, Region};
pub use inkapp_core::runtime::{
    app, collect_typst_sources, compile_document, document_source, render_document, App, Cycle,
    DocSet, RenderedDoc, REGION_PRELUDE,
};
pub use inkapp_core::secrets::{Scope, SecretStore};
pub use inkapp_core::single_flight::SingleFlight;
pub use inkapp_core::{components, flow};

mod deploy;
pub use deploy::{publish, sync_once, DeployConfig};
pub use inkapp_core::sync::DeviceTransport;

pub use rm_device::Remarkable;
