//! The Reader app — Library.pdf + Feed.pdf with a per-page ActionBand.
//! For v1 the `view` returns an empty Documents set; task 6 wires the full
//! composition. This task scaffolds the crate + CLI only.

use std::sync::Arc;

use inkapp::Documents;
use inkapp_core::connector::{Connector, ConnectorSet};
use inkapp_readwise_reader::{ArticleId, Location, Readwise};

/// The Model: no own state — the queue and highlights live in Readwise.
pub struct App;

/// The things a user can do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Msg {
    Highlighted { article: ArticleId, text: String },
    Move        { article: ArticleId, to: Location },
    Delete      { article: ArticleId },
}

/// The reader app's own config section.
#[derive(Debug, Clone, serde::Deserialize, inkapp_config::Config)]
#[serde(default)]
#[config(kind = "reader", namespace = "app")]
pub struct AppConfig {
    /// On-device folder path for this instance's documents (device-neutral).
    #[config(default = String::from("/Reader"))]
    pub device_folder: String,
    /// Which Readwise connector instance to bind ("readwise.<instance>").
    #[config(default = inkapp_config::ConnectorRef { kind: "readwise".into(), instance: "main".into() })]
    pub readwise: inkapp_config::ConnectorRef,
}

/// The app's connectors (one connector this slice). Held as `Arc<Readwise>` so a
/// connector — and its cache — can be shared across apps.
pub struct Connectors {
    pub readwise: Arc<Readwise>,
}

impl Connectors {
    pub fn fake() -> Self {
        Connectors {
            readwise: Arc::new(Readwise::fake()),
        }
    }

    /// Build connectors from config: resolve the bound Readwise instance and
    /// construct it (token from `secrets`, durable cache under `cache_dir`).
    pub async fn from_config(
        store: &inkapp_config::ConfigStore,
        app: &AppConfig,
        secrets: &inkapp_core::secrets::SecretStore,
        cache_dir: std::path::PathBuf,
    ) -> Result<Self, inkapp_config::ConfigError> {
        use inkapp_config::Namespace;
        let rw = &app.readwise;
        store.require_instance(Namespace::Connector, &rw.kind, &rw.instance)?;
        let cfg: inkapp_readwise_reader::ReaderConfig = store.resolve(&rw.instance)?;
        let conn = Readwise::from_config(cfg, secrets, cache_dir)
            .await
            .map_err(|e| inkapp_config::ConfigError::Connector(e.to_string()))?;
        Ok(Connectors {
            readwise: Arc::new(conn),
        })
    }
}

impl ConnectorSet for Connectors {
    fn connectors(&self) -> Vec<Arc<dyn Connector>> {
        vec![self.readwise.clone()]
    }
}

/// The only place app logic lives: mutate state (none) and call connectors.
pub fn update(msg: Msg, _m: &mut App, cx: &Connectors) {
    match msg {
        Msg::Highlighted { article, text } => cx.readwise.add_highlight(&article, &text),
        Msg::Move        { article, to }   => cx.readwise.move_to(&article, to),
        Msg::Delete      { article }       => cx.readwise.delete(&article),
    }
}

/// Stub view — task 6 replaces this with the full Library + Feed composition.
pub fn view(_m: &App, _cx: &Connectors) -> Documents<Msg> {
    Documents(Vec::new())
}
