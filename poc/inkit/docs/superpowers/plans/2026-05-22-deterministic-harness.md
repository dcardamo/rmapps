# Deterministic Harness Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers-extended-cc:subagent-driven-development (recommended) or superpowers-extended-cc:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the device-agnostic framework core, a faithful `.rm` writer, an in-software loop simulator, a layers inspector, and two exerciser widgets — all provable under `make test` with no hardware and no network.

**Architecture:** Four crates. `rm-files` (renamed from `rmfiles`) gains a v6 writer that is the byte-exact inverse of its reader. `inkapp-core` is the device-agnostic framework: Typst render + manifest, the `Widget` trait, readback attribution + diffing, and a minimal `Device` trait. `inkapp-remarkable` implements `Device` (PDF↔scene transform + `.rm` read/write via `rm-files`). `inkapp-harness` runs the full loop in-process and renders a layers inspector image. The simulator drives the *real* writer→parse path so a passing test exercises the same bytes a device would.

**Tech Stack:** Rust (workspace, edition 2021), Typst 0.14 as a library (`typst`, `typst-pdf`, `typst-render`, `typst-assets`), `tiny-skia` for overlay compositing, `lopdf` for manifest embed/extract, `serde`/`serde_json`, existing `rm-files` reader.

---

## Critical conventions (read once, apply to every task)

- **Commit form (repo-specific):** this repo's `.githooks/pre-commit` runs `cargo fmt --check`, but a separate `pre-commit-check-tasks` hook miscounts native task IDs and will block a literal `git commit`. Commit with the flag form so the real fmt hook still runs:
  `git -c core.hooksPath=.githooks commit -m "..."`
- **No `Co-Authored-By` lines** in commit messages.
- **Run tests via nix:** `nix develop -c cargo test -p <crate>` (the `Makefile` wraps the all-workspace run as `make test`).
- **Crate name vs import path:** Cargo package `rm-files` imports in Rust as `rm_files` (hyphen → underscore).
- **Coordinate-transform honesty:** the reMarkable scene↔PDF transform in `inkapp-remarkable` (Task 8) is a *self-consistent model*, used symmetrically by `write_ink` and `read_ink`. Its fidelity to a real device is **out of scope here** — it is calibrated/validated against real recorded ink in Spec 3 (gesture fixtures) and on-device acceptance. The harness proves internal consistency (render → manifest → synthesize → readback → attribute), which does not depend on the transform matching hardware.

---

### Task 0: Rename `rmfiles` → `rm-files`

**Goal:** Rename the absorbed crate to `rm-files` (package), keeping its tests green, and prepare the workspace for the new crates.

**Files:**
- Rename dir: `crates/rmfiles/` → `crates/rm-files/`
- Modify: `crates/rm-files/Cargo.toml` (`name = "rm-files"`)
- Modify: `Cargo.toml` (workspace members)
- Modify: `crates/rm-files/tests/strokes.rs`, `tests/highlights.rs`, `tests/bundle.rs` (`use rmfiles::` → `use rm_files::`; `rmfiles::` paths → `rm_files::`)

**Acceptance Criteria:**
- [ ] `crates/rm-files/` exists; `crates/rmfiles/` does not.
- [ ] Package name is `rm-files`; all imports use `rm_files`.
- [ ] `nix develop -c cargo test -p rm-files` passes (all existing reader tests green).

**Verify:** `nix develop -c cargo test -p rm-files` → all existing tests pass.

**Steps:**

- [ ] **Step 1: Move the directory and update workspace members**

```bash
git mv crates/rmfiles crates/rm-files
```

Edit root `Cargo.toml`:

```toml
[workspace]
resolver = "2"
members = [
    "crates/rm-files",
    "crates/inkapp-core",
    "crates/inkapp-remarkable",
    "crates/inkapp-harness",
    "spikes/typst-readback",
]

[workspace.package]
edition = "2021"
license = "MIT"

[profile.release]
lto = true
```

- [ ] **Step 2: Rename the package**

Edit `crates/rm-files/Cargo.toml`: change `name = "rmfiles"` to `name = "rm-files"`. Leave everything else.

- [ ] **Step 3: Update test imports**

In each of `crates/rm-files/tests/{strokes,highlights,bundle}.rs`, replace every `rmfiles` identifier with `rm_files` (e.g. `use rm_files::{Pen, PenColor, Scene};`, `&rm_files::Stroke`, `rm_files::Error::...`).

- [ ] **Step 4: Verify existing tests still pass**

The other workspace members don't exist yet, so build just this crate:

Run: `nix develop -c cargo test -p rm-files`
Expected: PASS (reader/highlights/bundle tests all green).

> Note: `make test` (whole workspace) will fail until later tasks create the empty crates. That's expected; verify per-crate during Tasks 0–11.

- [ ] **Step 5: Commit**

```bash
git add -A
git -c core.hooksPath=.githooks commit -m "Rename rmfiles crate to rm-files"
```

---

### Task 1: `rm-files` writer primitives

**Goal:** A `Writer` struct that emits the v6 tagged-block byte format — the exact inverse of `scene/reader.rs` — with length back-patching for sub-blocks and block headers.

**Files:**
- Create: `crates/rm-files/src/scene/writer.rs`
- Modify: `crates/rm-files/src/scene/mod.rs` (add `mod writer;`)
- Test: in `crates/rm-files/src/scene/writer.rs` (`#[cfg(test)]`), round-trip each primitive through `reader::Reader`.

**Acceptance Criteria:**
- [ ] Every reader primitive (`read_int`, `read_double`, `read_float`, `read_id`, `read_string`, `read_subblock`, `read_block_header`, `read_header`) has a writer inverse.
- [ ] Sub-block and block-header lengths are back-patched correctly.
- [ ] Round-trip tests pass: write value(s) → read with `Reader` → equal.

**Verify:** `nix develop -c cargo test -p rm-files writer` → PASS.

**Steps:**

- [ ] **Step 1: Write failing round-trip tests**

Add to the bottom of the new `crates/rm-files/src/scene/writer.rs`:

```rust
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
    }

    #[test]
    fn subblock_length_is_backpatched() {
        let mut w = Writer::new();
        let sb = w.begin_subblock(5);
        w.write_raw_u32(0xDEADBEEF);
        w.end_subblock(sb);
        // Reader: open sub-block 5, read the u32, land exactly at the end.
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
        // Content is exactly the one tagged int we wrote.
        assert_eq!(r.read_int(1).unwrap(), 42);
        assert_eq!(r.pos(), h.end(), "cursor lands at declared block end");
    }
}
```

These reference `Reader`/`TagType` and `CrdtId` fields used by tests. To let the writer's tests use `Reader`, make the reader module visible to siblings: in `crates/rm-files/src/scene/mod.rs` change `mod reader;` to `pub(crate) mod reader;` and ensure `TagType`, `Reader`, `CrdtId`, `BlockHeader` are `pub` (they already are within the module — confirm `pub use` is not required since tests use the path `crate::scene::reader::...`).

- [ ] **Step 2: Run tests to verify they fail**

Run: `nix develop -c cargo test -p rm-files writer`
Expected: FAIL (compile error — `Writer` and its methods do not exist).

- [ ] **Step 3: Implement the writer primitives**

Top of `crates/rm-files/src/scene/writer.rs`:

```rust
//! Low-level primitives for WRITING the reMarkable v6 tagged-block format.
//!
//! The byte-exact inverse of [`crate::scene::reader`]. A tag is a LEB128
//! varuint laid out as `(index << 4) | type`. Sub-blocks and block headers are
//! length-prefixed; the writer back-patches those lengths once the body is
//! known.

use crate::scene::reader::TagType;

/// 43-byte ASCII header that prefixes every v6 `.rm` file.
const HEADER_V6: &[u8] = b"reMarkable .lines file, version=6          ";

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
        let size = (self.buf.len() - mark.len_pos - 8) as u32;
        self.buf[mark.len_pos..mark.len_pos + 4].copy_from_slice(&size.to_le_bytes());
    }

    // Point-telemetry writers used by the line-item writer (Task 2).
    pub(crate) fn write_point_v2(&mut self, x: f32, y: f32, speed: u16, width: u16, dir: u8, pressure: u8) {
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
```

> The block `size` semantics MUST match `Reader::read_block_header`, which sets `offset` to *after* the 8 header bytes and computes `end() = offset + size`. So `size` = content bytes after the header. The test `block_header_size_is_backpatched` pins this.

- [ ] **Step 4: Run tests to verify they pass**

Run: `nix develop -c cargo test -p rm-files writer`
Expected: PASS (all primitive round-trips green).

- [ ] **Step 5: Commit**

```bash
git add -A
git -c core.hooksPath=.githooks commit -m "rm-files: v6 writer primitives with length back-patching"
```

---

### Task 2: `rm-files` line-item writer + `write_scene` + validation

**Goal:** Emit `SceneLineItemBlock`s and a complete `.rm` file from `Stroke`s, then prove fidelity via synthetic round-trip and real-fixture round-trip.

**Files:**
- Modify: `crates/rm-files/src/scene/writer.rs` (line writer + `write_scene`)
- Modify: `crates/rm-files/src/scene/mod.rs` (re-export `write_scene`)
- Modify: `crates/rm-files/src/lib.rs` (`pub use scene::write_scene;`)
- Test: `crates/rm-files/tests/writer_roundtrip.rs`

**Acceptance Criteria:**
- [ ] `rm_files::write_scene(version, &[SceneItem]) -> Vec<u8>` produces a parser-valid v6 file.
- [ ] Synthetic strokes survive write → parse unchanged (geometry + tool + color).
- [ ] Strokes parsed from `stamped-labels.rmdoc`, re-written and re-parsed, equal the originals.

**Verify:** `nix develop -c cargo test -p rm-files writer_roundtrip` → PASS.

**Steps:**

- [ ] **Step 1: Write failing tests**

`crates/rm-files/tests/writer_roundtrip.rs`:

