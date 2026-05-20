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

pub fn default_grid(device: &Device) -> GridSpec {
    dot_grid(device, 5.0, 6.0)
}
