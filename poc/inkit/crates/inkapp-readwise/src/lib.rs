//! Cassette-backed Readwise connector. Reads real-shaped article data from a
//! committed cassette; writes (archive / add highlight) are recorded in a
//! working overlay and merged back into reads — so the loop behaves for real
//! without touching a live account. No network here; the live refresh is a
//! manual `#[ignore]` bar (see the reading-queue crate).

use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// A Readwise article id.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ArticleId(pub String);

impl ArticleId {
    pub fn new(s: impl Into<String>) -> Self {
        ArticleId(s.into())
    }
}

/// An article: its id, title, body text, and highlighted spans.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Article {
    pub id: ArticleId,
    pub title: String,
    pub body: String,
    pub highlights: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct Cassette {
    articles: Vec<Article>,
}

#[derive(Default)]
struct Overlay {
    archived: Vec<ArticleId>,
    added: Vec<(ArticleId, String)>,
}

/// The connector. Reads are immutable (the cassette); writes mutate the overlay
/// behind a `Mutex` so methods take `&self` (as a shared `Arc<Readwise>` will).
pub struct Readwise {
    cassette: Vec<Article>,
    overlay: Mutex<Overlay>,
}

impl Readwise {
    /// Load from the committed cassette JSON.
    pub fn from_cassette() -> Self {
        let raw = include_str!("../fixtures/cassette/articles.json");
        let c: Cassette = serde_json::from_str(raw).expect("valid committed cassette");
        Self {
            cassette: c.articles,
            overlay: Mutex::new(Overlay::default()),
        }
    }

    /// A tiny inline cassette for unit tests (no committed file dependency).
    pub fn fake() -> Self {
        let articles = vec![
            Article {
                id: ArticleId::new("a1"),
                title: "One".into(),
                body: "the slow web rewards patience".into(),
                highlights: vec![],
            },
            Article {
                id: ArticleId::new("a2"),
                title: "Two".into(),
                body: "ink survives the round trip".into(),
                highlights: vec![],
            },
        ];
        Self {
            cassette: articles,
            overlay: Mutex::new(Overlay::default()),
        }
    }

    /// The current queue: cassette articles minus archived, with overlay
    /// highlights merged in.
    pub fn queue(&self) -> Vec<Article> {
        let ov = self.overlay.lock().unwrap();
        self.cassette
            .iter()
            .filter(|a| !ov.archived.contains(&a.id))
            .map(|a| {
                let mut a = a.clone();
                for (id, text) in &ov.added {
                    if id == &a.id && !a.highlights.contains(text) {
                        a.highlights.push(text.clone());
                    }
                }
                a
            })
            .collect()
    }

    /// Record an archive (recorded, returns nothing — appdx write shape).
    pub fn archive(&self, id: &ArticleId) {
        let mut ov = self.overlay.lock().unwrap();
        if !ov.archived.contains(id) {
            ov.archived.push(id.clone());
        }
    }

    /// Record a highlight (idempotent: a repeated (id, text) is recorded once,
    /// so `highlights()` and `queue()` agree).
    pub fn add_highlight(&self, id: &ArticleId, text: &str) {
        let mut ov = self.overlay.lock().unwrap();
        if !ov.added.iter().any(|(i, t)| i == id && t == text) {
            ov.added.push((id.clone(), text.to_string()));
        }
    }

    /// The archived ids (for assertions / surfacing).
    pub fn archived(&self) -> Vec<ArticleId> {
        self.overlay.lock().unwrap().archived.clone()
    }

    /// The recorded highlight texts for one article.
    pub fn highlights(&self, id: &ArticleId) -> Vec<String> {
        self.overlay
            .lock()
            .unwrap()
            .added
            .iter()
            .filter(|(i, _)| i == id)
            .map(|(_, t)| t.clone())
            .collect()
    }
}
