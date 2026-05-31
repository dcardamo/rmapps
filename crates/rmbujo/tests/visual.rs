use std::path::{Path, PathBuf};
use std::process::Command;

use rmbujo::calendar::build_month;
use rmbujo::device::get_device;
use rmbujo::geometry::default_grid;
use rmbujo::render::render_pdf;
use rmbujo::templates::{
    AgendaEvent, Cover, DailyPage, DayEvents, DayRow, DotGrid, FutureLog, MonthlyView, Reference,
    Tasks,
};
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
    let dev = get_device("paper-pro-move").unwrap();
    let grid = default_grid(&dev);
    let m = build_month(2026, 5, "sun").unwrap();

    // monthly_view — days 19 and 24 carry event badges
    let day_rows: Vec<DayRow> = m
        .days
        .iter()
        .map(|d| DayRow {
            day: d.day,
            weekday: d.weekday,
            week_start: d.week_start,
            event_count: if d.day == 19 {
                2
            } else if d.day == 24 {
                1
            } else {
                0
            },
        })
        .collect();
    let monthly_view = MonthlyView {
        month_name: "May",
        year: 2026,
        month_num: 5,
        spacing_pt: grid.spacing_pt,
        days: &day_rows,
    }
    .render()
    .unwrap();

    // daily_page with a badge
    let daily_page = DailyPage {
        day: 19,
        day_pad: "19".into(),
        month_num: 5,
        weekday: "Tue",
        event_count: 2,
    }
    .render()
    .unwrap();

    // day_events — one day's events, each shown in full (merged agenda+details).
    let day_evts = vec![
        AgendaEvent {
            label: "All Day".into(),
            end_label: None,
            title: "Victoria Day".into(),
            location: None,
            description: None,
            attendees: vec![],
            color: "accent".into(),
            is_all_day: true,
        },
        AgendaEvent {
            label: "14:00".into(),
            end_label: Some("15:00".into()),
            title: "Dentist".into(),
            location: Some("Downtown".into()),
            description: Some("Bring card".into()),
            attendees: vec!["Dr. Lee".into()],
            color: "rust".into(),
            is_all_day: false,
        },
    ];
    let day_events = DayEvents {
        month_num: 5,
        day: 19,
        day_pad: "19".into(),
        weekday: "Tue",
        events: &day_evts,
        continued: false,
        first_page: true,
    }
    .render()
    .unwrap();

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
            "future_log",
            FutureLog {
                months: &["January", "February", "March"],
            }
            .render()
            .unwrap(),
        ),
        ("reference", Reference.render().unwrap()),
        ("monthly_view", monthly_view),
        ("daily_page", daily_page),
        ("day_events", day_events),
    ]
}

/// A hidden page that defines every cross-page link target. Fragments are
/// rendered in isolation here, so their `link(label("day-N"))` etc. would
/// otherwise reference labels that only exist on sibling pages in a real
/// notebook. Appended as a second page; `-singlefile` only rasterizes page 1, so
/// it never shows up in the golden.
fn label_sink() -> String {
    let mut anchors = String::from("#box[x]#label(\"monthly\")");
    for n in 1..=31 {
        anchors.push_str(&format!("#box[x]#label(\"day-{n}\")"));
        anchors.push_str(&format!("#box[x]#label(\"agenda-{n}\")"));
    }
    format!("#plain-page[#hide[{anchors}]]\n")
}

/// Render one fragment to a PDF (plus a hidden label-sink page so cross-page
/// links resolve), then rasterize page 1 to PNG via pdftoppm.
fn render_png(fragment: &str, png: &Path) {
    let dev = get_device("paper-pro-move").unwrap();
    let grid = default_grid(&dev);
    let theme = load_theme("library").unwrap();
    let pdf = tmp("page", "pdf");
    render_pdf(
        &dev,
        &grid,
        &theme,
        &[fragment.to_string(), label_sink()],
        &pdf,
    )
    .unwrap();

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

/// Rasterize a fragment at an explicit DPI (the golden path uses 150; alignment
/// checks need more resolution for sub-point row detection).
fn render_png_dpi(fragment: &str, png: &Path, dpi: u32) {
    let dev = get_device("paper-pro-move").unwrap();
    let grid = default_grid(&dev);
    let theme = load_theme("library").unwrap();
    let pdf = tmp("align", "pdf");
    render_pdf(
        &dev,
        &grid,
        &theme,
        &[fragment.to_string(), label_sink()],
        &pdf,
    )
    .unwrap();
    let prefix = png.with_extension("");
    let status = Command::new("pdftoppm")
        .args([
            "-png",
            "-r",
            &dpi.to_string(),
            "-singlefile",
            pdf.to_str().unwrap(),
            prefix.to_str().unwrap(),
        ])
        .status()
        .expect("pdftoppm");
    assert!(status.success(), "pdftoppm failed");
}

/// Per-row count of "ink" pixels (luma < `thresh`) within the column band
/// `[x0, x1)` — a vertical projection used to locate dot rows and glyph rows.
fn ink_rows(img: &image::GrayImage, x0: u32, x1: u32, thresh: u8) -> Vec<u32> {
    let (_, h) = img.dimensions();
    (0..h)
        .map(|y| {
            (x0..x1)
                .filter(|&x| img.get_pixel(x, y)[0] < thresh)
                .count() as u32
        })
        .collect()
}

/// Collapse a projection into contiguous runs above `min_count` and at least
/// `min_h` rows tall, returned as `(top, bottom, intensity-weighted centroid)`
/// in pixels.
fn runs(profile: &[u32], min_count: u32, min_h: usize) -> Vec<(f64, f64, f64)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < profile.len() {
        if profile[i] >= min_count {
            let start = i;
            let mut num = 0.0f64;
            let mut den = 0.0f64;
            while i < profile.len() && profile[i] >= min_count {
                num += i as f64 * profile[i] as f64;
                den += profile[i] as f64;
                i += 1;
            }
            if i - start >= min_h {
                out.push((start as f64, (i - 1) as f64, num / den));
            }
        } else {
            i += 1;
        }
    }
    out
}

