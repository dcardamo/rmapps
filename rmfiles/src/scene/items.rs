//! Scene item types (strokes) and their decoders.

use crate::error::{Error, Result};
use crate::geometry::{Point, Rect};
use crate::scene::reader::Reader;

/// reMarkable pen / tool id, mirroring rmscene's `Pen` enum. Unknown ids are
/// preserved via [`Pen::Other`] so future tools don't break parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Pen {
    /// Brush, variant 1.
    Paintbrush1,
    /// Brush, variant 2.
    Paintbrush2,
    /// Pencil, variant 1.
    Pencil1,
    /// Pencil, variant 2.
    Pencil2,
    /// Ballpoint, variant 1.
    Ballpoint1,
    /// Ballpoint, variant 2.
    Ballpoint2,
    /// Marker, variant 1.
    Marker1,
    /// Marker, variant 2.
    Marker2,
    /// Fineliner, variant 1.
    Fineliner1,
    /// Fineliner, variant 2.
    Fineliner2,
    /// Highlighter, variant 1.
    Highlighter1,
    /// Highlighter, variant 2.
    Highlighter2,
    /// Eraser.
    Eraser,
    /// Area eraser.
    EraserArea,
    /// Mechanical pencil, variant 1.
    MechanicalPencil1,
    /// Mechanical pencil, variant 2.
    MechanicalPencil2,
    /// Calligraphy pen.
    Calligraphy,
    /// Shader.
    Shader,
    /// Any tool id not yet recognized. Stored as `u32` to avoid truncation of
    /// ids outside the 0–255 range.
    Other(u32),
}

impl Pen {
    /// Map a raw tool id to a [`Pen`]. Ids follow rmscene's `Pen` enum.
    ///
    /// Accepts the full `u32` returned by the wire decoder so that ids >255
    /// are preserved losslessly in [`Pen::Other`] rather than silently
    /// truncated by an `as u8` cast.
    pub fn from_id(id: u32) -> Pen {
        match id {
            0 => Pen::Paintbrush1,
            12 => Pen::Paintbrush2,
            1 => Pen::Pencil1,
            14 => Pen::Pencil2,
            2 => Pen::Ballpoint1,
            15 => Pen::Ballpoint2,
            3 => Pen::Marker1,
            16 => Pen::Marker2,
            4 => Pen::Fineliner1,
            17 => Pen::Fineliner2,
            5 => Pen::Highlighter1,
            18 => Pen::Highlighter2,
            6 => Pen::Eraser,
            8 => Pen::EraserArea,
            7 => Pen::MechanicalPencil1,
            13 => Pen::MechanicalPencil2,
            21 => Pen::Calligraphy,
            23 => Pen::Shader,
            other => Pen::Other(other),
        }
    }

    /// Whether this tool is one of the highlighters.
    pub fn is_highlighter(self) -> bool {
        matches!(self, Pen::Highlighter1 | Pen::Highlighter2)
    }
}

/// reMarkable pen color index, mirroring rmscene's `PenColor` enum. Unknown
/// values are preserved via [`PenColor::Other`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PenColor {
    /// Black.
    Black,
    /// Gray.
    Gray,
    /// White.
    White,
    /// Yellow.
    Yellow,
    /// Green.
    Green,
    /// Pink.
    Pink,
    /// Blue.
    Blue,
    /// Red.
    Red,
    /// Gray (overlap).
    GrayOverlap,
    /// Highlight (the actual RGBA may live in the optional color field).
    Highlight,
    /// Green, variant 2.
    Green2,
    /// Cyan.
    Cyan,
    /// Magenta.
    Magenta,
    /// Yellow, variant 2.
    Yellow2,
    /// Any color id not yet recognized. Stored as `u32` to avoid truncation of
    /// ids outside the 0–255 range.
    Other(u32),
}

impl PenColor {
    /// Map a raw color id to a [`PenColor`]. Ids follow rmscene's `PenColor`.
    ///
    /// Accepts the full `u32` returned by the wire decoder so that ids >255
    /// are preserved losslessly in [`PenColor::Other`] rather than silently
    /// truncated by an `as u8` cast.
    pub fn from_id(id: u32) -> PenColor {
        match id {
            0 => PenColor::Black,
            1 => PenColor::Gray,
            2 => PenColor::White,
            3 => PenColor::Yellow,
            4 => PenColor::Green,
            5 => PenColor::Pink,
            6 => PenColor::Blue,
            7 => PenColor::Red,
            8 => PenColor::GrayOverlap,
            9 => PenColor::Highlight,
            10 => PenColor::Green2,
            11 => PenColor::Cyan,
            12 => PenColor::Magenta,
            13 => PenColor::Yellow2,
            other => PenColor::Other(other),
        }
    }
}

/// An ink stroke (a v6 `Line` item): a tool, a color, and a polyline of points.
#[derive(Debug, Clone, PartialEq)]
pub struct Stroke {
    /// Tool used to draw the stroke.
    pub tool: Pen,
    /// Stroke color.
    pub color: PenColor,
    /// Points making up the stroke (scene coordinates).
    pub points: Vec<Point>,
}

impl Stroke {
    /// Whether the stroke was drawn with a highlighter.
    pub fn is_highlighter(&self) -> bool {
        self.tool.is_highlighter()
    }
}

