//! Wire-protocol constants shared by the scene reader and writer.

/// Block type for a `SceneLineItemBlock` (an ink stroke), per rmscene.
pub const BLOCK_TYPE_SCENE_LINE_ITEM: u8 = 0x05;
/// Item type byte inside a line item's value sub-block, per rmscene.
pub(crate) const ITEM_TYPE_LINE: u8 = 0x03;
/// Block type for a `SceneGlyphItemBlock` (a text highlight), per rmscene.
pub(crate) const BLOCK_TYPE_SCENE_GLYPH_ITEM: u8 = 0x03;
/// Item type byte inside a glyph item's value sub-block, per rmscene.
pub(crate) const ITEM_TYPE_GLYPH: u8 = 0x01;
