use lopdf::Document;
use rmbujo::device::get_device;
use rmbujo::geometry::default_grid;
use rmbujo::render::render_pdf;
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
