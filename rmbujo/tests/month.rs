use lopdf::Document;
use rmbujo::config::Config;
use rmbujo::notebooks::month::build_month_pdf;

fn tmp() -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    p.push(format!("rmbujo-month-{n}.pdf"));
    p
}

#[test]
fn month_page_count_static() {
    let cfg = Config {
        pages_per_day: 1,
        ..Config::new(2027)
    };
    let out = tmp();
    build_month_pdf(&cfg, 5, &out).unwrap();
    let doc = Document::load(&out).unwrap();
    // monthly view + tasks + 31 daily pages (May has 31 days)
    assert_eq!(doc.get_pages().len(), 2 + 31);
}

#[test]
fn month_pages_per_day_multiplies_daily() {
    let cfg = Config {
        pages_per_day: 2,
        ..Config::new(2027)
    };
    let out = tmp();
    // Feb 2027 = 28 days
    build_month_pdf(&cfg, 2, &out).unwrap();
    let doc = Document::load(&out).unwrap();
    assert_eq!(doc.get_pages().len(), 2 + 28 * 2);
}
