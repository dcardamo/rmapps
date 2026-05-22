use lopdf::Document;
use rmbujo::config::Config;
use rmbujo::notebooks::{collection, future_log, month, reference};

fn tmp() -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    p.push(format!("rmbujo-nb-{n}.pdf"));
    p
}

fn pages(p: &std::path::Path) -> usize {
    Document::load(p).unwrap().get_pages().len()
}

#[test]
fn month_pages() {
    // monthly view + tasks + one page per day (May 2026 has 31 days)
    let cfg = Config {
        pages_per_day: 1,
        ..Config::new(2026)
    };
    let out = tmp();
    month::build_month_pdf(&cfg, 5, &std::collections::BTreeMap::new(), &out).unwrap();
    assert_eq!(pages(&out), 2 + 31);
}

#[test]
fn future_log_pages() {
    let out = tmp();
    future_log::build_future_log_pdf(&Config::new(2026), &out).unwrap();
    assert_eq!(pages(&out), 5);
}

#[test]
fn collection_pages() {
    let cfg = Config {
        collection_pages: 4,
        ..Config::new(2026)
    };
    let out = tmp();
    collection::build_collection_pdf(&cfg, &out).unwrap();
    assert_eq!(pages(&out), 1 + 4);
}

#[test]
fn reference_pages() {
    let out = tmp();
    reference::build_reference_pdf(&Config::new(2026), &out).unwrap();
    assert_eq!(pages(&out), 3);
}
