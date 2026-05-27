//! `Index` — a Display-mode component: a typographically clean listing of entries
//! (the reader's Library / Feed contents pages). Generic over the app's `Msg`
//! (which it never emits, like `Notice`); `Index<()>` is the common case.
//!
//! Layout per row is a three-column grid — number / title+byline / reading-time —
//! wrapped in a non-breakable `#region` so the row never splits across a page
//! break. The list flows and Typst paginates between rows. Styling comes from
//! `cx.theme` — heading font for the masthead and the numbered prefix, body
//! font and grayscale tones for the rest — so the component names no literal
//! colors and stays device-blind. An optional `with_title(...)` masthead
//! prints a big-serif title plus an uppercase tracked count subtitle above
//! the first row, like the old rmreader's "Library / 56 articles" header.

use std::marker::PhantomData;

use crate::component::{Component, RenderCx};
use crate::components::esc_typst_str;
use crate::ink::RegionInk;
use crate::manifest::Manifest;

/// One row of an index listing. Built by an app's `view` from connector data (the
/// "dumb leaf conversion" pattern); e.g. `inkapp-readwise-reader`'s
/// `From<&Article> for IndexEntry`.
///
/// `summary` is retained on the struct for callers that want it, but the
/// default compact row layout does not render it (the old rmreader's Index
/// shape is title + byline + reading-time, one logical line per entry). A
/// future `Index::verbose(...)` could opt into rendering summaries; today
/// it's just unused metadata here.
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
    title: Option<String>,
    entries: Vec<IndexEntry>,
    _msg: PhantomData<fn() -> M>,
}

impl<M> Index<M> {
    /// An index over `entries` with no masthead. Region names are minted by
    /// position (`idx-0`, `idx-1`, …) within this instance, so two `Index`
    /// instances in one document would mint colliding names; today nothing
    /// does that (each contents doc has one index). A second instance per
    /// document would need an instance-level name prefix. (Mirrors the
    /// `evt-{i}` caveat on `CalendarView::editable`.)
    pub fn new(entries: Vec<IndexEntry>) -> Self {
        Self {
            title: None,
            entries,
            _msg: PhantomData,
        }
    }

    /// Like `new`, but prefixes the listing with a big-serif masthead — the
    /// `title` (e.g. "Library") on top, an uppercase tracked subtitle
    /// `"N articles"` below (where N is the entry count).
    #[must_use]
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }
}

impl<M> Component for Index<M> {
    type Msg = M;

