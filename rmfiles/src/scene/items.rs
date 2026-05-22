//! Scene item types (strokes) and their decoders.

use crate::error::{Error, Result};
use crate::geometry::Point;
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
    /// Any tool id not yet recognized.
    Other(u8),
}

impl Pen {
    /// Map a raw tool id to a [`Pen`]. Ids follow rmscene's `Pen` enum.
    pub fn from_id(id: u8) -> Pen {
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
    /// Any color id not yet recognized.
    Other(u8),
}

impl PenColor {
    /// Map a raw color id to a [`PenColor`]. Ids follow rmscene's `PenColor`.
    pub fn from_id(id: u8) -> PenColor {
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

/// An item in the scene. Additive: more variants will be added over time.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum SceneItem {
    /// An ink stroke.
    Line(Stroke),
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
    let tool = Pen::from_id(r.read_int(1)? as u8);
    let color = PenColor::from_id(r.read_int(2)? as u8);
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
