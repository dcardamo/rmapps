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

/// One document: a key plus an ordered flow of components.
pub struct Document<M> {
    pub key: DocKey,
    pub flow: Vec<Box<dyn Component<Msg = M>>>,
}

impl<M> Document<M> {
    pub fn keyed(key: impl Into<String>, flow: Vec<Box<dyn Component<Msg = M>>>) -> Self {
        Self {
            key: DocKey::new(key),
            flow,
        }
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
