//! Dot-grid geometry derived from a device's page size.

use crate::device::Device;

const MM_PER_PT: f32 = 25.4 / 72.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridSpec {
    pub spacing_pt: f32,
    pub margin_pt: f32,
    pub cols: u32,
    pub rows: u32,
}

pub fn dot_grid(device: &Device, spacing_mm: f32, margin_mm: f32) -> GridSpec {
    let spacing_pt = spacing_mm / MM_PER_PT;
    let margin_pt = margin_mm / MM_PER_PT;
    let usable_w = device.width_pt() - 2.0 * margin_pt;
    let usable_h = device.height_pt() - 2.0 * margin_pt;
    let cols = (usable_w / spacing_pt).floor() as u32 + 1;
    let rows = (usable_h / spacing_pt).floor() as u32 + 1;
    GridSpec {
        spacing_pt,
        margin_pt,
        cols,
        rows,
    }
}

/// Default dot pitch in mm. Matches reMarkable's built-in "Dots Small" template
/// (4.756 mm = 42.5 device-units at 0.31718 pt/unit), so user-inserted "Dots Small"
/// pages line up. Overridable per-year via config.
pub const DEFAULT_SPACING_MM: f32 = 4.756;
/// Default page margin in mm.
pub const DEFAULT_MARGIN_MM: f32 = 6.0;
/// Vertical space (pt) reserved at the top of every non-cover page so content
/// clears the reMarkable pen toolbar. Measured on the Paper Pro Move.
pub const TOOLBAR_SAFE_PT: f32 = 36.0;

pub fn default_grid(device: &Device) -> GridSpec {
    dot_grid(device, DEFAULT_SPACING_MM, DEFAULT_MARGIN_MM)
}

/// The monthly day-list centres each day in the *cell between* two dot rows, at
/// the device's own pitch, so text sits in the gap and no dot ever cuts through a
/// glyph (centring *on* a dot row puts the dot in the middle of the number). The
/// pitch matches the grid exactly, so rows never drift relative to the dots.
///
/// `dot_grid`'s phase puts dot-row `k`'s centre at `DOT_CENTER_Y0 + k·spacing` pt
/// from the page top — see `build_preamble`'s `dot-bg` (tile dot centred at
/// `(sp/2, sp/2)`, field offset `-0.04 - sp/2`). A day row centred at
/// `+ (k + 0.5)·spacing` therefore lands halfway between dot rows `k` and `k+1`.
pub const DOT_CENTER_Y0: f32 = -0.04;

/// Index of the dot row above day 1's cell. Rows 0–2 fall inside the 36pt
/// toolbar-safe band (which holds the masthead); day 1's cell sits between rows
/// 3 and 4 (centre ≈47pt, clear of the toolbar). 31 days then occupy cells
/// 3..=33, with the last cell centre (≈452pt) inside the page (462pt) — the only
/// way 31 on-grid rows fit one column on the Move, since a dedicated masthead row
/// would push day 31 off the page.
pub const MONTHLY_FIRST_ROW: u32 = 3;

/// Y (pt from page top) the i-th day (0-based) is vertically centred on — the
/// middle of the dot cell below dot row `MONTHLY_FIRST_ROW + i`.
pub fn monthly_row_center(spacing_pt: f32, i: usize) -> f32 {
    DOT_CENTER_Y0 + (MONTHLY_FIRST_ROW as f32 + i as f32 + 0.5) * spacing_pt
}
