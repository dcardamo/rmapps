use inkapp_core::device::Device;
use inkapp_core::geometry::PdfPoint;
use inkapp_core::ink::Stroke;
use rm_device::Remarkable;

// A4 height in pt. The 0.5pt round-trip tolerance below is calibrated to this
// page height: the f32 quantization error in `.rm` scene coordinates is
// proportional to page height, so a much taller page would need a looser bound.
const PAGE_H: f64 = 841.89;

// Conservative width: 0.5 * page_h gives ample interior room at any aspect.
// The transform inverts at any input, so we don't need the *real* page_w;
// we just need sample points in a known region.
fn page_w_for(rm: &Remarkable, page_h: f64) -> f64 {
    let _ = rm;
    page_h * 0.5
}

#[test]
fn transform_is_invertible_across_geometries() {
    let rm = Remarkable::new();
    for (page_h, label) in &[(560.0_f64, "inkapp-default"), (841.89_f64, "a4")] {
        let page_w = page_w_for(&rm, *page_h);
        let samples: &[(f64, f64, &str)] = &[
            (page_w / 2.0, *page_h / 2.0, "center"),
            (0.0, 0.0, "bottom-left"),
            (page_w, 0.0, "bottom-right"),
            (0.0, *page_h, "top-left"),
            (page_w, *page_h, "top-right"),
            (page_w / 2.0, 0.0, "mid-bottom"),
        ];
        for (x, y, name) in samples {
            let p = PdfPoint { x: *x, y: *y };
            let d = rm.pdf_to_device(p, *page_h);
            let back = rm.device_to_pdf(d, *page_h);
            assert!(
                (back.x - p.x).abs() < 1e-6,
                "[{label}/{name}] x inverts: {} vs {}",
                p.x,
                back.x
            );
            assert!(
                (back.y - p.y).abs() < 1e-6,
                "[{label}/{name}] y inverts: {} vs {}",
                p.y,
                back.y
            );
        }
    }
}

#[test]
fn ink_round_trips_through_rm() {
    let rm = Remarkable::new();
    let original = vec![Stroke {
        points: vec![
            PdfPoint { x: 50.0, y: 700.0 },
            PdfPoint { x: 150.0, y: 700.0 },
        ],
        highlighter: true,
    }];
    let bytes = rm.write_ink(&original, PAGE_H).unwrap();
    let got = rm.read_ink(&bytes, PAGE_H).unwrap();
    assert_eq!(got.len(), 1);
    assert!(got[0].highlighter, "highlighter flag preserved");
    for (a, b) in original[0].points.iter().zip(&got[0].points) {
        assert!(
            (a.x - b.x).abs() < 0.5,
            "x within tolerance: {} vs {}",
            a.x,
            b.x
        );
        assert!(
            (a.y - b.y).abs() < 0.5,
            "y within tolerance: {} vs {}",
            a.y,
            b.y
        );
    }
}

#[test]
fn non_highlighter_ink_round_trips() {
    let rm = Remarkable::new();
    let original = vec![Stroke {
        points: vec![
            PdfPoint { x: 100.0, y: 300.0 },
            PdfPoint { x: 120.0, y: 320.0 },
        ],
        highlighter: false,
    }];
    let bytes = rm.write_ink(&original, PAGE_H).unwrap();
    let got = rm.read_ink(&bytes, PAGE_H).unwrap();
    assert_eq!(got.len(), 1);
    assert!(!got[0].highlighter, "pen (non-highlighter) flag preserved");
    for (a, b) in original[0].points.iter().zip(&got[0].points) {
        assert!((a.x - b.x).abs() < 0.5, "x within tolerance");
        assert!((a.y - b.y).abs() < 0.5, "y within tolerance");
    }
}
