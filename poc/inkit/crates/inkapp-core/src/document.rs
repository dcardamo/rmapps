//! The keyed document set a `view` returns. A `Document` is a stable key plus a
//! flow of boxed components sharing one app `Msg`; the framework diffs the set
//! against the device by key (create/update/delete).

use crate::component::Component;

/// App-stable identity for a document (e.g. an article id). The reconciliation
/// key that preserves ink across re-renders.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DocKey(pub String);

impl DocKey {
    pub fn new(s: impl Into<String>) -> Self {
        DocKey(s.into())
    }
}

/// One document: a key plus an ordered flow of components, plus optional
/// app-owned document-level state carried in the sealed manifest, plus an
/// optional per-page header component (rendered on every page).
pub struct Document<M> {
    pub key: DocKey,
    pub flow: Vec<Box<dyn Component<Msg = M>>>,
    pub state: Option<serde_json::Value>,
    pub page_header: Option<Box<dyn Component<Msg = M>>>,
}

impl<M> Document<M> {
    pub fn keyed(key: impl Into<String>, flow: Vec<Box<dyn Component<Msg = M>>>) -> Self {
        Self {
            key: DocKey::new(key),
            flow,
            state: None,
            page_header: None,
        }
    }

    /// Like `keyed`, but carries document-level state sealed into the manifest.
    pub fn keyed_with_state(
        key: impl Into<String>,
        flow: Vec<Box<dyn Component<Msg = M>>>,
        state: serde_json::Value,
    ) -> Self {
        Self {
            key: DocKey::new(key),
            flow,
            state: Some(state),
            page_header: None,
        }
    }

    /// Attach a per-page header component. The framework wires its render into
    /// `#set page(header: ...)` and calls its `decode` alongside the flow decode.
    #[must_use]
    pub fn page_header(mut self, header: impl Component<Msg = M> + 'static) -> Self {
        self.page_header = Some(Box::new(header));
        self
    }
}

/// The complete set of documents that should exist.
pub struct Documents<M>(pub Vec<Document<M>>);

/// Build a component flow: `flow![a, b, c]` -> `Vec<Box<dyn Component<Msg = _>>>`.
/// The `Msg` is inferred from the surrounding `Document<M>`.
#[macro_export]
macro_rules! flow {
    ($($c:expr),* $(,)?) => {
        vec![ $( ::std::boxed::Box::new($c) as ::std::boxed::Box<dyn $crate::component::Component<Msg = _>> ),* ]
    };
}
