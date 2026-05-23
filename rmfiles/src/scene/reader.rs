//! Low-level primitives for reading the reMarkable v6 tagged-block format.
//!
//! Ported from the rmscene Python reference parser
//! (`tagged_block_common.py` and `tagged_block_reader.py`). The v6 format is a
//! little-endian stream of length-delimited blocks, each containing tagged
//! values. A tag is a LEB128 varuint whose layout is `(index << 4) | type`.

use crate::error::{Error, Result};

/// 43-byte ASCII header that prefixes every v6 `.rm` file.
pub const HEADER_V6: &[u8] = b"reMarkable .lines file, version=6          ";

/// Common prefix shared by `.lines` headers of any version, used to read the
/// version number out of files we don't otherwise support.
const HEADER_PREFIX: &[u8] = b"reMarkable .lines file, version=";
/// Total header length is fixed at 43 bytes (prefix + version + padding).
const HEADER_LEN: usize = 43;

/// Tag types describing the kind of value following a tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagType {
    /// A CRDT id (`0xF`).
    Id,
    /// A length-prefixed sub-block (`0xC`).
    Length4,
    /// An 8-byte value, e.g. f64 (`0x8`).
    Byte8,
    /// A 4-byte value, e.g. u32/f32 (`0x4`).
    Byte4,
    /// A 1-byte value, e.g. bool/u8 (`0x1`).
    Byte1,
}

impl TagType {
    fn from_nibble(n: u8) -> Result<TagType> {
        match n {
            0xF => Ok(TagType::Id),
            0xC => Ok(TagType::Length4),
            0x8 => Ok(TagType::Byte8),
            0x4 => Ok(TagType::Byte4),
            0x1 => Ok(TagType::Byte1),
            other => Err(Error::Parse(format!("bad tag type 0x{other:X}"))),
        }
    }
}

/// A cursor over the raw bytes of a `.rm` file.
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

/// A CRDT id: a `(part1, part2)` pair. We don't interpret it, only skip it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CrdtId {
    /// First component (single byte).
    pub part1: u8,
    /// Second component (varuint).
    pub part2: u64,
}

/// Header describing a top-level block.
#[derive(Debug, Clone, Copy)]
pub struct BlockHeader {
    /// Byte offset of the first content byte (just past the 4-byte header).
    pub offset: usize,
    /// Declared length of the block content, in bytes.
    pub size: usize,
    /// Block type discriminant.
    pub block_type: u8,
    /// Minimum reader version required. Parsed from the header for completeness;
    /// not currently consulted when decoding.
    #[allow(dead_code)]
    pub min_version: u8,
    /// Version this block was written with.
    pub current_version: u8,
}

impl BlockHeader {
    /// Offset one past the last content byte of this block.
    pub fn end(&self) -> usize {
        self.offset + self.size
    }
}

impl<'a> Reader<'a> {
    /// Create a reader over `data`.
    pub fn new(data: &'a [u8]) -> Reader<'a> {
        Reader { data, pos: 0 }
    }

    /// Current cursor position.
    pub fn pos(&self) -> usize {
        self.pos
    }

    /// Whether the cursor has reached the end of the input.
    pub fn at_end(&self) -> bool {
        self.pos >= self.data.len()
    }

    /// Read and validate the v6 header.
    ///
    /// Returns the parsed version number on any well-formed `.lines` header so
    /// the caller can produce a precise [`Error::UnsupportedVersion`].
    pub fn read_header(&mut self) -> Result<u32> {
        if self.data.len() < HEADER_LEN {
            return Err(Error::BadHeader);
        }
        let header = &self.data[..HEADER_LEN];
        if !header.starts_with(HEADER_PREFIX) {
            return Err(Error::BadHeader);
        }
        // Version digits sit between the prefix and the trailing space padding.
        let version_str = std::str::from_utf8(&header[HEADER_PREFIX.len()..])
            .map_err(|_| Error::BadHeader)?
            .trim();
        let version: u32 = version_str.parse().map_err(|_| Error::BadHeader)?;
        self.pos = HEADER_LEN;
        if header != HEADER_V6 {
            return Err(Error::UnsupportedVersion(version));
        }
        Ok(version)
    }

    fn read_bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.pos + n > self.data.len() {
            return Err(Error::Parse(format!(
                "unexpected end of stream: wanted {n} bytes at offset {}",
                self.pos
            )));
        }
        let out = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(out)
    }

    /// Read a little-endian u8.
    pub fn read_u8(&mut self) -> Result<u8> {
        Ok(self.read_bytes(1)?[0])
    }

