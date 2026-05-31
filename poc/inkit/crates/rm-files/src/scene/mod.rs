//! v6 scene parsing: walks the tagged-block stream and extracts scene items.

mod items;
pub(crate) mod protocol;
pub(crate) mod reader;
pub(crate) mod writer;

pub use items::{Pen, PenColor, SceneItem, Stroke, TextHighlight};
pub use protocol::BLOCK_TYPE_SCENE_LINE_ITEM;
pub use writer::write_scene;

use crate::error::Result;
use protocol::{BLOCK_TYPE_SCENE_GLYPH_ITEM, ITEM_TYPE_GLYPH, ITEM_TYPE_LINE};
use reader::Reader;

/// A parsed reMarkable scene: its format version plus its decoded items.
#[derive(Debug, Clone)]
pub struct Scene {
    version: u32,
    items: Vec<SceneItem>,
}

impl Scene {
    /// Parse a v6 `.rm` file from raw bytes.
    ///
    /// Detects the 43-byte header, then walks the top-level block stream. Each
    /// block's declared length bounds how far we read; unknown block types and
    /// trailing/newer-format data are skipped by seeking to the block end.
    pub fn parse(bytes: &[u8]) -> Result<Scene> {
        let mut r = Reader::new(bytes);
        let version = r.read_header()?;

        let mut items = Vec::new();
        while let Some(header) = r.read_block_header()? {
            // We decode "Line item" (ink) and "Glyph item" (snap-to-text
            // highlight) blocks. Everything else (and any trailing bytes within
            // a decoded block) is skipped by seeking to the block's declared end.
            match header.block_type {
                BLOCK_TYPE_SCENE_LINE_ITEM => {
                    if let Some(stroke) = parse_scene_line_item(&mut r, header.current_version)? {
                        items.push(SceneItem::Line(stroke));
                    }
                }
                BLOCK_TYPE_SCENE_GLYPH_ITEM => {
                    if let Some(hl) = parse_scene_glyph_item(&mut r)? {
                        items.push(SceneItem::Highlight(hl));
                    }
                }
                _ => {}
            }
            r.seek(header.end())?;
        }

        Ok(Scene { version, items })
    }

    /// The detected scene format version (6 for supported files).
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Iterate over all decoded scene items.
    pub fn items(&self) -> impl Iterator<Item = &SceneItem> {
        self.items.iter()
    }

    /// All ink strokes (`Line` items) in the scene.
    ///
    /// Uses `filter_map` so adding new `SceneItem` variants in the future
    /// does not require updating this function.
    pub fn strokes(&self) -> Vec<&Stroke> {
        self.items
            .iter()
            .filter_map(|item| match item {
                SceneItem::Line(stroke) => Some(stroke),
                #[allow(unreachable_patterns)]
                _ => None,
            })
            .collect()
    }

    /// All snap-to-text highlights (`GlyphRange` items) in the scene.
    ///
    /// Uses `filter_map` so adding new `SceneItem` variants in the future
    /// does not require updating this function.
    pub fn text_highlights(&self) -> Vec<&TextHighlight> {
        self.items
            .iter()
            .filter_map(|item| match item {
                SceneItem::Highlight(hl) => Some(hl),
                #[allow(unreachable_patterns)]
                _ => None,
            })
            .collect()
    }
}

/// A structural summary of one top-level v6 block: its framing only, not its
/// decoded body. Used to compare writer output against real device files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockSummary {
    /// Block type byte (e.g. `0x05` for a scene line item).
    pub block_type: u8,
    /// The block's `current_version` header byte.
    pub current_version: u8,
    /// Content length in bytes (excludes the 8-byte block header).
    pub content_len: usize,
}

/// Walk the top-level block stream and summarise each block's framing without
/// decoding item bodies. A structural inspector for writer-vs-device comparison.
pub fn block_structure(bytes: &[u8]) -> Result<Vec<BlockSummary>> {
    let mut r = Reader::new(bytes);
    let _version = r.read_header()?;
    let mut summaries = Vec::new();
    while let Some(h) = r.read_block_header()? {
        summaries.push(BlockSummary {
            block_type: h.block_type,
            current_version: h.current_version,
            content_len: h.size,
        });
        r.seek(h.end())?;
    }
    Ok(summaries)
}

/// Decode a `SceneLineItemBlock`. Mirrors rmscene's `SceneItemBlock`:
/// parent_id(1), item_id(2), left_id(3), right_id(4), deleted_length(5), then an
/// optional value sub-block(6) whose first byte is the item type, followed by
/// the `Line` body. Returns `Ok(None)` if the item carries no value (tombstone).
fn parse_scene_line_item(r: &mut Reader, version: u8) -> Result<Option<Stroke>> {
    let _parent_id = r.read_id(1)?;
    let _item_id = r.read_id(2)?;
    let _left_id = r.read_id(3)?;
    let _right_id = r.read_id(4)?;
    let _deleted_length = r.read_int(5)?;

    if !r.check_tag(6, reader::TagType::Length4) {
        return Ok(None);
    }
    let value_end = r.read_subblock(6)?;
    let item_type = r.read_u8()?;
    let stroke = if item_type == ITEM_TYPE_LINE {
        Some(items::read_line(r, version)?)
    } else {
        None
    };
    // Skip trailing/newer-format data inside the value sub-block.
    r.seek(value_end)?;
    Ok(stroke)
}

/// Decode a `SceneGlyphItemBlock`. Shares the `SceneItemBlock` framing with
/// line items: parent_id(1), item_id(2), left_id(3), right_id(4),
/// deleted_length(5), then an optional value sub-block(6) whose first byte is
/// the item type, followed by the `GlyphRange` body. Returns `Ok(None)` if the
/// item carries no value (tombstone).
fn parse_scene_glyph_item(r: &mut Reader) -> Result<Option<TextHighlight>> {
    let _parent_id = r.read_id(1)?;
    let _item_id = r.read_id(2)?;
    let _left_id = r.read_id(3)?;
    let _right_id = r.read_id(4)?;
    let _deleted_length = r.read_int(5)?;

    if !r.check_tag(6, reader::TagType::Length4) {
        return Ok(None);
    }
    let value_end = r.read_subblock(6)?;
    let item_type = r.read_u8()?;
    let highlight = if item_type == ITEM_TYPE_GLYPH {
        Some(items::read_glyph_range(r)?)
    } else {
        None
    };
    // Skip trailing/newer-format data inside the value sub-block.
    r.seek(value_end)?;
    Ok(highlight)
}
