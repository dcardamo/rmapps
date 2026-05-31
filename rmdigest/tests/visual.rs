//! Visual regression tests for the Digest PDF builder.
//!
//! These tests render a deterministic digest (fixed marks — 3 highlights with
//! different PenColors + 1 note), rasterise each page with `pdftoppm`, and
//! compare against golden PNG files in `tests/fixtures/goldens/`.
//!
//! To regenerate goldens (required after any visual change):
//!   nix develop -c env RMDIGEST_UPDATE_GOLDENS=1 cargo test digest visual
use rmdigest::digest_doc::{build_digest, DigestMeta};
use rmdigest::render::compile;
use rmfiles::{Pen, PenColor, Point, Stroke};
use std::path::PathBuf;
use std::process::Command;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a small valid PNG from a synthetic pen stroke (reuses the ink renderer).
fn note_png() -> Vec<u8> {
    let strokes = [Stroke {
        tool: Pen::Ballpoint1,
        color: PenColor::Black,
        points: vec![
            Point {
                x: 10.0,
                y: 10.0,
                speed: None,
                direction: None,
                width: Some(3.0),
                pressure: None,
            },
            Point {
                x: 80.0,
                y: 40.0,
                speed: None,
                direction: None,
                width: Some(3.0),
                pressure: None,
            },
            Point {
                x: 40.0,
                y: 70.0,
                speed: None,
                direction: None,
                width: Some(3.0),
                pressure: None,
            },
        ],
    }];
    let refs: Vec<&Stroke> = strokes.iter().collect();
    rmdigest::ink::render_strokes(
        &refs,
        &rmdigest::ink::InkOpts {
            background: rmdigest::ink::Background::White,
            scale: 2.0,
            margin_px: 8,
        },
    )
    .expect("render note PNG")
}

/// A fixed set of marks for deterministic golden renders.
fn fixed_marks() -> Vec<rmdigest::extract::Mark> {
    let png = note_png();
    vec![
        rmdigest::extract::Mark::Highlight {
            page: 2,
            text: "The quick brown fox jumps over the lazy dog.".into(),
            color: PenColor::Yellow,
        },
        rmdigest::extract::Mark::Highlight {
            page: 5,
            text: "We shall not cease from exploration, and the end of all our exploring \
                   will be to arrive where we started and know the place for the first time."
                .into(),
            color: PenColor::Green,
        },
        rmdigest::extract::Mark::Highlight {
            page: 8,
            text: "In the beginning was the Word.".into(),
            color: PenColor::Pink,
        },
        rmdigest::extract::Mark::Note { page: 11, png },
    ]
}

fn fixed_meta() -> DigestMeta {
    DigestMeta {
        title: "The Golden Digest".into(),
        author: "Visual Test Author".into(),
        n_highlights: 3,
        n_notes: 1,
        date_range: "May 2026".into(),
    }
}

// ---------------------------------------------------------------------------
// Golden helper
// ---------------------------------------------------------------------------

/// Compare `actual_png_bytes` against the named golden PNG, or write it when
/// `RMDIGEST_UPDATE_GOLDENS` is set.
fn assert_golden(name: &str, actual_png_bytes: &[u8]) {
    let golden_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/goldens")
        .join(format!("{name}.png"));

    if std::env::var("RMDIGEST_UPDATE_GOLDENS").is_ok() {
        std::fs::create_dir_all(golden_path.parent().unwrap()).expect("create golden dir");
        std::fs::write(&golden_path, actual_png_bytes).expect("write golden");
        println!("Updated golden: {}", golden_path.display());
        return;
    }

    let golden_bytes = std::fs::read(&golden_path).unwrap_or_else(|_| {
        panic!(
            "golden missing: {}. Run with RMDIGEST_UPDATE_GOLDENS=1 to create.",
            golden_path.display()
        )
    });

    let actual = image::load_from_memory(actual_png_bytes)
        .expect("decode actual PNG")
        .to_rgba8();
    let expected = image::load_from_memory(&golden_bytes)
        .expect("decode golden PNG")
        .to_rgba8();

    assert_eq!(
        (actual.width(), actual.height()),
        (expected.width(), expected.height()),
        "golden '{name}' dimensions mismatch: actual {}×{} vs golden {}×{}",
        actual.width(),
        actual.height(),
        expected.width(),
        expected.height()
    );

    // Mean per-channel absolute difference < 2.0/255 (allows for minor
    // platform rendering differences while catching real layout changes).
    let total_samples = (actual.width() * actual.height() * 4) as f64;
    let sum_diff: f64 = actual
        .pixels()
        .zip(expected.pixels())
        .map(|(a, e)| {
            a.0.iter()
                .zip(e.0.iter())
                .map(|(&ac, &ec)| (ac as i32 - ec as i32).unsigned_abs() as f64)
                .sum::<f64>()
        })
        .sum();
    let mean_diff = sum_diff / total_samples;
    assert!(
        mean_diff < 2.0,
        "golden '{name}' mean diff {mean_diff:.4} >= 2.0/255 threshold"
    );
}

/// Rasterise a page from a PDF file to PNG bytes using `pdftoppm`.
/// Returns `None` if pdftoppm is not available (skip the visual test gracefully).
fn rasterise_page(pdf_bytes: &[u8], page_1based: u32, dpi: u32) -> Option<Vec<u8>> {
    // Write PDF to a temp file.
    let tmp_pdf = tempfile::NamedTempFile::with_suffix(".pdf").ok()?;
    std::fs::write(tmp_pdf.path(), pdf_bytes).ok()?;

    let tmp_dir = tempfile::TempDir::new().ok()?;
    let prefix = tmp_dir.path().join("page");

    let status = Command::new("pdftoppm")
        .args([
            "-r",
            &dpi.to_string(),
            "-png",
            "-f",
            &page_1based.to_string(),
            "-l",
            &page_1based.to_string(),
            tmp_pdf.path().to_str()?,
            prefix.to_str()?,
        ])
        .status()
        .ok()?;

    if !status.success() {
        return None;
    }

    // pdftoppm writes <prefix>-<N>.png (zero-padded to match total page count).
    let mut pngs: Vec<_> = std::fs::read_dir(tmp_dir.path())
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".png"))
        .map(|e| e.path())
        .collect();
    pngs.sort();

    let path = pngs.into_iter().next()?;
    std::fs::read(path).ok()
}

// ---------------------------------------------------------------------------
// Visual test
// ---------------------------------------------------------------------------

#[test]
fn digest_visual_golden() {
    let meta = fixed_meta();
    let marks = fixed_marks();

    let (src, assets) = build_digest(&meta, &marks, &rmdigest::device::MOVE);
    let pdf = compile(&src, &assets).expect("compile digest PDF");

    let page_count = lopdf::Document::load_mem(&pdf)
        .expect("valid PDF")
        .get_pages()
        .len();

    // Cover + at least one content page.
    assert!(page_count >= 2, "expected ≥2 pages, got {page_count}");

    // Rasterise pages and compare to goldens. Requires pdftoppm in PATH.
    // If pdftoppm is not available, the test passes (we can't gate CI on it).
    for page in 1..=page_count {
        let Some(png_bytes) = rasterise_page(&pdf, page as u32, 150) else {
            eprintln!("pdftoppm not available, skipping visual golden for page {page}");
            return;
        };
        assert_golden(&format!("digest-p{page}"), &png_bytes);
    }
}
