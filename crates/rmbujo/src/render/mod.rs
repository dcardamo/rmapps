//! Render journal pages to PDF via Typst.
//!
//! Typst replaced fulgur/krilla here: fulgur emits a broken text layer for the
//! reMarkable's snap-to-text read-back, while Typst emits a clean per-glyph
//! layer. Each notebook builder produces a list of per-page Typst *fragments*
//! (see `crate::templates`); this module prepends the shared preamble
//! (`doc::build_preamble`) and compiles the whole document at once.

use std::path::Path;

use crate::device::Device;
use crate::geometry::GridSpec;
use crate::theme::Palette;

pub mod doc;
pub mod world;

pub use world::RmWorld;

/// Compile a Typst source (with `assets` served via `file()` under `/assets/…`)
/// to PDF bytes.
pub fn compile_pdf(src: &str, assets: &[(String, Vec<u8>)]) -> anyhow::Result<Vec<u8>> {
    // Start from a clean memoization cache. Typst memoizes layout/compile results
    // through comemo's process-global cache; rendering several notebooks in one
    // process (the whole-year regenerate, or the test binary) otherwise lets a
    // prior document's cached results bleed into the next, so e.g. the month
    // index's dot grid lays out with a stale pitch. typst-cli evicts every cycle
    // for the same reason; we render each document fresh, so evict everything.
    comemo::evict(0);
    let world = RmWorld::new(src, assets);
    let document = typst::compile::<typst::layout::PagedDocument>(&world)
        .output
        .map_err(|d| anyhow::anyhow!("typst compile failed: {d:?}"))?;
    let pdf = typst_pdf::pdf(&document, &typst_pdf::PdfOptions::default())
        .map_err(|d| anyhow::anyhow!("typst pdf export failed: {d:?}"))?;
    Ok(pdf)
}

/// Render a notebook's page fragments to a PDF file. `fragments` are per-page
/// Typst markup strings (each already wrapped in a `*-page(...)` helper), in
/// document order.
pub fn render_pdf(
    device: &Device,
    grid: &GridSpec,
    theme: &Palette,
    fragments: &[String],
    out_path: &Path,
) -> anyhow::Result<()> {
    let mut src = doc::build_preamble(device, grid, theme);
    for f in fragments {
        src.push_str(f);
        src.push('\n');
    }
    if std::env::var("RMBUJO_DUMP_TYPST").is_ok() {
        let _ = std::fs::write("/tmp/rmbujo_last.typ", &src);
    }
    // The dot grid and cover gradient are drawn natively by Typst, so there are
    // no image assets to serve.
    let pdf = compile_pdf(&src, &[])?;
    std::fs::write(out_path, pdf)?;
    Ok(())
}
