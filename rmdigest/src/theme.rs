//! Visual palette for the digest output + reMarkable pen-color mapping.
use rmfiles::PenColor;

/// Core colors for the digest theme.
pub struct Palette {
    /// Body text / ink color.
    pub ink: (u8, u8, u8),
    /// Hairline rule color.
    pub rule: (u8, u8, u8),
    /// Page background.
    pub paper: (u8, u8, u8),
}

/// The default "Newsprint" palette.
pub fn default_palette() -> Palette {
    Palette {
        ink: (26, 26, 26),
        rule: (180, 180, 180),
        paper: (250, 249, 246),
    }
}

/// Map a reMarkable pen color to an RGB triple for the digest theme.
pub fn pen_rgb(c: PenColor) -> (u8, u8, u8) {
    match c {
        // `Highlight` is the reMarkable highlighter's snap-to-text color (the
        // device doesn't expose the chosen RGBA); default it to the standard
        // highlighter yellow rather than ink, so real captures read as highlights.
        PenColor::Yellow | PenColor::Yellow2 | PenColor::Highlight => (245, 208, 66),
        PenColor::Green | PenColor::Green2 => (120, 190, 110),
        PenColor::Pink => (235, 130, 170),
        PenColor::Blue | PenColor::Cyan => (90, 150, 220),
        PenColor::Red | PenColor::Magenta => (210, 80, 80),
        PenColor::Gray | PenColor::GrayOverlap => (140, 140, 140),
        PenColor::White => (255, 255, 255),
        _ => (60, 60, 60), // Black, Other -> ink
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pen_rgb_yellow_maps_correctly() {
        assert_eq!(pen_rgb(PenColor::Yellow), (245, 208, 66));
    }

    #[test]
    fn pen_rgb_black_falls_back_to_ink() {
        assert_eq!(pen_rgb(PenColor::Black), (60, 60, 60));
    }

    #[test]
    fn pen_rgb_other_falls_back_to_ink() {
        assert_eq!(pen_rgb(PenColor::Other(999)), (60, 60, 60));
    }

    #[test]
    fn pen_rgb_highlight_is_yellow() {
        // The reMarkable highlighter color must read as a highlight, not ink.
        assert_eq!(pen_rgb(PenColor::Highlight), (245, 208, 66));
    }
}