/// A text highlight (a v6 `GlyphRange` item): the verbatim highlighted string,
/// its bounding rectangles, and its color.
///
/// Produced when the reMarkable highlighter's "snap to text" toggle is on and
/// the page has a real text layer — the device records the exact characters
/// covered rather than freehand ink. This is the cleanest read-back path.
#[derive(Debug, Clone, PartialEq)]
pub struct TextHighlight {
    /// The verbatim highlighted text.
    pub text: String,
    /// Bounding rectangles for the highlight (one per line run), device space.
    pub rectangles: Vec<Rect>,
    /// Highlight color index (typically [`PenColor::Highlight`], a generic
    /// marker — the actual chosen RGBA, when the device records it, is in
    /// [`color_rgba`](Self::color_rgba)).
    pub color: PenColor,
    /// The exact highlight color as packed `0xAARRGGBB`, when present. Newer
    /// firmware records the chosen highlighter color here; absent for the
    /// default highlight.
    pub color_rgba: Option<u32>,
}

/// An item in the scene. Additive: more variants will be added over time.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum SceneItem {
    /// An ink stroke.
    Line(Stroke),
    /// A snap-to-text highlight (`GlyphRange`).
    Highlight(TextHighlight),
}

/// Serialized size of one v2 point: f32 x, f32 y, u16 speed, u16 width,
/// u8 direction, u8 pressure = 14 bytes. v1 points are 24 bytes (all f32).
fn point_serialized_size(version: u8) -> Result<usize> {
    match version {
        1 => Ok(0x18),
        2 => Ok(0x0E),
        other => Err(Error::Parse(format!("unknown point version {other}"))),
    }
}

/// Read a single point given the line/block version.
fn read_point(r: &mut Reader, version: u8) -> Result<Point> {
    let x = r.read_f32()?;
    let y = r.read_f32()?;
    let (speed, direction, width, pressure) = if version == 1 {
        // v1 stores all telemetry as f32, scaled per ddvk's reader.
        let speed = r.read_f32()? * 4.0;
        let direction = 255.0 * r.read_f32()? / (std::f32::consts::PI * 2.0);
        let width = (r.read_f32()? * 4.0).round();
        let pressure = r.read_f32()? * 255.0;
        (speed, direction, width, pressure)
    } else {
        let speed = r.read_u16()? as f32;
        let width = r.read_u16()? as f32;
        let direction = r.read_u8()? as f32;
        let pressure = r.read_u8()? as f32;
        (speed, direction, width, pressure)
    };
    Ok(Point {
        x,
        y,
        speed: Some(speed),
        direction: Some(direction),
        width: Some(width),
        pressure: Some(pressure),
    })
}

/// Decode a `Line` item body. Field order mirrors rmscene's `line_from_stream`:
/// tool(1), color(2), thickness_scale(3), starting_length(4), points sub-block(5).
/// Trailing fields (timestamp, move_id, color_rgba) are tolerated but ignored;
/// the caller seeks past any unread bytes using the value sub-block length.
pub fn read_line(r: &mut Reader, version: u8) -> Result<Stroke> {
    // Pass the raw u32 directly — from_id handles any value, including ids
    // >255 that would have been silently truncated by an `as u8` cast.
    let tool = Pen::from_id(r.read_int(1)?);
    let color = PenColor::from_id(r.read_int(2)?);
    let _thickness_scale = r.read_double(3)?;
    let _starting_length = r.read_float(4)?;

    let points_end = r.read_subblock(5)?;
    let data_length = points_end - r.pos();
    let point_size = point_serialized_size(version)?;
    if !data_length.is_multiple_of(point_size) {
        return Err(Error::Parse(format!(
            "point data size {data_length} not a multiple of point size {point_size}"
        )));
    }
    let num_points = data_length / point_size;
    let mut points = Vec::with_capacity(num_points);
    for _ in 0..num_points {
        points.push(read_point(r, version)?);
    }
    // Skip any trailing bytes inside the points sub-block (defensive).
    r.seek(points_end)?;

    Ok(Stroke {
        tool,
        color,
        points,
    })
}

/// Decode a `GlyphRange` item body. Field order mirrors rmscene's
/// `glyph_range_from_stream`:
///   start(2, optional), length(3, optional), color(4), text(5),
///   rectangles sub-block(6), color_rgba(10, optional).
///
/// `start`/`length` are only present in older block versions (pre-3.6); they
/// are read optionally and ignored here. The rectangles sub-block holds a
/// varuint count followed by that many `{x,y,w,h}` f64 quads. Trailing/newer
/// fields (e.g. `color_rgba`) are tolerated; the caller seeks past any unread
/// bytes using the value sub-block length.
pub fn read_glyph_range(r: &mut Reader) -> Result<TextHighlight> {
    let _start = r.read_int_optional(2);
    let _length = r.read_int_optional(3);
    let color = PenColor::from_id(r.read_int(4)?);
    let text = r.read_string(5)?;

    let rects_end = r.read_subblock(6)?;
    let num_rects = r.read_varuint()? as usize;
    let mut rectangles = Vec::with_capacity(num_rects);
    for _ in 0..num_rects {
        let x = r.read_f64()?;
        let y = r.read_f64()?;
        let w = r.read_f64()?;
        let h = r.read_f64()?;
        rectangles.push(Rect { x, y, w, h });
    }
    // Skip any trailing bytes inside the rectangles sub-block (defensive).
    r.seek(rects_end)?;

    // Optional exact color (field 10, `color_rgba`) — present on newer firmware
    // when a non-default highlighter color was used.
    let color_rgba = r.read_int_optional(10);

    Ok(TextHighlight {
        text,
        rectangles,
        color,
        color_rgba,
    })
}
