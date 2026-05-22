//! Geometry primitives and device constants for reMarkable scenes.

/// Pixel width of the reMarkable Paper Pro canvas.
pub const SCREEN_WIDTH: f32 = 1404.0;
/// Pixel height of the reMarkable Paper Pro canvas.
pub const SCREEN_HEIGHT: f32 = 1872.0;
/// Nominal screen resolution in dots per inch.
pub const SCREEN_DPI: f32 = 226.0;

/// A single point along a stroke.
///
/// `x`/`y` are scene coordinates (centered around the page origin; they can be
/// negative). The remaining device telemetry is parsed when present but is
/// optional for geometry consumers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    /// Horizontal scene coordinate.
    pub x: f32,
    /// Vertical scene coordinate.
    pub y: f32,
    /// Pen speed sample (device units), if available.
    pub speed: Option<f32>,
    /// Pen direction sample (device units), if available.
    pub direction: Option<f32>,
    /// Stroke width sample (device units), if available.
    pub width: Option<f32>,
    /// Pen pressure sample (device units), if available.
    pub pressure: Option<f32>,
}