```rust
use std::io::Read;

use rm_files::{Pen, PenColor, Point, Scene, SceneItem, Stroke};

fn pt(x: f32, y: f32) -> Point {
    Point { x, y, speed: Some(0.0), direction: Some(0.0), width: Some(2.0), pressure: Some(0.0) }
}

#[test]
fn synthetic_strokes_round_trip() {
    let original = Stroke {
        tool: Pen::Highlighter2,
        color: PenColor::Highlight,
        points: vec![pt(-100.0, 50.0), pt(100.0, 50.0)],
    };
    let bytes = rm_files::write_scene(6, &[SceneItem::Line(original.clone())]);

    let scene = Scene::parse(&bytes).expect("parse written scene");
    assert_eq!(scene.version(), 6);
    let strokes = scene.strokes();
    assert_eq!(strokes.len(), 1);
    let got = strokes[0];
    assert_eq!(got.tool, Pen::Highlighter2);
    assert_eq!(got.color, PenColor::Highlight);
    let xs: Vec<f32> = got.points.iter().map(|p| p.x).collect();
    let ys: Vec<f32> = got.points.iter().map(|p| p.y).collect();
    assert_eq!(xs, vec![-100.0, 100.0]);
    assert_eq!(ys, vec![50.0, 50.0]);
}

fn load_fixture_bytes() -> Vec<u8> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/stamped-labels.rmdoc");
    let file = std::fs::File::open(path).expect("open rmdoc");
    let mut archive = zip::ZipArchive::new(file).expect("read zip");
    let rm_name = (0..archive.len())
        .map(|i| archive.by_index(i).unwrap().name().to_string())
        .find(|n| n.ends_with(".rm"))
        .expect(".rm entry");
    let mut e = archive.by_name(&rm_name).unwrap();
    let mut b = Vec::new();
    e.read_to_end(&mut b).unwrap();
    b
}

#[test]
fn real_fixture_strokes_round_trip() {
    let bytes = load_fixture_bytes();
    let original: Vec<Stroke> = Scene::parse(&bytes).unwrap().strokes().into_iter().cloned().collect();
    assert_eq!(original.len(), 4, "fixture has 4 strokes");

    let items: Vec<SceneItem> = original.iter().cloned().map(SceneItem::Line).collect();
    let rewritten = rm_files::write_scene(6, &items);

    let reparsed: Vec<Stroke> =
        Scene::parse(&rewritten).unwrap().strokes().into_iter().cloned().collect();

    assert_eq!(reparsed.len(), original.len());
    for (a, b) in original.iter().zip(&reparsed) {
        assert_eq!(a.tool, b.tool, "tool preserved");
        assert_eq!(a.color, b.color, "color preserved");
        let ax: Vec<i32> = a.points.iter().map(|p| p.x.round() as i32).collect();
        let bx: Vec<i32> = b.points.iter().map(|p| p.x.round() as i32).collect();
        let ay: Vec<i32> = a.points.iter().map(|p| p.y.round() as i32).collect();
        let by: Vec<i32> = b.points.iter().map(|p| p.y.round() as i32).collect();
        assert_eq!(ax, bx, "x geometry preserved");
        assert_eq!(ay, by, "y geometry preserved");
    }
}
```

This requires `Point` to be exported from `rm-files`. Add `Point` to the `pub use geometry::{...}` line in `lib.rs` if not already exported (it is: `pub use geometry::{Point, Rect, ...}`).

- [ ] **Step 2: Run to verify failure**

Run: `nix develop -c cargo test -p rm-files writer_roundtrip`
Expected: FAIL (`write_scene` not found).

- [ ] **Step 3: Implement the line-item writer and `write_scene`**

Append to `crates/rm-files/src/scene/writer.rs`:

```rust
use crate::scene::items::{Pen, SceneItem, Stroke};

/// Map a [`Pen`] back to its raw tool id (inverse of `Pen::from_id`).
fn pen_to_id(p: Pen) -> u32 {
    match p {
        Pen::Paintbrush1 => 0,
        Pen::Paintbrush2 => 12,
        Pen::Pencil1 => 1,
        Pen::Pencil2 => 14,
        Pen::Ballpoint1 => 2,
        Pen::Ballpoint2 => 15,
        Pen::Marker1 => 3,
        Pen::Marker2 => 16,
        Pen::Fineliner1 => 4,
        Pen::Fineliner2 => 17,
        Pen::Highlighter1 => 5,
        Pen::Highlighter2 => 18,
        Pen::Eraser => 6,
        Pen::EraserArea => 8,
        Pen::MechanicalPencil1 => 7,
        Pen::MechanicalPencil2 => 13,
        Pen::Calligraphy => 21,
        Pen::Shader => 23,
        Pen::Other(id) => id,
    }
}

/// Map a [`PenColor`] back to its raw color id (inverse of `PenColor::from_id`).
fn color_to_id(c: crate::scene::items::PenColor) -> u32 {
    use crate::scene::items::PenColor::*;
    match c {
        Black => 0, Gray => 1, White => 2, Yellow => 3, Green => 4, Pink => 5,
        Blue => 6, Red => 7, GrayOverlap => 8, Highlight => 9, Green2 => 10,
        Cyan => 11, Magenta => 12, Yellow2 => 13, Other(id) => id,
    }
}

const ITEM_TYPE_LINE: u8 = 0x03;
const BLOCK_TYPE_SCENE_LINE_ITEM: u8 = 0x05;

impl Writer {
    /// Write one `SceneLineItemBlock` for `stroke` using v2 point encoding.
    ///
    /// Mirrors `reader::parse_scene_line_item` framing: id(1), id(2), id(3),
    /// id(4), int(5)=deleted_length, then sub-block(6) = item_type byte + line
    /// body. The line body mirrors `items::read_line`: int(1)=tool, int(2)=color,
    /// double(3)=thickness, float(4)=starting_length, sub-block(5)=points.
    fn write_line_item(&mut self, stroke: &Stroke, item_id_counter: u64) {
        // current_version=2 selects 14-byte v2 points in the reader.
        let block = self.begin_block(0, 2, 2, BLOCK_TYPE_SCENE_LINE_ITEM);

        self.write_id(1, 0, 0); // parent_id (ignored by reader)
        self.write_id(2, 1, item_id_counter); // item_id
        self.write_id(3, 0, 0); // left_id
        self.write_id(4, 0, 0); // right_id
        self.write_int(5, 0); // deleted_length

        let value = self.begin_subblock(6);
        self.write_raw_u8_item_type(ITEM_TYPE_LINE);
        // Line body:
        self.write_int(1, pen_to_id(stroke.tool));
        self.write_int(2, color_to_id(stroke.color));
        self.write_double(3, 2.0); // thickness_scale (ignored)
        self.write_float(4, 0.0); // starting_length (ignored)
        let points = self.begin_subblock(5);
        for p in &stroke.points {
            let speed = p.speed.unwrap_or(0.0).round().clamp(0.0, u16::MAX as f32) as u16;
            let width = p.width.unwrap_or(2.0).round().clamp(0.0, u16::MAX as f32) as u16;
            let dir = p.direction.unwrap_or(0.0).round().clamp(0.0, u8::MAX as f32) as u8;
            let pressure = p.pressure.unwrap_or(0.0).round().clamp(0.0, u8::MAX as f32) as u8;
            self.write_point_v2(p.x, p.y, speed, width, dir, pressure);
        }
        self.end_subblock(points);
        self.end_subblock(value);

        self.end_block(block);
    }

    // Item-type byte sits raw at the start of the value sub-block (not tagged).
    fn write_raw_u8_item_type(&mut self, t: u8) {
        self.buf.push(t);
    }
}

/// Write a complete v6 `.rm` file from scene items. Only `Line` items are
/// emitted; other variants are skipped (the harness uses ink strokes).
pub fn write_scene(version: u32, items: &[SceneItem]) -> Vec<u8> {
    assert_eq!(version, 6, "only v6 output is supported");
    let mut w = Writer::new();
    w.write_header();
    let mut counter = 1u64;
    for item in items {
        if let SceneItem::Line(stroke) = item {
            w.write_line_item(stroke, counter);
            counter += 1;
        }
    }
    w.into_bytes()
}
```

`crates/rm-files/src/scene/mod.rs`: add `pub use writer::write_scene;` next to the existing `pub use items::{...}`.

`crates/rm-files/src/lib.rs`: add `pub use scene::write_scene;` to the `scene` re-export line.

- [ ] **Step 4: Run tests to verify they pass**

Run: `nix develop -c cargo test -p rm-files`
Expected: PASS (writer round-trips + all existing reader tests).

> If `real_fixture_strokes_round_trip` fails on the block header bytes, dump the fixture's first line-item block header (the 4 bytes after the u32 length) and set `begin_block`'s `unknown`/`min_version` to match; the reader only consults `current_version` (must stay 2), so this only affects byte-level parity, not the round-trip.

- [ ] **Step 5: Commit**

```bash
git add -A
git -c core.hooksPath=.githooks commit -m "rm-files: line-item writer, write_scene, round-trip validation"
```

---

### Task 3: `inkapp-core` scaffold + deterministic `World` + `compile_to_document`

**Goal:** New `inkapp-core` crate with a font-pinned, non-panicking Typst `World` and a single `compile_to_document` consumed by both PDF export and region recovery.

**Files:**
- Create: `crates/inkapp-core/Cargo.toml`
- Create: `crates/inkapp-core/src/lib.rs`
- Create: `crates/inkapp-core/src/world.rs`
- Create: `crates/inkapp-core/src/render.rs`
- Test: `crates/inkapp-core/tests/render.rs`

**Acceptance Criteria:**
- [ ] Fonts come from `typst-assets` (embedded), not system search; output is deterministic.
- [ ] `World::font` returns `Option` and never indexes-panics.
- [ ] `compile_to_document(src) -> Result<PagedDocument>` and `document_to_pdf(&doc) -> Result<Vec<u8>>` exist; the same source compiled twice yields identical PDF bytes.

**Verify:** `nix develop -c cargo test -p inkapp-core --test render` → PASS.

**Steps:**

- [ ] **Step 1: Create the crate manifest**

`crates/inkapp-core/Cargo.toml`:

```toml
[package]
name = "inkapp-core"
version = "0.1.0"
edition = "2021"
license = "MIT"
description = "Device-agnostic framework core for inkapp: Typst render, manifest, widgets, readback"

[dependencies]
typst = "0.14"
typst-pdf = "0.14"
typst-render = "0.14"
typst-assets = { version = "0.14", features = ["fonts"] }
comemo = "0.5"
thiserror = "2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
lopdf = "0.36"

[dev-dependencies]
lopdf = "0.36"
```

- [ ] **Step 2: Write a failing determinism test**

`crates/inkapp-core/tests/render.rs`:

```rust
use inkapp_core::render::{compile_to_document, document_to_pdf};

const SRC: &str = r#"#set page(width: 200pt, height: 200pt, margin: 10pt)
= Hello
Some body text."#;

#[test]
fn compiles_and_is_deterministic() {
    let d1 = compile_to_document(SRC).expect("compile 1");
    let p1 = document_to_pdf(&d1).expect("pdf 1");
    assert!(p1.starts_with(b"%PDF"), "produces a PDF");

    let d2 = compile_to_document(SRC).expect("compile 2");
    let p2 = document_to_pdf(&d2).expect("pdf 2");
    assert_eq!(p1, p2, "same source -> identical PDF bytes");
}
```

