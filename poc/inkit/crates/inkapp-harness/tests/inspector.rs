use inkapp_core::geometry::{PdfPoint, PdfRect};
use inkapp_core::ink::Stroke;
use inkapp_core::manifest::{Manifest, Region};
use inkapp_core::render::compile_to_document;
use inkapp_harness::inspector::inspect;

#[test]
fn produces_a_png_of_the_page() {
    let doc =
        compile_to_document("#set page(width: 100pt, height: 100pt, margin: 0pt)\nhi").unwrap();
    let manifest = Manifest {
        version: 1,
        regions: vec![Region {
            name: "a".into(),
            page: 0,
            rect: PdfRect {
                x0: 10.0,
                y0: 10.0,
                x1: 40.0,
                y1: 40.0,
            },
        }],
    };
    let ink = vec![Stroke {
        points: vec![PdfPoint { x: 15.0, y: 15.0 }, PdfPoint { x: 35.0, y: 35.0 }],
        highlighter: true,
    }];

    let png = inspect(&doc, &manifest, &ink).expect("inspect");
    // At 2x scale a 100pt page yields 200×200 px.
    let img = image::load_from_memory(&png).expect("decode png");
    assert_eq!(img.width(), 200);
    assert_eq!(img.height(), 200);
}

#[test]
fn overlays_paint_non_background_pixels() {
    use image::GenericImageView;
    let doc = compile_to_document("#set page(width: 100pt, height: 100pt, margin: 0pt)\n").unwrap();
    let manifest = Manifest {
        version: 1,
        regions: vec![Region {
            name: "a".into(),
            page: 0,
            rect: PdfRect {
                x0: 10.0,
                y0: 10.0,
                x1: 40.0,
                y1: 40.0,
            },
        }],
    };
    let png = inspect(&doc, &manifest, &[]).unwrap();
    let img = image::load_from_memory(&png).unwrap();

    // The region rect top edge (PDF y=40) maps to image y = (100-40)*2 = 120.
    // Sample mid-way along that edge at x=25pt -> 50px.
    let px = img.get_pixel(50, 120);
    assert!(
        px[2] > 5,
        "region outline (blue) should be visible at (50,120): {px:?}"
    );
}

#[test]
fn ink_strokes_paint_non_background_pixels() {
    use image::GenericImageView;
    let doc = compile_to_document("#set page(width: 100pt, height: 100pt, margin: 0pt)\n").unwrap();
    let manifest = Manifest {
        version: 1,
        regions: vec![],
    };
    // A horizontal pen stroke at PDF y=50 from x=20 to x=80.
    let ink = vec![Stroke {
        points: vec![PdfPoint { x: 20.0, y: 50.0 }, PdfPoint { x: 80.0, y: 50.0 }],
        highlighter: false,
    }];
    let png = inspect(&doc, &manifest, &ink).unwrap();
    let img = image::load_from_memory(&png).unwrap();
    // Midpoint x=50pt -> 100px; y=50pt -> (100-50)*2 = 100px. Pen is red.
    let px = img.get_pixel(100, 100);
    assert!(
        px[0] > 150 && px[2] < 80,
        "pen stroke (red) visible at (100,100): {px:?}"
    );
}
