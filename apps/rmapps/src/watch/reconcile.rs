//! Pure reconcile: snapshot-hash diff and folder-prefix routing. No I/O.
use crate::config::WatchAction;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
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

/// True if a doc looks like daemon-generated output.
#[allow(dead_code)]
pub fn is_self_write(name: &str, has_app_key: bool, suffixes: &[String]) -> bool {
    has_app_key || suffixes.iter().any(|s| name.ends_with(s.as_str()))
}

/// Remove daemon-generated docs (`app_key_ids` carry the `rmCloudKey` marker).
#[allow(dead_code)]
pub fn filter_self_writes(
    docs: Vec<ChangedDoc>,
    suffixes: &[String],
    app_key_ids: &BTreeSet<String>,
) -> Vec<ChangedDoc> {
    docs.into_iter()
        .filter(|d| !is_self_write(&d.name, app_key_ids.contains(&d.id), suffixes))
        .collect()
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

    #[test]
    fn is_self_write_suffix_appkey_and_plain() {
        let suffixes = vec![" — Digest".to_string()];
        // suffix match
        assert!(is_self_write("Book — Digest", false, &suffixes));
        // no match
        assert!(!is_self_write("Book", false, &suffixes));
        // app-key forces true regardless of name
        assert!(is_self_write("Book", true, &suffixes));
    }

    #[test]
    fn self_write_filtered_by_suffix_and_appkey() {
        let suffixes = vec![" — Digest".to_string(), " — Annotated".to_string()];
        let docs = vec![
            ChangedDoc { id: "1".into(), name: "Book".into(), parent: "p".into(), path: "/Books/Book".into() },
            ChangedDoc { id: "2".into(), name: "Book — Digest".into(), parent: "p".into(), path: "/Books/Book — Digest".into() },
            ChangedDoc { id: "3".into(), name: "Reader Index".into(), parent: "q".into(), path: "/Read/Reader Index".into() },
        ];
        let app_key_ids: std::collections::BTreeSet<String> = ["3".to_string()].into_iter().collect();
        let kept = filter_self_writes(docs, &suffixes, &app_key_ids);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].id, "1");
    }
}
