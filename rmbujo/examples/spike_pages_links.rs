//! Throwaway spike: prove two reMarkable facts the dynamic-calendar design rides on.
//!
//!   A. Trailing page count can change under `rmapi put --content-only` without
//!      disturbing annotations on the leading pages (append pages at the END).
//!   B. fulgur-emitted internal links (`<a href="#id">` -> `id="id"`) are
//!      tappable on the device.
//!
//! Produces two PDFs, BOTH named `spike.pdf` (so a content-only push replaces the
//! same cloud document by name) in separate dirs:
//!   /tmp/rmbujo-spike/spike.pdf      v1: 5 pages, page1 links to page4
//!   /tmp/rmbujo-spike-v2/spike.pdf   v2: 7 pages (2 appended at the end)
//!
//! Run: nix develop -c cargo run --example spike_pages_links

use std::path::Path;

use rmbujo::device::get_device;
use rmbujo::geometry::default_grid;
use rmbujo::render::render_pdf;
use rmbujo::theme::load_theme;

/// One dotted page with a heading and arbitrary body HTML.
fn page(title: &str, body: &str) -> String {
    format!("<section class=\"page dotpage\"><div class=\"h-month\">{title}</div>{body}</section>")
}

fn main() -> anyhow::Result<()> {
    let dev = get_device("paper-pro-move")?;
    let grid = default_grid(&dev);
    let theme = load_theme("library")?;

    // Internal link: page 1 -> the block with id="target" on page 4.
    let link = "<a href=\"#target\" \
        style=\"color:#1B365D;text-decoration:underline;font-size:14pt;\">\
        Tap to jump to Page 4 \u{2192}</a>";

    let v1 = vec![
        page("Spike Page 1", link),
        page(
            "Page 2 \u{2014} ANNOTATE THIS PAGE",
            "<div style=\"font-size:10pt;\">Write something here, then sync.</div>",
        ),
        page("Page 3", ""),
        page(
            "Page 4 \u{2014} LINK TARGET",
            "<div id=\"target\" style=\"font-size:12pt;\">You arrived via the link.</div>",
        ),
        page("Page 5 (last page in v1)", ""),
    ];

    let mut v2 = v1.clone();
    v2.push(page("Page 6 \u{2014} NEW (appended in v2)", ""));
    v2.push(page("Page 7 \u{2014} NEW (appended in v2)", ""));

    // v3: two MORE pages appended, to test page growth AFTER a user has inserted
    // and annotated a page in the middle on-device.
    let mut v3 = v2.clone();
    v3.push(page("Page 8 \u{2014} NEW (appended in v3)", ""));
    v3.push(page("Page 9 \u{2014} NEW (appended in v3)", ""));

    let d1 = Path::new("/tmp/rmbujo-spike");
    let d2 = Path::new("/tmp/rmbujo-spike-v2");
    let d3 = Path::new("/tmp/rmbujo-spike-v3");
    std::fs::create_dir_all(d1)?;
    std::fs::create_dir_all(d2)?;
    std::fs::create_dir_all(d3)?;
    render_pdf(&dev, &grid, &theme, &v1, &d1.join("spike.pdf"))?;
    render_pdf(&dev, &grid, &theme, &v2, &d2.join("spike.pdf"))?;
    render_pdf(&dev, &grid, &theme, &v3, &d3.join("spike.pdf"))?;

    println!("v1: {} pages -> {}", v1.len(), d1.join("spike.pdf").display());
    println!("v2: {} pages -> {}", v2.len(), d2.join("spike.pdf").display());
    println!("v3: {} pages -> {}", v3.len(), d3.join("spike.pdf").display());
    Ok(())
}
