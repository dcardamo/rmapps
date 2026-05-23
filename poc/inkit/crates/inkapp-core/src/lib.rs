//! inkapp-core — the device-agnostic framework: render, manifest, widgets,
//! readback, and the minimal `Device` seam.

pub mod embed;
pub mod error;
pub mod geometry;
pub mod ink;
pub mod manifest;
pub mod render;
pub mod widget;
pub mod widgets;
pub mod world;

pub use error::{Error, Result};
pub use geometry::{DevicePoint, PdfPoint, PdfRect};
pub use manifest::{Manifest, Region};
