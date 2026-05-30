//! A minimal Typst `World` for rmbujo: an in-memory main source plus the
//! journal fonts embedded from `assets/fonts` (deterministic — no host font
//! search) and image assets served through `file()`.
//!
//! We render with Typst (not fulgur) because fulgur/krilla emit a broken text
//! layer for the reMarkable's snap-to-text read-back; Typst emits a clean
//! per-glyph text layer. The whole page (dot grid, cover gradient, links) is
//! drawn in-flow by Typst.
use std::collections::HashMap;

use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime};
use typst::syntax::{FileId, Source, VirtualPath};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};

/// Journal fonts vendored into the binary so a render resolves identically on any
/// machine. These are the exact families the fulgur CSS used: Lora (body serif,
/// regular + semibold), Hanken Grotesk (sans, labels/badges/meta), Fraunces 72pt
/// (display serif, mastheads + page headings).
const VENDORED_FONTS: &[&[u8]] = &[
    include_bytes!("../../assets/fonts/Lora-Regular.ttf"),
    include_bytes!("../../assets/fonts/Lora-SemiBold.ttf"),
    include_bytes!("../../assets/fonts/HankenGrotesk-Regular.ttf"),
    include_bytes!("../../assets/fonts/HankenGrotesk-Medium.ttf"),
    include_bytes!("../../assets/fonts/Fraunces72pt-SemiBold.ttf"),
];

/// A Typst world backed by an in-memory main source. Fonts come from the
/// vendored journal set plus the `typst-assets` defaults (so any symbol glyph
/// the legend needs resolves via fallback); images are served from an in-memory
/// map keyed by root-absolute virtual path.
pub struct RmWorld {
    library: LazyHash<Library>,
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
    main: Source,
    assets: HashMap<FileId, Bytes>,
}

impl RmWorld {
    /// Build a world for `src`. `assets` is a list of `(virtual_path, bytes)`
    /// where `virtual_path` is root-absolute (e.g. `/assets/dot.svg`) to match
    /// `#image("/assets/dot.svg")` in the source.
    pub fn new(src: &str, assets: &[(String, Vec<u8>)]) -> Self {
        let mut fonts = Vec::new();
        // Vendored journal fonts first, then the typst-assets defaults for symbol
        // fallback. Order only affects the FontBook index, not resolution by name.
        for data in VENDORED_FONTS {
            for face in Font::iter(Bytes::new(data.to_vec())) {
                fonts.push(face);
            }
        }
        for data in typst_assets::fonts() {
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
        // the byte-determinism guarantee the generate pipeline relies on.
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vendored_journal_fonts_resolve_by_name() {
        let world = RmWorld::new("hi", &[]);
        for family in ["Lora", "Hanken Grotesk", "Fraunces 72pt"] {
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
        let assets = vec![("/assets/x.svg".to_string(), vec![1u8, 2, 3])];
        let world = RmWorld::new("hi", &assets);
        let id = FileId::new(None, VirtualPath::new("/assets/x.svg"));
        assert_eq!(world.file(id).unwrap().as_ref(), &[1u8, 2, 3]);
        let missing = FileId::new(None, VirtualPath::new("/assets/zzz.svg"));
        assert!(world.file(missing).is_err());
    }
}
