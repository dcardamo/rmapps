use inkapp_core::device::Device;
use inkapp_core::geometry::PdfPoint;
use inkapp_core::ink::Stroke;
use inkapp_remarkable::Remarkable;

const PAGE_H: f64 = 841.89; // A4 height in pt

#[test]
fn transform_is_invertible() {
    let rm = Remarkable::new();
    let p = PdfPoint { x: 123.0, y: 456.0 };
    let d = rm.pdf_to_device(p, PAGE_H);
    let back = rm.device_to_pdf(d, PAGE_H);
    assert!((back.x - p.x).abs() < 1e-6, "x inverts");
    assert!((back.y - p.y).abs() < 1e-6, "y inverts");
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