- [ ] **Step 3: Run to verify failure**

Run: `nix develop -c cargo test -p inkapp-core --test render`
Expected: FAIL (crate/functions missing).

- [ ] **Step 4: Implement `World`, `render`, error type, `lib.rs`**

`crates/inkapp-core/src/world.rs`:

```rust
use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime};
use typst::syntax::{FileId, Source, VirtualPath};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};

/// A Typst world backed by an in-memory main source and fonts embedded from
/// `typst-assets` (deterministic; no host font search).
pub struct InkWorld {
    library: LazyHash<Library>,
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
    main: Source,
}

impl InkWorld {
    pub fn new(src: &str) -> Self {
        let mut fonts = Vec::new();
        for data in typst_assets::fonts() {
            let bytes = Bytes::new(data.to_vec());
            // A single TTF/OTF file may contain multiple faces.
            for face in Font::iter(bytes) {
                fonts.push(face);
            }
        }
        let book = FontBook::from_fonts(&fonts);
        let main_id = FileId::new(None, VirtualPath::new("main.typ"));
        let main = Source::new(main_id, src.into());
        Self {
            library: LazyHash::new(Library::default()),
            book: LazyHash::new(book),
            fonts,
            main,
        }
    }
}

impl World for InkWorld {
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }
    fn book(&self) -> &LazyHash<FontBook> {
        &self.book
    }
    fn main(&self) -> FileId {
        self.main.id()
    }
    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main.id() {
            Ok(self.main.clone())
        } else {
            Err(FileError::NotFound(id.vpath().as_rootless_path().to_owned()))
        }
    }
    fn file(&self, id: FileId) -> FileResult<Bytes> {
        Err(FileError::NotFound(id.vpath().as_rootless_path().to_owned()))
    }
    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.get(index).cloned()
    }
    fn today(&self, _offset: Option<i64>) -> Option<Datetime> {
        None
    }
}
```

`crates/inkapp-core/src/render.rs`:

```rust
use typst::layout::PagedDocument;

use crate::error::{Error, Result};
use crate::world::InkWorld;

/// Compile Typst source to a laid-out document (shared by PDF export and region
/// recovery — the single compile path for the whole framework).
pub fn compile_to_document(src: &str) -> Result<PagedDocument> {
    let world = InkWorld::new(src);
    typst::compile::<PagedDocument>(&world)
        .output
        .map_err(|d| Error::Compile(format!("{d:?}")))
}

/// Export a laid-out document to PDF bytes.
pub fn document_to_pdf(doc: &PagedDocument) -> Result<Vec<u8>> {
    typst_pdf::pdf(doc, &typst_pdf::PdfOptions::default())
        .map_err(|d| Error::Pdf(format!("{d:?}")))
}
```

`crates/inkapp-core/src/error.rs`:

```rust
/// Errors produced by the framework core.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("typst compile failed: {0}")]
    Compile(String),
    #[error("typst pdf export failed: {0}")]
    Pdf(String),
    #[error("region recovery failed: {0}")]
    Region(String),
    #[error("manifest (de)serialisation failed: {0}")]
    Manifest(String),
}

pub type Result<T> = std::result::Result<T, Error>;
```

`crates/inkapp-core/src/lib.rs`:

```rust
//! inkapp-core — the device-agnostic framework: render, manifest, widgets,
//! readback, and the minimal `Device` seam.

pub mod error;
pub mod render;
pub mod world;

pub use error::{Error, Result};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `nix develop -c cargo test -p inkapp-core --test render`
Expected: PASS.

> If `Font::iter`/`Bytes::new` signatures differ in the pinned 0.14.x, adjust to the exact API (the spike used `typst-kit`; here we use `typst-assets` directly). The determinism assertion is the contract.

- [ ] **Step 6: Commit**

```bash
git add -A
git -c core.hooksPath=.githooks commit -m "inkapp-core: deterministic Typst World and single compile path"
```

---

### Task 4: Manifest, region recovery, per-page coordinate conversion

**Goal:** Recover labelled region rects from a compiled document in PDF-point coordinates using the *per-page* height, into a `Manifest`.

**Files:**
- Create: `crates/inkapp-core/src/manifest.rs`
- Create: `crates/inkapp-core/src/geometry.rs`
- Modify: `crates/inkapp-core/src/lib.rs`
- Test: `crates/inkapp-core/tests/regions.rs`
- Test fixture helper inline in the test.

**Acceptance Criteria:**
- [ ] `PdfRect`, `Region { name, page, rect }`, `Manifest { version, regions }` defined.
- [ ] `recover_regions(&PagedDocument) -> Result<Manifest>` recovers `<region>`-labelled metadata with **0.0pt** delta vs. the expected rect (port of the spike's Bar 1 check).
- [ ] Conversion uses the height of each region's own page (multi-page correctness).

**Verify:** `nix develop -c cargo test -p inkapp-core --test regions` → PASS.

**Steps:**

- [ ] **Step 1: Write failing region tests**

`crates/inkapp-core/tests/regions.rs`:

```rust
use inkapp_core::manifest::recover_regions;
use inkapp_core::render::compile_to_document;

// A 200x200pt page; a 50x30pt metadata-labelled region at top-left (20,20).
const SRC: &str = r#"#set page(width: 200pt, height: 200pt, margin: 0pt)
#place(top + left, dx: 20pt, dy: 20pt,
  box(width: 50pt, height: 30pt)[
    #metadata((name: "done", page: 0, x: 20.0, y: 20.0, w: 50.0, h: 30.0)) <region>
  ]
)"#;

#[test]
fn recovers_region_in_pdf_coords() {
    let doc = compile_to_document(SRC).unwrap();
    let manifest = recover_regions(&doc).unwrap();
    assert_eq!(manifest.regions.len(), 1);
    let r = &manifest.regions[0];
    assert_eq!(r.name, "done");
    assert_eq!(r.page, 0);
    // Typst top-left (20,20,50,30) on a 200pt-high page -> PDF bottom-left:
    //   x0=20, y0=200-(20+30)=150, x1=70, y1=200-20=180
    assert!((r.rect.x0 - 20.0).abs() < 1e-9, "x0");
    assert!((r.rect.y0 - 150.0).abs() < 1e-9, "y0");
    assert!((r.rect.x1 - 70.0).abs() < 1e-9, "x1");
    assert!((r.rect.y1 - 180.0).abs() < 1e-9, "y1");
}
```

> The `metadata` value here carries the rect declared by the author. In the spike, the author hand-declared coordinates that matched the placed box. Task 6's region-declaration helper will instead compute these from the introspector position so authors don't hand-write coordinates — but recovery (this task) reads whatever the metadata holds and converts page-relative. To prove conversion independent of placement, this test asserts the converted rect equals the by-hand PDF rect.

- [ ] **Step 2: Run to verify failure**

Run: `nix develop -c cargo test -p inkapp-core --test regions`
Expected: FAIL (`manifest` module missing).

- [ ] **Step 3: Implement geometry + manifest**

`crates/inkapp-core/src/geometry.rs`:

```rust
use serde::{Deserialize, Serialize};

/// A rectangle in PDF user space (bottom-left origin, y up), in points.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PdfRect {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl PdfRect {
    /// Whether a point (PDF space) lies within this rect (inclusive).
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x0 && x <= self.x1 && y >= self.y0 && y <= self.y1
    }

    /// Whether this rect overlaps `other`.
    pub fn overlaps(&self, other: &PdfRect) -> bool {
        self.x0 <= other.x1 && self.x1 >= other.x0 && self.y0 <= other.y1 && self.y1 >= other.y0
    }
}

/// A point in PDF user space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PdfPoint {
    pub x: f64,
    pub y: f64,
}

/// A point in a device's native ink space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DevicePoint {
    pub x: f64,
    pub y: f64,
}

/// Convert a Typst top-left-origin rect to a PDF bottom-left-origin rect using
/// the height of the rect's own page.
pub fn typst_to_pdf_rect(x: f64, y: f64, w: f64, h: f64, page_height_pt: f64) -> PdfRect {
    PdfRect {
        x0: x,
        y0: page_height_pt - (y + h),
        x1: x + w,
        y1: page_height_pt - y,
    }
}
```

`crates/inkapp-core/src/manifest.rs`:

```rust
use serde::{Deserialize, Serialize};
use typst::foundations::{Label, Selector};
use typst::introspection::MetadataElem;
use typst::layout::PagedDocument;
use typst::utils::PicoStr;

use crate::error::{Error, Result};
use crate::geometry::{typst_to_pdf_rect, PdfRect};

/// The raw metadata an author/widget emits next to a `<region>` label.
/// Coordinates are Typst-space (top-left origin), in points.
#[derive(Debug, Clone, Deserialize)]
struct RawRegion {
    name: String,
    page: usize,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

/// A labelled rectangle on a page, in PDF-point coordinates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Region {
    pub name: String,
    pub page: usize,
    pub rect: PdfRect,
}

/// The document's self-describing layout: regions plus a version marker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u64,
    pub regions: Vec<Region>,
}

/// Recover all `<region>`-labelled metadata from a compiled document and convert
/// each rect to PDF coordinates using its own page's height. `version` defaults
/// to 0; callers set it via [`Manifest::with_version`].
pub fn recover_regions(doc: &PagedDocument) -> Result<Manifest> {
    let page_heights: Vec<f64> = doc.pages.iter().map(|p| p.frame.height().to_pt()).collect();

    let label = Label::new(PicoStr::intern("region"))
        .ok_or_else(|| Error::Region("empty region label".into()))?;
    let elems = doc.introspector.query(&Selector::Label(label));

    let mut regions = Vec::with_capacity(elems.len());
    for elem in &elems {
        let packed = elem
            .to_packed::<MetadataElem>()
            .ok_or_else(|| Error::Region("labelled element is not metadata".into()))?;
        let json = serde_json::to_value(&packed.value).map_err(|e| Error::Region(e.to_string()))?;
        let raw: RawRegion = serde_json::from_value(json).map_err(|e| Error::Region(e.to_string()))?;
        let page_h = *page_heights
            .get(raw.page)
            .ok_or_else(|| Error::Region(format!("region '{}' references missing page {}", raw.name, raw.page)))?;
        regions.push(Region {
            name: raw.name,
            page: raw.page,
            rect: typst_to_pdf_rect(raw.x, raw.y, raw.w, raw.h, page_h),
        });
    }
    Ok(Manifest { version: 0, regions })
}

