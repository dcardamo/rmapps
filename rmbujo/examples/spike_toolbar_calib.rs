//! Throwaway spike: measure the reMarkable toolbar's safe height.
//!
//! Renders one page with labeled horizontal rulers from the TRUE top of the page
//! (padding zeroed) plus a header at the very top. View it on the Move with the
//! toolbar SHOWN and read off the first ruler line fully visible below the
//! toolbar -> that pt/mm value is the top safe-area to reserve on every page.
//!
//! Run: nix develop -c cargo run --example spike_toolbar_calib

use std::path::Path;

use rmbujo::device::get_device;
use rmbujo::geometry::default_grid;
use rmbujo::render::render_pdf;
use rmbujo::theme::load_theme;

fn main() -> anyhow::Result<()> {
    let dev = get_device("paper-pro-move")?;
    let grid = default_grid(&dev);
    let theme = load_theme("library")?;

    // padding:0 so absolute offsets are measured from the real page top edge.
    let mut body = String::from(
        "<section class=\"page\" style=\"padding:0;position:relative;\">\
         <div style=\"position:absolute;top:1pt;left:6pt;font-size:13pt;\
         font-weight:bold;color:#1B365D;\">May 2027 (header at true top)</div>",
    );
    // Ruler lines every 5pt from 10..=95pt, labeled in pt and mm (1pt = 0.3528mm).
    let mut y = 10;
    while y <= 95 {
        let mm = y as f32 * 0.3528;
        body.push_str(&format!(
            "<div style=\"position:absolute;top:{y}pt;left:0;right:0;\
             border-top:0.4pt solid #555;\"></div>\
             <div style=\"position:absolute;top:{lbl}pt;left:4pt;font-size:6pt;\
             color:#555;\">{y}pt / {mm:.1}mm</div>",
            lbl = y - 7,
        ));
        y += 5;
    }
    body.push_str("</section>");

    let dir = Path::new("/tmp/rmbujo-calib");
    std::fs::create_dir_all(dir)?;
    let out = dir.join("toolbar-calib.pdf");
    render_pdf(&dev, &grid, &theme, &[body], &out)?;
    println!("calibration page -> {}", out.display());
    Ok(())
}
