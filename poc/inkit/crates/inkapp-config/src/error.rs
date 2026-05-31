//! Configuration errors. Distinguished so the CLI and connectors can react
//! (a missing file is fine → defaults; a malformed/unknown key is loud).

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("config io error: {0}")]
    Io(String),
    #[error("config parse error: {0}")]
    Parse(String),
    /// An unknown key inside a section whose schema we *do* know (typo guard).
    #[error("unknown key '{key}' in [{section}] (known keys: {known})")]
    UnknownKey {
        section: String,
        key: String,
        known: String,
    },
    /// A required value (e.g. a SecretRef whose secret is absent) is missing.
    #[error("missing required config '{0}'")]
    Missing(String),
    /// Connector construction failed (auth, transport, cache open, …).
    #[error("connector construction failed: {0}")]
    Connector(String),
    /// A ConnectorRef points at an instance that is not configured.
    #[error("no [{namespace}.{kind}.{instance}] configured (available: {available})")]
    NoSuchInstance {
        namespace: String,
        kind: String,
        instance: String,
        available: String,
    },
    #[error("malformed connector reference '{0}' (expected \"kind.instance\")")]
    BadConnectorRef(String),
}

pub type Result<T> = std::result::Result<T, ConfigError>;
