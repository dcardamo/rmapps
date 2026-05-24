//! inkapp-core — the device-agnostic framework: render, manifest, components,
//! readback, and the minimal `Device` seam.

pub mod calendar;
pub mod component;
pub mod components;
pub mod connector;
pub mod crypto;
pub mod device;
pub mod document;
pub mod embed;
pub mod error;
pub mod geometry;
pub mod ink;
pub mod manifest;
pub mod mode;
pub mod readback;
pub mod reconcile;
pub mod render;
pub mod runtime;
pub mod secrets;
pub mod single_flight;
pub mod widget;
pub mod world;

pub use calendar::EventRow;
pub use component::Component;
pub use connector::{Connector, ConnectorError, ConnectorSet};
pub use crypto::{open, seal, Key};
pub use device::Device;
pub use document::{DocKey, Document, Documents};
pub use error::{Error, Result};
pub use geometry::{DevicePoint, PdfPoint, PdfRect};
pub use manifest::{Manifest, Region};
pub use mode::Mode;
pub use reconcile::{reconcile, DocOp};
pub use runtime::{
    app, collect_typst_sources, compile_document, document_source, render_document, App, Cycle,
    DocSet, RenderedDoc, REGION_PRELUDE,
};
pub use secrets::{Scope, SecretStore};
pub use single_flight::SingleFlight;
