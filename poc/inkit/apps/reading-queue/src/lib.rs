//! The reading-queue app — `appdx`'s worked example, made real. Readwise is the
//! source of truth (a cassette-backed connector here), so the Model is empty.

use inkapp::{flow, Document, Documents};
use inkapp_core::component::Component;
use inkapp_core::component::RenderCx;
use inkapp_core::components::highlight_text::HighlightableText;
use inkapp_core::components::notice::Notice;
use inkapp_core::ink::RegionInk;
use inkapp_core::manifest::Manifest;
use inkapp_readwise_reader::{Article, ArticleId, Readwise};
use std::sync::Arc;

use inkapp_core::connector::{Connector, ConnectorSet};

/// Re-export so the app's tests/wiring use one `Checkbox` path.
pub use inkapp_core::components::checkbox::Checkbox;

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

/// The complete document set: a sync-failure notice (only when writes failed)
/// followed by one document per queued article. The notice is the framework's
/// reusable `Notice` display component — the app supplies the text from
/// `failed_writes()`; the component never touches connectors.
pub fn view(_m: &App, cx: &Connectors) -> Documents<Msg> {
    let mut docs: Vec<Document<Msg>> = Vec::new();

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

/// A bespoke, app-specific content component. Renders real article HTML via the
/// content pipeline when present (decoding to coalesced highlight spans), and
/// falls back to whitespace-split plaintext for articles without `html_content`.
enum Body {
    Html(inkapp_content::Article<Msg>),
    Plain(HighlightableText),
}

pub struct ArticleBody {
    article: ArticleId,
    body: Body,
}

impl ArticleBody {
    pub fn new(a: &Article) -> Self {
        let body = match a.html_content.as_deref() {
            Some(html) if !html.trim().is_empty() => {
                let id = a.id.clone();
                Body::Html(inkapp_content::Article::new(
                    html,
                    &a.highlights,
                    move |s| Msg::Highlighted {
                        article: id.clone(),
                        text: s.to_string(),
                    },
                ))
            }
            _ => {
                let tokens: Vec<&str> = a.body.split_whitespace().collect();
                Body::Plain(HighlightableText::with_highlights(&tokens, &a.highlights))
            }
        };
        Self {
            article: a.id.clone(),
            body,
        }
    }
}

impl Component for ArticleBody {
    type Msg = Msg;

    fn render(&self, cx: &mut RenderCx) -> String {
        match &self.body {
            Body::Html(a) => a.render(cx),
            Body::Plain(h) => h.render(cx),
        }
    }

    fn decode(&self, ink: &[RegionInk], manifest: &Manifest) -> Vec<Msg> {
        match &self.body {
            Body::Html(a) => a.decode(ink, manifest),
            Body::Plain(h) => h
                .read(ink, manifest)
                .into_iter()
                .map(|text| Msg::Highlighted {
                    article: self.article.clone(),
                    text,
                })
                .collect(),
        }
    }
}