impl Manifest {
    /// Set the version marker (builder style).
    pub fn with_version(mut self, version: u64) -> Self {
        self.version = version;
        self
    }
}
```

`crates/inkapp-core/src/lib.rs`: add `pub mod geometry;` and `pub mod manifest;`, and re-export `pub use geometry::{PdfRect, PdfPoint, DevicePoint};` and `pub use manifest::{Manifest, Region};`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `nix develop -c cargo test -p inkapp-core --test regions`
Expected: PASS (0-delta region recovery).

- [ ] **Step 5: Commit**

```bash
git add -A
git -c core.hooksPath=.githooks commit -m "inkapp-core: manifest + per-page region recovery in PDF coords"
```

---

### Task 5: Embed manifest in the PDF + extract it

**Goal:** Carry the manifest *inside* the PDF so a handler can read layout back without recompiling.

**Files:**
- Create: `crates/inkapp-core/src/embed.rs`
- Modify: `crates/inkapp-core/src/lib.rs`
- Test: `crates/inkapp-core/tests/embed.rs`

**Acceptance Criteria:**
- [ ] `embed_manifest(pdf_bytes, &Manifest) -> Result<Vec<u8>>` writes the manifest JSON into the PDF's Info dictionary under a custom key.
- [ ] `extract_manifest(pdf_bytes) -> Result<Manifest>` reads it back.
- [ ] A manifest embedded then extracted equals the original.

**Verify:** `nix develop -c cargo test -p inkapp-core --test embed` → PASS.

**Steps:**

- [ ] **Step 1: Write failing test**

`crates/inkapp-core/tests/embed.rs`:

```rust
use inkapp_core::embed::{embed_manifest, extract_manifest};
use inkapp_core::geometry::PdfRect;
use inkapp_core::manifest::{Manifest, Region};
use inkapp_core::render::{compile_to_document, document_to_pdf};

