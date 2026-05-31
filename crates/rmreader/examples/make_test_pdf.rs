//! Generate a small, controlled 2-article test PDF through the real pipeline.
//!
//! Usage: `cargo run --example make_test_pdf`
//!
//! Writes `/tmp/rmtest/RMTest.pdf` with known content and a visible build tag.
//! No network required (images disabled).

fn main() -> anyhow::Result<()> {
    // Build tag: HH:MM:SS using std::time (no chrono dep needed).
    let tag = {
        use std::time::{SystemTime, UNIX_EPOCH};
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        // Offset to local time: derive UTC seconds-of-day and convert via local offset.
        // Use libc localtime_r for correct local time without pulling in chrono.
        // Simpler approach: just use UTC time — the tag only needs to be distinct.
        let s = secs % 86400;
        let h = s / 3600;
        let m = (s % 3600) / 60;
        let sec = s % 60;
        format!("{h:02}:{m:02}:{sec:02}")
    };

    let docs = vec![
        rmreader::readwise::Document {
            id: "alpha".to_string(),
            url: "https://example.com/alpha".to_string(),
            source_url: "https://example.com/alpha".to_string(),
            title: format!("Alpha Test Article — build {tag}"),
            author: "Tester".to_string(),
            site_name: String::new(),
            category: "articles".to_string(),
            location: String::new(),
            summary: String::new(),
            image_url: String::new(),
            word_count: None,
            reading_time: None,
            published_date: None,
            saved_at: "2026-05-22T00:00:00Z".to_string(),
            html_content: Some(format!(
                "<p><b>BUILD {tag}</b></p>\
                 <p>The quick brown fox jumps over the lazy dog.</p>\
                 <p>Alpha body line two for content-highlight testing.</p>\
                 <p>Alpha body line three.</p>"
            )),
        },
        rmreader::readwise::Document {
            id: "beta".to_string(),
            url: "https://example.com/beta".to_string(),
            source_url: "https://example.com/beta".to_string(),
            title: "Beta Test Article".to_string(),
            author: "Tester".to_string(),
            site_name: String::new(),
            category: "articles".to_string(),
            location: String::new(),
            summary: String::new(),
            image_url: String::new(),
            word_count: None,
            reading_time: None,
            published_date: None,
            saved_at: "2026-05-21T00:00:00Z".to_string(),
            html_content: Some(
                "<p>Sphinx of black quartz, judge my vow.</p>\
                 <p>Beta body line two for content-highlight testing.</p>\
                 <p>Beta body line three.</p>"
                    .to_string(),
            ),
        },
    ];

    let cfg: rmreader::config::Config = toml::from_str(
        r#"
device = "paper-pro-move"
output_dir = "/tmp/rmtest"
theme = "reader"
[readwise]
token = "x"
[images]
enabled = false
"#,
    )?;

    std::fs::create_dir_all("/tmp/rmtest")?;

    let path = rmreader::generate::build_pdf_from_docs(
        "RMTest",
        &docs,
        &cfg,
        std::path::Path::new("/tmp/rmtest"),
    )?;

    println!("wrote {} (build {tag})", path.display());
    Ok(())
}
