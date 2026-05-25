//! `Theme` — a code-level reading aesthetic. Holds font families, a type scale, and
//! grayscale tones, and emits a Typst styling prelude injected into every document's
//! source (see `runtime::document_source_in`). Pure code API: no config dependency.

/// A reading aesthetic: font families, a type scale, and grayscale tones, emitted as
/// a Typst styling prelude. Tones are `u8` luma (0 = black, 255 = white), which makes
/// grayscale structural — correct for every reMarkable device.
#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    /// Body text font family.
    pub body: String,
    /// Heading font family.
    pub heading: String,
    /// Monospace / raw font family.
    pub mono: String,
    /// Base text size in points.
    pub size_pt: f64,
    /// Paragraph leading as an em multiple.
    pub leading_em: f64,
    /// Justify body paragraphs.
    pub justify: bool,
    /// Heading colour, luma 0..=255.
    pub heading_tone: u8,
    /// Body text colour, luma 0..=255.
    pub body_tone: u8,
    /// Secondary text (quotes, captions), luma 0..=255.
    pub muted_tone: u8,
    /// Hairlines / quote bar, luma 0..=255.
    pub rule_tone: u8,
}

impl Theme {
    /// The default reading aesthetic: Newsreader body, Fraunces headings, DejaVu Sans
    /// Mono for raw, 11pt justified with airy leading, dark-on-light grayscale tones.
    pub fn reader() -> Self {
        Self {
            body: "Newsreader".to_string(),
            heading: "Fraunces".to_string(),
            mono: "DejaVu Sans Mono".to_string(),
            size_pt: 11.0,
            leading_em: 0.75,
            justify: true,
            heading_tone: 26,
            body_tone: 34,
            muted_tone: 110,
            rule_tone: 216,
        }
    }

    /// Set the body font family.
    #[must_use]
    pub fn body(mut self, family: impl Into<String>) -> Self {
        self.body = family.into();
        self
    }
    /// Set the heading font family.
    #[must_use]
    pub fn heading(mut self, family: impl Into<String>) -> Self {
        self.heading = family.into();
        self
    }
    /// Set the monospace font family.
    #[must_use]
    pub fn mono(mut self, family: impl Into<String>) -> Self {
        self.mono = family.into();
        self
    }
    /// Set the base text size (points).
    #[must_use]
    pub fn size_pt(mut self, pt: f64) -> Self {
        self.size_pt = pt;
        self
    }
    /// Set the paragraph leading (em multiple).
    #[must_use]
    pub fn leading_em(mut self, em: f64) -> Self {
        self.leading_em = em;
        self
    }
    /// Toggle body justification.
    #[must_use]
    pub fn justify(mut self, on: bool) -> Self {
        self.justify = on;
        self
    }
    /// Set the heading tone (luma 0..=255).
    #[must_use]
    pub fn heading_tone(mut self, luma: u8) -> Self {
        self.heading_tone = luma;
        self
    }
    /// Set the body tone (luma 0..=255).
    #[must_use]
    pub fn body_tone(mut self, luma: u8) -> Self {
        self.body_tone = luma;
        self
    }
    /// Set the muted tone (luma 0..=255).
    #[must_use]
    pub fn muted_tone(mut self, luma: u8) -> Self {
        self.muted_tone = luma;
        self
    }
    /// Set the rule tone (luma 0..=255).
    #[must_use]
    pub fn rule_tone(mut self, luma: u8) -> Self {
        self.rule_tone = luma;
        self
    }

    /// Emit the Typst styling prelude: text/par defaults, heading treatment, and raw
    /// and quote styling. Injected in place of the bare `#set text` line. Does NOT
    /// emit `#set page` — page geometry is owned by `PageGeom`.
    pub fn prelude(&self) -> String {
        let body = escape_typst_str(&self.body);
        let heading = escape_typst_str(&self.heading);
        let mono = escape_typst_str(&self.mono);

        // The quote rule is the most intricate line: an indented, muted, italic block
        // with a left rule. Built separately for readability.
        let quote_rule = format!(
            "#show quote: it => block(inset: (left: 1em), stroke: (left: 0.5pt + luma({rule})), text(fill: luma({muted}), style: \"italic\", it.body))\n",
            rule = self.rule_tone,
            muted = self.muted_tone,
        );

        format!(
            "#set text(font: \"{body}\", size: {size}pt, fill: luma({body_tone}))\n\
             #set par(leading: {leading}em, justify: {justify})\n\
             #show heading: set text(font: \"{heading}\", fill: luma({heading_tone}))\n\
             #show heading.where(level: 1): set text(size: 1.6em)\n\
             #show heading.where(level: 2): set text(size: 1.3em)\n\
             #show raw: set text(font: \"{mono}\")\n\
             {quote_rule}",
            body = body,
            size = self.size_pt,
            body_tone = self.body_tone,
            leading = self.leading_em,
            justify = self.justify,
            heading = heading,
            heading_tone = self.heading_tone,
            mono = mono,
            quote_rule = quote_rule,
        )
    }
}

/// Escape a string for safe interpolation into a Typst double-quoted string literal.
/// Font family names reach `prelude()` from app code (and later from config), so a
/// stray `"` or `\` must not be able to alter the generated Typst.
fn escape_typst_str(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

impl Default for Theme {
    fn default() -> Self {
        Self::reader()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reader_defaults() {
        let t = Theme::reader();
        assert_eq!(t.body, "Newsreader");
        assert_eq!(t.heading, "Fraunces");
        assert_eq!(t.mono, "DejaVu Sans Mono");
        assert_eq!(t.size_pt, 11.0);
        assert_eq!(t.leading_em, 0.75);
        assert!(t.justify);
        assert_eq!(t.heading_tone, 26);
        assert_eq!(t.body_tone, 34);
        assert_eq!(t.muted_tone, 110);
        assert_eq!(t.rule_tone, 216);
    }

    #[test]
    fn default_is_reader() {
        assert_eq!(Theme::default(), Theme::reader());
    }

    #[test]
    fn prelude_emits_grayscale_fonts_and_rules() {
        let p = Theme::reader().prelude();
        assert!(p.contains("font: \"Newsreader\""), "body font set");
        assert!(p.contains("luma(34)"), "body tone as luma");
        assert!(p.contains("#show heading"), "heading treatment present");
        assert!(p.contains("font: \"DejaVu Sans Mono\""), "raw font set");
        assert!(p.contains("justify: true"), "justify reflected");
        assert!(
            !p.contains("rgb("),
            "tones must be grayscale luma, never rgb"
        );
    }

    #[test]
    fn prelude_escapes_font_names() {
        // A font name with a quote must not break out of the Typst string literal.
        let p = Theme::reader().body("Ev\"il").prelude();
        assert!(
            p.contains("font: \"Ev\\\"il\""),
            "quote in font name is escaped"
        );
        assert!(
            !p.contains("font: \"Ev\"il\""),
            "raw unescaped quote must not appear"
        );
    }

    #[test]
    fn builder_overrides_knobs() {
        let t = Theme::reader()
            .size_pt(13.0)
            .justify(false)
            .body("Literata");
        assert_eq!(t.size_pt, 13.0);
        assert!(!t.justify);
        assert_eq!(t.body, "Literata");
        let p = t.prelude();
        assert!(p.contains("justify: false"));
        assert!(p.contains("font: \"Literata\""));
    }
}
