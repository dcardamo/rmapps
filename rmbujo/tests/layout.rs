use rmbujo::config::Config;
use rmbujo::device::get_device;
use rmbujo::notebooks::{future_log, month, reference};
use rmbujo::render::{inspect, TextItem};

const TOL: f32 = 0.5; // pt — accommodates inspect()'s estimated text widths

// fulgur's inspect() estimates glyph advance widths as chars*font_size*0.5.
// For CID-encoded fonts (used by krilla/fulgur) each character occupies a 2-byte
// glyph ID, so the raw char count is 2× the actual glyph count, inflating every
// estimated width by 2×.  Dividing by this factor restores a usable approximation
// for overlap and bounds checks.
const WIDTH_SCALE: f32 = 0.5;

fn tmp(tag: &str) -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    p.push(format!("rmbujo-layout-{tag}-{n}.pdf"));
    p
}

fn overlaps(a: &TextItem, b: &TextItem) -> bool {
    if a.page != b.page {
        return false;
    }
    let aw = a.width * WIDTH_SCALE;
    let bw = b.width * WIDTH_SCALE;
    let x_overlap = a.x < b.x + bw - TOL && b.x < a.x + aw - TOL;
    let y_overlap = a.y < b.y + b.height - TOL && b.y < a.y + a.height - TOL;
    x_overlap && y_overlap
}

fn assert_no_overlap_and_in_bounds(pdf: &std::path::Path) {
    let dev = get_device("paper-pro-move").unwrap();
    let (w, h) = (dev.width_pt(), dev.height_pt());
    let result = inspect(pdf).unwrap();

    // In-bounds: every text box origin within [0,w] x [0,h] (with tolerance).
    // Right-edge (x + width*WIDTH_SCALE) is also checked; WIDTH_SCALE corrects for
    // the 2× width inflation that CID 2-byte glyph encoding causes in inspect().
    for t in &result.text_items {
        let right = t.x + t.width * WIDTH_SCALE;
        let bottom = t.y + t.height;
        assert!(
            t.x >= -TOL && t.y >= -TOL && right <= w + TOL && bottom <= h + TOL,
            "text {:?} out of page bounds: x={} y={} estimated_right={:.2} bottom={:.2} (page {}x{})",
            t.text, t.x, t.y, right, bottom, w, h,
        );
    }
    // No-overlap: pairwise on the same page.
    let items = &result.text_items;
    for i in 0..items.len() {
        for j in (i + 1)..items.len() {
            assert!(
                !overlaps(&items[i], &items[j]),
                "text overlap on page {}: {:?} <-> {:?}",
                items[i].page, items[i].text, items[j].text,
            );
        }
    }
}

fn assert_text_present(pdf: &std::path::Path) {
    // fulgur's inspect() decodes text as raw glyph IDs for CID fonts, not Unicode,
    // so we cannot search for readable strings like "May" or "2026". We verify that
    // at least some text items were emitted — confirming text rendered into the PDF
    // (not just into the intermediate HTML) and that the PDF is non-trivial.
    let result = inspect(pdf).unwrap();
    assert!(
        !result.text_items.is_empty(),
        "expected text items in rendered PDF, got none",
    );
}

#[test]
fn month_layout_clean() {
    let cfg = Config { daily_pages: 1, ..Config::new(2026) };
    let out = tmp("month");
    month::build_month_pdf(&cfg, 5, &out).unwrap();
    assert_no_overlap_and_in_bounds(&out);
    // Text actually rendered into the PDF (not just present in the HTML).
    assert_text_present(&out);
}

#[test]
fn future_log_layout_clean() {
    let out = tmp("fl");
    future_log::build_future_log_pdf(&Config::new(2026), &out).unwrap();
    assert_no_overlap_and_in_bounds(&out);
}

#[test]
fn reference_layout_clean() {
    let out = tmp("ref");
    reference::build_reference_pdf(&Config::new(2026), &out).unwrap();
    assert_no_overlap_and_in_bounds(&out);
}
