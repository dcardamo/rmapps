# Annotation Read-back (rmfiles + rmreader) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers-extended-cc:subagent-driven-development (recommended) or superpowers-extended-cc:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the Reader PDFs a round-trip: the user triages (by highlighting an action-label word) and highlights text on the reMarkable; on the next sync `rmreader` reads those marks back from the downloaded bundle and applies them to Readwise, then regenerates and full-replaces a fresh PDF.

**Architecture:** A new reusable pure-Rust crate `rmfiles` (at `../rmfiles`) parses reMarkable `.rmdoc` bundles and v6 `.rm` scene files, exposing snap-to-text highlights. `rmreader` embeds a self-describing manifest *inside* each generated PDF (page→doc map + per-doc metadata), so the downloaded bundle is the single source of truth — no local state. The sync command becomes: `rmapi get` → read embedded manifest → extract highlights via `rmfiles` → classify (action vs content) → drive Readwise → regenerate → full-replace upload.

**Tech Stack:** Rust, `zip` + `serde_json` (rmfiles), `lopdf` (PDF embed/stamp, already a dep), `ureq` (Readwise HTTP), `fulgur` (render, unchanged), `rmapi` (cloud sync). Spec: `docs/superpowers/specs/2026-05-22-annotation-readback-design.md`.

