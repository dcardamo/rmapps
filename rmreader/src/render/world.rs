//! A minimal Typst `World` for rmreader: an in-memory main source plus the
//! reader fonts embedded from `assets/fonts` (deterministic — no host font
//! search) and image assets served through `file()`.
//!
//! We render with Typst (not fulgur) because fulgur/krilla emit a broken text
//! layer for wrapped paragraphs: every glyph is tagged with the *whole*
//! paragraph as its `/ActualText` + ToUnicode, so the reMarkable's snap-to-text
//! highlights read back as shifted/duplicated text. Typst emits a clean
//! per-glyph text layer, so snapped highlights round-trip exactly.
use std::collections::HashMap;

use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime};
use typst::syntax::{FileId, Source, VirtualPath};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};

/// Reader fonts vendored into the binary so a render resolves identically on any
/// machine. These are the exact families the fulgur CSS used: Lora (body serif,
/// incl. true italic + semibold), Hanken Grotesk (sans, labels/byline/nav),
/// Fraunces (display serif, index masthead).
const VENDORED_FONTS: &[&[u8]] = &[
    include_bytes!("../../assets/fonts/Lora-Regular.ttf"),
    include_bytes!("../../assets/fonts/Lora-Italic.ttf"),
    include_bytes!("../../assets/fonts/Lora-SemiBold.ttf"),
    include_bytes!("../../assets/fonts/HankenGrotesk-Regular.ttf"),
    include_bytes!("../../assets/fonts/HankenGrotesk-Medium.ttf"),
    include_bytes!("../../assets/fonts/HankenGrotesk-SemiBold.ttf"),
    include_bytes!("../../assets/fonts/Fraunces-Regular.ttf"),
    include_bytes!("../../assets/fonts/Fraunces-SemiBold.ttf"),
];

/// A Typst world backed by an in-memory main source. Fonts come from the
/// vendored reader set plus the `typst-assets` defaults (so monospace and any
/// fallback glyphs resolve); images are served from an in-memory map.
pub struct RmWorld {
    library: LazyHash<Library>,
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
    main: Source,
    assets: HashMap<FileId, Bytes>,
}

impl RmWorld {
    /// Build a world for `src`. `assets` is a list of `(virtual_path, bytes)`
    /// where `virtual_path` is root-absolute (e.g. `/assets/img-1.png`) to match
    /// `#image("/assets/img-1.png")` in the source.
    pub fn new(src: &str, assets: &[(String, Vec<u8>)]) -> Self {
        let mut fonts = Vec::new();
        // typst-assets defaults first (monospace + fallbacks), then our reader
        // fonts. Order only affects the FontBook index, not resolution by name.
        for data in typst_assets::fonts() {
            for face in Font::iter(Bytes::new(data.to_vec())) {
                fonts.push(face);
            }
        }
        for data in VENDORED_FONTS {
            for face in Font::iter(Bytes::new(data.to_vec())) {
                fonts.push(face);
            }
        }
        let book = FontBook::from_fonts(&fonts);
        let main_id = FileId::new(None, VirtualPath::new("main.typ"));
        let main = Source::new(main_id, src.into());
        let assets = assets
            .iter()
            .map(|(path, bytes)| {
                let id = FileId::new(None, VirtualPath::new(path));
                (id, Bytes::new(bytes.clone()))
            })
            .collect();
        Self {
            library: LazyHash::new(Library::default()),
            book: LazyHash::new(book),
            fonts,
            main,
            assets,
        }
    }
}

impl World for RmWorld {
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
            Err(FileError::NotFound(
                id.vpath().as_rootless_path().to_owned(),
            ))
        }
    }
    fn file(&self, id: FileId) -> FileResult<Bytes> {
        self.assets
            .get(&id)
            .cloned()
            .ok_or_else(|| FileError::NotFound(id.vpath().as_rootless_path().to_owned()))
    }
    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.get(index).cloned()
    }
    fn today(&self, _offset: Option<i64>) -> Option<Datetime> {
        // None keeps renders deterministic (no wall-clock in PDF bytes), matching
        // the existing byte-determinism guarantee of the generate pipeline.
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vendored_reader_fonts_resolve_by_name() {
        let world = RmWorld::new("hi", &[]);
        for family in ["Lora", "Hanken Grotesk", "Fraunces"] {
            assert!(
                world
                    .book()
                    .families()
                    .any(|(n, _)| n.eq_ignore_ascii_case(family)),
                "{family} must be in the embedded font book",
            );
        }
    }

    #[test]
    fn file_serves_registered_assets() {
        let assets = vec![("/assets/x.png".to_string(), vec![1u8, 2, 3])];
        let world = RmWorld::new("hi", &assets);
        let id = FileId::new(None, VirtualPath::new("/assets/x.png"));
        assert_eq!(world.file(id).unwrap().as_ref(), &[1u8, 2, 3]);
        let missing = FileId::new(None, VirtualPath::new("/assets/zzz.png"));
        assert!(world.file(missing).is_err());
    }
}
