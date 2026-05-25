//! `Index` — a Display-mode component: a typographically clean listing of entries
//! (the reader's Library / Feed contents pages). Generic over the app's `Msg`
//! (which it never emits, like `Notice`); `Index<()>` is the common case. Each
//! entry is a non-breakable `#region` box, so an entry never splits across a page
//! break; the list flows and Typst paginates between entries. Styling comes from
//! `cx.theme` — the heading font and the grayscale `*_tone` lumas — so the
//! component names no literal colors and stays device-blind.

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
    /// An index over `entries`. Region names are minted by position (`idx-0`,
    /// `idx-1`, …) within this instance, so two `Index` instances in one document
    /// would mint colliding names; today nothing does that (each contents doc has
    /// one index). A second instance per document would need an instance-level
    /// name prefix. (Mirrors the `evt-{i}` caveat on `CalendarView::editable`.)
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
        let heading_font = esc_typst_str(&theme.heading);
        let mut s = String::new();
        for (i, e) in self.entries.iter().enumerate() {
            // Each entry's body: title, an optional meta line, an optional summary.
            // Tones are emitted as `luma(<u8>)` (the grayscale convention the theme
            // prelude uses); the title also takes the heading font family. All user
            // text is injected as a Typst string expression (`#"..."`) via
            // esc_typst_str so `[`, `]`, `#` stay literal (the Notice recipe).
            let mut body = String::new();
            body.push_str(&format!(
                "#text(font: \"{heading_font}\", fill: luma({tone}), weight: \"bold\", size: 1.3em)[#\"{title}\"]\n\n",
                tone = theme.heading_tone,
                title = esc_typst_str(&e.title),
            ));

            // Meta line "byline · reading_time" in the muted tone; the separator
            // appears only when both parts are present.
            let byline = e.byline.as_deref().filter(|b| !b.is_empty());
            let rt = e.reading_time.as_deref().filter(|r| !r.is_empty());
            if byline.is_some() || rt.is_some() {
                if let Some(b) = byline {
                    body.push_str(&format!(
                        "#text(fill: luma({tone}), size: 0.85em)[#\"{b}\"]",
                        tone = theme.muted_tone,
                        b = esc_typst_str(b),
                    ));
                }
                if let Some(r) = rt {
                    if byline.is_some() {
                        body.push_str(&format!(
                            "#text(fill: luma({tone}), size: 0.85em)[#\" · \"]",
                            tone = theme.muted_tone,
                        ));
                    }
                    body.push_str(&format!(
                        "#text(fill: luma({tone}), size: 0.85em)[#\"{r}\"]",
                        tone = theme.muted_tone,
                        r = esc_typst_str(r),
                    ));
                }
                body.push_str("\n\n");
            }

            if let Some(sum) = e.summary.as_deref().filter(|s| !s.is_empty()) {
                body.push_str(&format!(
                    "#text(fill: luma({tone}), size: 0.95em)[#\"{sum}\"]\n\n",
                    tone = theme.body_tone,
                    sum = esc_typst_str(&truncate_summary(sum)),
                ));
            }

            // The entry as one non-breakable region box (layout/recovery anchor;
            // decode ignores it). `#region(name, body)` is the prelude default.
            s.push_str(&format!("#region(\"idx-{i}\", [{body}])\n"));

            // Hairline between entries (not after the last), in the rule tone.
            if i + 1 < self.entries.len() {
                s.push_str(&format!(
                    "#line(length: 100%, stroke: 0.5pt + luma({tone}))\n\n",
                    tone = theme.rule_tone,
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
    fn title_uses_heading_font_and_tone() {
        // RenderCx::new defaults to Theme::reader(): heading font Fraunces, heading_tone 26.
        let idx = Index::<()>::new(vec![entry("Hello World")]);
        let src = idx.render(&mut RenderCx::new(0));
        assert!(src.contains("font: \"Fraunces\""), "heading font: {src}");
        assert!(src.contains("fill: luma(26)"), "heading tone: {src}");
        assert!(src.contains("#\"Hello World\""), "title text: {src}");
        assert!(
            !src.contains("rgb("),
            "tones are grayscale luma, never rgb: {src}"
        );
    }

    #[test]
    fn byline_and_reading_time_joined_with_separator() {
        let e = IndexEntry {
            title: "T".into(),
            byline: Some("Ada Lovelace".into()),
            reading_time: Some("5 min".into()),
            summary: None,
        };
        // reader() muted_tone is 110.
        let src = Index::<()>::new(vec![e]).render(&mut RenderCx::new(0));
        assert!(src.contains("#\"Ada Lovelace\""), "byline: {src}");
        assert!(src.contains("#\" · \""), "separator present: {src}");
        assert!(src.contains("#\"5 min\""), "reading_time verbatim: {src}");
        // Both byline and reading_time use the muted tone.
        assert!(
            src.contains("#text(fill: luma(110), size: 0.85em)[#\"Ada Lovelace\"]"),
            "byline in muted tone: {src}"
        );
        assert!(
            src.contains("#text(fill: luma(110), size: 0.85em)[#\"5 min\"]"),
            "reading_time in muted tone: {src}"
        );
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
    fn custom_theme_tones_are_used() {
        let theme = Theme::reader().heading_tone(200).rule_tone(50);
        let src = Index::<()>::new(vec![entry("one"), entry("two")])
            .render(&mut RenderCx::new(0).with_theme(theme));
        assert!(
            src.contains("fill: luma(200)"),
            "custom heading tone: {src}"
        );
        assert!(src.contains("luma(50)"), "custom rule tone: {src}");
    }

    #[test]
    fn escapes_quotes_in_title() {
        let src = Index::<()>::new(vec![entry(r#"a "quote""#)]).render(&mut RenderCx::new(0));
        assert!(src.contains(r#"a \"quote\""#), "title escaped: {src}");
    }

    #[test]
    fn escapes_backslash_in_title() {
        let src = Index::<()>::new(vec![entry(r#"a \ backslash"#)]).render(&mut RenderCx::new(0));
        assert!(
            src.contains(r#"a \\ backslash"#),
            "title backslash escaped: {src}"
        );
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