**Reference parsers for the v6 format (consult for byte-exact layout):** `rmscene` (Python, authoritative — https://github.com/ricklupton/rmscene, `src/rmscene/`), `remarkable_lines` (Rust — https://github.com/Lyr-7D1h/remarkable-lines, `src/v6/`). We port only what we need; the fixture-driven tests lock correctness.

---

## Spikes are front-loaded and fixture-gated

Three risks are retired before the bulk of the feature is built:

- **Parser** (Task 1): can we extract highlight text from a real Paper Pro `.rm`? — gated by **fixture #1** (Dan highlights a known phrase on an existing PDF and captures it).
- **Snap-to-text on stamped labels + embedded-manifest preservation** (Task 5): does the device snap to our lopdf-stamped label text, and does our embedded manifest survive the cloud round trip? — gated by **fixture #2** (Dan uploads a prototype PDF, highlights a label + a body sentence, captures it).

Dan captures both fixtures; each ships with a `*.expected.json` describing what he highlighted, so assertions stay out of hand-edited Rust. If a spike fails, its task records the result and triggers the documented contingency before downstream tasks proceed.

---

## File Structure

**New crate `/home/dan/git/rmfiles/`:**

```
Cargo.toml
src/
  lib.rs            // re-exports + crate docs
  error.rs          // Error enum (thiserror)
  geometry.rs       // Rect, Color, device-space constants (1404×1872, SCALE, X_SHIFT)
  scene/
    mod.rs          // Scene, SceneItem (#[non_exhaustive]), version detect, items()/highlights()
    reader.rs       // BlockReader: LE ints, varuint, length-prefixed bytes, tagged subblocks
    items.rs        // Highlight struct; GlyphRange parse
  bundle/
    mod.rs          // Bundle::open (zip OR dir), Page, pages(), source_pdf()
    content.rs      // .content JSON (page order) — serde
    metadata.rs     // .metadata JSON — serde
tests/
  fixtures/         // real Paper Pro .rmdoc(s) + *.expected.json (committed)
  parse_rm.rs       // unit tests on raw .rm bytes
  highlights.rs     // fixture-driven: bundle -> highlights
  bundle.rs         // zip vs dir, page order, source_pdf, non-v6 error
```

**`rmreader` changes (`/home/dan/git/rmreader/`):**

```
src/readback/
  mod.rs            // orchestration: fetch -> read manifest -> extract -> classify -> execute
  classify.rs       // Plan, ActionKind, classification (pure)
src/embed.rs        // EmbeddedManifest write/read in a PDF (lopdf)
src/manifest.rs     // ManifestItem gains page_range/author/source_url/category; EmbeddedManifest type
src/postprocess.rs  // stamp action band on every article page; embed manifest; fill page_range
src/readwise/
  mod.rs            // update_location, delete_document, create_highlights, ActionKind mapping
  http.rs           // HttpTransport: method + optional body
src/deploy/
  mod.rs            // Deployer: + fetch, + replace
  rmapi.rs          // rmapi get / rm+put
  local.rs          // none backend fetch/replace
src/assemble.rs     // populate new ManifestItem fields
src/generate.rs     // build EmbeddedManifest; wire read-back into the sync flow
src/cli.rs          // sync flow (read-back before regenerate; full-replace)
src/config.rs       // default deploy folders -> /RMDev/Reader
src/wizard.rs       // folder prompt default
examples/spike_stamp.rs  // Task 5 prototype: stamp labels + embed manifest onto an existing PDF
```

---

## Task 1: rmfiles crate bootstrap + v6 highlight-text parser (PARSER SPIKE)

**Goal:** Stand up the `rmfiles` crate and parse a real Paper Pro `.rm` far enough to extract the *text* of snap-to-text highlights, proven against a real fixture.

**Files:**
- Create: `/home/dan/git/rmfiles/Cargo.toml`
- Create: `/home/dan/git/rmfiles/src/lib.rs`
- Create: `/home/dan/git/rmfiles/src/error.rs`
- Create: `/home/dan/git/rmfiles/src/scene/mod.rs`
- Create: `/home/dan/git/rmfiles/src/scene/reader.rs`
- Create: `/home/dan/git/rmfiles/src/scene/items.rs`
- Create: `/home/dan/git/rmfiles/tests/parse_rm.rs`
- Create: `/home/dan/git/rmfiles/tests/fixtures/README.md`
- Add (Dan captures): `/home/dan/git/rmfiles/tests/fixtures/highlight-basic.rmdoc` + `highlight-basic.expected.json`

**Acceptance Criteria:**
- [ ] `cargo test` in `../rmfiles` passes.
- [ ] A test extracts the exact highlighted string(s) listed in `highlight-basic.expected.json` from the fixture's `.rm` page(s).
- [ ] Non-v6 header input yields `Error::UnsupportedVersion`.

**Verify:** `cd /home/dan/git/rmfiles && cargo test --test parse_rm` → PASS

**Steps:**

- [ ] **Step 1: Dan captures fixture #1.** Take any PDF already in `/RMDev/Reader` (or upload the current `danout/Library.pdf`). On the Paper Pro, with the **highlighter + snap-to-text**, highlight one known phrase in an article body. Then:
  ```bash
  cd /home/dan/git/rmfiles/tests/fixtures
  rmapi get "/RMDev/Reader/Library"        # writes Library.rmdoc here
  mv Library.rmdoc highlight-basic.rmdoc
  ```
  Create `highlight-basic.expected.json` recording exactly what was highlighted:
  ```json
  { "highlights": ["the exact phrase I highlighted"] }
  ```
  Document the capture in `tests/fixtures/README.md` (device, software version, what was highlighted, date).

- [ ] **Step 2: Cargo.toml.**
  ```toml
  [package]
  name = "rmfiles"
  version = "0.1.0"
  edition = "2021"
  description = "Read reMarkable document bundles and v6 .rm scene files"
  license = "MIT"

  [dependencies]
  zip = { version = "2", default-features = false, features = ["deflate"] }
  serde = { version = "1", features = ["derive"] }
  serde_json = "1"
  thiserror = "2"

  [dev-dependencies]
  tempfile = "3"
  ```

- [ ] **Step 3: error.rs.**
  ```rust
  //! Error type for rmfiles.
  use thiserror::Error;

  #[derive(Debug, Error)]
  pub enum Error {
      #[error("io: {0}")]
      Io(#[from] std::io::Error),
      #[error("zip: {0}")]
      Zip(#[from] zip::result::ZipError),
      #[error("json: {0}")]
      Json(#[from] serde_json::Error),
      #[error("unsupported .rm version: {0}")]
      UnsupportedVersion(u32),
      #[error("not a reMarkable .rm file (bad header)")]
      BadHeader,
      #[error("parse error: {0}")]
      Parse(String),
  }

  pub type Result<T> = std::result::Result<T, Error>;
  ```

- [ ] **Step 4: scene/reader.rs — the low-level tagged-block reader.** Port the v6 binary primitives from rmscene's `tagged_block_common.py` / `tagged_block_reader.py`. v6 uses LEB128 varuints, fixed little-endian ints/floats, length-prefixed UTF-8 strings, and tagged sub-blocks (a tag byte = `(index << 4) | type`). Provide a cursor over `&[u8]`:
  ```rust
  //! Low-level reader for the v6 tagged-block binary format.
  //! Reference: rmscene/src/rmscene/tagged_block_common.py and tagged_block_reader.py.
  use crate::error::{Error, Result};

  pub struct BlockReader<'a> {
      buf: &'a [u8],
      pos: usize,
  }

  impl<'a> BlockReader<'a> {
      pub fn new(buf: &'a [u8]) -> Self { Self { buf, pos: 0 } }
      pub fn pos(&self) -> usize { self.pos }
      pub fn remaining(&self) -> usize { self.buf.len().saturating_sub(self.pos) }

      pub fn u8(&mut self) -> Result<u8> {
          let b = *self.buf.get(self.pos).ok_or(Error::Parse("eof u8".into()))?;
          self.pos += 1; Ok(b)
      }
      pub fn u32_le(&mut self) -> Result<u32> {
          let s = self.take(4)?;
          Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
      }
      pub fn f32_le(&mut self) -> Result<f32> {
          let s = self.take(4)?;
          Ok(f32::from_le_bytes([s[0], s[1], s[2], s[3]]))
      }
      pub fn f64_le(&mut self) -> Result<f64> {
          let s = self.take(8)?;
          let mut a = [0u8; 8]; a.copy_from_slice(s);
          Ok(f64::from_le_bytes(a))
      }
      /// LEB128 unsigned varint.
      pub fn varuint(&mut self) -> Result<u64> {
          let mut result: u64 = 0;
          let mut shift = 0;
          loop {
              let b = self.u8()?;
              result |= ((b & 0x7f) as u64) << shift;
              if b & 0x80 == 0 { break; }
              shift += 7;
              if shift >= 64 { return Err(Error::Parse("varuint overflow".into())); }
          }
          Ok(result)
      }
      pub fn take(&mut self, n: usize) -> Result<&'a [u8]> {
          let end = self.pos.checked_add(n).ok_or(Error::Parse("len overflow".into()))?;
          let s = self.buf.get(self.pos..end).ok_or(Error::Parse("eof take".into()))?;
          self.pos = end; Ok(s)
      }
      pub fn skip(&mut self, n: usize) -> Result<()> { self.take(n).map(|_| ()) }

      /// A tag byte: `(index << 4) | tag_type`. Returns (index, tag_type).
      pub fn read_tag(&mut self) -> Result<(u8, u8)> {
          let b = self.u8()?;
          Ok((b >> 4, b & 0x0f))
      }
  }
  ```
  NOTE: confirm the exact tag-type constants (Length4 / Byte1 / Byte4 / Byte8 / ID / etc.) and block-header layout against rmscene while implementing; the fixture test in Step 7 is the source of truth.

- [ ] **Step 5: scene/items.rs — the Highlight type + GlyphRange field order.** Port `GlyphRange` from rmscene `scene_items.py` (and `remarkable_lines/src/v6/scene_item/glyph_range.rs`). v6 GlyphRange fields: `start` (optional, only in SceneGlyphItemBlock version 0), `length` (varuint), `text` (length-prefixed UTF-8), `color`, then a list of `rectangles` (each `x,y,w,h` as f64). Use `crate::geometry::{Rect, Color}` (Task 2 finalizes geometry; for Task 1 a minimal local `Color`/`Rect` is fine if geometry.rs is not yet split — but prefer creating geometry.rs now with just what's needed).
  ```rust
  //! Scene items extracted from a v6 .rm file. v0.1 implements Highlight (GlyphRange).
  use crate::geometry::{Color, Rect};

  #[derive(Debug, Clone, PartialEq)]
  pub struct Highlight {
      pub text: String,
      pub rectangles: Vec<Rect>,
      pub color: Color,
  }

  #[derive(Debug, Clone, PartialEq)]
  #[non_exhaustive]
  pub enum SceneItem {
      Highlight(Highlight),
      // Line(Line)  — strokes, added when a consumer needs them
  }
  ```
  For Task 1 the `rectangles` may be parsed but only `text` is asserted; Task 2 asserts rectangles/color.

- [ ] **Step 6: scene/mod.rs — header detect + block walk + collect highlights.**
  ```rust
  //! Parse a v6 .rm "scene blocks" file.
  pub mod items;
  pub mod reader;

  use crate::error::{Error, Result};
  use items::{Highlight, SceneItem};
  use reader::BlockReader;

  /// 43-byte ASCII header, space-padded. Example v6:
  /// "reMarkable .lines file, version=6          "
  const HEADER_LEN: usize = 43;

  pub struct Scene {
      version: u32,
      items: Vec<SceneItem>,
  }

  impl Scene {
      pub fn parse(bytes: &[u8]) -> Result<Scene> {
          let version = parse_version(bytes)?;
          if version != 6 { return Err(Error::UnsupportedVersion(version)); }
          let mut r = BlockReader::new(&bytes[HEADER_LEN..]);
          let mut items = Vec::new();
          // Walk top-level blocks. Each block: <u32 LE data length><u8 unknown>
          // <u8 min_version><u8 block_type><payload...>. Dispatch SceneGlyphItemBlock
          // to GlyphRange parsing; skip all other block types by their length.
          // Reference: rmscene/src/rmscene/tagged_block_reader.py (read_block_header)
          // and scene_stream.py (block type ids). Confirm block_type id for
          // SceneGlyphItemBlock against rmscene while implementing.
          while r.remaining() >= 6 {
              let data_len = r.u32_le()? as usize;
              let _unknown = r.u8()?;
              let _min_version = r.u8()?;
              let block_type = r.u8()?;
              let start = r.pos();
              if is_glyph_item_block(block_type) {
                  if let Some(h) = parse_glyph_range(&mut r, start + data_len)? {
                      items.push(SceneItem::Highlight(h));
                  }
              }
              // Always resync to the declared end of this block.
              r.seek_to(start + data_len)?;
          }
          Ok(Scene { version, items })
      }

      pub fn version(&self) -> u32 { self.version }
      pub fn items(&self) -> impl Iterator<Item = &SceneItem> { self.items.iter() }
      pub fn highlights(&self) -> Vec<Highlight> {
          self.items.iter().filter_map(|i| match i {
              SceneItem::Highlight(h) => Some(h.clone()),
          }).collect()
      }
  }

  fn parse_version(bytes: &[u8]) -> Result<u32> {
      let head = bytes.get(..HEADER_LEN).ok_or(Error::BadHeader)?;
      let s = std::str::from_utf8(head).map_err(|_| Error::BadHeader)?;
      let v = s.split("version=").nth(1).ok_or(Error::BadHeader)?;
      v.trim().parse::<u32>().map_err(|_| Error::BadHeader)
  }
  ```
  Add `BlockReader::seek_to(&mut self, abs: usize)` to reader.rs. Implement `is_glyph_item_block` and `parse_glyph_range` per the rmscene reference; `parse_glyph_range` reads the tagged sub-blocks for `text`, `length`, `color`, `rectangles`.

- [ ] **Step 7: lib.rs.**
  ```rust
  //! rmfiles — read reMarkable document bundles and v6 .rm scene files.
  pub mod error;
  pub mod geometry;
  pub mod scene;

  pub use error::{Error, Result};
  pub use geometry::{Color, Rect};
  pub use scene::items::{Highlight, SceneItem};
  pub use scene::Scene;
  ```
  Create a minimal `geometry.rs` now (expanded in Task 2):
  ```rust
  //! Geometry + colour types in reMarkable device space.
  #[derive(Debug, Clone, Copy, PartialEq)]
  pub struct Rect { pub x: f64, pub y: f64, pub w: f64, pub h: f64 }

  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub struct Color { pub r: u8, pub g: u8, pub b: u8, pub a: u8 }
  ```

- [ ] **Step 8: Failing test, then implement until green.** `tests/parse_rm.rs`:
  ```rust
  use std::path::Path;

  // Reads the page .rm files out of the .rmdoc zip and asserts every expected
  // highlight string appears in some page's parsed highlights.
  #[test]
  fn extracts_known_highlight_text() {
      let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
      let rmdoc = dir.join("highlight-basic.rmdoc");
      let expected: serde_json::Value =
          serde_json::from_slice(&std::fs::read(dir.join("highlight-basic.expected.json")).unwrap()).unwrap();
      let want: Vec<String> = expected["highlights"].as_array().unwrap()
          .iter().map(|v| v.as_str().unwrap().to_string()).collect();

      // Pull every *.rm entry from the zip, parse, collect all highlight texts.
      let f = std::fs::File::open(&rmdoc).unwrap();
      let mut zip = zip::ZipArchive::new(f).unwrap();
      let mut got: Vec<String> = Vec::new();
      for i in 0..zip.len() {
          let mut e = zip.by_index(i).unwrap();
          if !e.name().ends_with(".rm") { continue; }
          let mut bytes = Vec::new();
          std::io::Read::read_to_end(&mut e, &mut bytes).unwrap();
          if let Ok(scene) = rmfiles::Scene::parse(&bytes) {
              got.extend(scene.highlights().into_iter().map(|h| h.text));
          }
      }
      for w in &want {
          assert!(got.iter().any(|g| g.contains(w)),
              "expected highlight {w:?} not found; got {got:?}");
      }
  }

  #[test]
  fn rejects_non_v6() {
      let bytes = b"reMarkable .lines file, version=5          \x00\x00";
      match rmfiles::Scene::parse(bytes) {
          Err(rmfiles::Error::UnsupportedVersion(5)) => {}
          other => panic!("expected UnsupportedVersion(5), got {other:?}"),
      }
  }
  ```
  Run `cargo test --test parse_rm -v`, expect FAIL, implement Steps 4–7 until PASS. Adjust byte-offset details against the fixture + rmscene reference as needed — the fixture is the oracle.

- [ ] **Step 9: Commit.**
  ```bash
  cd /home/dan/git/rmfiles
  git add -A && git commit -m "rmfiles: v6 .rm highlight-text parser + real fixture (parser spike)"
  ```

---

## Task 2: rmfiles — rectangles, colour, and the Scene API surface

**Goal:** Finalize `Highlight` with bounding rectangles + colour and the public `geometry`/`SceneItem` surface, asserted against the fixture.

**Files:**
- Modify: `/home/dan/git/rmfiles/src/geometry.rs`
- Modify: `/home/dan/git/rmfiles/src/scene/items.rs`
- Modify: `/home/dan/git/rmfiles/src/scene/mod.rs` (`parse_glyph_range` fills rectangles + color)
- Modify: `/home/dan/git/rmfiles/tests/fixtures/highlight-basic.expected.json` (Dan adds rect/color facts)
- Create: `/home/dan/git/rmfiles/tests/highlights.rs`

**Acceptance Criteria:**
- [ ] Each extracted `Highlight` has ≥1 `Rect` with sane device-space coordinates (within ±1404/±1872 bounds after centering).
- [ ] `geometry.rs` exposes device constants (`SCREEN_WIDTH=1404`, `SCREEN_HEIGHT=1872`, `SCALE=72.0/226.0`, `X_SHIFT`).
- [ ] `Scene::items()` yields `SceneItem::Highlight` variants; `highlights()` filter returns the same set.

**Verify:** `cd /home/dan/git/rmfiles && cargo test` → PASS

**Steps:**

- [ ] **Step 1: Expand geometry.rs.**
  ```rust
  //! Geometry + colour in reMarkable v6 device space (1404×1872, origin top-left,
  //! X centered on zero). Constants mirror rmc/svg.py.
  pub const SCREEN_WIDTH: f64 = 1404.0;
  pub const SCREEN_HEIGHT: f64 = 1872.0;
  pub const SCREEN_DPI: f64 = 226.0;
  /// device px -> PDF points.
  pub const SCALE: f64 = 72.0 / SCREEN_DPI;
  /// v6 X is centered on zero; shift right by half the page width to get 0..width.
  pub const X_SHIFT: f64 = (SCREEN_WIDTH * SCALE) / 2.0;

  #[derive(Debug, Clone, Copy, PartialEq)]
  pub struct Rect { pub x: f64, pub y: f64, pub w: f64, pub h: f64 }

  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub struct Color { pub r: u8, pub g: u8, pub b: u8, pub a: u8 }
  ```

- [ ] **Step 2: Add rect/color expectations to the fixture.** Dan extends `highlight-basic.expected.json` (no Rust edits):
  ```json
  { "highlights": ["the exact phrase I highlighted"],
    "min_rect_count": 1 }
  ```

- [ ] **Step 3: Failing test** `tests/highlights.rs`:
  ```rust
  use std::path::Path;
  fn parse_all(rmdoc: &Path) -> Vec<rmfiles::Highlight> {
      let f = std::fs::File::open(rmdoc).unwrap();
      let mut zip = zip::ZipArchive::new(f).unwrap();
      let mut out = Vec::new();
      for i in 0..zip.len() {
          let mut e = zip.by_index(i).unwrap();
          if !e.name().ends_with(".rm") { continue; }
          let mut b = Vec::new(); std::io::Read::read_to_end(&mut e, &mut b).unwrap();
          if let Ok(s) = rmfiles::Scene::parse(&b) { out.extend(s.highlights()); }
      }
      out
  }

  #[test]
  fn highlights_have_rectangles_in_device_bounds() {
      let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
      let hs = parse_all(&dir.join("highlight-basic.rmdoc"));
      assert!(!hs.is_empty(), "no highlights parsed");
      for h in &hs {
          assert!(!h.rectangles.is_empty(), "highlight {:?} has no rects", h.text);
          for r in &h.rectangles {
              assert!(r.x.abs() <= rmfiles::geometry::SCREEN_WIDTH, "x out of bounds: {r:?}");
              assert!(r.y >= -1.0 && r.y <= rmfiles::geometry::SCREEN_HEIGHT, "y out of bounds: {r:?}");
          }
      }
  }
  ```
  (`zip` is a dev-dependency for tests too — already present.)

- [ ] **Step 4:** Implement `parse_glyph_range` rectangle/color parsing in `scene/mod.rs` until PASS. Each rectangle is four `f64_le` reads (`x,y,w,h`); the count is a varuint preceding the list (confirm order against rmscene `glyph_range` / `CrdtSequence` read).

- [ ] **Step 5: Run** `cargo test` → PASS.

- [ ] **Step 6: Commit.**
  ```bash
  cd /home/dan/git/rmfiles
  git add -A && git commit -m "rmfiles: parse highlight rectangles + colour; geometry constants"
  ```

---

## Task 3: rmfiles — Bundle / Page (zip or dir), metadata, source PDF

**Goal:** A `Bundle` that opens a `.rmdoc`/zip OR an unpacked directory, lists pages in reading order, parses each page's `.rm`, and returns the original PDF.

**Files:**
- Create: `/home/dan/git/rmfiles/src/bundle/mod.rs`
- Create: `/home/dan/git/rmfiles/src/bundle/content.rs`
- Create: `/home/dan/git/rmfiles/src/bundle/metadata.rs`
- Modify: `/home/dan/git/rmfiles/src/lib.rs` (re-export `Bundle`, `Page`, `Metadata`)
- Create: `/home/dan/git/rmfiles/tests/bundle.rs`

**Acceptance Criteria:**
- [ ] `Bundle::open` accepts both `highlight-basic.rmdoc` and the same bundle unzipped into a temp dir; both yield identical highlight sets.
- [ ] `bundle.pages()` returns pages in `.content` reading order with 0-based `index`.
- [ ] `bundle.source_pdf()` returns the original PDF bytes.
- [ ] `bundle.metadata().visible_name` is non-empty for the fixture.

**Verify:** `cd /home/dan/git/rmfiles && cargo test --test bundle` → PASS

**Steps:**

- [ ] **Step 1: content.rs / metadata.rs (serde).** `.content` holds the page list under `cPages.pages[]` (newer) or `pages[]` (older) with per-page `id`; `.metadata` holds `visibleName`, `lastModified`, `type`. Model leniently:
  ```rust
  //! .content JSON: page order + ids. Shapes vary by software version.
  use serde::Deserialize;
  #[derive(Debug, Deserialize, Default)]
  pub struct Content {
      #[serde(default)] pub pages: Vec<String>,                 // legacy: ["uuid", ...]
      #[serde(default, rename = "cPages")] pub c_pages: Option<CPages>,
      #[serde(default, rename = "fileType")] pub file_type: Option<String>,
  }
  #[derive(Debug, Deserialize, Default)]
  pub struct CPages { #[serde(default)] pub pages: Vec<CPage> }
  #[derive(Debug, Deserialize)]
  pub struct CPage { pub id: String }

  impl Content {
      /// Page ids in reading order, from whichever schema is present.
      pub fn page_ids(&self) -> Vec<String> {
          if let Some(cp) = &self.c_pages { if !cp.pages.is_empty() {
              return cp.pages.iter().map(|p| p.id.clone()).collect();
          }}
          self.pages.clone()
      }
  }
  ```
  ```rust
  //! .metadata JSON.
  use serde::Deserialize;
  #[derive(Debug, Deserialize, Default, Clone)]
  pub struct Metadata {
      #[serde(default, rename = "visibleName")] pub visible_name: String,
      #[serde(default, rename = "lastModified")] pub last_modified: String,
      #[serde(default, rename = "type")] pub doc_type: String,
  }
  ```

- [ ] **Step 2: bundle/mod.rs.** Read all entries into memory (bundles are small) from either a zip file or a directory, key by file name. Find the `<uuid>.content`, `<uuid>.metadata`, `<uuid>.pdf`, and `<uuid>/<page-uuid>.rm`.
  ```rust
  //! A reMarkable document bundle (.rmdoc/zip or unpacked dir).
  pub mod content;
  pub mod metadata;

  use std::collections::HashMap;
  use std::io::Read;
  use std::path::Path;

  use crate::error::{Error, Result};
  use crate::scene::Scene;
  pub use metadata::Metadata;

  pub struct Bundle {
      files: HashMap<String, Vec<u8>>, // entry name -> bytes
      uuid: String,
      meta: Metadata,
      page_ids: Vec<String>,
  }

  pub struct Page<'a> {
      pub index: usize,
      pub id: String,
      bundle: &'a Bundle,
  }

  impl Bundle {
      pub fn open(path: &Path) -> Result<Bundle> {
          let files = if path.is_dir() { read_dir(path)? } else { read_zip(path)? };
          // The document uuid is the basename of the *.content entry.
          let uuid = files.keys()
              .find_map(|k| k.strip_suffix(".content").map(strip_dir))
              .ok_or(Error::Parse("no .content in bundle".into()))?
              .to_string();
          let content: content::Content =
              serde_json::from_slice(files.get(&format!("{uuid}.content"))
                  .ok_or(Error::Parse("missing .content".into()))?)?;
          let meta: Metadata = files.get(&format!("{uuid}.metadata"))
              .map(|b| serde_json::from_slice(b)).transpose()?.unwrap_or_default();
          Ok(Bundle { page_ids: content.page_ids(), files, uuid, meta })
      }

      pub fn metadata(&self) -> &Metadata { &self.meta }
      pub fn source_pdf(&self) -> Option<&[u8]> {
          self.files.get(&format!("{}.pdf", self.uuid)).map(|v| v.as_slice())
      }
      pub fn pages(&self) -> Vec<Page<'_>> {
          self.page_ids.iter().enumerate()
              .map(|(index, id)| Page { index, id: id.clone(), bundle: self })
              .collect()
      }
      fn rm_bytes(&self, page_id: &str) -> Option<&[u8]> {
          self.files.get(&format!("{}/{}.rm", self.uuid, page_id)).map(|v| v.as_slice())
      }
  }

  impl Page<'_> {
      /// Parse this page's v6 .rm, if it has one. `Ok(None)` = unannotated page.
      pub fn scene(&self) -> Result<Option<Scene>> {
          match self.bundle.rm_bytes(&self.id) {
              Some(b) => Scene::parse(b).map(Some),
              None => Ok(None),
          }
      }
  }

  fn strip_dir(s: &str) -> &str { s.rsplit('/').next().unwrap_or(s) }

  fn read_zip(path: &Path) -> Result<HashMap<String, Vec<u8>>> {
      let mut zip = zip::ZipArchive::new(std::fs::File::open(path)?)?;
      let mut out = HashMap::new();
      for i in 0..zip.len() {
          let mut e = zip.by_index(i)?;
          if e.is_dir() { continue; }
          let name = e.name().to_string();
          let mut b = Vec::new(); e.read_to_end(&mut b)?;
          out.insert(name, b);
      }
      Ok(out)
  }
  fn read_dir(root: &Path) -> Result<HashMap<String, Vec<u8>>> {
      let mut out = HashMap::new();
      fn walk(base: &Path, dir: &Path, out: &mut HashMap<String, Vec<u8>>) -> Result<()> {
          for entry in std::fs::read_dir(dir)? {
              let p = entry?.path();
              if p.is_dir() { walk(base, &p, out)?; }
              else {
                  let rel = p.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/");
                  out.insert(rel, std::fs::read(&p)?);
              }
          }
          Ok(())
      }
      walk(root, root, &mut out)?;
      Ok(out)
  }
  ```

- [ ] **Step 3: lib.rs re-exports.** Add `pub mod bundle;` and `pub use bundle::{Bundle, Page, Metadata};`.

- [ ] **Step 4: Failing test** `tests/bundle.rs`:
  ```rust
  use std::path::Path;

  fn highlight_texts_from_bundle(p: &Path) -> Vec<String> {
      let b = rmfiles::Bundle::open(p).unwrap();
      let mut out = Vec::new();
      for page in b.pages() {
          if let Some(scene) = page.scene().unwrap() {
              out.extend(scene.highlights().into_iter().map(|h| h.text));
          }
      }
      out
  }

  #[test]
  fn opens_zip_and_dir_identically() {
      let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
      let rmdoc = dir.join("highlight-basic.rmdoc");
      let from_zip = highlight_texts_from_bundle(&rmdoc);
      assert!(!from_zip.is_empty());

      // Unzip into a temp dir and open that.
      let tmp = tempfile::tempdir().unwrap();
      let mut zip = zip::ZipArchive::new(std::fs::File::open(&rmdoc).unwrap()).unwrap();
      zip.extract(tmp.path()).unwrap();
      let from_dir = highlight_texts_from_bundle(tmp.path());
      assert_eq!(from_zip, from_dir);
  }

  #[test]
  fn exposes_source_pdf_and_metadata() {
      let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
      let b = rmfiles::Bundle::open(&dir.join("highlight-basic.rmdoc")).unwrap();
      assert!(b.source_pdf().is_some(), "annotated PDF bundle must carry source PDF");
      assert!(!b.metadata().visible_name.is_empty());
      assert!(!b.pages().is_empty());
  }
  ```

- [ ] **Step 5: Run** `cargo test` → PASS. (`zip`/`tempfile` are dev-deps already.)

- [ ] **Step 6: Commit.**
  ```bash
  cd /home/dan/git/rmfiles
  git add -A && git commit -m "rmfiles: Bundle/Page (zip or dir), metadata, source PDF"
  ```

---

## Task 4: rmreader — embedded PDF manifest (write + read, in-process)

**Goal:** Define the embedded manifest and read/write it inside a PDF with lopdf, proven by an in-process round trip.

**Files:**
- Modify: `/home/dan/git/rmreader/src/manifest.rs` (add `EmbeddedManifest`, `EmbeddedDoc`, `PageRange`; extend `ManifestItem`)
- Create: `/home/dan/git/rmreader/src/embed.rs`
- Modify: `/home/dan/git/rmreader/src/lib.rs` (add `pub mod embed;`)
- Create: `/home/dan/git/rmreader/tests/embed.rs`

**Acceptance Criteria:**
- [ ] `embed::write(&mut doc, &manifest)` stores the manifest in the PDF catalog.
- [ ] `embed::read(&doc)` returns an identical `EmbeddedManifest`.
- [ ] Round trip survives `doc.save()` + reload from disk.

**Verify:** `cd /home/dan/git/rmreader && cargo test --test embed` → PASS

**Steps:**

- [ ] **Step 1: manifest types** in `src/manifest.rs` (add to the existing file):
  ```rust
  /// 0-based inclusive PDF page range an article occupies.
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
  pub struct PageRange { pub first: usize, pub last: usize }

  /// Per-doc record embedded in the PDF for read-back.
  #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
  pub struct EmbeddedDoc {
      pub id: String,
      pub title: String,
      pub url: String,
      #[serde(default)] pub author: String,
      #[serde(default)] pub category: String,
      pub page_range: PageRange,
  }

  /// The self-describing manifest embedded inside each generated PDF.
  #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
  pub struct EmbeddedManifest {
      pub v: u32,                 // schema version (1)
      pub collection: String,     // "Library" | "Feed"
      pub docs: Vec<EmbeddedDoc>,
  }

  impl EmbeddedManifest {
      /// doc whose page_range contains `page` (0-based).
      pub fn doc_for_page(&self, page: usize) -> Option<&EmbeddedDoc> {
          self.docs.iter().find(|d| page >= d.page_range.first && page <= d.page_range.last)
      }
  }
  ```
  Also extend `ManifestItem` with `#[serde(default)] author`, `source_url`, `category`, and `page_range: Option<PageRange>` (Option because it is filled post-render).

- [ ] **Step 2: embed.rs** — store JSON as a Flate-compressed stream referenced by a custom catalog key `/RMReaderManifest`.
  ```rust
  //! Embed a self-describing manifest inside the generated PDF so the downloaded
  //! bundle is the single source of truth (no local manifest state). Stored as a
  //! Flate-compressed stream referenced by a custom Catalog key.
  use lopdf::{Dictionary, Document, Object, Stream};
  use crate::manifest::EmbeddedManifest;

  const CATALOG_KEY: &[u8] = b"RMReaderManifest";

  pub fn write(doc: &mut Document, manifest: &EmbeddedManifest) -> anyhow::Result<()> {
      let json = serde_json::to_vec(manifest)?;
      let mut stream = Stream::new(Dictionary::new(), json);
      stream.compress()?; // Flate
      let sid = doc.add_object(Object::Stream(stream));
      let catalog_id = doc.trailer.get(b"Root")?.as_reference()?;
      let catalog = doc.get_dictionary_mut(catalog_id)?;
      catalog.set(CATALOG_KEY, Object::Reference(sid));
      Ok(())
  }

  pub fn read(doc: &Document) -> anyhow::Result<Option<EmbeddedManifest>> {
      let catalog_id = doc.trailer.get(b"Root")?.as_reference()?;
      let catalog = doc.get_dictionary(catalog_id)?;
      let sid = match catalog.get(CATALOG_KEY) {
          Ok(Object::Reference(id)) => *id,
          _ => return Ok(None),
      };
      let stream = doc.get_object(sid)?.as_stream()?;
      let bytes = stream.decompressed_content().unwrap_or_else(|_| stream.content.clone());
      Ok(Some(serde_json::from_slice(&bytes)?))
  }
  ```

- [ ] **Step 3: Failing test** `tests/embed.rs`:
  ```rust
  use rmreader::embed;
  use rmreader::manifest::{EmbeddedDoc, EmbeddedManifest, PageRange};

  fn sample() -> EmbeddedManifest {
      EmbeddedManifest { v: 1, collection: "Library".into(), docs: vec![
          EmbeddedDoc { id: "abc".into(), title: "T".into(), url: "https://x/y".into(),
              author: "A".into(), category: "article".into(),
              page_range: PageRange { first: 1, last: 3 } },
      ]}
  }

  #[test]
  fn round_trips_through_save_and_reload() {
      // minimal one-page PDF via lopdf
      let mut doc = lopdf::Document::with_version("1.5");
      let pages_id = doc.new_object_id();
      let page_id = doc.add_object(lopdf::dictionary!{
          "Type" => "Page", "Parent" => pages_id,
          "MediaBox" => vec![0.into(), 0.into(), 200.into(), 200.into()],
      });
      let pages = lopdf::dictionary!{ "Type" => "Pages",
          "Kids" => vec![page_id.into()], "Count" => 1 };
      doc.objects.insert(pages_id, lopdf::Object::Dictionary(pages));
      let catalog_id = doc.add_object(lopdf::dictionary!{ "Type" => "Catalog", "Pages" => pages_id });
      doc.trailer.set("Root", catalog_id);

      let m = sample();
      embed::write(&mut doc, &m).unwrap();
      let tmp = tempfile::NamedTempFile::new().unwrap();
      doc.save(tmp.path()).unwrap();

      let reloaded = lopdf::Document::load(tmp.path()).unwrap();
      let got = embed::read(&reloaded).unwrap().unwrap();
      assert_eq!(got, m);
  }

  #[test]
  fn read_returns_none_when_absent() {
      let doc = lopdf::Document::with_version("1.5");
      // No Root yet -> read should error or None; ensure graceful None on missing key.
      // Build a catalog with no manifest key:
      let mut doc = doc;
      let catalog_id = doc.add_object(lopdf::dictionary!{ "Type" => "Catalog" });
      doc.trailer.set("Root", catalog_id);
      assert!(embed::read(&doc).unwrap().is_none());
  }
  ```

- [ ] **Step 4:** Run `cargo test --test embed -v`, implement until PASS.

- [ ] **Step 5: Commit.**
  ```bash
  cd /home/dan/git/rmreader
  git add -A && git commit -m "rmreader: embed/read self-describing manifest in PDF (lopdf)"
  ```

---

## Task 5: SPIKE — snap-to-text on stamped labels + embedded-manifest preservation

**Goal:** Prove on real hardware that (a) the device's snap-to-text highlighter snaps to our lopdf-stamped label text, and (b) our embedded manifest survives the cloud round trip. Decide the label-rendering approach for Task 7.

**Files:**
- Create: `/home/dan/git/rmreader/examples/spike_stamp.rs`
- Add (Dan captures): `/home/dan/git/rmfiles/tests/fixtures/stamped-labels.rmdoc` + `stamped-labels.expected.json`
- Create: `/home/dan/git/rmreader/tests/spike_roundtrip.rs`
- Create: `/home/dan/git/rmreader/docs/superpowers/spikes/2026-05-22-snap-and-embed.md`

**Acceptance Criteria:**
- [ ] `cargo run --example spike_stamp -- <in.pdf> <out.pdf>` writes a PDF with the four labels stamped as real text on page 1 and an embedded manifest.
- [ ] After Dan's capture: `embed::read` recovers the manifest from the downloaded PDF (preservation ✅).
- [ ] After Dan's capture: `rmfiles` extracts a highlight whose text matches the label Dan highlighted (snap-to-text on stamped text ✅), recorded in `stamped-labels.expected.json`.
- [ ] The spike doc records both results and the chosen label-rendering approach.

**Verify:** `cd /home/dan/git/rmreader && cargo test --test spike_roundtrip` → PASS (after capture)

**Steps:**

- [ ] **Step 1: examples/spike_stamp.rs** — stamp labels as real text via lopdf (mirrors how `postprocess` stamps the nav bar) + embed a manifest:
  ```rust
  //! Spike prototype: stamp the four action labels as real text on page 1 of an
  //! existing PDF and embed a sample manifest. Throwaway harness for the on-device
  //! snap-to-text + embedded-manifest-preservation spike.
  use lopdf::{dictionary, Document, Object, Stream};
  use rmreader::embed;
  use rmreader::manifest::{EmbeddedDoc, EmbeddedManifest, PageRange};

  fn main() -> anyhow::Result<()> {
      let args: Vec<String> = std::env::args().collect();
      let (inp, outp) = (&args[1], &args[2]);
      let mut doc = Document::load(inp)?;
      let page1 = *doc.get_pages().values().next().unwrap();

      // A Helvetica font + a text-drawing content stream for the labels.
      let font_id = doc.add_object(dictionary!{
          "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica" });
      // Resources/Font/SPK -> font
      let res = doc.get_dictionary(page1)?.get(b"Resources").ok().cloned();
      let mut resources = match res {
          Some(Object::Dictionary(d)) => d,
          Some(Object::Reference(id)) => doc.get_dictionary(id)?.clone(),
          _ => dictionary!{},
      };
      let mut fonts = match resources.get(b"Font") {
          Ok(Object::Dictionary(d)) => d.clone(), _ => dictionary!{},
      };
      fonts.set("SPK", Object::Reference(font_id));
      resources.set("Font", Object::Dictionary(fonts));
      doc.get_dictionary_mut(page1)?.set("Resources", Object::Dictionary(resources));

      // Draw the labels near the top (MediaBox-relative); tune Y to taste.
      let content = b"q 0 0 0 rg BT /SPK 14 Tf 60 740 Td (INBOX   ARCHIVE   LATER   DELETE) Tj ET Q\n".to_vec();
      let sid = doc.add_object(Object::Stream(Stream::new(dictionary!{}, content)));
      // append to page contents
      let page = doc.get_dictionary_mut(page1)?;
      match page.get(b"Contents").ok().cloned() {
          Some(Object::Reference(old)) => page.set("Contents", Object::Array(vec![old.into(), sid.into()])),
          Some(Object::Array(mut a)) => { a.push(sid.into()); page.set("Contents", Object::Array(a)); }
          _ => page.set("Contents", Object::Array(vec![sid.into()])),
      }

      embed::write(&mut doc, &EmbeddedManifest { v: 1, collection: "Spike".into(),
          docs: vec![EmbeddedDoc{ id:"spike-doc".into(), title:"Spike".into(),
              url:"https://example.com/spike".into(), author:"".into(),
              category:"article".into(), page_range: PageRange{first:0,last:0} }] })?;
      doc.save(outp)?;
      println!("wrote {outp}");
      Ok(())
  }
  ```

- [ ] **Step 2: Dan runs the spike.**
  ```bash
  cd /home/dan/git/rmreader
  cargo run --example spike_stamp -- danout/Library.pdf /tmp/spike.pdf
  rmapi -ni put /tmp/spike.pdf /RMDev/Reader
  ```
  On the Paper Pro: open `spike`, **highlight one label word** (e.g. `ARCHIVE`) with snap-to-text, and **highlight one body sentence**. Then capture:
  ```bash
  cd /home/dan/git/rmfiles/tests/fixtures
  rmapi get /RMDev/Reader/spike && mv spike.rmdoc stamped-labels.rmdoc
  ```
  Write `stamped-labels.expected.json`:
  ```json
  { "label": "ARCHIVE", "body": ["the body sentence I highlighted"],
    "snap_worked": true }
  ```
  If the label highlight produced **no** text (only ink), set `"snap_worked": false`.

- [ ] **Step 3: spike_roundtrip.rs** asserts both outcomes from the captured fixture:
  ```rust
  use std::path::Path;

  fn fixtures() -> std::path::PathBuf {
      // rmfiles fixtures dir (sibling crate)
      Path::new(env!("CARGO_MANIFEST_DIR")).join("../rmfiles/tests/fixtures")
  }

  #[test]
  fn embedded_manifest_survives_roundtrip() {
      let b = rmfiles::Bundle::open(&fixtures().join("stamped-labels.rmdoc")).unwrap();
      let pdf = b.source_pdf().expect("bundle has source pdf");
      let doc = lopdf::Document::load_mem(pdf).unwrap();
      let m = rmreader::embed::read(&doc).unwrap().expect("manifest present after round trip");
      assert_eq!(m.collection, "Spike");
  }

  #[test]
  fn snap_to_text_on_stamped_label() {
      let dir = fixtures();
      let exp: serde_json::Value = serde_json::from_slice(
          &std::fs::read(dir.join("stamped-labels.expected.json")).unwrap()).unwrap();
      assert_eq!(exp["snap_worked"], serde_json::Value::Bool(true),
          "snap-to-text did NOT snap to stamped label text — switch label rendering to fulgur (see spike doc)");
      let label = exp["label"].as_str().unwrap();
      let b = rmfiles::Bundle::open(&dir.join("stamped-labels.rmdoc")).unwrap();
      let mut texts = Vec::new();
      for p in b.pages() { if let Some(s) = p.scene().unwrap() {
          texts.extend(s.highlights().into_iter().map(|h| h.text)); } }
      assert!(texts.iter().any(|t| t.contains(label)),
          "expected label {label:?} among highlights {texts:?}");
  }
  ```

- [ ] **Step 4: Record the result** in `docs/superpowers/spikes/2026-05-22-snap-and-embed.md`: did snap-to-text work on stamped text? did the manifest survive (catalog key) or do we need the embedded-file fallback? State the label-rendering decision for Task 7 (stamp / fulgur running-header / first-page-only).

- [ ] **Step 5: Commit.**
  ```bash
  cd /home/dan/git/rmreader && git add -A && git commit -m "Spike: snap-to-text on stamped labels + embedded-manifest round trip"
  cd /home/dan/git/rmfiles && git add -A && git commit -m "fixtures: stamped-labels capture for snap/embed spike"
  ```

**Contingency:** if `snap_worked == false`, Task 7 renders labels via fulgur (running-header text in `assemble.rs`/`render.rs`) instead of lopdf stamping; if the manifest did not survive, switch `embed.rs` to a `/Names /EmbeddedFiles` attachment. Either way, downstream tasks are unaffected in shape.

---

## Task 6: rmreader — manifest plumbing through assemble + generate

**Goal:** Carry each doc's `author`, `source_url`, `category` into `ManifestItem`, and build the `EmbeddedManifest` (minus page ranges, filled in Task 7) during the build.

**Files:**
- Modify: `/home/dan/git/rmreader/src/assemble.rs` (populate new `ManifestItem` fields)
- Modify: `/home/dan/git/rmreader/src/manifest.rs` (helper to derive `EmbeddedManifest` from `Manifest`)
- Modify: `/home/dan/git/rmreader/tests/assemble.rs` (assert new fields)

**Acceptance Criteria:**
- [ ] `ManifestItem`s carry `author`, `source_url`, `category` from the `Document`.
- [ ] `Manifest::to_embedded(collection)` produces `EmbeddedDoc`s with empty/placeholder page ranges (filled later).

**Verify:** `cd /home/dan/git/rmreader && cargo test --test assemble` → PASS

**Steps:**

- [ ] **Step 1:** In `assemble.rs`, set the new fields when building each `ManifestItem`:
  ```rust
  items.push(ManifestItem {
      id: d.id.clone(),
      title: title.clone(),
      url: d.url.clone(),
      author: d.author.clone(),
      source_url: d.source_url.clone(),
      category: d.category.clone(),
      page_range: None,
      article_anchor,
  });
  ```

- [ ] **Step 2:** In `manifest.rs`, add:
  ```rust
  impl Manifest {
      /// Build the embeddable manifest. page_range defaults to (0,0) here and is
      /// overwritten by postprocess once page numbers are known.
      pub fn to_embedded(&self) -> EmbeddedManifest {
          EmbeddedManifest {
              v: 1,
              collection: self.collection.clone(),
              docs: self.items.iter().map(|i| EmbeddedDoc {
                  id: i.id.clone(), title: i.title.clone(),
                  url: if i.source_url.is_empty() { i.url.clone() } else { i.source_url.clone() },
                  author: i.author.clone(), category: i.category.clone(),
                  page_range: i.page_range.unwrap_or(PageRange { first: 0, last: 0 }),
              }).collect(),
          }
      }
  }
  ```

- [ ] **Step 3: Test** (extend `tests/assemble.rs`):
  ```rust
  #[test]
  fn manifest_items_carry_readwise_metadata() {
      // build a doc with author/source_url/category, assemble, assert fields propagate
      // (use the existing assemble test harness/builders in this file)
  }
  ```
  Fill with the file's existing `Document` builder pattern; assert `items[0].author/source_url/category` match the input.

- [ ] **Step 4:** Run `cargo test --test assemble` → PASS.

- [ ] **Step 5: Commit.**
  ```bash
  cd /home/dan/git/rmreader && git add -A && git commit -m "rmreader: carry author/source_url/category into manifest; to_embedded()"
  ```

---

## Task 7: rmreader — stamp action band + fill page ranges + embed manifest (postprocess)

**Goal:** In `postprocess::finalize_pdf`, stamp the four action labels on every article page, compute per-doc page ranges, and embed the final manifest into the PDF.

**Files:**
- Modify: `/home/dan/git/rmreader/src/postprocess.rs`
- Modify: `/home/dan/git/rmreader/src/generate.rs` (pass docs/manifest into postprocess; embed)
- Modify: `/home/dan/git/rmreader/tests/postprocess.rs`

**Acceptance Criteria:**
- [ ] Every article page contains the four labels as extractable text (per the Task 5 decision: lopdf stamp by default).
- [ ] The embedded manifest's `page_range`s match the actual article page spans.
- [ ] `embed::read` on the saved PDF returns the manifest with correct ranges.

**Verify:** `cd /home/dan/git/rmreader && cargo test --test postprocess` → PASS

**Steps:**

- [ ] **Step 1:** Extend `finalize_pdf` to also accept the `EmbeddedManifest` (mutable, to fill page ranges) and stamp labels. The function already computes `starts: Vec<usize>` (article first-page indices). Derive ranges:
  ```rust
  // After `starts` is known (article i spans starts[i]..starts[i+1]-1, last to end):
  for (i, doc) in manifest.docs.iter_mut().enumerate() {
      let first = starts[i];
      let last = if i + 1 < starts.len() { starts[i + 1] - 1 } else { pages.len() - 1 };
      doc.page_range = crate::manifest::PageRange { first, last };
  }
  ```

- [ ] **Step 2:** Stamp the action band on every article page (mirrors the nav-bar stamping already in this file; place the band just below the nav band). Add a second text line below the nav, drawn with the same `/NAVF` font:
  ```rust
  let band_y = bar_y - 16.0; // just under the nav bar
  content.push_str(&format!(
      "q {fr:.3} {fg:.3} {fb:.3} rg BT /NAVF 9 Tf 16 {band_y:.2} Td (INBOX   ARCHIVE   LATER   DELETE) Tj ET Q\n"
  ));
  ```
  Enlarge the top content margin (in `render.rs` `@page` CSS) so the band does not overlap body text. If Task 5 decided fulgur rendering instead, render the band as a running header in `assemble.rs`/`render.rs` and skip the stamp.

- [ ] **Step 3:** In `generate.rs` `build_one`, after `finalize_pdf`, embed:
  ```rust
  let mut embedded = built.manifest.to_embedded();
  crate::postprocess::finalize_pdf(/* …existing… */, &mut embedded)?;
  // open, embed, save
  {
      let mut doc = lopdf::Document::load(&pdf_path)?;
      crate::embed::write(&mut doc, &embedded)?;
      doc.save(&pdf_path)?;
  }
  ```
  (Or fold the embed into `finalize_pdf`'s single load/save to avoid reopening — preferred.)

- [ ] **Step 4: Test** in `tests/postprocess.rs`:
  ```rust
  #[test]
  fn embeds_manifest_with_correct_page_ranges() {
      // Build a small multi-page PDF via the existing test helper (or render path),
      // run finalize_pdf with a 2-doc manifest, reload, embed::read, and assert
      // page_range.first/last line up with the article start pages.
  }

  #[test]
  fn stamps_action_labels_as_text() {
      // After finalize, extract text from an article page (lopdf content stream or
      // pdftotext via a helper) and assert it contains "ARCHIVE".
  }
  ```
  Use the file's existing PDF-building test helpers; if none, build a minimal 2-article PDF inline.

- [ ] **Step 5:** Run `cargo test --test postprocess` → PASS.

- [ ] **Step 6: Commit.**
  ```bash
  cd /home/dan/git/rmreader && git add -A && git commit -m "rmreader: stamp action band, fill page ranges, embed manifest in postprocess"
  ```

---

## Task 8: rmreader — Readwise transport (method+body) + location/delete actions

**Goal:** Extend the HTTP seam to issue PATCH/DELETE with a body + auth, and implement `update_location` and `delete_document`.

**Files:**
- Modify: `/home/dan/git/rmreader/src/readwise/http.rs` (transport: method + body)
- Modify: `/home/dan/git/rmreader/src/readwise/mod.rs` (`ActionKind`, `update_location`, `delete_document`)
- Modify: `/home/dan/git/rmreader/tests/readwise.rs`

**Acceptance Criteria:**
- [ ] `update_location(id, ActionKind::Archive)` issues `PATCH /api/v3/update/<id>/` with body `{"location":"archive"}` and the `Authorization: Token …` header.
- [ ] `ActionKind` maps Inbox→`new`, Later→`later`, Archive→`archive`.
- [ ] `delete_document(id)` issues `DELETE /api/v3/delete/<id>/`.

**Verify:** `cd /home/dan/git/rmreader && cargo test --test readwise` → PASS

**Steps:**

- [ ] **Step 1:** Extend the transport trait. Today `HttpTransport::get(url, token)`. Add a general method:
  ```rust
  pub enum HttpMethod { Get, Patch, Delete, Post }

  pub trait HttpTransport {
      fn request(&self, method: HttpMethod, url: &str, token: &str, body: Option<&str>)
          -> anyhow::Result<HttpResponse>;
  }
  ```
  Provide a default `get` shim or update existing call sites (`fetch_documents`, `validate_token`) to `request(Get, …, None)`. Update `UreqTransport` in `http.rs` to dispatch by method (ureq `ureq::request(method, url)`, `.set("Authorization", &format!("Token {token}"))`, `.send_string(body)` when present).

- [ ] **Step 2:** `ActionKind` + calls in `readwise/mod.rs`:
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum ActionKind { Inbox, Later, Archive, Delete }

  impl ActionKind {
      pub fn location(self) -> Option<&'static str> {
          match self {
              ActionKind::Inbox => Some("new"),
              ActionKind::Later => Some("later"),
              ActionKind::Archive => Some("archive"),
              ActionKind::Delete => None, // delete uses a different endpoint
          }
      }
      pub fn parse_label(s: &str) -> Option<ActionKind> {
          match s.trim().to_ascii_lowercase().as_str() {
              "inbox" => Some(ActionKind::Inbox),
              "later" => Some(ActionKind::Later),
              "archive" => Some(ActionKind::Archive),
              "delete" => Some(ActionKind::Delete),
              _ => None,
          }
      }
  }

  const UPDATE_URL: &str = "https://readwise.io/api/v3/update/";
  const DELETE_URL: &str = "https://readwise.io/api/v3/delete/";

  pub fn update_location(t: &dyn HttpTransport, token: &str, id: &str, loc: &str) -> anyhow::Result<()> {
      let url = format!("{UPDATE_URL}{id}/");
      let body = serde_json::json!({ "location": loc }).to_string();
      let r = t.request(HttpMethod::Patch, &url, token, Some(&body))?;
      anyhow::ensure!((200..300).contains(&r.status), "update {id} -> {loc} failed: HTTP {}", r.status);
      Ok(())
  }
  pub fn delete_document(t: &dyn HttpTransport, token: &str, id: &str) -> anyhow::Result<()> {
      let url = format!("{DELETE_URL}{id}/");
      let r = t.request(HttpMethod::Delete, &url, token, None)?;
      anyhow::ensure!(r.status == 204 || (200..300).contains(&r.status), "delete {id} failed: HTTP {}", r.status);
      Ok(())
  }
  ```

- [ ] **Step 3: Tests** (extend `tests/readwise.rs`) using a fake transport that records `(method, url, token, body)`:
  ```rust
  #[test]
  fn update_location_issues_patch_with_body_and_auth() {
      let fake = RecordingTransport::new(/* respond 200 */);
      update_location(&fake, "TKN", "doc123", "archive").unwrap();
      let call = fake.last();
      assert_eq!(call.method, HttpMethod::Patch);
      assert_eq!(call.url, "https://readwise.io/api/v3/update/doc123/");
      assert_eq!(call.token, "TKN");
      assert_eq!(call.body.as_deref(), Some(r#"{"location":"archive"}"#));
  }
  #[test]
  fn delete_issues_delete() {
      let fake = RecordingTransport::new(/* respond 204 */);
      delete_document(&fake, "TKN", "doc9").unwrap();
      assert_eq!(fake.last().url, "https://readwise.io/api/v3/delete/doc9/");
      assert_eq!(fake.last().method, HttpMethod::Delete);
  }
  ```
  Add the `RecordingTransport` fake near the existing fake transport in this test file.

- [ ] **Step 4:** Run `cargo test --test readwise` → PASS (and `cargo build` to confirm `fetch_documents`/`validate_token` still compile against the new trait).

- [ ] **Step 5: Commit.**
  ```bash
  cd /home/dan/git/rmreader && git add -A && git commit -m "rmreader: Readwise transport method+body; update_location + delete_document"
  ```

---

## Task 9: rmreader — Readwise create_highlights (v2)

**Goal:** Push content highlights to Readwise via the classic v2 endpoint, matched to documents by `source_url`.

**Files:**
- Modify: `/home/dan/git/rmreader/src/readwise/mod.rs` (`HighlightCreate`, `create_highlights`)
- Modify: `/home/dan/git/rmreader/tests/readwise.rs`

**Acceptance Criteria:**
- [ ] `create_highlights` POSTs `https://readwise.io/api/v2/highlights/` with `{"highlights":[…]}` carrying `text`, `title`, `author`, `source_url`.
- [ ] An empty input list is a no-op (no request).

**Verify:** `cd /home/dan/git/rmreader && cargo test --test readwise` → PASS

**Steps:**

- [ ] **Step 1:** Types + call in `readwise/mod.rs`:
  ```rust
  #[derive(Debug, Clone, serde::Serialize)]
  pub struct HighlightCreate {
      pub text: String,
      #[serde(skip_serializing_if = "String::is_empty")] pub title: String,
      #[serde(skip_serializing_if = "String::is_empty")] pub author: String,
      #[serde(skip_serializing_if = "String::is_empty")] pub source_url: String,
      #[serde(default = "article_cat", skip_serializing_if = "String::is_empty")] pub category: String,
  }
  fn article_cat() -> String { "articles".into() }

  const HL_URL: &str = "https://readwise.io/api/v2/highlights/";

  pub fn create_highlights(t: &dyn HttpTransport, token: &str, items: &[HighlightCreate]) -> anyhow::Result<()> {
      if items.is_empty() { return Ok(()); }
      let body = serde_json::json!({ "highlights": items }).to_string();
      let r = t.request(HttpMethod::Post, HL_URL, token, Some(&body))?;
      anyhow::ensure!((200..300).contains(&r.status), "create_highlights failed: HTTP {}", r.status);
      Ok(())
  }
  ```

- [ ] **Step 2: Tests:**
  ```rust
  #[test]
  fn create_highlights_posts_v2_with_source_url() {
      let fake = RecordingTransport::new(/* 200 */);
      create_highlights(&fake, "TKN", &[HighlightCreate{
          text: "hello".into(), title: "T".into(), author: "A".into(),
          source_url: "https://x/y".into(), category: "articles".into() }]).unwrap();
      let c = fake.last();
      assert_eq!(c.method, HttpMethod::Post);
      assert_eq!(c.url, "https://readwise.io/api/v2/highlights/");
      assert!(c.body.as_deref().unwrap().contains("\"source_url\":\"https://x/y\""));
      assert!(c.body.as_deref().unwrap().contains("\"text\":\"hello\""));
  }
  #[test]
  fn create_highlights_empty_is_noop() {
      let fake = RecordingTransport::new(/* never called */);
      create_highlights(&fake, "TKN", &[]).unwrap();
      assert_eq!(fake.call_count(), 0);
  }
  ```

- [ ] **Step 3:** Run `cargo test --test readwise` → PASS.

- [ ] **Step 4: Commit.**
  ```bash
  cd /home/dan/git/rmreader && git add -A && git commit -m "rmreader: Readwise v2 create_highlights (match by source_url)"
  ```

---

## Task 10: rmreader — classification (highlights → plan)

**Goal:** Pure logic that turns extracted highlights + the embedded manifest into a `Plan` of Readwise operations (actions + content highlights), with conflict skip+warn.

**Files:**
- Create: `/home/dan/git/rmreader/src/readback/mod.rs` (module decl + re-exports)
- Create: `/home/dan/git/rmreader/src/readback/classify.rs`
- Modify: `/home/dan/git/rmreader/src/lib.rs` (`pub mod readback;`)
- Create: `/home/dan/git/rmreader/tests/classify.rs`

**Acceptance Criteria:**
- [ ] A label-text highlight in the top band on a doc's page → an action for that doc.
- [ ] A non-label highlight → a content highlight for the doc owning that page.
- [ ] ≥2 distinct actions on one doc → no action + a warning; content highlights still emitted.
- [ ] A highlight on a page absent from the manifest → a warning, skipped.

**Verify:** `cd /home/dan/git/rmreader && cargo test --test classify` → PASS

**Steps:**

- [ ] **Step 1:** Define inputs/outputs in `classify.rs`. Model an extracted highlight independent of `rmfiles` for testability:
  ```rust
  use std::collections::BTreeMap;
  use crate::manifest::EmbeddedManifest;
  use crate::readwise::{ActionKind, HighlightCreate};

  /// One highlight located on a page, normalized from rmfiles output.
  #[derive(Debug, Clone)]
  pub struct PageHighlight {
      pub page: usize,
      pub text: String,
      pub top_band: bool, // device-space relative: in the page's top band
  }

  #[derive(Debug, Default, PartialEq)]
  pub struct Plan {
      pub actions: Vec<(String, ActionKind)>,  // (doc_id, kind)
      pub highlights: Vec<HighlightCreate>,
      pub warnings: Vec<String>,
  }

  pub fn classify(m: &EmbeddedManifest, hs: &[PageHighlight]) -> Plan {
      let mut plan = Plan::default();
      // doc_id -> set of distinct actions seen
      let mut acted: BTreeMap<String, Vec<ActionKind>> = BTreeMap::new();
      for h in hs {
          let Some(doc) = m.doc_for_page(h.page) else {
              plan.warnings.push(format!("highlight on page {} not in manifest; skipped", h.page));
              continue;
          };
          let as_action = if h.top_band { ActionKind::parse_label(&h.text) } else { None };
          match as_action {
              Some(kind) => acted.entry(doc.id.clone()).or_default().push(kind),
              None => plan.highlights.push(HighlightCreate {
                  text: h.text.clone(), title: doc.title.clone(),
                  author: doc.author.clone(), source_url: doc.url.clone(),
                  category: if doc.category.is_empty() { "articles".into() } else { doc.category.clone() },
              }),
          }
      }
      for (id, mut kinds) in acted {
          kinds.dedup(); kinds.sort_by_key(|k| *k as u8); kinds.dedup();
          if kinds.len() == 1 {
              plan.actions.push((id, kinds[0]));
          } else {
              plan.warnings.push(format!("doc {id}: {} action labels highlighted; skipped", kinds.len()));
          }
      }
      plan
  }
  ```
  (Derive `PartialEq, Eq, Ord` on `ActionKind` for sort/dedup, or sort by `parse`/discriminant.)

- [ ] **Step 2:** `readback/mod.rs`:
  ```rust
  //! Read on-device annotations and turn them into Readwise operations.
  pub mod classify;
  pub use classify::{classify, PageHighlight, Plan};
  ```

- [ ] **Step 3: Table tests** `tests/classify.rs`:
  ```rust
  use rmreader::manifest::{EmbeddedDoc, EmbeddedManifest, PageRange};
  use rmreader::readback::{classify, PageHighlight};
  use rmreader::readwise::ActionKind;

  fn manifest() -> EmbeddedManifest {
      EmbeddedManifest { v:1, collection:"Library".into(), docs: vec![
          EmbeddedDoc{ id:"d1".into(), title:"One".into(), url:"https://a".into(),
              author:"A".into(), category:"articles".into(), page_range: PageRange{first:0,last:1} },
          EmbeddedDoc{ id:"d2".into(), title:"Two".into(), url:"https://b".into(),
              author:"B".into(), category:"articles".into(), page_range: PageRange{first:2,last:2} },
      ]}
  }
  fn h(page: usize, text: &str, top: bool) -> PageHighlight {
      PageHighlight { page, text: text.into(), top_band: top }
  }

  #[test]
  fn label_in_top_band_becomes_action() {
      let p = classify(&manifest(), &[h(1, "ARCHIVE", true)]);
      assert_eq!(p.actions, vec![("d1".into(), ActionKind::Archive)]);
      assert!(p.highlights.is_empty());
  }
  #[test]
  fn body_highlight_becomes_content() {
      let p = classify(&manifest(), &[h(2, "a great sentence", false)]);
      assert_eq!(p.actions.len(), 0);
      assert_eq!(p.highlights.len(), 1);
      assert_eq!(p.highlights[0].source_url, "https://b");
      assert_eq!(p.highlights[0].text, "a great sentence");
  }
  #[test]
  fn label_word_in_body_not_top_band_is_content() {
      let p = classify(&manifest(), &[h(0, "archive", false)]);
      assert_eq!(p.actions.len(), 0);
      assert_eq!(p.highlights.len(), 1);
  }
  #[test]
  fn two_actions_skip_with_warning() {
      let p = classify(&manifest(), &[h(0,"ARCHIVE",true), h(1,"DELETE",true)]);
      assert!(p.actions.is_empty());
      assert_eq!(p.warnings.len(), 1);
  }
  #[test]
  fn highlight_off_manifest_warns() {
      let p = classify(&manifest(), &[h(99,"x",false)]);
      assert!(p.actions.is_empty() && p.highlights.is_empty());
      assert_eq!(p.warnings.len(), 1);
  }
  ```

- [ ] **Step 4:** Run `cargo test --test classify` → PASS.

- [ ] **Step 5: Commit.**
  ```bash
  cd /home/dan/git/rmreader && git add -A && git commit -m "rmreader: classify highlights into a Readwise op plan"
  ```

---

## Task 11: rmreader — deploy fetch + replace

**Goal:** Add `fetch` (download bundle) and `replace` (full-replace upload) to the deploy layer for both backends.

**Files:**
- Modify: `/home/dan/git/rmreader/src/deploy/mod.rs` (trait methods)
- Modify: `/home/dan/git/rmreader/src/deploy/rmapi.rs` (`get`, `rm`+`put`)
- Modify: `/home/dan/git/rmreader/src/deploy/local.rs` (none backend)
- Modify: `/home/dan/git/rmreader/tests/deploy.rs`

**Acceptance Criteria:**
- [ ] `RmapiDeployer::fetch(folder, name)` runs `rmapi -ni get <folder>/<name>` and returns the downloaded bundle path, or `Ok(None)` if missing.
- [ ] `RmapiDeployer::replace(pdf, folder)` runs `rmapi -ni rm <folder>/<name>` (ignoring "not found") then `rmapi -ni put <pdf> <folder>`.
- [ ] The `none` backend's `fetch` returns `Ok(None)` and `replace` overwrites the local file.

**Verify:** `cd /home/dan/git/rmreader && cargo test --test deploy` → PASS

**Steps:**

- [ ] **Step 1:** Extend the trait in `deploy/mod.rs`:
  ```rust
  pub trait Deployer: std::fmt::Debug {
      fn deploy(&self, targets: &[(PathBuf, String)]) -> anyhow::Result<()>;
      fn refresh(&self, targets: &[(PathBuf, String)]) -> anyhow::Result<()>;
      /// Download the bundle for <folder>/<name> into a temp dir; None if absent.
      fn fetch(&self, folder: &str, name: &str) -> anyhow::Result<Option<PathBuf>>;
      /// Full replace: remove the existing doc then upload `pdf`.
      fn replace(&self, pdf: &Path, folder: &str) -> anyhow::Result<()>;
  }
  ```
  The `RmapiRunner` trait currently returns `anyhow::Result<()>`; `fetch` needs to know success vs not-found and the output path. Add a runner method that captures success without erroring on a missing doc — e.g. `fn try_run(&self, args: &[&str]) -> anyhow::Result<bool>` (Ok(false) on non-zero exit). Implement for `ProcessRmapi` (reuse `attempt`) and the test fake.

- [ ] **Step 2:** `rmapi.rs` impl:
  ```rust
  fn fetch(&self, folder: &str, name: &str) -> anyhow::Result<Option<PathBuf>> {
      let tmp = tempfile::tempdir()?; // keep: return its path; caller holds it
      let remote = format!("{folder}/{name}");
      // rmapi get writes <name>.rmdoc into CWD; run inside tmp dir.
      let ok = self.runner.try_run_in(tmp.path(), &["-ni", "get", &remote])?;
      if !ok { return Ok(None); }
      let out = tmp.path().join(format!("{name}.rmdoc"));
      // leak the tempdir into a stable path the caller owns:
      let dest = std::env::temp_dir().join(format!("rmreader-{name}.rmdoc"));
      std::fs::rename(&out, &dest).or_else(|_| std::fs::copy(&out, &dest).map(|_| ()))?;
      Ok(Some(dest))
  }
  fn replace(&self, pdf: &Path, folder: &str) -> anyhow::Result<()> {
      let name = pdf.file_stem().unwrap().to_string_lossy().to_string();
      let _ = self.runner.run(&["-ni", "rm", &format!("{folder}/{name}")]); // ignore not-found
      self.runner.run(&["-ni", "put", path_str(pdf)?, folder])
  }
  ```
  Add `try_run_in(dir, args)` to the runner (sets the child CWD). Keep `deploy`/`refresh` as-is.

- [ ] **Step 3:** `local.rs` (none backend): `fetch` → `Ok(None)`; `replace(pdf, _)` → no-op (PDF already on disk; nothing to remove).

- [ ] **Step 4: Tests** in `tests/deploy.rs` with the fake runner recording arg sequences:
  ```rust
  #[test]
  fn replace_removes_then_puts() {
      let runner = FakeRunner::new(); // run() always Ok
      let d = RmapiDeployer::new(runner.clone());
      d.replace(Path::new("/tmp/Library.pdf"), "/RMDev/Reader").unwrap();
      assert_eq!(runner.calls(), vec![
          vec!["-ni","rm","/RMDev/Reader/Library"],
          vec!["-ni","put","/tmp/Library.pdf","/RMDev/Reader"],
      ]);
  }
  #[test]
  fn fetch_missing_returns_none() {
      let runner = FakeRunner::failing(); // try_run -> Ok(false)
      let d = RmapiDeployer::new(runner);
      assert!(d.fetch("/RMDev/Reader", "Library").unwrap().is_none());
  }
  ```
  Extend the existing `FakeRunner` to support `try_run`/`try_run_in` recording.

- [ ] **Step 5:** Run `cargo test --test deploy` → PASS.

- [ ] **Step 6: Commit.**
  ```bash
  cd /home/dan/git/rmreader && git add -A && git commit -m "rmreader: deploy fetch (rmapi get) + replace (rm+put)"
  ```

---

## Task 12: rmreader — read-back orchestration + sync wiring

**Goal:** Tie it together: on `rmreader <config>`, fetch each collection's bundle, read its embedded manifest, extract highlights via `rmfiles`, classify, execute the plan, then regenerate and full-replace.

**Files:**
- Modify: `/home/dan/git/rmreader/src/readback/mod.rs` (the orchestrator)
- Modify: `/home/dan/git/rmreader/src/generate.rs` and/or `src/cli.rs` (sync flow)
- Create: `/home/dan/git/rmreader/tests/readback.rs`

**Acceptance Criteria:**
- [ ] Given a fake deployer that returns the `stamped-labels.rmdoc` fixture and a fake transport, the orchestrator produces and executes the expected Readwise calls.
- [ ] First run (deployer `fetch` → None) skips read-back and proceeds to generate + upload.
- [ ] The sync command runs read-back **before** regeneration and uses `replace` (not `refresh`).

**Verify:** `cd /home/dan/git/rmreader && cargo test --test readback` → PASS

**Steps:**

- [ ] **Step 1:** Orchestrator in `readback/mod.rs`:
  ```rust
  use std::path::Path;
  use crate::deploy::Deployer;
  use crate::readwise::{self, ActionKind, HttpTransport};

  /// Read annotations for one collection and apply them to Readwise.
  /// Returns the executed Plan (for logging/tests). Best-effort: per-op failures
  /// are collected as warnings, not hard errors.
  pub fn sync_collection(
      deployer: &dyn Deployer, transport: &dyn HttpTransport, token: &str,
      folder: &str, name: &str,
  ) -> anyhow::Result<Plan> {
      let Some(bundle_path) = deployer.fetch(folder, name)? else {
          return Ok(Plan::default()); // first run / no doc
      };
      let bundle = rmfiles::Bundle::open(&bundle_path)?;
      // read embedded manifest from the source PDF
      let Some(pdf) = bundle.source_pdf() else { return Ok(Plan::default()); };
      let doc = lopdf::Document::load_mem(pdf)?;
      let Some(manifest) = crate::embed::read(&doc)? else { return Ok(Plan::default()); };

      // extract page highlights via rmfiles -> PageHighlight (compute top_band)
      let mut phs = Vec::new();
      for page in bundle.pages() {
          if let Ok(Some(scene)) = page.scene() {
              for h in scene.highlights() {
                  let top_band = in_top_band(&h, &scene); // device-space relative
                  phs.push(classify::PageHighlight { page: page.index, text: h.text, top_band });
              }
          }
      }
      let plan = classify::classify(&manifest, &phs);
      execute(transport, token, &plan);
      Ok(plan)
  }

  fn execute(t: &dyn HttpTransport, token: &str, plan: &Plan) {
      for (id, kind) in &plan.actions {
          let r = match kind {
              ActionKind::Delete => readwise::delete_document(t, token, id),
              k => readwise::update_location(t, token, id, k.location().unwrap()),
          };
          if let Err(e) = r { eprintln!("[rmreader] action {id} failed: {e:#}"); }
      }
      if let Err(e) = readwise::create_highlights(t, token, &plan.highlights) {
          eprintln!("[rmreader] create_highlights failed: {e:#}");
      }
      for w in &plan.warnings { eprintln!("[rmreader] {w}"); }
  }
  ```
  Implement `in_top_band(&Highlight, &Scene)` device-space relative: the highlight's min `rect.y` lies within the top fraction of the page's content. For v1 use a fixed threshold (e.g. `min_y < TOP_BAND_FRACTION * SCREEN_HEIGHT`, tuned against the Task 5 fixture). Document the heuristic; refine later with the `.content` fit transform if needed.

- [ ] **Step 2:** Wire into the sync flow. In `cli.rs` regenerate branch (`(None, Some(path))`), before `generate::generate`, run read-back for each enabled collection; after generate, use `replace` instead of `refresh`:
  ```rust
  let deployer = deploy::get_deployer(&cfg)?;
  // read-back BEFORE regeneration (uses the on-device PDF's own manifest)
  let _ = readback::sync_collection(&*deployer, &transport, &cfg.readwise.token,
                                    &cfg.deploy.library_folder, "Library");
  if cfg.feed.enabled {
      let _ = readback::sync_collection(&*deployer, &transport, &cfg.readwise.token,
                                        &cfg.deploy.feed_folder, "Feed");
  }
  let targets = generate::generate(&cfg, &transport, &fetcher)?;
  for (pdf, folder) in &targets { deployer.replace(pdf, folder)?; }
  ```
  (Keep `deploy()`/`refresh()` available; the `init` path can still use `deploy()`.)

- [ ] **Step 3: Test** `tests/readback.rs` with a fake deployer returning the fixture path and a recording transport:
  ```rust
  #[test]
  fn orchestrator_executes_expected_calls_from_fixture() {
      // FakeDeployer::fetch -> Some(path to ../rmfiles/tests/fixtures/stamped-labels.rmdoc)
      // RecordingTransport responds 200/204.
      // Run sync_collection; assert the recorded calls include the action implied by
      // the label Dan highlighted (from stamped-labels.expected.json) and a v2
      // highlights POST for the body highlight.
  }
  #[test]
  fn first_run_no_doc_is_noop() {
      // FakeDeployer::fetch -> None ; assert Plan::default and zero transport calls.
  }
  ```
  Pull the expected label from `stamped-labels.expected.json` to keep the assertion fixture-driven.

- [ ] **Step 4:** Run `cargo test --test readback` → PASS; `cargo build` clean.

- [ ] **Step 5: Commit.**
  ```bash
  cd /home/dan/git/rmreader && git add -A && git commit -m "rmreader: read-back orchestration; sync = readback -> regenerate -> replace"
  ```

---

## Task 13: rmreader — /RMDev cloud root defaults + wizard + docs

**Goal:** Default deploy/fetch folders to `/RMDev/Reader`, prompt for it in the wizard, and note the convention + migration.

**Files:**
- Modify: `/home/dan/git/rmreader/src/config.rs` (default folders)
- Modify: `/home/dan/git/rmreader/src/wizard.rs` (prompt default)
- Modify: `/home/dan/git/rmreader/tests/config.rs` and/or `tests/wizard.rs`

**Acceptance Criteria:**
- [ ] A config with no `[deploy]` folders defaults both to `/RMDev/Reader`.
- [ ] The wizard offers `/RMDev/Reader` as the default folder.

**Verify:** `cd /home/dan/git/rmreader && cargo test --test config && cargo test --test wizard` → PASS

**Steps:**

- [ ] **Step 1:** In `config.rs`, give the folders real defaults instead of empty strings:
  ```rust
  fn default_reader_folder() -> String { "/RMDev/Reader".into() }
  // in DeployConfig:
  #[serde(default = "default_reader_folder")] pub library_folder: String,
  #[serde(default = "default_reader_folder")] pub feed_folder: String,
  // and Default impl uses default_reader_folder() for both.
  ```

- [ ] **Step 2:** In `wizard.rs`, set the folder prompts' default to `/RMDev/Reader`.

- [ ] **Step 3: Tests:**
  ```rust
  #[test]
  fn deploy_folders_default_under_rmdev() {
      let cfg: rmreader::config::Config =
          toml::from_str("device=\"paper-pro\"\noutput_dir=\".\"\n[readwise]\ntoken=\"x\"\n[deploy]\nbackend=\"rmapi\"\n").unwrap();
      assert_eq!(cfg.deploy.library_folder, "/RMDev/Reader");
      assert_eq!(cfg.deploy.feed_folder, "/RMDev/Reader");
  }
  ```
  Adjust to the file's existing config-parse test style.

- [ ] **Step 4:** Update `danout/rmreader.toml` (gitignored, local) to `/RMDev/Reader` so Dan's next run targets the new root. Run `cargo test` (whole suite) → PASS.

- [ ] **Step 5: Commit.**
  ```bash
  cd /home/dan/git/rmreader && git add -A && git commit -m "rmreader: default deploy folders to /RMDev/Reader; wizard + tests"
  ```

---

## Final integration check

- [ ] `cd /home/dan/git/rmfiles && cargo test` → all PASS
- [ ] `cd /home/dan/git/rmreader && cargo test` → all PASS
- [ ] `cd /home/dan/git/rmreader && make` (fmt + clippy + test, per the Makefile) → clean
- [ ] Manual end-to-end (Dan, once): run `rmreader danout/rmreader.toml` twice with a highlight in between; confirm the highlighted article changes location in Readwise and the body highlight appears.

---

## PIVOT (2026-05-22): geometry-based read-back — revised task list

The hardware spike (see `docs/superpowers/spikes/2026-05-22-snap-and-embed.md`) showed
the Paper Pro stores highlighter **ink strokes (geometry), not text**. Read-back is
re-architected around stroke geometry. Original Tasks 1,2,3,5,7,10,12 are superseded by
the geometry tasks below; Tasks 4,6,8,9,11,13 are complete and unchanged (mechanism-
independent). The real fixture is committed at `rmfiles/tests/fixtures/stamped-labels.rmdoc`.

- **G1 — rmfiles: v6 parser → highlighter strokes.** Bootstrap the crate; parse the v6
  header + blocks; extract `SceneLineItemBlock` `Line` items (`tool`, `color`,
  `points:[Point{x,y}]`). Tolerate the Paper Pro "newer format" (respect block lengths,
  skip unread bytes). `Scene::strokes()`, `SceneItem::Line(Stroke)`. Fixture test: ≥2
  `HIGHLIGHTER_2`/`HIGHLIGHT` strokes; points in the expected device ranges.
  Files: `../rmfiles/{Cargo.toml,src/lib.rs,src/error.rs,src/geometry.rs,src/scene/{mod,reader,items}.rs,tests/strokes.rs}`.
- **G2 — rmfiles: Bundle/Page + canvas dims + source PDF.** `Bundle::open` (zip OR dir),
  `.content` (page order + `customZoomPageWidth/Height`), `.metadata`, `source_pdf()`,
  `pages()`/`page.scene()`. Expose the device canvas size. Fixture test: opens zip+dir
  identically, source_pdf present, canvas = 1404×1872.
  Files: `../rmfiles/src/bundle/{mod,content,metadata}.rs`, `tests/bundle.rs`.
- **G3 — rmreader: device→PDF coordinate transform** (`src/readback/coords.rs`). Map
  device(x,y) [canvas WxH, X-centered, y-down] → PDF points [page WxH, y-up] with uniform
  fit + letterbox handling. Fixture-validated: the `ARCHIVE` stroke (device y≈154,
  x∈[-588,-339]) maps into the page's top band / label column.
  Files: `src/readback/coords.rs`, `tests/coords.rs`.
- **G4 — rmreader: PDF text-layer word boxes** (`src/readback/textlayer.rs`). Run
  `pdftotext -bbox` (poppler) on the source PDF; parse word boxes (PDF points);
  `words_under(page, rect) -> String`. Test on the spike source PDF: finds ARCHIVE / quick
  / brown / fox with sane boxes; `words_under` over the body region returns the sentence.
  Files: `src/readback/textlayer.rs`, `tests/textlayer.rs`.
- **G5 — rmreader: postprocess stamps labels + records rects in manifest.** Stamp the four
  labels on every article page (real text, known coords); record the per-page **label
  rects** (PDF coords) + `page_range` in the `EmbeddedManifest`; embed it. Extend
  `EmbeddedManifest`/`EmbeddedDoc` with the label band rects.
  Files: `src/postprocess.rs`, `src/manifest.rs`, `src/generate.rs`, `tests/postprocess.rs`.
- **G6 — rmreader: geometric classification** (`src/readback/classify.rs`, rewrite). Input:
  per-page transformed strokes (PDF coords) + manifest (label rects, page_range) + a
  `words_under` closure. A stroke overlapping a label rect → `Action`; else
  `ContentHighlight{ text: words_under(stroke) }`. Per-doc resolution (0/1/≥2 → skip+warn).
  Files: `src/readback/classify.rs`, `tests/classify.rs`.
- **G7 — rmreader: orchestration + sync wiring.** `fetch` bundle → `embed::read` manifest
  → rmfiles strokes per page → `coords` transform → `classify` (with `textlayer`) →
  execute Readwise ops → regenerate → `replace`. Wire into `rmreader <config>`.
  Files: `src/readback/mod.rs`, `src/cli.rs`, `src/generate.rs`, `tests/readback.rs`.

Dependencies: G2←G1; G3←G2; G6←{G3,G4,G5}; G7←{G6, deploy(11), readwise(8,9), embed(4)}.