    /// Read a little-endian u16.
    pub fn read_u16(&mut self) -> Result<u16> {
        let b = self.read_bytes(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    /// Read a little-endian u32.
    pub fn read_u32(&mut self) -> Result<u32> {
        let b = self.read_bytes(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// Read a little-endian f32.
    pub fn read_f32(&mut self) -> Result<f32> {
        let b = self.read_bytes(4)?;
        Ok(f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// Read a little-endian f64.
    pub fn read_f64(&mut self) -> Result<f64> {
        let b = self.read_bytes(8)?;
        Ok(f64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    /// Read a LEB128-encoded unsigned varint.
    pub fn read_varuint(&mut self) -> Result<u64> {
        let mut shift = 0u32;
        let mut result = 0u64;
        loop {
            let byte = self.read_u8()?;
            result |= ((byte & 0x7F) as u64) << shift;
            shift += 7;
            if byte & 0x80 == 0 {
                break;
            }
            if shift >= 64 {
                return Err(Error::Parse("varuint too long".into()));
            }
        }
        Ok(result)
    }

    fn read_crdt_id(&mut self) -> Result<CrdtId> {
        let part1 = self.read_u8()?;
        let part2 = self.read_varuint()?;
        Ok(CrdtId { part1, part2 })
    }

    /// Read a tag, returning `(index, tag_type)` without further validation.
    fn read_tag_values(&mut self) -> Result<(u32, TagType)> {
        let x = self.read_varuint()?;
        let index = (x >> 4) as u32;
        let tag_type = TagType::from_nibble((x & 0xF) as u8)?;
        Ok((index, tag_type))
    }

    /// Peek whether the next tag matches `(index, tag_type)`, without advancing.
    pub fn check_tag(&self, index: u32, tag_type: TagType) -> bool {
        let mut probe = Reader {
            data: self.data,
            pos: self.pos,
        };
        match probe.read_tag_values() {
            Ok((i, t)) => i == index && t == tag_type,
            Err(_) => false,
        }
    }

    /// Read a tag, erroring if it doesn't match the expected index/type. The
    /// cursor is left unchanged on mismatch so optional reads can recover.
    fn read_tag(&mut self, index: u32, tag_type: TagType) -> Result<()> {
        let saved = self.pos;
        let (i, t) = self.read_tag_values()?;
        if i != index {
            self.pos = saved;
            return Err(Error::Parse(format!(
                "expected index {index}, got {i} at offset {saved}"
            )));
        }
        if t != tag_type {
            self.pos = saved;
            return Err(Error::Parse(format!(
                "expected tag {tag_type:?}, got {t:?} at offset {saved}"
            )));
        }
        Ok(())
    }

    /// Read a tagged CRDT id at `index`.
    pub fn read_id(&mut self, index: u32) -> Result<CrdtId> {
        self.read_tag(index, TagType::Id)?;
        self.read_crdt_id()
    }

    /// Read a tagged 4-byte unsigned integer at `index`.
    pub fn read_int(&mut self, index: u32) -> Result<u32> {
        self.read_tag(index, TagType::Byte4)?;
        self.read_u32()
    }

    /// Read a tagged 4-byte unsigned integer at `index` if present, else `None`.
    ///
    /// Mirrors rmscene's `read_int_optional`: the field is absent if the next
    /// tag doesn't match, in which case the cursor is left unmoved.
    pub fn read_int_optional(&mut self, index: u32) -> Option<u32> {
        if self.check_tag(index, TagType::Byte4) {
            self.read_int(index).ok()
        } else {
            None
        }
    }

    /// Read a length-prefixed UTF-8 string sub-block at `index`.
    ///
    /// Mirrors rmscene's `read_string`: a sub-block containing a varuint byte
    /// length, a 1-byte "is-ascii" flag, then that many UTF-8 bytes. Trailing
    /// bytes inside the sub-block are skipped via the declared sub-block length.
    pub fn read_string(&mut self, index: u32) -> Result<String> {
        let end = self.read_subblock(index)?;
        let len = self.read_varuint()? as usize;
        let _is_ascii = self.read_u8()?;
        if self.pos + len > end {
            return Err(Error::Parse(format!(
                "string length {len} overflows sub-block at offset {}",
                self.pos
            )));
        }
        let bytes = self.read_bytes(len)?;
        let s = std::str::from_utf8(bytes)
            .map_err(|e| Error::Parse(format!("invalid UTF-8 in string: {e}")))?
            .to_string();
        // Skip any trailing bytes inside the string sub-block.
        self.seek(end)?;
        Ok(s)
    }

    /// Read a tagged 4-byte float at `index`.
    pub fn read_float(&mut self, index: u32) -> Result<f32> {
        self.read_tag(index, TagType::Byte4)?;
        self.read_f32()
    }

    /// Read a tagged 8-byte double at `index`.
    pub fn read_double(&mut self, index: u32) -> Result<f64> {
        self.read_tag(index, TagType::Byte8)?;
        self.read_f64()
    }

    /// Read the 4-byte length header of a top-level block.
    ///
    /// Returns `Ok(None)` at clean end-of-stream (no more blocks).
    pub fn read_block_header(&mut self) -> Result<Option<BlockHeader>> {
        if self.at_end() {
            return Ok(None);
        }
        // A block needs at least the 4-byte length plus the 4 header bytes.
        if self.pos + 8 > self.data.len() {
            return Err(Error::Parse(format!(
                "truncated block header at offset {}",
                self.pos
            )));
        }
        let size = self.read_u32()? as usize;
        let _unknown = self.read_u8()?;
        let min_version = self.read_u8()?;
        let current_version = self.read_u8()?;
        let block_type = self.read_u8()?;
        let offset = self.pos;
        Ok(Some(BlockHeader {
            offset,
            size,
            block_type,
            min_version,
            current_version,
        }))
    }

    /// Open a length-prefixed sub-block at `index`, returning its end offset.
    ///
    /// The caller is responsible for advancing the cursor past the sub-block;
    /// use [`Reader::seek`] with the returned end offset to skip any trailing
    /// bytes the format leaves unread.
    pub fn read_subblock(&mut self, index: u32) -> Result<usize> {
        self.read_tag(index, TagType::Length4)?;
        let len = self.read_u32()? as usize;
        Ok(self.pos + len)
    }

    /// Seek the cursor to an absolute offset, tolerating skips past unknown
    /// trailing data (but never past the end of input).
    pub fn seek(&mut self, offset: usize) -> Result<()> {
        if offset > self.data.len() {
            return Err(Error::Parse(format!(
                "seek past end of stream: {offset} > {}",
                self.data.len()
            )));
        }
        self.pos = offset;
        Ok(())
    }
}
