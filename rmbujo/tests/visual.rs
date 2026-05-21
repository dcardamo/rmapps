use std::path::{Path, PathBuf};
use std::process::Command;

use askama::Template;
use rmbujo::calendar::build_month;
use rmbujo::device::get_device;
use rmbujo::geometry::default_grid;
use rmbujo::render::render_pdf;
use rmbujo::templates::{Cover, DayView, DotGrid, FutureLog, MonthIndex, Reference, Tasks};
use rmbujo::theme::load_theme;

const TOLERANCE: f64 = 0.01; // max fraction of differing pixels

fn goldens_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/goldens")
}

fn tmp(tag: &str, ext: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    p.push(format!("rmbujo-vis-{tag}-{n}.{ext}"));
    p
}

fn fragment_pages() -> Vec<(&'static str, String)> {
    let m = build_month(2026, 5, "sun").unwrap();
    let days: Vec<DayView> = m
        .days
        .iter()
        .map(|d| DayView {
            day: d.day,
            weekday: d.weekday,
            week_start: d.week_start,
        })
        .collect();
    vec![
        (
            "cover",
            Cover {
                year: 2026,
                title: "Future Log",
                blank_title: false,
            }
            .render()
            .unwrap(),
        ),
        (
            "cover_blank",
            Cover {
                year: 2026,
                title: "",
                blank_title: true,
            }
            .render()
            .unwrap(),
        ),
        ("dotgrid", DotGrid.render().unwrap()),
        ("tasks", Tasks.render().unwrap()),
        (
            "month_index",
            MonthIndex {
                month_name: "May",
                year: 2026,
                days: &days,
            }
            .render()
            .unwrap(),
        ),
        (
            "future_log",
            FutureLog {
                months: &["January", "February", "March"],
            }
            .render()
            .unwrap(),
        ),
        ("reference", Reference.render().unwrap()),
    ]
}

/// Render one fragment to a single-page PDF, then rasterize page 1 to PNG via pdftoppm.
fn render_png(fragment: &str, png: &Path) {
    let dev = get_device("paper-pro-move").unwrap();
    let grid = default_grid(&dev);
    let theme = load_theme("library").unwrap();
    let pdf = tmp("page", "pdf");
    render_pdf(&dev, &grid, &theme, &[fragment.to_string()], &pdf).unwrap();

    // pdftoppm writes "<prefix>.png" with -singlefile; use prefix without extension.
    let prefix = png.with_extension("");
    let status = Command::new("pdftoppm")
        .args([
            "-png",
            "-r",
            "150",
            "-singlefile",
            pdf.to_str().unwrap(),
            prefix.to_str().unwrap(),
        ])
        .status()
        .expect("pdftoppm");
    assert!(status.success(), "pdftoppm failed");
}

fn diff_ratio(a: &Path, b: &Path) -> f64 {
    let ia = image::open(a).unwrap().to_rgb8();
    let ib = image::open(b).unwrap().to_rgb8();
    if ia.dimensions() != ib.dimensions() {
        return 1.0;
    }
    let total = (ia.width() * ia.height()) as f64;
    let mut diff = 0u64;
    for (pa, pb) in ia.pixels().zip(ib.pixels()) {
        if pa != pb {
            diff += 1;
        }
    }
    diff as f64 / total
}

#[test]
fn visual_regression() {
    let update = std::env::var("RMBUJO_UPDATE_GOLDENS").is_ok();
    std::fs::create_dir_all(goldens_dir()).unwrap();

    for (name, fragment) in fragment_pages() {
        let shot = tmp(name, "png");
        render_png(&fragment, &shot);
        let golden = goldens_dir().join(format!("{name}.png"));
        if update {
            std::fs::copy(&shot, &golden).unwrap();
            continue;
        }
        assert!(
            golden.exists(),
            "missing golden {name}; run `make update-goldens`"
        );
        let ratio = diff_ratio(&shot, &golden);
        assert!(
            ratio < TOLERANCE,
            "{name} differs by {ratio:.4} (> {TOLERANCE})"
        );
    }
}
