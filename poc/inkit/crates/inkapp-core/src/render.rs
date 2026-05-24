use typst::layout::PagedDocument;

use crate::error::{Error, Result};
use crate::world::InkWorld;

/// Compile Typst source to a laid-out document. The single-arg form authors no
/// component `.typ` files (used by the harness and most tests).
pub fn compile_to_document(src: &str) -> Result<PagedDocument> {
    compile_to_document_with_sources(src, &[])
}

/// Compile with additional registered Typst sources the main source may `#import`
/// (component render halves + the framework prelude).
pub fn compile_to_document_with_sources(
    src: &str,
    sources: &[(String, String)],
) -> Result<PagedDocument> {
    let world = InkWorld::with_sources(src, sources);
    typst::compile::<PagedDocument>(&world)
        .output
        .map_err(|d| Error::Compile(format!("{d:?}")))
}

/// Export a laid-out document to PDF bytes.
pub fn document_to_pdf(doc: &PagedDocument) -> Result<Vec<u8>> {
    typst_pdf::pdf(doc, &typst_pdf::PdfOptions::default()).map_err(|d| Error::Pdf(format!("{d:?}")))
}

/// Whether a region name is safe to interpolate into Typst markup.
///
/// Region names are embedded into a Typst string literal by [`region_metadata`];
/// a name containing `"`, `)`, `]`, or other markup characters would silently
/// break compilation of the whole document. We constrain names to an identifier
/// alphabet (plus `:` for namespacing, e.g. `box:checkmark:0`) so that failure
/// mode cannot occur. Component authors mint names from this alphabet.
pub fn is_valid_region_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == ':')
}

/// Emit the `#place`d metadata markup that [`crate::manifest::recover_regions`]
/// reads back. Coordinates are Typst-space (top-left origin) points.
///
/// # Panics
/// Panics if `name` is not a valid region name (see [`is_valid_region_name`]).
/// This is a programmer error: names are developer-/component-chosen, never
/// end-user input, so a constraint violation indicates a bug at the call site.
pub fn region_metadata(name: &str, page: usize, x: f64, y: f64, w: f64, h: f64) -> String {
    assert!(
        is_valid_region_name(name),
        "region name must be non-empty ASCII alphanumeric/_/-/:, got: {name:?}"
    );
    format!(
        "#place(top + left, dx: {x}pt, dy: {y}pt, box(width: {w}pt, height: {h}pt)[#metadata((name: \"{name}\", page: {page}, x: {x}, y: {y}, w: {w}, h: {h})) <region>])\n"
    )
}

#[cfg(test)]
mod region_tests {
    use super::*;

    #[test]
    fn region_name_validation() {
        assert!(is_valid_region_name("done"));
        assert!(is_valid_region_name("tok-3"));
        assert!(is_valid_region_name("habit_streak"));
        assert!(
            is_valid_region_name("box:checkmark:0"),
            "colon namespace separator is allowed"
        );
        assert!(!is_valid_region_name(""), "empty is rejected");
        assert!(!is_valid_region_name("has space"), "space is rejected");
        assert!(!is_valid_region_name("quote\"inside"), "quote is rejected");
        assert!(!is_valid_region_name("paren)inside"), "paren is rejected");
    }

    #[test]
    #[should_panic(expected = "region name must be")]
    fn region_metadata_panics_on_unsafe_name() {
        // A name with a quote would close the Typst string literal and silently
        // break compilation, so it must be rejected loudly at the call site.
        let _ = region_metadata("bad\"name", 0, 0.0, 0.0, 1.0, 1.0);
    }
}
