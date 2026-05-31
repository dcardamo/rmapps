use crate::schema::ConfigSchema;

/// Where a config section lives in the file's table hierarchy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Namespace {
    /// Top-level `[<kind>]` (e.g. `[page]`); instance is ignored.
    Framework,
    /// `[connector.<kind>.<instance>]`.
    Connector,
    /// `[app.<kind>.<instance>]`.
    App,
}

impl Namespace {
    /// The leading path segment (empty for `Framework`, which is top-level).
    pub fn prefix(self) -> &'static str {
        match self {
            Namespace::Framework => "",
            Namespace::Connector => "connector",
            Namespace::App => "app",
        }
    }
    /// The namespace label used in diagnostics (e.g. error messages). Differs
    /// from [`prefix`](Self::prefix) for `Framework`: `prefix` is "" (top-level
    /// `[<kind>]` tables) while this returns "framework" for human-readable text.
    pub fn as_str(self) -> &'static str {
        match self {
            Namespace::Framework => "framework",
            Namespace::Connector => "connector",
            Namespace::App => "app",
        }
    }
}

/// A typed configuration section. Implemented by `#[derive(Config)]`; the
/// derive registers a `ConfigSchema` (via `inventory`) and emits `Default`
/// with per-field defaults. Authors add `#[derive(serde::Deserialize)]` +
/// `#[serde(default)]` to the same struct so absent TOML fields fall through
/// to the generated `Default`.
pub trait Config: serde::de::DeserializeOwned + Default {
    const KIND: &'static str;
    const NAMESPACE: Namespace;
    /// The registered schema for this kind (looked up from the registry).
    fn schema() -> &'static ConfigSchema {
        crate::schema::Registry::find(Self::NAMESPACE, Self::KIND)
            .expect("every Config type registers its schema")
    }
}
