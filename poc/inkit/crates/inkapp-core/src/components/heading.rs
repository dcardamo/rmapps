//! `Heading` — a reusable Display component for long-form article/section
//! openers: title, optional byline (author OR site_name fallback at the call
//! site), optional reading-time, optional subtitle. Theme-aware via RenderCx.

use crate::component::{Component, RenderCx};
use crate::components::esc_typst_str;
use crate::ink::RegionInk;
use crate::manifest::Manifest;

/// Title + optional byline/reading-time/subtitle. Mirrors the metadata Readwise
/// exposes (title/author/site_name/reading_time/summary), but the component is
/// content-agnostic — pass whatever strings make sense.
pub struct Heading {
    title: String,
    byline: Option<String>,
    reading_time: Option<String>,
    subtitle: Option<String>,
}

impl Heading {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            byline: None,
            reading_time: None,
            subtitle: None,
        }
    }

    #[must_use]
    pub fn byline(mut self, s: impl Into<String>) -> Self {
        self.byline = Some(s.into());
        self
    }

    #[must_use]
    pub fn reading_time(mut self, s: impl Into<String>) -> Self {
        self.reading_time = Some(s.into());
        self
    }

    #[must_use]
    pub fn subtitle(mut self, s: impl Into<String>) -> Self {
        self.subtitle = Some(s.into());
        self
    }
}

impl Component for Heading {
    type Msg = ();

    fn render(&self, cx: &mut RenderCx) -> String {
        let theme = &cx.theme;
        let title = esc_typst_str(&self.title);
        let heading_tone = theme.heading_tone;
        let muted_tone = theme.muted_tone;

        // Build the #heading-block(...) call, passing optional named args only
        // when present to keep the output clean.
        let mut call = format!(
            "#heading-block(\"{}\"",
            title
        );

        if let Some(b) = &self.byline {
            call.push_str(&format!(", byline: \"{}\"", esc_typst_str(b)));
        }
        if let Some(rt) = &self.reading_time {
            call.push_str(&format!(", meta: \"{}\"", esc_typst_str(rt)));
        }
        if let Some(sub) = &self.subtitle {
            call.push_str(&format!(", subtitle: \"{}\"", esc_typst_str(sub)));
        }
        // Pass luma tones so the authored Typst module stays colour-agnostic.
        call.push_str(&format!(
            ", heading-tone: {heading_tone}, muted-tone: {muted_tone})"
        ));

        call
    }

    fn decode(&self, _ink: &[RegionInk], _manifest: &Manifest) -> Vec<()> {
        vec![]
    }

    fn typst_sources(&self) -> Vec<(String, String)> {
        vec![(
            "/inkapp/heading.typ".into(),
            include_str!("../../typst/heading.typ").into(),
        )]
    }
}
