//! The Reader app — Library.pdf + Feed.pdf with a per-page ActionBand.

use std::sync::Arc;

use inkapp::{flow, Document, Documents};
use inkapp_content::Article as ContentArticle;
use inkapp_core::component::Component;
use inkapp_core::components::action_band::ActionBand;
use inkapp_core::components::heading::Heading;
use inkapp_core::components::index::{Index, IndexEntry};
use inkapp_core::components::nav_band::NavBand;
use inkapp_core::components::notice::Notice;
use inkapp_core::components::section::Section;
use inkapp_core::components::stack::Stack;
use inkapp_core::connector::{Connector, ConnectorSet};
use inkapp_readwise_reader::{Article as ApiArticle, ArticleId, Location, Readwise};

/// The Model: no own state — the queue and highlights live in Readwise.
pub struct App;

/// The things a user can do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Msg {
    Highlighted { article: ArticleId, text: String },
    Move { article: ArticleId, to: Location },
    Delete { article: ArticleId },
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
        Msg::Move { article, to } => cx.readwise.move_to(&article, to),
        Msg::Delete { article } => cx.readwise.delete(&article),
    }
}

/// Build the four-cell ActionBand header for every page in a collection document.
/// Labels must not contain '-' (validated by ActionBand::new).
fn action_band() -> ActionBand<Msg> {
    ActionBand::new([
        (
            "Inbox".to_string(),
            Box::new(|id: &str| Msg::Move {
                article: ArticleId::new(id),
                to: Location::New,
            }) as Box<dyn Fn(&str) -> Msg + Send + Sync>,
        ),
        (
            "Archive".to_string(),
            Box::new(|id: &str| Msg::Move {
                article: ArticleId::new(id),
                to: Location::Archive,
            }) as Box<dyn Fn(&str) -> Msg + Send + Sync>,
        ),
        (
            "Later".to_string(),
            Box::new(|id: &str| Msg::Move {
                article: ArticleId::new(id),
                to: Location::Later,
            }) as Box<dyn Fn(&str) -> Msg + Send + Sync>,
        ),
        (
            "Delete".to_string(),
            Box::new(|id: &str| Msg::Delete {
                article: ArticleId::new(id),
            }) as Box<dyn Fn(&str) -> Msg + Send + Sync>,
        ),
    ])
}

/// Build the Heading for an article: title + byline (author preferred, site_name
/// fallback) + optional reading time. Returns `Heading<Msg>` so it can be placed
/// directly in a `Section<Msg>` body — no adaptor needed.
fn heading_for(a: &ApiArticle) -> Heading<Msg> {
    let mut h = Heading::<Msg>::new(a.title.clone());
    let byline = if !a.author.is_empty() {
        Some(a.author.clone())
    } else if !a.site_name.is_empty() {
        Some(a.site_name.clone())
    } else {
        None
    };
    if let Some(b) = byline {
        h = h.byline(b);
    }
    if let Some(rt) = a.reading_time.as_deref().filter(|s| !s.is_empty()) {
        h = h.reading_time(rt);
    }
    h
}

/// Build the content Article body wired with an on-highlight closure. The
/// article id (with a trailing `-`) is passed as the token-region prefix so
/// each article's `tok-N` regions are uniquely namespaced — critical now
/// that one Document holds many Articles. Without this, the manifest lookup
/// `find(name == "tok-N")` returns the first Article's region, silently
/// misattributing every highlight downstream.
fn article_body(a: &ApiArticle) -> ContentArticle<Msg> {
    let id = a.id.clone();
    let prefix = format!("{}-", a.id.0);
    ContentArticle::new_with_prefix(
        a.html_content.as_deref().unwrap_or(""),
        &a.highlights,
        &prefix,
        move |s| Msg::Highlighted {
            article: id.clone(),
            text: s.to_string(),
        },
    )
}

/// Build a collection Document from a slice of articles; returns `None` when the
/// slice is empty (so the Document is simply omitted from the set).
fn collection_doc(key: &str, articles: Vec<ApiArticle>) -> Option<Document<Msg>> {
    if articles.is_empty() {
        return None;
    }

    let entries: Vec<IndexEntry> = articles.iter().map(IndexEntry::from).collect();

    // Start the flow with the index page. The DocKey doubles as the masthead
    // title ("Library" / "Feed") — matches the old rmreader masthead.
    let mut items: Vec<Box<dyn Component<Msg = Msg>>> =
        vec![Box::new(Index::<Msg>::new(entries).with_title(key))];

    // One Section per article: Heading + Article body.
    for a in &articles {
        let section_body: Vec<Box<dyn Component<Msg = Msg>>> =
            vec![Box::new(heading_for(a)), Box::new(article_body(a))];
        items.push(Box::new(Section::<Msg>::new(&a.id.0, section_body)));
    }

    // Build the page header: NavBand (Prev / Home / Next, baked with the
    // ordered article ids in this collection) ABOVE the ActionBand
    // (Inbox/Archive/Later/Delete, drawn only on article pages). Combined
    // via Stack so the framework's single `page_header` slot can hold both.
    let order: Vec<String> = articles.iter().map(|a| a.id.0.clone()).collect();
    let header: Stack<Msg> = Stack::new(vec![
        Box::new(NavBand::<Msg>::new(order)),
        Box::new(action_band()),
    ]);
    Some(Document::keyed(key, items).page_header(header))
}

/// The view: Library + Feed Documents, with an optional sync-failure banner prepended.
pub fn view(_m: &App, cx: &Connectors) -> Documents<Msg> {
    let mut docs: Vec<Document<Msg>> = Vec::new();

    // Prepend a banner when previous writes failed (mirrors reading-queue pattern).
    let failed = cx.readwise.failed_writes();
    if !failed.is_empty() {
        docs.push(Document::keyed(
            "_banner",
            flow![Notice::line(&format!(
                "couldn't sync {} change(s) to Readwise",
                failed.len()
            ))],
        ));
    }

    if let Some(d) = collection_doc("Library", cx.readwise.library()) {
        docs.push(d);
    }
    if let Some(d) = collection_doc("Feed", cx.readwise.feed()) {
        docs.push(d);
    }

    Documents(docs)
}
