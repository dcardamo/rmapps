//! inkapp-core — the device-agnostic framework: render, manifest, widgets,
//! readback, and the minimal `Device` seam.

pub mod component;
pub mod device;
pub mod document;
pub mod embed;
pub mod error;
pub mod geometry;
pub mod ink;
pub mod manifest;
pub mod readback;
pub mod reconcile;
pub mod render;
pub mod runtime;
pub mod widget;
pub mod widgets;
pub mod world;

pub use component::Component;
pub use device::Device;
pub use document::{DocKey, Document, Documents};
pub use error::{Error, Result};
pub use geometry::{DevicePoint, PdfPoint, PdfRect};
pub use manifest::{Manifest, Region};
pub use reconcile::{reconcile, DocOp};
pub use runtime::{app, render_document, App, Cycle, DocSet, RenderedDoc};
