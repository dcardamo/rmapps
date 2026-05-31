use std::collections::HashMap;

use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime};
use typst::syntax::{FileId, Source, VirtualPath};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};

/// Fonts vendored into the framework so a `Theme` resolves deterministically with
/// no host font search. Curated reading set: Newsreader (body serif, incl. true
/// italic), Fraunces (display serif), Hanken Grotesk (sans, for labels). Monospace
/// is served by the typst-assets `DejaVu Sans Mono`, so none is vendored here.
const VENDORED_FONTS: &[&[u8]] = &[
    include_bytes!("../assets/fonts/Newsreader-Regular.ttf"),
    include_bytes!("../assets/fonts/Newsreader-Italic.ttf"),
    include_bytes!("../assets/fonts/Newsreader-SemiBold.ttf"),
    include_bytes!("../assets/fonts/Fraunces-Regular.ttf"),
    include_bytes!("../assets/fonts/Fraunces-SemiBold.ttf"),
    include_bytes!("../assets/fonts/HankenGrotesk-Regular.ttf"),
    include_bytes!("../assets/fonts/HankenGrotesk-SemiBold.ttf"),
];

/// A Typst world backed by an in-memory main source and fonts embedded from
/// `typst-assets` (deterministic; no host font search).
pub struct InkWorld {
    library: LazyHash<Library>,
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
    main: Source,
    sources: HashMap<FileId, Source>,
    assets: HashMap<FileId, Bytes>,
}

impl InkWorld {
    pub fn new(src: &str) -> Self {
        Self::with_sources(src, &[])
    }

    /// Like `new`, but registers additional named Typst sources (e.g. component
    /// `.typ` files) so the main source can `#import` them. `sources` is a list of
    /// `(virtual_path, source_text)`; paths are root-absolute (leading `/`) to
    /// match `#import "/path.typ"`.
    pub fn with_sources(src: &str, sources: &[(String, String)]) -> Self {
        Self::with_sources_and_assets(src, sources, &[])
    }

    /// Like `with_sources`, but also registers image assets served by `file()`.
    /// `assets` is a list of `(virtual_path, bytes)`; paths are root-absolute
    /// (e.g. `/assets/{key}.png`) to match `#image("/assets/{key}.png")`.
    pub fn with_sources_and_assets(
        src: &str,
        sources: &[(String, String)],
        assets: &[(String, Vec<u8>)],
    ) -> Self {
        let mut fonts = Vec::new();
        for data in typst_assets::fonts() {
            let bytes = Bytes::new(data.to_vec());
            // A single TTF/OTF file may contain multiple faces.
            for face in Font::iter(bytes) {
                fonts.push(face);
            }
        }
        // Vendored reading fonts share the same book as the typst-assets defaults.
        for data in VENDORED_FONTS {
            let bytes = Bytes::new(data.to_vec());
            for face in Font::iter(bytes) {
                fonts.push(face);
            }
        }
        let book = FontBook::from_fonts(&fonts);
        let main_id = FileId::new(None, VirtualPath::new("main.typ"));
        let main = Source::new(main_id, src.into());
        let sources = sources
            .iter()
            .map(|(path, text)| {
                let id = FileId::new(None, VirtualPath::new(path));
                (id, Source::new(id, text.clone()))
            })
            .collect();
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
            sources,
            assets,
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
        } else if let Some(s) = self.sources.get(&id) {
            Ok(s.clone())
        } else {
            Err(FileError::NotFound(
                id.vpath().as_rootless_path().to_owned(),
            ))
        }
    }
    fn file(&self, id: FileId) -> FileResult<Bytes> {
        match self.assets.get(&id) {
            Some(bytes) => Ok(bytes.clone()),
            None => Err(FileError::NotFound(
                id.vpath().as_rootless_path().to_owned(),
            )),
        }
    }
    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.get(index).cloned()
    }
    fn today(&self, _offset: Option<i64>) -> Option<Datetime> {
        // Returning None makes Typst date/time calls produce `none`, by design:
        // the harness must never embed wall-clock time in compiled documents or
        // PDF bytes, which would break determinism (and the no-secrets rule).
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_serves_registered_assets() {
        let assets = vec![("/assets/abc.png".to_string(), vec![1u8, 2, 3, 4])];
        let world = InkWorld::with_sources_and_assets("hello", &[], &assets);

        let id = FileId::new(None, VirtualPath::new("/assets/abc.png"));
        assert_eq!(world.file(id).unwrap().as_ref(), &[1u8, 2, 3, 4]);

        let missing = FileId::new(None, VirtualPath::new("/assets/zzz.png"));
        assert!(world.file(missing).is_err());
    }

    #[test]
    fn vendored_fonts_in_book() {
        // The framework must embed the reading fonts so `#set text(font: ...)`
        // resolves with no host font search. Check every vendored family.
        let world = InkWorld::new("hello");
        for family in ["Newsreader", "Fraunces", "Hanken Grotesk"] {
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
    fn plain_world_serves_no_files() {
        let world = InkWorld::new("hello");
        let id = FileId::new(None, VirtualPath::new("/assets/abc.png"));
        assert!(world.file(id).is_err());
    }
}
