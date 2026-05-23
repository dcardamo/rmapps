//! inkapp-core — the device-agnostic framework: render, manifest, widgets,
//! readback, and the minimal `Device` seam.

pub mod error;
pub mod render;
pub mod world;

pub use error::{Error, Result};
