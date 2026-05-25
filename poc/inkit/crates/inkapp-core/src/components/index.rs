//! `Index` — a Display-mode component: a typographically clean listing of entries
//! (the reader's Library / Feed contents pages). Generic over the app's `Msg`
//! (which it never emits, like `Notice`); `Index<()>` is the common case. Each
//! entry is a non-breakable `#region` box, so an entry never splits across a page
//! break; the list flows and Typst paginates between entries. Colors come from
//! `cx.theme` (semantic roles), never literals — the component is device-blind.

use std::marker::PhantomData;

use crate::component::{Component, RenderCx};
use crate::components::esc_typst_str;
use crate::ink::RegionInk;
use crate::manifest::Manifest;

/// Summaries longer than this many chars are truncated (with an ellipsis) so a
/// non-breakable entry box stays within one page. Contents pages want short
/// standfirsts anyway.
const DEFAULT_SUMMARY_CHARS: usize = 200;

/// One row of an index listing. Built by an app's `view` from connector data (the
/// "dumb leaf conversion" pattern); e.g. `inkapp-readwise-reader`'s
/// `From<&Article> for IndexEntry`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IndexEntry {
    pub title: String,
    /// Author, or site name as a fallback; `None` when neither is known.
    pub byline: Option<String>,
    /// Verbatim reading-time label (e.g. "5 min") — never parsed/reformatted.
    pub reading_time: Option<String>,
    pub summary: Option<String>,
}

impl IndexEntry {
    /// An entry with just a title; byline/reading_time/summary default to `None`.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Self::default()
        }
    }
}

/// A display-only index. `M` is the app message type — `Index` never emits one,
/// so it's a phantom; `Index<()>` works when no surrounding `Msg` is needed.
pub struct Index<M = ()> {
    entries: Vec<IndexEntry>,
    _msg: PhantomData<fn() -> M>,
}

impl<M> Index<M> {
    pub fn new(entries: Vec<IndexEntry>) -> Self {
        Self {
            entries,
            _msg: PhantomData,
        }
    }
}

/// Truncate on a char boundary, appending "…" only when actually shortened.
fn truncate_summary(s: &str) -> String {
    if s.chars().count() <= DEFAULT_SUMMARY_CHARS {
        s.to_string()
    } else {
        let cut: String = s.chars().take(DEFAULT_SUMMARY_CHARS).collect();
        format!("{}…", cut.trim_end())
    }
}

impl<M> Component for Index<M> {
    type Msg = M;

    fn render(&self, cx: &mut RenderCx) -> String {
        let theme = &cx.theme;
        let mut s = String::new();
        for (i, e) in self.entries.iter().enumerate() {
            // Each entry's body: title, an optional meta line, an optional summary.
            // All user text is injected as a Typst string expression (`#"..."`)
            // via esc_typst_str so `[`, `]`, `#` stay literal (the Notice recipe).
            let mut body = String::new();
            body.push_str(&format!(
                "#text(fill: {}, weight: \"bold\", size: 13pt)[#\"{}\"]\n\n",
                theme.heading,
                esc_typst_str(&e.title)
            ));

            let byline = e.byline.as_deref().filter(|b| !b.is_empty());
            let rt = e.reading_time.as_deref().filter(|r| !r.is_empty());
            if byline.is_some() || rt.is_some() {
                if let Some(b) = byline {
                    body.push_str(&format!(
                        "#text(fill: {}, size: 9pt)[#\"{}\"]",
                        theme.byline,
                        esc_typst_str(b)
                    ));
                }
                if let Some(r) = rt {
                    if byline.is_some() {
                        body.push_str(&format!(
                            "#text(fill: {}, size: 9pt)[#\" · \"]",
                            theme.muted
                        ));
                    }
                    body.push_str(&format!(
                        "#text(fill: {}, size: 9pt)[#\"{}\"]",
                        theme.muted,
                        esc_typst_str(r)
                    ));
                }
                body.push_str("\n\n");
            }

            if let Some(sum) = e.summary.as_deref().filter(|s| !s.is_empty()) {
                body.push_str(&format!(
                    "#text(fill: {}, size: 10pt)[#\"{}\"]\n\n",
                    theme.ink,
                    esc_typst_str(&truncate_summary(sum))
                ));
            }

            // The entry as one non-breakable region box (layout/recovery anchor;
            // decode ignores it). `#region(name, body)` is the prelude default.
            s.push_str(&format!("#region(\"idx-{i}\", [{body}])\n"));

            // Hairline between entries (not after the last).
            if i + 1 < self.entries.len() {
                s.push_str(&format!(
                    "#line(length: 100%, stroke: 0.5pt + {})\n\n",
                    theme.rule
                ));
            }
        }
        s
    }