#[test]
fn manifest_round_trips_through_pdf() {
    let doc = compile_to_document("#set page(width: 100pt, height: 100pt)\nhi").unwrap();
    let pdf = document_to_pdf(&doc).unwrap();

    let manifest = Manifest {
        version: 7,
        regions: vec![Region {
            name: "done".into(),
            page: 0,
            rect: PdfRect { x0: 1.0, y0: 2.0, x1: 3.0, y1: 4.0 },
        }],
    };

    let embedded = embed_manifest(&pdf, &manifest).unwrap();
    assert!(embedded.starts_with(b"%PDF"));
    let got = extract_manifest(&embedded).unwrap();
    assert_eq!(got, manifest);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `nix develop -c cargo test -p inkapp-core --test embed`
Expected: FAIL (`embed` module missing).

- [ ] **Step 3: Implement embed/extract with lopdf**

`crates/inkapp-core/src/embed.rs`:

```rust
use lopdf::{Dictionary, Document, Object};

use crate::error::{Error, Result};
use crate::manifest::Manifest;

/// Info-dictionary key under which the manifest JSON is stored.
const MANIFEST_KEY: &[u8] = b"InkappManifest";

/// Embed the manifest as JSON in the PDF's Info dictionary.
pub fn embed_manifest(pdf: &[u8], manifest: &Manifest) -> Result<Vec<u8>> {
    let json = serde_json::to_string(manifest).map_err(|e| Error::Manifest(e.to_string()))?;
    let mut doc = Document::load_mem(pdf).map_err(|e| Error::Manifest(e.to_string()))?;

    // Ensure an Info dictionary exists and is referenced by the trailer.
    let info_id = match doc.trailer.get(b"Info") {
        Ok(Object::Reference(id)) => *id,
        _ => {
            let id = doc.add_object(Object::Dictionary(Dictionary::new()));
            doc.trailer.set("Info", Object::Reference(id));
            id
        }
    };
    if let Ok(Object::Dictionary(info)) = doc.get_object_mut(info_id) {
        info.set(MANIFEST_KEY, Object::string_literal(json));
    } else {
        return Err(Error::Manifest("Info object is not a dictionary".into()));
    }

    let mut out = Vec::new();
    doc.save_to(&mut out).map_err(|e| Error::Manifest(e.to_string()))?;
    Ok(out)
}

/// Extract the manifest JSON from the PDF's Info dictionary.
pub fn extract_manifest(pdf: &[u8]) -> Result<Manifest> {
    let doc = Document::load_mem(pdf).map_err(|e| Error::Manifest(e.to_string()))?;
    let info_id = match doc.trailer.get(b"Info") {
        Ok(Object::Reference(id)) => *id,
        _ => return Err(Error::Manifest("no Info dictionary".into())),
    };
    let info = doc.get_object(info_id).and_then(|o| o.as_dict())
        .map_err(|e| Error::Manifest(e.to_string()))?;
    let raw = info.get(MANIFEST_KEY).and_then(|o| o.as_str())
        .map_err(|e| Error::Manifest(format!("manifest key missing: {e}")))?;
    serde_json::from_slice(raw).map_err(|e| Error::Manifest(e.to_string()))
}
```

`crates/inkapp-core/src/lib.rs`: add `pub mod embed;`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `nix develop -c cargo test -p inkapp-core --test embed`
Expected: PASS.

> If `lopdf` 0.36's `Object`/`Dictionary` accessor names differ (`as_str`/`as_dict`/`string_literal`), adjust to the pinned API; the round-trip equality is the contract.

- [ ] **Step 5: Commit**

```bash
git add -A
git -c core.hooksPath=.githooks commit -m "inkapp-core: embed/extract manifest in PDF Info dict"
```

---

### Task 6: `Widget` trait, region-declaration helper, `checkbox`

**Goal:** The keystone abstraction — a widget that renders Typst markup declaring its regions and interprets ink attributed to them — plus the trivial `checkbox`.

**Files:**
- Create: `crates/inkapp-core/src/widget.rs`
- Create: `crates/inkapp-core/src/ink.rs` (core stroke + `RegionInk` types)
- Create: `crates/inkapp-core/src/widgets/checkbox.rs`
- Create: `crates/inkapp-core/src/widgets/mod.rs`
- Modify: `crates/inkapp-core/src/lib.rs`
- Test: `crates/inkapp-core/tests/checkbox.rs`

**Acceptance Criteria:**
- [ ] `Stroke { points: Vec<PdfPoint>, highlighter: bool }` and `RegionInk { region: String, strokes: Vec<Stroke> }` defined.
- [ ] `Widget` trait with `render(&self, cx) -> String` and `read(&self, ink, manifest) -> Self::Output`.
- [ ] `region_metadata(name, page, x, y, w, h) -> String` emits the `#metadata(..) <region>` markup the recovery in Task 4 reads.
- [ ] `Checkbox::read` returns `true` iff a stroke point falls in its region.

**Verify:** `nix develop -c cargo test -p inkapp-core --test checkbox` → PASS.

**Steps:**

- [ ] **Step 1: Write failing checkbox tests**

`crates/inkapp-core/tests/checkbox.rs`:

```rust
use inkapp_core::geometry::{PdfPoint, PdfRect};
use inkapp_core::ink::{RegionInk, Stroke};
use inkapp_core::manifest::{Manifest, Region};
use inkapp_core::widget::Widget;
use inkapp_core::widgets::checkbox::Checkbox;

fn manifest_with(name: &str, rect: PdfRect) -> Manifest {
    Manifest { version: 1, regions: vec![Region { name: name.into(), page: 0, rect }] }
}

#[test]
fn checkbox_reads_true_when_marked() {
    let cb = Checkbox::new("done");
    let rect = PdfRect { x0: 10.0, y0: 10.0, x1: 30.0, y1: 30.0 };
    let manifest = manifest_with("done", rect);
    let ink = RegionInk {
        region: "done".into(),
        strokes: vec![Stroke { points: vec![PdfPoint { x: 20.0, y: 20.0 }], highlighter: false }],
    };
    assert!(cb.read(&[ink], &manifest));
}

#[test]
fn checkbox_reads_false_when_empty() {
    let cb = Checkbox::new("done");
    let rect = PdfRect { x0: 10.0, y0: 10.0, x1: 30.0, y1: 30.0 };
    let manifest = manifest_with("done", rect);
    assert!(!cb.read(&[], &manifest));
}

#[test]
fn checkbox_render_declares_its_region() {
    let cb = Checkbox::new("done");
    let markup = cb.render_at(0, 10.0, 10.0, 20.0, 20.0);
    assert!(markup.contains("<region>"), "declares a region label");
    assert!(markup.contains("\"done\"") || markup.contains("name: \"done\""), "names the region");
}
```

- [ ] **Step 2: Run to verify failure**

Run: `nix develop -c cargo test -p inkapp-core --test checkbox`
Expected: FAIL (modules missing).

- [ ] **Step 3: Implement ink types, widget trait, helper, checkbox**

`crates/inkapp-core/src/ink.rs`:

```rust
use crate::geometry::PdfPoint;

/// A device-agnostic ink stroke in PDF-point coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct Stroke {
    pub points: Vec<PdfPoint>,
    /// True if drawn with a highlighter tool (the only distinction widgets need).
    pub highlighter: bool,
}

impl Stroke {
    /// Axis-aligned bounding box `(x0, y0, x1, y1)` of the stroke, or `None` if empty.
    pub fn bbox(&self) -> Option<(f64, f64, f64, f64)> {
        let mut it = self.points.iter();
        let first = it.next()?;
        let (mut x0, mut y0, mut x1, mut y1) = (first.x, first.y, first.x, first.y);
        for p in it {
            x0 = x0.min(p.x);
            y0 = y0.min(p.y);
            x1 = x1.max(p.x);
            y1 = y1.max(p.y);
        }
        Some((x0, y0, x1, y1))
    }
}

/// Strokes attributed to one named region.
#[derive(Debug, Clone, PartialEq)]
pub struct RegionInk {
    pub region: String,
    pub strokes: Vec<Stroke>,
}
```

`crates/inkapp-core/src/widget.rs`:

```rust
use crate::ink::RegionInk;
use crate::manifest::Manifest;

/// A widget renders Typst markup that declares named regions, and interprets the
/// ink attributed to those regions. Render and readback co-located.
pub trait Widget {
    type Output;
    /// Emit Typst markup (including `<region>` metadata for each region).
    fn render(&self, cx: &mut RenderCx) -> String;
    /// Interpret the strokes attributed to this widget's region(s).
    fn read(&self, ink: &[RegionInk], manifest: &Manifest) -> Self::Output;
}

/// Render-time context: supplies the current page index and a monotonically
/// increasing id so widgets can mint unique region names if needed.
#[derive(Debug, Default)]
pub struct RenderCx {
    pub page: usize,
    next_id: u64,
}

impl RenderCx {
    pub fn new(page: usize) -> Self {
        Self { page, next_id: 0 }
    }
    pub fn fresh_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}

/// Emit the `#place`d metadata markup that [`crate::manifest::recover_regions`]
/// reads back. Coordinates are Typst-space (top-left origin) points.
pub fn region_metadata(name: &str, page: usize, x: f64, y: f64, w: f64, h: f64) -> String {
    format!(
        "#place(top + left, dx: {x}pt, dy: {y}pt, box(width: {w}pt, height: {h}pt)[#metadata((name: \"{name}\", page: {page}, x: {x}, y: {y}, w: {w}, h: {h})) <region>])\n"
    )
}
```

`crates/inkapp-core/src/widgets/mod.rs`:

```rust
pub mod checkbox;
```

`crates/inkapp-core/src/widgets/checkbox.rs`:

```rust
use crate::ink::RegionInk;
use crate::manifest::Manifest;
use crate::widget::{region_metadata, RenderCx, Widget};

/// A single tappable checkbox bound to a named region.
pub struct Checkbox {
    name: String,
}

impl Checkbox {
    pub fn new(name: &str) -> Self {
        Self { name: name.to_string() }
    }

    /// Render the checkbox glyph and its region at an explicit position
    /// (Typst-space points). Used directly by tests and apps that lay out
    /// absolutely; `render` wraps this with a default box.
    pub fn render_at(&self, page: usize, x: f64, y: f64, w: f64, h: f64) -> String {
        let mut s = region_metadata(&self.name, page, x, y, w, h);
        s.push_str(&format!(
            "#place(top + left, dx: {x}pt, dy: {y}pt, rect(width: {w}pt, height: {h}pt, stroke: 0.5pt))\n"
        ));
        s
    }
}

impl Widget for Checkbox {
    type Output = bool;

    fn render(&self, cx: &mut RenderCx) -> String {
        // Default placement; apps that need control call render_at directly.
        self.render_at(cx.page, 20.0, 40.0, 16.0, 16.0)
    }

    fn read(&self, ink: &[RegionInk], manifest: &Manifest) -> bool {
        let Some(region) = manifest.regions.iter().find(|r| r.name == self.name) else {
            return false;
        };
        ink.iter()
            .filter(|ri| ri.region == self.name)
            .flat_map(|ri| &ri.strokes)
            .flat_map(|s| &s.points)
            .any(|p| region.rect.contains(p.x, p.y))
    }
}
```

`crates/inkapp-core/src/lib.rs`: add `pub mod ink;`, `pub mod widget;`, `pub mod widgets;`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `nix develop -c cargo test -p inkapp-core --test checkbox`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git -c core.hooksPath=.githooks commit -m "inkapp-core: Widget trait, region helper, checkbox widget"
```

---

### Task 7: `Device` trait + readback attribution + diffing + stale-version guard

**Goal:** Map PDF-space strokes to regions, surface only *new* ink across cycles, and reject ink written against a stale manifest version.

**Files:**
- Create: `crates/inkapp-core/src/device.rs`
- Create: `crates/inkapp-core/src/readback.rs`
- Modify: `crates/inkapp-core/src/lib.rs`
- Test: `crates/inkapp-core/tests/readback.rs`

**Acceptance Criteria:**
- [ ] `Device` trait declared (4 methods) in core.
- [ ] `attribute(strokes, &Manifest) -> Vec<RegionInk>` groups strokes by containing region (a stroke is attributed to a region if any point is inside it).
- [ ] `diff_new(prev, current) -> Vec<Stroke>` returns strokes present in `current` but not `prev`.
- [ ] `guard_version(ink_version, &Manifest) -> Result<()>` errors when versions differ.

**Verify:** `nix develop -c cargo test -p inkapp-core --test readback` → PASS.

**Steps:**

- [ ] **Step 1: Write failing tests**

`crates/inkapp-core/tests/readback.rs`:

```rust
use inkapp_core::geometry::{PdfPoint, PdfRect};
use inkapp_core::ink::Stroke;
use inkapp_core::manifest::{Manifest, Region};
use inkapp_core::readback::{attribute, diff_new, guard_version};

fn stroke(x: f64, y: f64) -> Stroke {
    Stroke { points: vec![PdfPoint { x, y }], highlighter: false }
}

fn manifest() -> Manifest {
    Manifest {
        version: 3,
        regions: vec![
            Region { name: "a".into(), page: 0, rect: PdfRect { x0: 0.0, y0: 0.0, x1: 10.0, y1: 10.0 } },
            Region { name: "b".into(), page: 0, rect: PdfRect { x0: 20.0, y0: 20.0, x1: 30.0, y1: 30.0 } },
        ],
    }
}

#[test]
fn attributes_strokes_to_regions() {
    let m = manifest();
    let strokes = vec![stroke(5.0, 5.0), stroke(25.0, 25.0), stroke(100.0, 100.0)];
    let ink = attribute(&strokes, &m);
    let a = ink.iter().find(|ri| ri.region == "a").unwrap();
    let b = ink.iter().find(|ri| ri.region == "b").unwrap();
    assert_eq!(a.strokes.len(), 1);
    assert_eq!(b.strokes.len(), 1);
    // The (100,100) stroke matches no region and is dropped.
    assert_eq!(ink.iter().map(|ri| ri.strokes.len()).sum::<usize>(), 2);
}

#[test]
fn diff_returns_only_new_strokes() {
    let prev = vec![stroke(5.0, 5.0)];
    let current = vec![stroke(5.0, 5.0), stroke(25.0, 25.0)];
    let new = diff_new(&prev, &current);
    assert_eq!(new, vec![stroke(25.0, 25.0)]);
}

#[test]
fn stale_version_is_rejected() {
    let m = manifest(); // version 3
    assert!(guard_version(3, &m).is_ok());
    assert!(guard_version(2, &m).is_err());
}
```

- [ ] **Step 2: Run to verify failure**

Run: `nix develop -c cargo test -p inkapp-core --test readback`
Expected: FAIL (modules missing).

- [ ] **Step 3: Implement device trait + readback**

`crates/inkapp-core/src/device.rs`:

```rust
use crate::error::Result;
use crate::geometry::{DevicePoint, PdfPoint};
use crate::ink::Stroke;

/// The minimal device seam the harness substitutes ink at. Transport (sync) is
/// intentionally excluded — it is hardware and out of scope for the harness.
pub trait Device {
    /// Map a PDF-space point into this device's ink space.
    fn pdf_to_device(&self, p: PdfPoint, page_h_pt: f64) -> DevicePoint;
    /// Map a device-space point back to PDF space.
    fn device_to_pdf(&self, p: DevicePoint, page_h_pt: f64) -> PdfPoint;
    /// Parse native ink bytes into PDF-space strokes.
    fn read_ink(&self, bytes: &[u8], page_h_pt: f64) -> Result<Vec<Stroke>>;
    /// Synthesize native ink bytes from PDF-space strokes.
    fn write_ink(&self, strokes: &[Stroke], page_h_pt: f64) -> Result<Vec<u8>>;
}
```

`crates/inkapp-core/src/readback.rs`:

```rust
use crate::error::{Error, Result};
use crate::ink::{RegionInk, Stroke};
use crate::manifest::Manifest;

/// Group strokes by the region that contains them. A stroke is attributed to a
/// region if any of its points falls inside that region's rect. A stroke may
/// match multiple regions (e.g. overlapping); it is added to each. Strokes
/// matching no region are dropped. Output preserves manifest region order and
/// only includes regions that received at least one stroke.
pub fn attribute(strokes: &[Stroke], manifest: &Manifest) -> Vec<RegionInk> {
    let mut out: Vec<RegionInk> = Vec::new();
    for region in &manifest.regions {
        let mut matched = Vec::new();
        for s in strokes {
            if s.points.iter().any(|p| region.rect.contains(p.x, p.y)) {
                matched.push(s.clone());
            }
        }
        if !matched.is_empty() {
            out.push(RegionInk { region: region.name.clone(), strokes: matched });
        }
    }
    out
}

/// Return strokes present in `current` that are not in `prev` (by value).
pub fn diff_new(prev: &[Stroke], current: &[Stroke]) -> Vec<Stroke> {
    current.iter().filter(|s| !prev.contains(s)).cloned().collect()
}

/// Reject ink whose source version doesn't match the current manifest version.
pub fn guard_version(ink_version: u64, manifest: &Manifest) -> Result<()> {
    if ink_version == manifest.version {
        Ok(())
    } else {
        Err(Error::Manifest(format!(
            "stale ink: written against version {ink_version}, manifest is {}",
            manifest.version
        )))
    }
}
```

`crates/inkapp-core/src/lib.rs`: add `pub mod device;`, `pub mod readback;`, re-export `pub use device::Device;`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `nix develop -c cargo test -p inkapp-core --test readback`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git -c core.hooksPath=.githooks commit -m "inkapp-core: Device trait, attribution, diffing, version guard"
```

---

### Task 8: `inkapp-remarkable` — the reMarkable `Device` impl

**Goal:** Implement `Device` for reMarkable: a self-consistent PDF↔scene transform and `.rm` read/write via `rm-files`.

**Files:**
- Create: `crates/inkapp-remarkable/Cargo.toml`
- Create: `crates/inkapp-remarkable/src/lib.rs`
- Test: `crates/inkapp-remarkable/tests/device.rs`

**Acceptance Criteria:**
- [ ] `Remarkable::new()` with default canvas (1404×1872) and a fit-to-width transform.
- [ ] `pdf_to_device` then `device_to_pdf` is identity (within 1e-6) for any page height.
- [ ] `write_ink` then `read_ink` round-trips stroke geometry (PDF space) within tolerance, with `highlighter` preserved.

**Verify:** `nix develop -c cargo test -p inkapp-remarkable` → PASS.

**Steps:**

- [ ] **Step 1: Manifest**

`crates/inkapp-remarkable/Cargo.toml`:

```toml
[package]
name = "inkapp-remarkable"
version = "0.1.0"
edition = "2021"
license = "MIT"
description = "reMarkable Device implementation for inkapp (PDF<->scene transform, .rm read/write)"

[dependencies]
inkapp-core = { path = "../inkapp-core" }
rm-files = { path = "../rm-files" }
```

- [ ] **Step 2: Write failing tests**

`crates/inkapp-remarkable/tests/device.rs`:

```rust
use inkapp_core::device::Device;
use inkapp_core::geometry::{PdfPoint};
use inkapp_core::ink::Stroke;
use inkapp_remarkable::Remarkable;

const PAGE_H: f64 = 841.89; // A4 height in pt

#[test]
fn transform_is_invertible() {
    let rm = Remarkable::new();
    let p = PdfPoint { x: 123.0, y: 456.0 };
    let d = rm.pdf_to_device(p, PAGE_H);
    let back = rm.device_to_pdf(d, PAGE_H);
    assert!((back.x - p.x).abs() < 1e-6, "x inverts");
    assert!((back.y - p.y).abs() < 1e-6, "y inverts");
}

#[test]
fn ink_round_trips_through_rm() {
    let rm = Remarkable::new();
    let original = vec![Stroke {
        points: vec![PdfPoint { x: 50.0, y: 700.0 }, PdfPoint { x: 150.0, y: 700.0 }],
        highlighter: true,
    }];
    let bytes = rm.write_ink(&original, PAGE_H).unwrap();
    let got = rm.read_ink(&bytes, PAGE_H).unwrap();
    assert_eq!(got.len(), 1);
    assert!(got[0].highlighter, "highlighter flag preserved");
    for (a, b) in original[0].points.iter().zip(&got[0].points) {
        assert!((a.x - b.x).abs() < 0.5, "x within tolerance: {} vs {}", a.x, b.x);
        assert!((a.y - b.y).abs() < 0.5, "y within tolerance: {} vs {}", a.y, b.y);
    }
}
```

- [ ] **Step 3: Run to verify failure**

Run: `nix develop -c cargo test -p inkapp-remarkable`
Expected: FAIL (crate missing).

- [ ] **Step 4: Implement the device**

`crates/inkapp-remarkable/src/lib.rs`:

```rust
//! reMarkable implementation of the inkapp `Device` seam.
//!
//! The PDF<->scene transform here is a *self-consistent model* used symmetrically
//! by `write_ink`/`read_ink`. Fidelity to a real device is validated separately
//! by Spec 3 (gesture fixtures + on-device acceptance); the deterministic harness
//! only relies on `write_ink`/`read_ink` being mutual inverses.

use inkapp_core::device::Device;
use inkapp_core::error::Result;
use inkapp_core::geometry::{DevicePoint, PdfPoint};
use inkapp_core::ink::Stroke;
use rm_files::{Pen, PenColor, Point, Scene, SceneItem};

/// Default reMarkable Paper Pro canvas width/height in pixels.
const CANVAS_W: f64 = 1404.0;
const CANVAS_H: f64 = 1872.0;

/// A reMarkable device with a fit-to-width coordinate model.
pub struct Remarkable {
    canvas_w: f64,
    canvas_h: f64,
}

impl Remarkable {
    pub fn new() -> Self {
        Self { canvas_w: CANVAS_W, canvas_h: CANVAS_H }
    }

    /// Pixels-per-point: the page is fit to the canvas width. Scene space shares
    /// this scale on both axes; x is centered on the page, y runs from the top.
    fn scale(&self, page_w_pt: f64) -> f64 {
        self.canvas_w / page_w_pt
    }

    /// Page width in points implied by a page height, assuming the A-series-ish
    /// canvas aspect. The harness passes page height; width is derived so the
    /// model is fully determined by (page_h, canvas aspect).
    fn page_w_pt(&self, page_h_pt: f64) -> f64 {
        page_h_pt * (self.canvas_w / self.canvas_h)
    }
}

impl Default for Remarkable {
    fn default() -> Self {
        Remarkable::new()
    }
}

impl Device for Remarkable {
    fn pdf_to_device(&self, p: PdfPoint, page_h_pt: f64) -> DevicePoint {
        let page_w = self.page_w_pt(page_h_pt);
        let scale = self.scale(page_w);
        // x centered: scene_x = (pdf_x - page_w/2) * scale
        // y top-down: scene_y = (page_h - pdf_y) * scale
        DevicePoint {
            x: (p.x - page_w / 2.0) * scale,
            y: (page_h_pt - p.y) * scale,
        }
    }

    fn device_to_pdf(&self, p: DevicePoint, page_h_pt: f64) -> PdfPoint {
        let page_w = self.page_w_pt(page_h_pt);
        let scale = self.scale(page_w);
        PdfPoint {
            x: p.x / scale + page_w / 2.0,
            y: page_h_pt - p.y / scale,
        }
    }

    fn read_ink(&self, bytes: &[u8], page_h_pt: f64) -> Result<Vec<Stroke>> {
        let scene = Scene::parse(bytes)
            .map_err(|e| inkapp_core::error::Error::Manifest(format!("rm parse: {e}")))?;
        let mut out = Vec::new();
        for s in scene.strokes() {
            let points = s
                .points
                .iter()
                .map(|pt| self.device_to_pdf(DevicePoint { x: pt.x as f64, y: pt.y as f64 }, page_h_pt))
                .collect();
            out.push(Stroke { points, highlighter: s.is_highlighter() });
        }
        Ok(out)
    }

    fn write_ink(&self, strokes: &[Stroke], page_h_pt: f64) -> Result<Vec<u8>> {
        let items: Vec<SceneItem> = strokes
            .iter()
            .map(|s| {
                let points = s
                    .points
                    .iter()
                    .map(|p| {
                        let d = self.pdf_to_device(*p, page_h_pt);
                        Point {
                            x: d.x as f32,
                            y: d.y as f32,
                            speed: Some(0.0),
                            direction: Some(0.0),
                            width: Some(2.0),
                            pressure: Some(0.0),
                        }
                    })
                    .collect();
                let (tool, color) = if s.highlighter {
                    (Pen::Highlighter2, PenColor::Highlight)
                } else {
                    (Pen::Fineliner1, PenColor::Black)
                };
                SceneItem::Line(rm_files::Stroke { tool, color, points })
            })
            .collect();
        Ok(rm_files::write_scene(6, &items))
    }
}
```

> `inkapp_core::error::Error` is reused for device errors to avoid a second error type in the harness. If preferred, add an `Error::Device(String)` variant in Task 3's error enum and use it here.

- [ ] **Step 5: Run tests to verify they pass**

Run: `nix develop -c cargo test -p inkapp-remarkable`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add -A
git -c core.hooksPath=.githooks commit -m "inkapp-remarkable: reMarkable Device (transform + .rm read/write)"
```

---

### Task 9: `highlightable-text` widget + span-level region recovery (central risk)

**Goal:** Prove the region mechanism scales from blocks to per-token spans, and read which tokens a highlighter swipe covers.

**Files:**
- Create: `crates/inkapp-core/src/widgets/highlight_text.rs`
- Modify: `crates/inkapp-core/src/widgets/mod.rs`
- Test: `crates/inkapp-core/tests/highlight_text.rs`

**Acceptance Criteria:**
- [ ] `HighlightableText::new(tokens)` renders each token wrapped so its rect is individually recoverable as a region named `tok-<i>`.
- [ ] Rendering then `recover_regions` yields one region per token, left-to-right in reading order.
- [ ] `read` returns the set of token strings whose region rect overlaps any highlighter stroke's bbox (non-highlighter strokes ignored).

**Verify:** `nix develop -c cargo test -p inkapp-core --test highlight_text` → PASS.

**Steps:**

- [ ] **Step 1: Write failing tests**

`crates/inkapp-core/tests/highlight_text.rs`:

```rust
use inkapp_core::geometry::PdfPoint;
use inkapp_core::ink::{RegionInk, Stroke};
use inkapp_core::manifest::recover_regions;
use inkapp_core::render::compile_to_document;
use inkapp_core::widget::{RenderCx, Widget};
use inkapp_core::widgets::highlight_text::HighlightableText;

const TOKENS: &[&str] = &["the", "quick", "brown", "fox", "lazy", "dog"];

fn rendered_manifest(w: &HighlightableText) -> inkapp_core::manifest::Manifest {
    let mut cx = RenderCx::new(0);
    let body = w.render(&mut cx);
    let src = format!("#set page(width: 300pt, height: 120pt, margin: 10pt)\n{body}");
    let doc = compile_to_document(&src).unwrap();
    recover_regions(&doc).unwrap()
}

#[test]
fn renders_one_region_per_token() {
    let w = HighlightableText::new(TOKENS);
    let m = rendered_manifest(&w);
    let toks: Vec<&inkapp_core::manifest::Region> =
        m.regions.iter().filter(|r| r.name.starts_with("tok-")).collect();
    assert_eq!(toks.len(), TOKENS.len(), "one region per token");
    // Reading order: tok-0 left of tok-1 left of tok-2 ... on the same line region.
    for pair in toks.windows(2) {
        assert!(pair[0].rect.x0 <= pair[1].rect.x0, "tokens ordered left-to-right");
    }
}

#[test]
fn read_returns_highlighted_tokens() {
    let w = HighlightableText::new(TOKENS);
    let m = rendered_manifest(&w);

    // Build a highlighter swipe spanning the rects of "lazy" (idx 4) and "dog" (idx 5).
    let lazy = m.regions.iter().find(|r| r.name == "tok-4").unwrap().rect;
    let dog = m.regions.iter().find(|r| r.name == "tok-5").unwrap().rect;
    let y = (lazy.y0 + lazy.y1) / 2.0;
    let swipe = Stroke {
        points: vec![PdfPoint { x: lazy.x0, y }, PdfPoint { x: dog.x1, y }],
        highlighter: true,
    };

    // Attribute the swipe to every token region it overlaps (the simulator does
    // this in the full pipeline; here we feed both regions' ink directly).
    let ink = vec![
        RegionInk { region: "tok-4".into(), strokes: vec![swipe.clone()] },
        RegionInk { region: "tok-5".into(), strokes: vec![swipe] },
    ];

    let mut got = w.read(&ink, &m);
    got.sort();
    assert_eq!(got, vec!["dog".to_string(), "lazy".to_string()]);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `nix develop -c cargo test -p inkapp-core --test highlight_text`
Expected: FAIL (module missing).

- [ ] **Step 3: Implement the widget**

`crates/inkapp-core/src/widgets/highlight_text.rs`:

```rust
use crate::ink::RegionInk;
use crate::manifest::Manifest;
use crate::widget::{RenderCx, Widget};

/// A run of words, each individually highlightable. Each token is wrapped in a
/// labelled box whose laid-out rect is recovered as a region named `tok-<i>`.
pub struct HighlightableText {
    tokens: Vec<String>,
}

impl HighlightableText {
    pub fn new(tokens: &[&str]) -> Self {
        Self { tokens: tokens.iter().map(|t| t.to_string()).collect() }
    }
}

impl Widget for HighlightableText {
    /// The set of highlighted token strings.
    type Output = Vec<String>;

    fn render(&self, _cx: &mut RenderCx) -> String {
        // Each token: a context-expression box that emits its own position as
        // metadata. `context`/`here().position()` give the laid-out location;
        // `measure` gives its size. This is the span-level analog of the
        // block-level region pattern.
        let mut s = String::new();
        for (i, tok) in self.tokens.iter().enumerate() {
            s.push_str(&format!(
                "#box[#context {{ \
                   let p = here().position(); \
                   let m = measure([{tok}]); \
                   metadata((name: \"tok-{i}\", page: p.page - 1, \
                     x: p.x / 1pt, y: p.y / 1pt, \
                     w: m.width / 1pt, h: m.height / 1pt)) \
                 }}<region>{tok}] "
            ));
        }
        s.push('\n');
        s
    }

    fn read(&self, ink: &[RegionInk], manifest: &Manifest) -> Vec<String> {
        let mut out = Vec::new();
        for (i, tok) in self.tokens.iter().enumerate() {
            let name = format!("tok-{i}");
            let Some(region) = manifest.regions.iter().find(|r| r.name == name) else {
                continue;
            };
            let highlighted = ink
                .iter()
                .filter(|ri| ri.region == name)
                .flat_map(|ri| &ri.strokes)
                .filter(|s| s.highlighter)
                .any(|s| match s.bbox() {
                    Some((x0, y0, x1, y1)) => region.rect.overlaps(&crate::geometry::PdfRect { x0, y0, x1, y1 }),
                    None => false,
                });
            if highlighted {
                out.push(tok.clone());
            }
        }
        out
    }
}
```

`crates/inkapp-core/src/widgets/mod.rs`: add `pub mod highlight_text;`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `nix develop -c cargo test -p inkapp-core --test highlight_text`
Expected: PASS.

> **This is the spec's central technical risk.** If `here().position()`/`measure` inside `#context` do not yield correct per-token rects in the pinned Typst 0.14, this is the finding to surface (record it in a short note under `docs/superpowers/spikes/` and reconsider the readback model for fine-grained selection). Do not paper over it by widening tolerances.

- [ ] **Step 5: Commit**

```bash
git add -A
git -c core.hooksPath=.githooks commit -m "inkapp-core: highlightable-text widget with span-level regions"
```

---

### Task 10: `inkapp-harness` scaffold + layers inspector

**Goal:** Rasterize a page with `typst-render` and composite ink + region overlays into a single PNG — the one artifact for human eyes now and the Spec 3 vision model later.

**Files:**
- Create: `crates/inkapp-harness/Cargo.toml`
- Create: `crates/inkapp-harness/src/lib.rs`
- Create: `crates/inkapp-harness/src/inspector.rs`
- Test: `crates/inkapp-harness/tests/inspector.rs`

**Acceptance Criteria:**
- [ ] `inspect(&PagedDocument, &Manifest, &[Stroke]) -> Result<Vec<u8>>` returns PNG bytes.
- [ ] Region rects and ink strokes are drawn over the page raster.
- [ ] The PNG decodes to the expected pixel dimensions for the page; a golden snapshot of a fixed input matches.

**Verify:** `nix develop -c cargo test -p inkapp-harness --test inspector` → PASS.

**Steps:**

- [ ] **Step 1: Manifest**

`crates/inkapp-harness/Cargo.toml`:

```toml
[package]
name = "inkapp-harness"
version = "0.1.0"
edition = "2021"
license = "MIT"
description = "In-software loop simulator and layers inspector for inkapp"

[dependencies]
inkapp-core = { path = "../inkapp-core" }
inkapp-remarkable = { path = "../inkapp-remarkable" }
typst = "0.14"
typst-render = "0.14"
tiny-skia = "0.11"

[dev-dependencies]
image = { version = "0.25", default-features = false, features = ["png"] }
```

- [ ] **Step 2: Write failing test**

`crates/inkapp-harness/tests/inspector.rs`:

```rust
use inkapp_core::geometry::{PdfPoint, PdfRect};
use inkapp_core::ink::Stroke;
use inkapp_core::manifest::{Manifest, Region};
use inkapp_core::render::compile_to_document;
use inkapp_harness::inspector::inspect;

#[test]
fn produces_a_png_of_the_page() {
    let doc = compile_to_document("#set page(width: 100pt, height: 100pt, margin: 0pt)\nhi").unwrap();
    let manifest = Manifest {
        version: 1,
        regions: vec![Region {
            name: "a".into(),
            page: 0,
            rect: PdfRect { x0: 10.0, y0: 10.0, x1: 40.0, y1: 40.0 },
        }],
    };
    let ink = vec![Stroke { points: vec![PdfPoint { x: 15.0, y: 15.0 }, PdfPoint { x: 35.0, y: 35.0 }], highlighter: true }];

    let png = inspect(&doc, &manifest, &ink).expect("inspect");
    // Decode and check dimensions: at 2x scale a 100pt page -> 200x200 px.
    let img = image::load_from_memory(&png).expect("decode png");
    assert_eq!(img.width(), 200);
    assert_eq!(img.height(), 200);
}
```

- [ ] **Step 3: Run to verify failure**

Run: `nix develop -c cargo test -p inkapp-harness --test inspector`
Expected: FAIL (crate/function missing).

- [ ] **Step 4: Implement the inspector**

`crates/inkapp-harness/src/inspector.rs`:

```rust
use inkapp_core::error::{Error, Result};
use inkapp_core::ink::Stroke;
use inkapp_core::manifest::Manifest;
use tiny_skia::{Paint, PathBuilder, Pixmap, Stroke as SkStroke, Transform};
use typst::layout::{Abs, PagedDocument};

/// Pixels per point used for the inspector raster.
const SCALE: f32 = 2.0;

/// Render page 0 of `doc` and composite region rects (blue) and ink strokes
/// (red for pen, semi-transparent yellow for highlighter) over it. Returns PNG
/// bytes. PDF-space y (origin bottom-left) is flipped to image y (top-left).
pub fn inspect(doc: &PagedDocument, manifest: &Manifest, ink: &[Stroke]) -> Result<Vec<u8>> {
    let page = doc.pages.first().ok_or_else(|| Error::Region("no pages".into()))?;
    let page_h_pt = page.frame.height().to_pt() as f32;

    // typst-render returns a tiny_skia::Pixmap.
    let mut pixmap: Pixmap = typst_render::render(page, SCALE);

    // Region rectangles (blue outline).
    let mut blue = Paint::default();
    blue.set_color_rgba8(0, 80, 220, 255);
    for r in &manifest.regions {
        if r.page != 0 {
            continue;
        }
        let mut pb = PathBuilder::new();
        // Flip y: image_y = (page_h - pdf_y) * SCALE.
        let x0 = r.rect.x0 as f32 * SCALE;
        let x1 = r.rect.x1 as f32 * SCALE;
        let y_top = (page_h_pt - r.rect.y1 as f32) * SCALE;
        let y_bot = (page_h_pt - r.rect.y0 as f32) * SCALE;
        pb.move_to(x0, y_top);
        pb.line_to(x1, y_top);
        pb.line_to(x1, y_bot);
        pb.line_to(x0, y_bot);
        pb.close();
        if let Some(path) = pb.finish() {
            pixmap.stroke_path(&path, &blue, &SkStroke { width: 1.0, ..Default::default() }, Transform::identity(), None);
        }
    }

    // Ink strokes.
    for s in ink {
        let mut paint = Paint::default();
        if s.highlighter {
            paint.set_color_rgba8(230, 210, 0, 120);
        } else {
            paint.set_color_rgba8(220, 0, 0, 255);
        }
        let mut pb = PathBuilder::new();
        let mut started = false;
        for p in &s.points {
            let x = p.x as f32 * SCALE;
            let y = (page_h_pt - p.y as f32) * SCALE;
            if started {
                pb.line_to(x, y);
            } else {
                pb.move_to(x, y);
                started = true;
            }
        }
        if let Some(path) = pb.finish() {
            let w = if s.highlighter { 8.0 } else { 2.0 };
            pixmap.stroke_path(&path, &paint, &SkStroke { width: w, ..Default::default() }, Transform::identity(), None);
        }
    }

    pixmap.encode_png().map_err(|e| Error::Region(format!("png encode: {e}")))
}

// Keep the unused import meaningful across typst versions.
#[allow(unused_imports)]
use Abs as _AbsKeepImport;
```

`crates/inkapp-harness/src/lib.rs`:

```rust
//! inkapp-harness — in-software loop simulator and layers inspector.

pub mod inspector;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `nix develop -c cargo test -p inkapp-harness --test inspector`
Expected: PASS.

> Pin the exact `typst_render::render` signature for 0.14 (it may be `render(&page, scale)` or take a `RenderOptions`). The dimension assertion (200×200 for a 100pt page at 2×) is the contract; adjust the call to satisfy it. Remove the `_AbsKeepImport` shim if `Abs` is unused.

- [ ] **Step 6: Commit**

```bash
git add -A
git -c core.hooksPath=.githooks commit -m "inkapp-harness: layers inspector (typst-render + tiny-skia)"
```

---

### Task 11: The loop simulator

**Goal:** Run the full loop in-process through the real writer→parse path, driven by a gesture scenario, recording a per-step trace.

**Files:**
- Create: `crates/inkapp-harness/src/simulator.rs`
- Modify: `crates/inkapp-harness/src/lib.rs`
- Test: `crates/inkapp-harness/tests/simulator.rs`

**Acceptance Criteria:**
- [ ] `Scenario` lets a test declare gestures ("a stroke from A to B, highlighter or pen") targeted at a region by name.
- [ ] `simulate(render_src, &Manifest source, &Device, &Scenario) -> Result<StepTrace>` renders, synthesizes ink via `Device::write_ink`, reads it back via `Device::read_ink`, attributes + diffs, and returns the per-region ink plus the inspector PNG.
- [ ] A scenario placing a stroke inside a region yields that region in the readback.

**Verify:** `nix develop -c cargo test -p inkapp-harness --test simulator` → PASS.

**Steps:**

- [ ] **Step 1: Write failing test**

`crates/inkapp-harness/tests/simulator.rs`:

```rust
use inkapp_core::manifest::recover_regions;
use inkapp_core::render::compile_to_document;
use inkapp_core::widget::region_metadata;
use inkapp_harness::simulator::{simulate, Gesture, Scenario};
use inkapp_remarkable::Remarkable;

#[test]
fn stroke_in_region_is_read_back() {
    // Render a 200pt page with one region "done" at (20,40,16,16) Typst-space.
    let body = region_metadata("done", 0, 20.0, 40.0, 16.0, 16.0);
    let src = format!("#set page(width: 200pt, height: 200pt, margin: 0pt)\n{body}");

    let doc = compile_to_document(&src).unwrap();
    let manifest = recover_regions(&doc).unwrap().with_version(1);

    let device = Remarkable::new();
    let scenario = Scenario::new().mark("done", Gesture::Tap);

    let trace = simulate(&src, &manifest, &device, &scenario).expect("simulate");

    let done = trace.readback.iter().find(|ri| ri.region == "done");
    assert!(done.is_some(), "the 'done' region received ink");
    assert!(!trace.inspector_png.is_empty(), "an inspector image was produced");
}
```

- [ ] **Step 2: Run to verify failure**

Run: `nix develop -c cargo test -p inkapp-harness --test simulator`
Expected: FAIL (module missing).

- [ ] **Step 3: Implement the simulator**

`crates/inkapp-harness/src/simulator.rs`:

```rust
use inkapp_core::device::Device;
use inkapp_core::error::Result;
use inkapp_core::geometry::PdfPoint;
use inkapp_core::ink::{RegionInk, Stroke};
use inkapp_core::manifest::Manifest;
use inkapp_core::readback::attribute;
use inkapp_core::render::compile_to_document;

use crate::inspector::inspect;

/// A synthesized user gesture targeted at a region.
#[derive(Debug, Clone)]
pub enum Gesture {
    /// A single dot in the center of the region (pen).
    Tap,
    /// A horizontal highlighter swipe across the full region width.
    Swipe,
}

/// A script of gestures, each bound to a region name.
#[derive(Debug, Default)]
pub struct Scenario {
    steps: Vec<(String, Gesture)>,
}

impl Scenario {
    pub fn new() -> Self {
        Self::default()
    }
    /// Add a gesture targeting `region`.
    pub fn mark(mut self, region: &str, g: Gesture) -> Self {
        self.steps.push((region.to_string(), g));
        self
    }
}

/// The result of one simulated cycle.
pub struct StepTrace {
    /// All synthesized strokes (PDF space).
    pub strokes: Vec<Stroke>,
    /// Strokes attributed to regions.
    pub readback: Vec<RegionInk>,
    /// The composited inspector image (PNG bytes).
    pub inspector_png: Vec<u8>,
}

/// Synthesize strokes for a scenario against a manifest's regions.
fn synthesize(manifest: &Manifest, scenario: &Scenario) -> Vec<Stroke> {
    let mut strokes = Vec::new();
    for (region_name, gesture) in &scenario.steps {
        let Some(region) = manifest.regions.iter().find(|r| &r.name == region_name) else {
            continue;
        };
        let r = &region.rect;
        let cx = (r.x0 + r.x1) / 2.0;
        let cy = (r.y0 + r.y1) / 2.0;
        match gesture {
            Gesture::Tap => strokes.push(Stroke {
                points: vec![PdfPoint { x: cx, y: cy }],
                highlighter: false,
            }),
            Gesture::Swipe => strokes.push(Stroke {
                points: vec![PdfPoint { x: r.x0, y: cy }, PdfPoint { x: r.x1, y: cy }],
                highlighter: true,
            }),
        }
    }
    strokes
}

/// Run one loop cycle entirely in software, through the real writer→parse path.
pub fn simulate(
    render_src: &str,
    manifest: &Manifest,
    device: &dyn Device,
    scenario: &Scenario,
) -> Result<StepTrace> {
    let doc = compile_to_document(render_src)?;
    let page_h_pt = doc
        .pages
        .first()
        .map(|p| p.frame.height().to_pt())
        .unwrap_or(0.0);

    // Synthesize the user's ink (PDF space), round-trip it through the device's
    // real .rm write+read so the test exercises the byte path.
    let synthesized = synthesize(manifest, scenario);
    let bytes = device.write_ink(&synthesized, page_h_pt)?;
    let strokes = device.read_ink(&bytes, page_h_pt)?;

    let readback = attribute(&strokes, manifest);
    let inspector_png = inspect(&doc, manifest, &strokes)?;

    Ok(StepTrace { strokes, readback, inspector_png })
}
```

`crates/inkapp-harness/src/lib.rs`: add `pub mod simulator;`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `nix develop -c cargo test -p inkapp-harness --test simulator`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git -c core.hooksPath=.githooks commit -m "inkapp-harness: in-software loop simulator"
```

---

### Task 12: Exerciser e2e + final wiring + green `make test`

**Goal:** Drive `checkbox` and `highlightable-text` through the full simulator with golden inspector images, then confirm the whole workspace is green.

**Files:**
- Test: `crates/inkapp-harness/tests/exercisers.rs`
- Create: `crates/inkapp-harness/tests/golden/` (committed golden PNGs)
- Modify: `flake.nix` (note `typst-assets` provides fonts for core; keep `dejavu`/`noto` only for the spike)
- Modify: `docs/superpowers/spikes/2026-05-22-typst-readback-findings.md` (one line: superseded by `inkapp-core`)

**Acceptance Criteria:**
- [ ] A checkbox app: a `Tap` scenario on the box → `Checkbox::read` returns `true`; a no-gesture run → `false`.
- [ ] A highlightable-text app: a `Swipe` over the "lazy"/"dog" token regions → `HighlightableText::read` returns `{lazy, dog}`.
- [ ] Each exerciser writes its inspector PNG and asserts it byte-matches a committed golden image.
- [ ] `make test` (whole workspace) is green.

**Verify:** `make test` → all crates pass.

**Steps:**

- [ ] **Step 1: Write the exerciser tests (golden bootstrap allowed on first run)**

`crates/inkapp-harness/tests/exercisers.rs`:

```rust
use inkapp_core::manifest::recover_regions;
use inkapp_core::render::compile_to_document;
use inkapp_core::widget::{RenderCx, Widget};
use inkapp_core::widgets::checkbox::Checkbox;
use inkapp_core::widgets::highlight_text::HighlightableText;
use inkapp_harness::simulator::{simulate, Gesture, Scenario};
use inkapp_remarkable::Remarkable;

const TOKENS: &[&str] = &["the", "quick", "brown", "fox", "lazy", "dog"];

/// Compare `png` to the committed golden at `tests/golden/<name>.png`.
/// On first run (golden absent), write it and fail with a clear message so the
/// developer reviews and commits it.
fn assert_golden(name: &str, png: &[u8]) {
    let path = format!("{}/tests/golden/{name}.png", env!("CARGO_MANIFEST_DIR"));
    match std::fs::read(&path) {
        Ok(expected) => assert_eq!(png, expected.as_slice(), "inspector image differs from golden {name}"),
        Err(_) => {
            std::fs::create_dir_all(format!("{}/tests/golden", env!("CARGO_MANIFEST_DIR"))).unwrap();
            std::fs::write(&path, png).unwrap();
            panic!("golden {name} did not exist; wrote it — review and re-run");
        }
    }
}

#[test]
fn checkbox_exerciser() {
    let cb = Checkbox::new("done");
    let body = cb.render_at(0, 20.0, 40.0, 16.0, 16.0);
    let src = format!("#set page(width: 200pt, height: 200pt, margin: 0pt)\n{body}");
    let doc = compile_to_document(&src).unwrap();
    let manifest = recover_regions(&doc).unwrap().with_version(1);
    let device = Remarkable::new();

    let trace = simulate(&src, &manifest, &device, &Scenario::new().mark("done", Gesture::Tap)).unwrap();
    assert!(cb.read(&trace.readback, &manifest), "tap marks the checkbox");
    assert_golden("checkbox_marked", &trace.inspector_png);

    let empty = simulate(&src, &manifest, &device, &Scenario::new()).unwrap();
    assert!(!cb.read(&empty.readback, &manifest), "no gesture leaves it unmarked");
}

#[test]
fn highlight_exerciser() {
    let w = HighlightableText::new(TOKENS);
    let mut cx = RenderCx::new(0);
    let body = w.render(&mut cx);
    let src = format!("#set page(width: 300pt, height: 120pt, margin: 10pt)\n{body}");
    let doc = compile_to_document(&src).unwrap();
    let manifest = recover_regions(&doc).unwrap().with_version(1);
    let device = Remarkable::new();

    let scenario = Scenario::new().mark("tok-4", Gesture::Swipe).mark("tok-5", Gesture::Swipe);
    let trace = simulate(&src, &manifest, &device, &scenario).unwrap();

    let mut got = w.read(&trace.readback, &manifest);
    got.sort();
    assert_eq!(got, vec!["dog".to_string(), "lazy".to_string()]);
    assert_golden("highlight_lazy_dog", &trace.inspector_png);
}
```

- [ ] **Step 2: Run once to bootstrap goldens, review, then run to pass**

Run: `nix develop -c cargo test -p inkapp-harness --test exercisers`
Expected: first run FAILS writing two golden PNGs. Open `tests/golden/checkbox_marked.png` and `tests/golden/highlight_lazy_dog.png`, confirm the regions and ink are drawn where expected, then re-run:

Run: `nix develop -c cargo test -p inkapp-harness --test exercisers`
Expected: PASS (images now match goldens).

- [ ] **Step 3: Update flake note and supersede the spike findings**

In `flake.nix`, update the font comment to record that `inkapp-core` embeds fonts via `typst-assets` (deterministic) and the `dejavu`/`noto` packages remain only for the legacy spike. (No functional change required; the dev shell can keep the fonts.)

In `docs/superpowers/spikes/2026-05-22-typst-readback-findings.md`, append one line under the verdict: "Superseded by `inkapp-core` (Spec #2): the render + region recovery here are reimplemented there with per-page height and embedded fonts."

- [ ] **Step 4: Whole-workspace green**

Run: `make test`
Expected: PASS across `rm-files`, `inkapp-core`, `inkapp-remarkable`, `inkapp-harness`, and the spike.

Run: `make clippy`
Expected: no warnings (fix any that appear).

- [ ] **Step 5: Commit**

```bash
git add -A
git -c core.hooksPath=.githooks commit -m "inkapp-harness: checkbox + highlight exercisers with golden inspector images"
```

---

## Self-review notes (coverage map)

| Spec section | Task(s) |
|--------------|---------|
| Crate layout & `Device` seam | 0 (rename), 3/4/6/7 (core), 7 (Device trait), 8 (rm impl), 10/11 (harness) |
| Render + manifest extraction (findings-doc fixes) | 3 (single compile path, fonts, non-panic font), 4 (per-page height) |
| `.rm` writer + validation | 1 (primitives), 2 (line writer + synthetic & real-fixture round-trip) |
| Widget keystone | 6 (trait + checkbox), 9 (highlightable-text) |
| Readback + diffing + stale-version guard | 7 |
| Loop simulator | 11 |
| Layers inspector (one artifact, two audiences) | 10 |
| Exercisers + golden snapshots | 12 |
| Done-when: all crates build, writer validated, simulator round-trips both widgets, inspector PNGs, diffing+guard tested | 12 (make test green) covers integration; 2/7/10/11 cover the parts |

**Central risk** (span-level rects) is isolated in Task 9 with an explicit "surface the finding" instruction.
