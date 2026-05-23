//! Keyed document-set reconciliation: diff the previous set against the next by
//! key, so the framework creates/updates/deletes documents and preserves ink on
//! surviving keys.

use std::collections::{HashMap, HashSet};

use crate::document::DocKey;

/// One reconciliation operation against the device's document set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocOp {
    Create(DocKey),
    Update(DocKey),
    Delete(DocKey),
}

/// Diff `prev` against `next`, each a list of `(key, content_hash)`.
/// - key in next only            -> Create
/// - key in both, hash differs   -> Update
/// - key in both, hash equal     -> no-op (omitted)
/// - key in prev only            -> Delete
///
/// Order is deterministic: creates/updates in `next` order, then deletes in
/// `prev` order.
pub fn reconcile(prev: &[(DocKey, u64)], next: &[(DocKey, u64)]) -> Vec<DocOp> {
    let prev_map: HashMap<&str, u64> = prev.iter().map(|(k, h)| (k.0.as_str(), *h)).collect();
    let next_keys: HashSet<&str> = next.iter().map(|(k, _)| k.0.as_str()).collect();

    let mut ops = Vec::new();
    for (k, h) in next {
        match prev_map.get(k.0.as_str()) {
            None => ops.push(DocOp::Create(k.clone())),
            Some(&ph) if ph != *h => ops.push(DocOp::Update(k.clone())),
            Some(_) => {} // unchanged -> no-op
        }
    }
    for (k, _) in prev {
        if !next_keys.contains(k.0.as_str()) {
            ops.push(DocOp::Delete(k.clone()));
        }
    }
    ops
}
