use std::collections::HashMap;

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
    sources: HashMap<FileId, Source>,
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
        let sources = sources
            .iter()
            .map(|(path, text)| {
                let id = FileId::new(None, VirtualPath::new(path));
                (id, Source::new(id, text.clone()))
            })
            .collect();
        Self {
            library: LazyHash::new(Library::default()),
            book: LazyHash::new(book),
            fonts,
            main,
            sources,
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
        Err(FileError::NotFound(
            id.vpath().as_rootless_path().to_owned(),
        ))
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
