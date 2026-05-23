//! Low-level primitives for WRITING the reMarkable v6 tagged-block format.
//!
//! The byte-exact inverse of [`crate::scene::reader`]. A tag is a LEB128
//! varuint laid out as `(index << 4) | type`. Sub-blocks and block headers are
//! length-prefixed; the writer back-patches those lengths once the body is
//! known.

use crate::scene::reader::{TagType, HEADER_V6};

/// A growable byte buffer that emits the v6 format.
pub struct Writer {
    buf: Vec<u8>,
}

/// Opaque marker for an open sub-block whose length must be back-patched.
pub struct SubblockMark {
    len_pos: usize,
}

/// Opaque marker for an open top-level block whose size must be back-patched.
pub struct BlockMark {
    len_pos: usize,
}

fn tag_nibble(t: TagType) -> u8 {
    match t {
        TagType::Id => 0xF,
        TagType::Length4 => 0xC,
        TagType::Byte8 => 0x8,
        TagType::Byte4 => 0x4,
        TagType::Byte1 => 0x1,
    }
}

impl Writer {
    /// Create an empty writer.
    pub fn new() -> Writer {
        Writer { buf: Vec::new() }
    }

    /// Finished bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.buf
    }

    /// Consume and return the bytes.
    pub fn into_bytes(self) -> Vec<u8> {
        self.buf
    }

    /// Write the fixed 43-byte v6 header.
    pub fn write_header(&mut self) {
        self.buf.extend_from_slice(HEADER_V6);
    }

    fn write_u8(&mut self, v: u8) {
        self.buf.push(v);
    }
    fn write_u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    /// Write a raw little-endian u32 (no tag). Public for tests/sub-block bodies.
    pub fn write_raw_u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    fn write_f32(&mut self, v: f32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    fn write_f64(&mut self, v: f64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    /// Write a LEB128 unsigned varint.
    fn write_varuint(&mut self, mut v: u64) {
        loop {
            let mut byte = (v & 0x7F) as u8;
            v >>= 7;
            if v != 0 {
                byte |= 0x80;
            }
            self.buf.push(byte);
            if v == 0 {
                break;
            }
        }
    }

    fn write_tag(&mut self, index: u32, t: TagType) {
        let x = ((index as u64) << 4) | (tag_nibble(t) as u64);
        self.write_varuint(x);
    }

    /// Write a tagged CRDT id at `index`.
    pub fn write_id(&mut self, index: u32, part1: u8, part2: u64) {
        self.write_tag(index, TagType::Id);
        self.write_u8(part1);
        self.write_varuint(part2);
    }

    /// Write a tagged 4-byte unsigned integer at `index`.
    pub fn write_int(&mut self, index: u32, v: u32) {
        self.write_tag(index, TagType::Byte4);
        self.write_raw_u32(v);
    }

    /// Write a tagged 4-byte float at `index`.
    pub fn write_float(&mut self, index: u32, v: f32) {
        self.write_tag(index, TagType::Byte4);
        self.write_f32(v);
    }

    /// Write a tagged 8-byte double at `index`.
    pub fn write_double(&mut self, index: u32, v: f64) {
        self.write_tag(index, TagType::Byte8);
        self.write_f64(v);
    }

    /// Begin a length-prefixed sub-block at `index`. Reserves 4 bytes for the
    /// length, returns a mark to close it with [`Writer::end_subblock`].
    pub fn begin_subblock(&mut self, index: u32) -> SubblockMark {
        self.write_tag(index, TagType::Length4);
        let len_pos = self.buf.len();
        self.write_raw_u32(0); // placeholder
        SubblockMark { len_pos }
    }

    /// Close a sub-block, back-patching its length.
    pub fn end_subblock(&mut self, mark: SubblockMark) {
        let content_len = (self.buf.len() - mark.len_pos - 4) as u32;
        self.buf[mark.len_pos..mark.len_pos + 4].copy_from_slice(&content_len.to_le_bytes());
    }

    /// Write a length-prefixed UTF-8 string sub-block at `index`
    /// (varuint length, 1-byte is-ascii flag, then the bytes).
    pub fn write_string(&mut self, index: u32, s: &str) {
        let mark = self.begin_subblock(index);
        self.write_varuint(s.len() as u64);
        self.write_u8(u8::from(s.is_ascii()));
        self.buf.extend_from_slice(s.as_bytes());
        self.end_subblock(mark);
    }

    /// Begin a top-level block. Writes the 4-byte size placeholder, then the
    /// 4 header bytes (`unknown`, `min_version`, `current_version`, `block_type`).
    pub fn begin_block(
        &mut self,
        unknown: u8,
        min_version: u8,
        current_version: u8,
        block_type: u8,
    ) -> BlockMark {
        let len_pos = self.buf.len();
        self.write_raw_u32(0); // size placeholder
        self.write_u8(unknown);
        self.write_u8(min_version);
        self.write_u8(current_version);
        self.write_u8(block_type);
        BlockMark { len_pos }
    }

    /// Close a top-level block, back-patching its content size (the size field
    /// counts content bytes only — everything after the 8 header bytes).
    pub fn end_block(&mut self, mark: BlockMark) {
        // size = bytes after the 8-byte header (4 size bytes + 4 metadata bytes)
        let size = (self.buf.len() - mark.len_pos - 8) as u32;
        self.buf[mark.len_pos..mark.len_pos + 4].copy_from_slice(&size.to_le_bytes());
    }

    /// Point-telemetry writer used by the line-item writer (next task).
    pub(crate) fn write_point_v2(
        &mut self,
        x: f32,
        y: f32,
        speed: u16,
        width: u16,
        dir: u8,
        pressure: u8,
    ) {
        self.write_f32(x);
        self.write_f32(y);
        self.write_u16(speed);
        self.write_u16(width);
        self.write_u8(dir);
        self.write_u8(pressure);
    }
}

impl Default for Writer {
    fn default() -> Self {
        Writer::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::reader::Reader;

    #[test]
    fn header_round_trips() {
        let mut w = Writer::new();
        w.write_header();
        let mut r = Reader::new(w.as_bytes());
        assert_eq!(r.read_header().unwrap(), 6);
    }

    #[test]
    fn tagged_scalars_round_trip() {
        let mut w = Writer::new();
        w.write_int(1, 18);
        w.write_int(2, 9);
        w.write_double(3, 2.0);
        w.write_float(4, 0.0);
        let mut r = Reader::new(w.as_bytes());
        assert_eq!(r.read_int(1).unwrap(), 18);
        assert_eq!(r.read_int(2).unwrap(), 9);
        assert_eq!(r.read_double(3).unwrap(), 2.0);
        assert_eq!(r.read_float(4).unwrap(), 0.0);
    }

    #[test]
    fn id_round_trips() {
        let mut w = Writer::new();
        w.write_id(1, 0, 0);
        let mut r = Reader::new(w.as_bytes());
        let id = r.read_id(1).unwrap();
        assert_eq!((id.part1, id.part2), (0, 0));

        // Multi-byte varuint part2 (300 encodes as two LEB128 bytes).
        let mut w2 = Writer::new();
        w2.write_id(2, 7, 300);
        let mut r2 = Reader::new(w2.as_bytes());
        let id2 = r2.read_id(2).unwrap();
        assert_eq!((id2.part1, id2.part2), (7, 300));
    }

    #[test]
    fn subblock_length_is_backpatched() {
        let mut w = Writer::new();
        let sb = w.begin_subblock(5);
        w.write_raw_u32(0xDEADBEEF);
        w.end_subblock(sb);
        let mut r = Reader::new(w.as_bytes());
        let end = r.read_subblock(5).unwrap();
        assert_eq!(r.read_u32().unwrap(), 0xDEADBEEF);
        assert_eq!(r.pos(), end, "cursor lands at declared sub-block end");
    }

    #[test]
    fn string_round_trips() {
        let mut w = Writer::new();
        w.write_string(5, "lazy dog");
        let mut r = Reader::new(w.as_bytes());
        assert_eq!(r.read_string(5).unwrap(), "lazy dog");
    }

    #[test]
    fn block_header_size_is_backpatched() {
        let mut w = Writer::new();
        let b = w.begin_block(0, 2, 2, 0x05);
        w.write_int(1, 42);
        w.end_block(b);
        let mut r = Reader::new(w.as_bytes());
        let h = r.read_block_header().unwrap().unwrap();
        assert_eq!(h.block_type, 0x05);
        assert_eq!(h.current_version, 2);
        assert_eq!(r.read_int(1).unwrap(), 42);
        assert_eq!(r.pos(), h.end(), "cursor lands at declared block end");
    }
}
