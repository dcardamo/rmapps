//! v6 scene parsing: walks the tagged-block stream and extracts scene items.

mod items;
mod reader;

pub use items::{Pen, PenColor, SceneItem, Stroke};

use crate::error::Result;
use reader::Reader;

/// Block type for a `SceneLineItemBlock` (an ink stroke), per rmscene.
const BLOCK_TYPE_SCENE_LINE_ITEM: u8 = 0x05;
/// Item type byte inside a line item's value sub-block, per rmscene.
const ITEM_TYPE_LINE: u8 = 0x03;

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
            // The "Line item" is the only block we currently decode. Everything
            // else (and any trailing bytes within a Line block) is skipped by
            // seeking to the block's declared end.
            if header.block_type == BLOCK_TYPE_SCENE_LINE_ITEM {
                if let Some(stroke) = parse_scene_line_item(&mut r, header.current_version)? {
                    items.push(SceneItem::Line(stroke));
                }
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