    fn render(&self, cx: &mut RenderCx) -> String {
        let theme = &cx.theme;
        let heading_font = esc_typst_str(&theme.heading);
        let body_font = esc_typst_str(&theme.body);
        let mut s = String::new();

        // Optional masthead — title in the heading font at a display size, plus an
        // uppercase tracked count subtitle. Both centered to the left margin.
        if let Some(t) = &self.title {
            s.push_str(&format!(
                "#v(8pt)\n\
                 #text(font: \"{hf}\", weight: \"bold\", size: 2.6em, fill: luma({heading_tone}))[#\"{t}\"]\n\n\
                 #text(font: \"{bf}\", size: 0.7em, tracking: 0.12em, fill: luma({muted_tone}))[#upper[#\"{count} articles\"]]\n\n\
                 #v(14pt)\n",
                hf = heading_font,
                bf = body_font,
                heading_tone = theme.heading_tone,
                muted_tone = theme.muted_tone,
                t = esc_typst_str(t),
                count = self.entries.len(),
            ));
        }

        for (i, e) in self.entries.iter().enumerate() {
            // Build the three grid cells per row: numbered prefix / title (+
            // optional byline appended in muted tone) / reading-time.
            let num_cell = format!(
                "[#text(font: \"{hf}\", weight: \"semibold\", size: 0.95em, fill: luma({heading_tone}))[#\"{n:02}\"]]",
                hf = heading_font,
                heading_tone = theme.heading_tone,
                n = i + 1,
            );

            // Title + optional " — byline" on one logical line. The byline is in
            // the muted tone and at a smaller size so the title leads visually.
            let byline_fragment = match e.byline.as_deref().filter(|b| !b.is_empty()) {
                Some(b) => format!(
                    "#text(fill: luma({muted_tone}), size: 0.85em)[#\" — {by}\"]",
                    muted_tone = theme.muted_tone,
                    by = esc_typst_str(b),
                ),
                None => String::new(),
            };
            let title_cell = format!(
                "[#text(fill: luma({body_tone}), size: 1em)[#\"{t}\"]{by}]",
                body_tone = theme.body_tone,
                t = esc_typst_str(&e.title),
                by = byline_fragment,
            );

            // Reading-time cell — right-aligned, small uppercase tracked. Empty
            // cell when missing so the grid layout stays uniform.
            let rt_cell = match e.reading_time.as_deref().filter(|r| !r.is_empty()) {
                Some(r) => format!(
                    "[#text(font: \"{bf}\", size: 0.7em, tracking: 0.08em, fill: luma({muted_tone}))[#upper[#\"{rt}\"]]]",
                    bf = body_font,
                    muted_tone = theme.muted_tone,
                    rt = esc_typst_str(r),
                ),
                None => "[]".to_string(),
            };

            let row_grid = format!(
                "#grid(\n\
                   columns: (28pt, 1fr, 56pt),\n\
                   rows: auto,\n\
                   align: (right + horizon, left + horizon, right + horizon),\n\
                   column-gutter: 10pt,\n\
                   inset: (x: 0pt, y: 6pt),\n\
                   {num_cell},\n\
                   {title_cell},\n\
                   {rt_cell},\n\
                 )\n"
            );

            // The entry as one non-breakable region box (layout/recovery anchor;
            // decode ignores it). `#region(name, body)` is the prelude default.
            s.push_str(&format!("#region(\"idx-{i}\", [{row_grid}])\n"));

            // Hairline between entries (not after the last), in the rule tone.
            if i + 1 < self.entries.len() {
                s.push_str(&format!(
                    "#line(length: 100%, stroke: 0.4pt + luma({tone}))\n",
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
    fn byline_is_appended_to_title_with_em_dash() {
        let e = IndexEntry {
            title: "T".into(),
            byline: Some("Ada Lovelace".into()),
            reading_time: Some("5 min".into()),
            summary: None,
        };
        let src = Index::<()>::new(vec![e]).render(&mut RenderCx::new(0));
        // New shape: byline appears as `#" — Ada Lovelace"` in the muted tone,
        // immediately after the title text in the title cell.
        assert!(
            src.contains("#\" — Ada Lovelace\""),
            "byline appended with em-dash: {src}"
        );
        assert!(src.contains("#\"5 min\""), "reading_time verbatim: {src}");
        // reader() muted_tone is 110 — byline in muted tone, smaller size.
        assert!(
            src.contains("fill: luma(110), size: 0.85em"),
            "byline in muted small tone: {src}"
        );
    }

    #[test]
    fn missing_meta_renders_a_row_anyway() {
        // No byline / no reading_time: the row still renders with just the title.
        let src = Index::<()>::new(vec![entry("Just a title")]).render(&mut RenderCx::new(0));
        assert!(src.contains("#\"Just a title\""), "title present: {src}");
        // No " — " fragment because there's no byline to append.
        assert!(
            !src.contains("#\" — "),
            "no em-dash byline fragment when byline absent: {src}"
        );
    }

    #[test]
    fn reading_time_alone_renders_in_its_own_cell() {
        let e = IndexEntry {
            title: "T".into(),
            byline: None,
            reading_time: Some("3 min".into()),
            summary: None,
        };
        let src = Index::<()>::new(vec![e]).render(&mut RenderCx::new(0));
        assert!(src.contains("#\"3 min\""), "reading_time present: {src}");
        // The reading-time cell uses uppercase tracking — the upper/tracking
        // hint is what distinguishes the new compact look.
        assert!(src.contains("#upper["), "reading_time uppercased: {src}");
    }

    #[test]
    fn numbered_prefix_uses_one_based_two_digit_format() {
        let src = Index::<()>::new(vec![entry("a"), entry("b"), entry("c")])
            .render(&mut RenderCx::new(0));
        assert!(src.contains("#\"01\""), "first row numbered 01: {src}");
        assert!(src.contains("#\"02\""), "second row numbered 02: {src}");
        assert!(src.contains("#\"03\""), "third row numbered 03: {src}");
    }

    #[test]
    fn with_title_emits_masthead_and_count_subtitle() {
        let src = Index::<()>::new(vec![entry("a"), entry("b"), entry("c")])
            .with_title("Library")
            .render(&mut RenderCx::new(0));
        // Big-serif masthead in the heading font + heading_tone.
        assert!(
            src.contains("size: 2.6em") && src.contains("#\"Library\""),
            "masthead title rendered: {src}"
        );
        // Uppercase tracked count subtitle.
        assert!(
            src.contains("#\"3 articles\""),
            "count subtitle present: {src}"
        );
    }

    #[test]
    fn no_masthead_when_with_title_not_called() {
        let src = Index::<()>::new(vec![entry("a")]).render(&mut RenderCx::new(0));
        assert!(
            !src.contains("size: 2.6em"),
            "no masthead by default: {src}"
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
    fn summary_is_not_rendered_in_default_layout() {
        // The compact layout deliberately drops the summary (kept in the struct
        // for future opt-in callers). Long or short, it must not appear in
        // the rendered Typst.
        let long = "L".repeat(500);
        let e = IndexEntry {
            title: "T".into(),
            byline: None,
            reading_time: None,
            summary: Some(long.clone()),
        };
        let src = Index::<()>::new(vec![e]).render(&mut RenderCx::new(0));
        assert!(
            !src.contains("LLLLL"),
            "summary text must not appear in default compact layout"
        );
    }

    #[test]
    fn decode_is_always_empty() {
        let idx = Index::<u8>::new(vec![entry("x")]);
        assert!(idx.decode(&[], &Manifest::default()).is_empty());
    }
}
