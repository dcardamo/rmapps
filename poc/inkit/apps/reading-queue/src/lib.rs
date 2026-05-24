//! The reading-queue app — `appdx`'s worked example, made real. Readwise is the
//! source of truth (a cassette-backed connector here), so the Model is empty.

pub mod serve;

use inkapp::{flow, Document, Documents};
use inkapp_core::component::Component;
use inkapp_core::ink::RegionInk;
use inkapp_core::manifest::Manifest;
use inkapp_core::widget::{RenderCx, Widget};
use inkapp_core::widgets::highlight_text::HighlightableText;
use inkapp_readwise::{Article, ArticleId, Readwise};
use std::sync::Arc;

use inkapp_core::connector::{Connector, ConnectorSet};

/// Re-export so the app's tests/wiring use one `Checkbox` path.
pub use inkapp_core::widgets::checkbox::Checkbox;

/// The Model: no own state — the queue and highlights live in Readwise.
pub struct App;

/// The things a user can do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Msg {
    Highlighted { article: ArticleId, text: String },
    Archived { article: ArticleId },
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

    pub fn from_cassette() -> Self {
        Connectors {
            readwise: Arc::new(Readwise::from_cassette()),
        }
    }

    pub fn persisted(path: impl Into<std::path::PathBuf>) -> Self {
        Connectors {
            readwise: Arc::new(Readwise::persisted(path)),
        }
    }

    /// Build from an existing shared connector (so two apps share one cache).
    pub fn from_arc(readwise: Arc<Readwise>) -> Self {
        Connectors { readwise }
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
        Msg::Archived { article } => cx.readwise.archive(&article),
    }
}

/// The complete document set: a sync-failure banner (only when writes failed)
/// followed by one document per queued article.
pub fn view(_m: &App, cx: &Connectors) -> Documents<Msg> {
    let mut docs: Vec<Document<Msg>> = Vec::new();

    let failed = cx.readwise.failed_writes();
    if !failed.is_empty() {
        docs.push(Document::keyed(
            "_banner",
            flow![Banner::new(&format!(
                "couldn't sync {} change(s) to Readwise",
                failed.len()
            ))],
        ));
    }

    for a in cx.readwise.queue() {
        let id = a.id.clone();
        docs.push(Document::keyed(
            id.0.clone(),
            flow![
                ArticleBody::new(&a),
                Checkbox::with_msg("done", Msg::Archived { article: id }).label("Archive"),
            ],
        ));
    }

    Documents(docs)
}

/// A bespoke, app-specific content component: renders the article body with its
/// existing highlights, and decodes freeform highlighter ink into `Highlighted`
/// messages (building the Msg directly — the appdx app-specific path).
pub struct ArticleBody {
    article: ArticleId,
    text: HighlightableText,
}

impl ArticleBody {
    pub fn new(a: &Article) -> Self {
        let tokens: Vec<&str> = a.body.split_whitespace().collect();
        Self {
            article: a.id.clone(),
            text: HighlightableText::with_highlights(&tokens, &a.highlights),
        }
    }
}

impl Component for ArticleBody {
    type Msg = Msg;

    fn render(&self, cx: &mut RenderCx) -> String {
        Widget::render(&self.text, cx)
    }

    fn decode(&self, ink: &[RegionInk], manifest: &Manifest) -> Vec<Msg> {
        self.text
            .read(ink, manifest)
            .into_iter()
            .map(|text| Msg::Highlighted {
                article: self.article.clone(),
                text,
            })
            .collect()
    }
}

/// A Display-mode banner: renders a line of text, decodes nothing. Used to
/// surface connector write failures (the framework owns no presentation).
pub struct Banner {
    text: String,
}

impl Banner {
    pub fn new(text: &str) -> Self {
        Self {
            text: text.to_string(),
        }
    }
}

impl Component for Banner {
    type Msg = Msg;

    fn render(&self, _cx: &mut RenderCx) -> String {
        // Inject as a Typst string expression so `[`, `]`, `#` in arbitrary
        // banner text stay literal (only `\` and `"` need escaping for the
        // string literal). Keeps the content block from breaking on user text.
        let t = self.text.replace('\\', "\\\\").replace('"', "\\\"");
        format!("#text(fill: red)[#\"{t}\"]\n")
    }

    fn decode(&self, _ink: &[RegionInk], _manifest: &Manifest) -> Vec<Msg> {
        vec![]
    }
}