/// The month index must sit text *in* the dot cells, never letting a dot row cut
/// through a day number — across the whole page. This is a geometric check on the
/// rendered raster (not a golden compare): it fails both if rows are centred *on*
/// the dots (every glyph pierced) and if the row pitch drifts from the dot pitch
/// (lower glyphs pierced) — the two bugs golden images silently blessed.
#[test]
fn monthly_day_rows_clear_the_dot_grid() {
    let dev = get_device("paper-pro-move").unwrap();
    let grid = default_grid(&dev);
    let dpi = 300u32;
    let px = dpi as f64 / 72.0;
    let sp_px = grid.spacing_pt as f64 * px;

    // July 2026 is a 31-day month (the tight, worst-case column) starting midweek.
    let m = build_month(2026, 7, "sun").unwrap();
    let day_rows: Vec<DayRow> = m
        .days
        .iter()
        .map(|d| DayRow {
            day: d.day,
            weekday: d.weekday,
            week_start: d.week_start,
            event_count: 2, // badge on every row, exercising the tallest content
        })
        .collect();
    let frag = MonthlyView {
        month_name: "July",
        year: 2026,
        month_num: 7,
        spacing_pt: grid.spacing_pt,
        days: &day_rows,
    }
    .render()
    .unwrap();

    let png = tmp("monthly-align", "png");
    render_png_dpi(&frag, &png, dpi);
    let img = image::open(&png).unwrap().to_luma8();
    let (w, _h) = img.dimensions();

    // Dot rows: project the text-free band right of the badges (x 40%..99%).
    let dprof = ink_rows(&img, (w * 40) / 100, (w * 99) / 100, 160);
    let dmax = *dprof.iter().max().unwrap();
    let dots: Vec<f64> = runs(&dprof, (dmax as f64 * 0.35) as u32, 1)
        .into_iter()
        .map(|r| r.2)
        .collect();
    assert!(dots.len() >= 30, "expected a full dot grid, found {} rows", dots.len());

    // Dot pitch must match the configured spacing — a tiling pattern whose tile
    // step the rasterizer quantizes to whole pixels would read off here.
    let measured_pitch = (dots[dots.len() - 1] - dots[0]) / (dots.len() - 1) as f64;
    assert!(
        (measured_pitch - sp_px).abs() < 0.15 * px,
        "dot pitch {:.3}pt drifts from sp {:.3}pt",
        measured_pitch / px,
        sp_px / px
    );

    // Day-number glyph rows: project the number column (x 4.5%..13.5%). Skip the
    // masthead, which lives in the toolbar band above the first day row.
    let first_day_y = rmbujo::geometry::monthly_row_center(grid.spacing_pt, 0) as f64 * px;
    let nprof = ink_rows(&img, (w * 45) / 1000, (w * 135) / 1000, 150);
    let glyphs: Vec<(f64, f64, f64)> = runs(&nprof, 2, (0.4 * sp_px) as usize)
        .into_iter()
        .filter(|&(_, _, c)| c > first_day_y - sp_px)
        .collect();
    assert_eq!(
        glyphs.len(),
        31,
        "expected 31 day-number rows, detected {}",
        glyphs.len()
    );

    // No dot row may fall inside a glyph's vertical extent, and every glyph must
    // keep real clearance from the nearest dot above and below.
    let mut min_clear = f64::INFINITY;
    for (t, b, _) in &glyphs {
        let pierced = dots.iter().any(|&d| d > *t && d < *b);
        assert!(!pierced, "a dot row cuts through the day number at y={:.0}px", t);
        let above = dots.iter().cloned().filter(|&d| d <= *t).fold(f64::MIN, f64::max);
        let below = dots.iter().cloned().filter(|&d| d >= *b).fold(f64::MAX, f64::min);
        min_clear = min_clear.min((*t - above).min(below - *b));
    }
    assert!(
        min_clear / px > 0.5,
        "day numbers sit too close to the dot grid: min clearance {:.2}pt",
        min_clear / px
    );
}