    fn decode(&self, _ink: &[RegionInk], _manifest: &Manifest) -> Vec<M> {
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Theme;

    fn entry(title: &str) -> IndexEntry {
        IndexEntry::new(title)
    }

    #[test]
    fn title_uses_heading_role_and_is_present() {
        let idx = Index::<()>::new(vec![entry("Hello World")]);
        let src = idx.render(&mut RenderCx::new(0).with_theme(Theme::indigo_tomato()));
        assert!(
            src.contains("fill: rgb(\"#2A2F6B\")"),
            "heading color: {src}"
        );
        assert!(src.contains("#\"Hello World\""), "title text: {src}");
    }

    #[test]
    fn byline_and_reading_time_joined_with_separator() {
        let e = IndexEntry {
            title: "T".into(),
            byline: Some("Ada Lovelace".into()),
            reading_time: Some("5 min".into()),
            summary: None,
        };
        let src = Index::<()>::new(vec![e]).render(&mut RenderCx::new(0));
        assert!(src.contains("#\"Ada Lovelace\""), "byline: {src}");
        assert!(src.contains("#\" · \""), "separator present: {src}");
        assert!(src.contains("#\"5 min\""), "reading_time verbatim: {src}");
    }

    #[test]
    fn missing_meta_is_omitted() {
        let src = Index::<()>::new(vec![entry("Just a title")]).render(&mut RenderCx::new(0));
        assert!(
            !src.contains("#\" · \""),
            "no separator without meta: {src}"
        );
    }

    #[test]
    fn reading_time_alone_has_no_separator() {
        let e = IndexEntry {
            title: "T".into(),
            byline: None,
            reading_time: Some("3 min".into()),
            summary: None,
        };
        let src = Index::<()>::new(vec![e]).render(&mut RenderCx::new(0));
        assert!(src.contains("#\"3 min\""), "reading_time present: {src}");
        assert!(
            !src.contains("#\" · \""),
            "no separator without byline: {src}"
        );
    }

    #[test]
    fn escapes_quotes_in_title() {
        let src = Index::<()>::new(vec![entry(r#"a "quote""#)]).render(&mut RenderCx::new(0));
        assert!(src.contains(r#"a \"quote\""#), "title escaped: {src}");
    }

    #[test]
    fn emits_a_region_per_entry_with_rule_between() {
        let src = Index::<()>::new(vec![entry("one"), entry("two")]).render(&mut RenderCx::new(0));
        assert!(src.contains("#region(\"idx-0\""), "first region: {src}");
        assert!(src.contains("#region(\"idx-1\""), "second region: {src}");
        assert_eq!(
            src.matches("#line(").count(),
            1,
            "one rule between two: {src}"
        );
    }

    #[test]
    fn long_summary_is_truncated() {
        let long = "x".repeat(DEFAULT_SUMMARY_CHARS + 50);
        let e = IndexEntry {
            title: "T".into(),
            byline: None,
            reading_time: None,
            summary: Some(long),
        };
        let src = Index::<()>::new(vec![e]).render(&mut RenderCx::new(0));
        assert!(src.contains("…"), "truncation ellipsis present: {src}");
    }

    #[test]
    fn decode_is_always_empty() {
        let idx = Index::<u8>::new(vec![entry("x")]);
        assert!(idx.decode(&[], &Manifest::default()).is_empty());
    }
}
