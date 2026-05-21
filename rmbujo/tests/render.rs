use lopdf::Document;
use rmbujo::device::get_device;
use rmbujo::geometry::default_grid;
use rmbujo::render::{build_css, render_pdf};
use rmbujo::theme::load_theme;
use rmbujo::templates::DotGrid;
use askama::Template;

fn tmp(name: &str) -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    p.push(format!("rmbujo-{name}-{n}.pdf"));
    p
}

fn fragments() -> Vec<String> {
    vec![DotGrid.render().unwrap(), DotGrid.render().unwrap()]
}

#[test]
fn page_count_and_size() {
    let dev = get_device("paper-pro-move").unwrap();
    let grid = default_grid(&dev);
    let theme = load_theme("library").unwrap();
    let out = tmp("size");
    render_pdf(&dev, &grid, &theme, &fragments(), &out).unwrap();

    let doc = Document::load(&out).unwrap();
    assert_eq!(doc.get_pages().len(), 2);
    let page_id = *doc.get_pages().get(&1).unwrap();
    let mb = doc.get_object(page_id).unwrap().as_dict().unwrap()
        .get(b"MediaBox").unwrap().as_array().unwrap();
    let w = mb[2].as_float().unwrap();
    let h = mb[3].as_float().unwrap();
    assert!((w - dev.width_pt()).abs() < 1.0, "width {w}");
    assert!((h - dev.height_pt()).abs() < 1.0, "height {h}");
}

#[test]
fn month_index_dots_sit_between_rows() {
    // The month index paints its dot grid on the day LIST (not the page), offset
    // half a pitch. With each day row exactly one pitch tall, a dot row then lands
    // on every row boundary and the centred label falls in the gap between two
    // dots — never astride one (the "dot strikethrough"). Anchoring the grid to the
    // list keeps this true regardless of heading height, page count, or spacing,
    // none of which the engine pins down for a page-fixed grid. Guard the two
    // values that make it work; the visual golden covers the rendered result.
    let dev = get_device("paper-pro-move").unwrap();
    let grid = default_grid(&dev);
    let css = build_css(&dev, &grid, &load_theme("library").unwrap());
    let sp = grid.spacing_pt;
    let half = 0.5 * sp;
    assert!(css.contains(&format!(".day {{ height: {sp}pt")), "day rows must be one dot pitch tall:\n{css}");
    assert!(css.contains(&format!("background-position: 0pt {half}pt")),
        "month-list dot grid must be offset half a pitch so labels sit between dots:\n{css}");
}

#[test]
fn deterministic_bytes() {
    let dev = get_device("paper-pro-move").unwrap();
    let grid = default_grid(&dev);
    let theme = load_theme("library").unwrap();
    let a = tmp("a");
    let b = tmp("b");
    render_pdf(&dev, &grid, &theme, &fragments(), &a).unwrap();
    render_pdf(&dev, &grid, &theme, &fragments(), &b).unwrap();
    assert_eq!(std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap());
}
