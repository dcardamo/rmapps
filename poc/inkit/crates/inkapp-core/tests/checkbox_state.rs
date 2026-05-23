use inkapp_core::geometry::{PdfPoint, PdfRect};
use inkapp_core::ink::{RegionInk, Stroke};
use inkapp_core::manifest::{Manifest, Region};
use inkapp_core::widgets::checkbox::{CheckState, Checkbox};

fn manifest_with(rect: PdfRect) -> Manifest {
    Manifest {
        version: 1,
        regions: vec![Region {
            name: "done".into(),
            page: 0,
            rect,
        }],
    }
}

fn ink(points: Vec<PdfPoint>) -> Vec<RegionInk> {
    vec![RegionInk {
        region: "done".into(),
        strokes: vec![Stroke {
            points,
            highlighter: false,
        }],
    }]
}

const RECT: PdfRect = PdfRect {
    x0: 0.0,
    y0: 0.0,
    x1: 20.0,
    y1: 20.0,
};

#[test]
fn empty_when_no_ink() {
    let cb = Checkbox::new("done");
    assert_eq!(cb.read_state(&[], &manifest_with(RECT)), CheckState::Empty);
}

#[test]
fn marked_for_short_check() {
    // A tick: down-right then up-right. Total length ~ 1.5x the ~28pt diagonal.
    let cb = Checkbox::new("done");
    let pts = vec![
        PdfPoint { x: 4.0, y: 12.0 },
        PdfPoint { x: 9.0, y: 5.0 },
        PdfPoint { x: 16.0, y: 16.0 },
    ];
    assert_eq!(
        cb.read_state(&ink(pts), &manifest_with(RECT)),
        CheckState::Marked
    );
}

#[test]
fn scribbled_out_for_dense_zigzag() {
    // A back-and-forth scribble: many segments, total length >> diagonal.
    let cb = Checkbox::new("done");
    let mut pts = Vec::new();
    for i in 0..12 {
        let x = 2.0 + (i as f64) * 1.4;
        let y = if i % 2 == 0 { 3.0 } else { 17.0 };
        pts.push(PdfPoint { x, y });
    }
    assert_eq!(
        cb.read_state(&ink(pts), &manifest_with(RECT)),
        CheckState::ScribbledOut
    );
}

#[test]
fn read_bool_tracks_state() {
    let cb = Checkbox::new("done");
    assert!(!{
        use inkapp_core::widget::Widget;
        cb.read(&[], &manifest_with(RECT))
    });
}
