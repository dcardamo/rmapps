use askama::Template;
use lopdf::Document;
use rmbujo::device::get_device;
use rmbujo::geometry::default_grid;
use rmbujo::render::{build_css, render_pdf};
use rmbujo::templates::DotGrid;
use rmbujo::theme::load_theme;

fn tmp(name: &str) -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
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
    let mb = doc
        .get_object(page_id)
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"MediaBox")
        .unwrap()
        .as_array()
        .unwrap();
    let w = mb[2].as_float().unwrap();
    let h = mb[3].as_float().unwrap();
    assert!((w - dev.width_pt()).abs() < 1.0, "width {w}");
    assert!((h - dev.height_pt()).abs() < 1.0, "height {h}");
}

#[test]
fn month_index_shares_device_dot_grid() {
    // The monthly page uses the SAME device-aligned dot grid as the daily/tasks
    // pages (so an inserted "Dots Small" page lines up on every page), and each day
    // label gets a paper knockout so it stays crisp over the dots. The visual golden
    // covers the rendered result; this guards the CSS that makes it work.
    let dev = get_device("paper-pro-move").unwrap();
    let grid = default_grid(&dev);
    let css = build_css(&dev, &grid, &load_theme("library").unwrap());
    let sp = grid.spacing_pt;
    assert!(
        css.contains(".dotpage, .month-index {"),
        "month index must share the device dot grid with .dotpage:\n{css}"
    );
    assert!(
        css.contains(&format!("background-size: {sp}pt {sp}pt")),
        "dots must tile at the full device pitch (matching the template):\n{css}"
    );
    assert!(
        css.contains("width: 44pt; background: var(--paper)"),
        "day labels need a paper knockout to stay crisp over the dots:\n{css}"
    );
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
