//! inkapp — the app-authoring facade. Re-exports the framework surface from
//! `inkapp-core` plus the default reMarkable device, so apps read as in the docs.

pub use inkapp_core::assets::{
    asset_key, asset_path, resolve_assets, AssetMap, FakeFetcher, HttpImageFetcher, ImageFetcher,
    OfflineFetcher, PLACEHOLDER_PNG,
};
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
pub use inkapp_core::Theme;
pub use inkapp_core::{components, flow};

mod deploy;
pub use deploy::{publish, resolve_transport, sync_once};
pub use inkapp_core::geometry::DeviceConfig;
pub use inkapp_core::sync::DeviceTransport;

pub use rm_device::Remarkable;

pub use inkapp_config::store::{select_instance, ConfigStore};
pub use inkapp_config::{cli, Config, ConfigError, ConnectorRef, Namespace, SecretRef};
