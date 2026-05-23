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

/// The app's connectors (one connector this slice — a concrete struct, no
/// framework codegen).
pub struct Connectors {
    pub readwise: Readwise,
}

impl Connectors {
    pub fn fake() -> Self {
        Connectors {
            readwise: Readwise::fake(),
        }
    }

    pub fn from_cassette() -> Self {
        Connectors {
            readwise: Readwise::from_cassette(),
        }
    }
}

/// The only place app logic lives: mutate state (none) and call connectors.
pub fn update(msg: Msg, _m: &mut App, cx: &Connectors) {
    match msg {
        Msg::Highlighted { article, text } => cx.readwise.add_highlight(&article, &text),
        Msg::Archived { article } => cx.readwise.archive(&article),
    }
}

/// The complete document set: one per queued article.
pub fn view(_m: &App, cx: &Connectors) -> Documents<Msg> {
    Documents(
        cx.readwise
            .queue()
            .into_iter()
            .map(|a| -> Document<Msg> {
                let id = a.id.clone();
                Document::keyed(
                    id.0.clone(),
                    flow![
                        ArticleBody::new(&a),
                        Checkbox::with_msg("done", Msg::Archived { article: id }).label("Archive"),
                    ],
                )
            })
            .collect::<Vec<_>>(),
    )
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
