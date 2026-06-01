//! Pure reconcile: snapshot-hash diff and folder-prefix routing. No I/O.
use crate::config::WatchAction;
use std::collections::BTreeMap;
use std::time::Duration;

/// A watch rule with its debounce parsed.
#[derive(Clone)]
#[allow(dead_code)]
pub struct ResolvedRule {
    pub path: String,
    pub action: WatchAction,
    pub debounce: Duration,
}

/// A changed document with enough identity to route and act on it.
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub struct ChangedDoc {
    pub id: String,
    pub name: String,
    pub parent: String, // parent folder id
    pub path: String,   // full slash path, e.g. /Books/Author/Title
}

/// A unit of reactive work.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct Job {
    pub action: WatchAction,
    pub rule_path: String,
    pub debounce: Duration,
    pub doc: ChangedDoc,
}

/// Ids present-and-changed or newly-added between baseline and current. Removed ids are
/// intentionally excluded — there is nothing to process for a gone doc.
#[allow(dead_code)]
pub fn diff_ids(
    baseline: &BTreeMap<String, String>,
    current: &BTreeMap<String, String>,
) -> Vec<String> {
    current
        .iter()
        .filter(|(id, h)| baseline.get(*id).map_or(true, |b| b != *h))
        .map(|(id, _)| id.clone())
        .collect()
}

/// Match each changed doc against rule path-prefixes; one job per (rule, doc) match.
#[allow(dead_code)]
pub fn route(docs: &[ChangedDoc], rules: &[ResolvedRule]) -> Vec<Job> {
    let mut jobs = Vec::new();
    for d in docs {
        for r in rules {
            let under = d.path == r.path || d.path.starts_with(&format!("{}/", r.path));
            if under {
                jobs.push(Job {
                    action: r.action,
                    rule_path: r.path.clone(),
                    debounce: r.debounce,
                    doc: d.clone(),
                });
            }
        }
    }
    jobs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::WatchAction;

    fn rule(path: &str, action: WatchAction) -> ResolvedRule {
        ResolvedRule { path: path.into(), action, debounce: Duration::from_secs(30) }
    }
    fn doc(id: &str, path: &str) -> ChangedDoc {
        ChangedDoc {
            id: id.into(),
            name: path.rsplit('/').next().unwrap().into(),
            parent: "p".into(),
            path: path.into(),
        }
    }

    #[test]
    fn diff_reports_added_and_changed_not_removed() {
        let base = BTreeMap::from([
            ("a".to_string(), "h1".to_string()),
            ("b".to_string(), "h2".to_string()),
        ]);
        let cur = BTreeMap::from([
            ("a".to_string(), "h1".to_string()),
            ("b".to_string(), "h2b".to_string()),
            ("c".to_string(), "h3".to_string()),
        ]);
        let mut ids = diff_ids(&base, &cur);
        ids.sort();
        assert_eq!(ids, vec!["b".to_string(), "c".to_string()]);
    }

    #[test]
    fn routes_under_path_prefix() {
        let docs = vec![
            doc("1", "/Books/Book"),
            doc("2", "/Read/Library/Art"),
            doc("3", "/Other/Misc"),
        ];
        let rules = vec![
            rule("/Books", WatchAction::Digest),
            rule("/Read/Library", WatchAction::Readback),
        ];
        let jobs = route(&docs, &rules);
        assert_eq!(jobs.len(), 2);
        assert!(jobs.iter().any(|j| j.doc.id == "1" && j.action == WatchAction::Digest));
        assert!(jobs.iter().any(|j| j.doc.id == "2" && j.action == WatchAction::Readback));
    }

    #[test]
    fn prefix_does_not_match_sibling_with_shared_stem() {
        let docs = vec![doc("1", "/BooksClub/x")];
        let rules = vec![rule("/Books", WatchAction::Digest)];
        assert!(route(&docs, &rules).is_empty());
    }

    #[test]
    fn exact_path_matches() {
        let docs = vec![doc("1", "/Books")];
        let rules = vec![rule("/Books", WatchAction::Digest)];
        assert_eq!(route(&docs, &rules).len(), 1);
    }

    #[test]
    fn multiple_rules_each_yield_job() {
        let docs = vec![doc("1", "/Books/X")];
        let rules = vec![
            rule("/Books", WatchAction::Digest),
            rule("/Books", WatchAction::Readback),
        ];
        assert_eq!(route(&docs, &rules).len(), 2);
    }
}
