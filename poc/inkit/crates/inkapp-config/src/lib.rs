//! End-user configuration for inkapp. See the spec:
//! docs/superpowers/specs/2026-05-25-configuration-design.md
//!
//! `extern crate self as inkapp_config` lets `#[derive(Config)]` emit
//! `::inkapp_config::…` paths that resolve even inside this crate.
extern crate self as inkapp_config;

#[cfg(feature = "cli")]
pub mod cli;
pub mod config;
pub mod error;
pub mod refs;
pub mod schema;
pub mod store;

pub use config::{Config, Namespace};
pub use error::{ConfigError, Result};
pub use refs::{ConnectorRef, SecretRef};
pub use schema::{ConfigSchema, FieldKind, FieldSchema, Registry};
pub use store::ConfigStore;

// The `Config` derive macro and `inventory` (the derive emits
// `::inkapp_config::inventory::...` paths, so it must be reachable here).
pub use inkapp_config_derive::Config;
pub use inventory;
