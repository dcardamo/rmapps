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
    GridSpec { spacing_pt, margin_pt, cols, rows }
}

/// Default dot pitch in mm. 4.5 (vs the 5.0 paper-BuJo standard) is tuned for the
/// small Paper Pro Move: it fits a 31-day monthly log one-per-row and gives more
/// columns for trackers without crowding handwriting. Overridable per-year via config.
pub const DEFAULT_SPACING_MM: f32 = 4.5;
/// Default page margin in mm.
pub const DEFAULT_MARGIN_MM: f32 = 6.0;

pub fn default_grid(device: &Device) -> GridSpec {
    dot_grid(device, DEFAULT_SPACING_MM, DEFAULT_MARGIN_MM)
}
