//! Leaf conversion: a Readwise `Article` → the framework's `IndexEntry`. Apps
//! call this in `view` to feed the device-agnostic `Index` component connector
//! data ("dumb leaf conversion"). It lives here, not in `inkapp-core`, because
//! core must stay connector-blind; the orphan rule permits it since `Article` is
//! local to this crate.

use inkapp_core::components::index::IndexEntry;

use crate::Article;

impl From<&Article> for IndexEntry {
    fn from(a: &Article) -> Self {
        let byline = if !a.author.is_empty() {
            Some(a.author.clone())
        } else if !a.site_name.is_empty() {
            Some(a.site_name.clone())
        } else {
            None
        };
        IndexEntry {
            title: a.title.clone(),
            byline,
            // Verbatim String passthrough — reading_time is a label like "5 min",
            // never a number; do not parse or reformat it.
            reading_time: a.reading_time.clone(),
            summary: (!a.summary.is_empty()).then(|| a.summary.clone()),
            // Match the id used by `Section::new(&a.id.0, ...)` in the app's
            // view, so an Index row's #link to `<art-{id}>` lands on the right
            // Section's anchor.
            link_id: Some(a.id.0.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ArticleId;

    fn base() -> Article {
        Article {
            id: ArticleId::new("a1"),
            title: "A Title".into(),
            ..Article::default()
        }
    }

    #[test]
    fn byline_prefers_author() {
        let a = Article {
            author: "Ada".into(),
            site_name: "example.com".into(),
            ..base()
        };
        assert_eq!(IndexEntry::from(&a).byline, Some("Ada".into()));
    }

    #[test]
    fn byline_falls_back_to_site_name() {
        let a = Article {
            author: String::new(),
            site_name: "example.com".into(),
            ..base()
        };
        assert_eq!(IndexEntry::from(&a).byline, Some("example.com".into()));
    }

    #[test]
    fn byline_none_when_both_empty() {
        assert_eq!(IndexEntry::from(&base()).byline, None);
    }

    #[test]
    fn reading_time_passthrough() {
        let a = Article {
            reading_time: Some("5 min".into()),
            ..base()
        };
        assert_eq!(IndexEntry::from(&a).reading_time, Some("5 min".into()));
        assert_eq!(IndexEntry::from(&base()).reading_time, None);
    }

    #[test]
    fn summary_empty_becomes_none() {
        assert_eq!(IndexEntry::from(&base()).summary, None);
        let a = Article {
            summary: "hi".into(),
            ..base()
        };
        assert_eq!(IndexEntry::from(&a).summary, Some("hi".into()));
    }

    #[test]
    fn title_copied() {
        assert_eq!(IndexEntry::from(&base()).title, "A Title");
    }
}
