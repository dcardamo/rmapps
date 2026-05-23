use inkapp_core::geometry::{PdfPoint, PdfRect};
use inkapp_core::ink::Stroke;
use inkapp_core::manifest::{Manifest, Region};
use inkapp_core::readback::{attribute, diff_new, guard_version};

fn stroke(x: f64, y: f64) -> Stroke {
    Stroke {
        points: vec![PdfPoint { x, y }],
        highlighter: false,
    }
}

fn manifest() -> Manifest {
    Manifest {
        version: 3,
        regions: vec![
            Region {
                name: "a".into(),
                page: 0,
                rect: PdfRect {
                    x0: 0.0,
                    y0: 0.0,
                    x1: 10.0,
                    y1: 10.0,
                },
            },
            Region {
                name: "b".into(),
                page: 0,
                rect: PdfRect {
                    x0: 20.0,
                    y0: 20.0,
                    x1: 30.0,
                    y1: 30.0,
                },
            },
        ],
    }
}

#[test]
fn attributes_strokes_to_regions() {
    let m = manifest();
    let strokes = vec![stroke(5.0, 5.0), stroke(25.0, 25.0), stroke(100.0, 100.0)];
    let ink = attribute(&strokes, &m);
    let a = ink.iter().find(|ri| ri.region == "a").unwrap();
    let b = ink.iter().find(|ri| ri.region == "b").unwrap();
    assert_eq!(a.strokes.len(), 1);
    assert_eq!(b.strokes.len(), 1);
    // The (100,100) stroke matches no region and is dropped.
    assert_eq!(ink.iter().map(|ri| ri.strokes.len()).sum::<usize>(), 2);
}

#[test]
fn diff_returns_only_new_strokes() {
    let prev = vec![stroke(5.0, 5.0)];
    let current = vec![stroke(5.0, 5.0), stroke(25.0, 25.0)];
    let new = diff_new(&prev, &current);
    assert_eq!(new, vec![stroke(25.0, 25.0)]);
}

#[test]
fn stale_version_is_rejected() {
    let m = manifest(); // version 3
    assert!(guard_version(3, &m).is_ok());
    assert!(guard_version(2, &m).is_err());
}
