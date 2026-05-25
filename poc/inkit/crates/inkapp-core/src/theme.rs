//! `Theme` — the device-blind palette seam. Components name semantic *roles*
//! (heading, byline, muted, …); the framework fills them with Typst
//! fill-expression strings tuned for the target device. Threaded through
//! `RenderCx` exactly as `PageGeom` is threaded for "page-blind" layout, so a
//! component never names a literal color or knows which device it renders for.

/// A palette of semantic roles. Each field is a Typst color *expression* string
/// interpolated into `#text(fill: …)` — e.g. `"black"`, `"luma(45%)"`,
/// `"rgb(\"#2A2F6B\")"`. `paper` is the optional page fill: `None` leaves Typst's
/// default white, so existing renders are byte-identical under the default theme.
#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub ink: String,
    pub heading: String,
    pub byline: String,
    pub muted: String,
    pub rule: String,
    pub paper: Option<String>,
}

impl Theme {
    /// The safe default: grayscale lumas for any e-ink device.
    pub fn grayscale() -> Self {
        Self {
            ink: "luma(10%)".into(),
            heading: "black".into(),
            byline: "luma(35%)".into(),
            muted: "luma(45%)".into(),
            rule: "luma(80%)".into(),
            paper: None,
        }
    }

    /// reMarkable Paper Pro color palette ("Indigo + Tomato"). On Paper Pro
    /// e-ink, blues/reds hold their color while ambers/greens wash out, so the
    /// chrome is indigo headings + rust bylines on warm paper (proven in rmreader).
    pub fn indigo_tomato() -> Self {
        Self {
            ink: "rgb(\"#1A1A18\")".into(),
            heading: "rgb(\"#2A2F6B\")".into(),
            byline: "rgb(\"#9C3A1B\")".into(),
            muted: "rgb(\"#5E6166\")".into(),
            rule: "rgb(\"#E0DDD2\")".into(),
            paper: Some("rgb(\"#F3F1EA\")".into()),
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::grayscale()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grayscale_has_no_paper_fill() {
        assert_eq!(Theme::grayscale().paper, None);
    }

    #[test]
    fn default_is_grayscale() {
        assert_eq!(Theme::default(), Theme::grayscale());
    }

    #[test]
    fn indigo_tomato_is_color_with_paper() {
        let t = Theme::indigo_tomato();
        assert_eq!(t.heading, "rgb(\"#2A2F6B\")");
        assert_eq!(t.byline, "rgb(\"#9C3A1B\")");
        assert_eq!(t.paper, Some("rgb(\"#F3F1EA\")".into()));
    }
}
