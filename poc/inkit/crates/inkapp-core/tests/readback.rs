use inkapp_core::geometry::{PdfPoint, PdfRect};
use inkapp_core::ink::Stroke;
use inkapp_core::manifest::{Manifest, Region};
use inkapp_core::readback::{attribute, attribute_page, diff_new, guard_version};

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
        ..Default::default()
    }
}

#[test]
fn attributes_strokes_to_regions() {
    let m = manifest();
    let strokes = vec![stroke(5.0, 5.0), stroke(25.0, 25.0), stroke(100.0, 100.0)];
    let ink = attribute_page(&strokes, &m);
    let a = ink.iter().find(|ri| ri.region == "a").unwrap();
    let b = ink.iter().find(|ri| ri.region == "b").unwrap();
    assert_eq!(a.strokes.len(), 1);
    assert_eq!(b.strokes.len(), 1);
    // The (100,100) stroke matches no region and is dropped.
    assert_eq!(ink.iter().map(|ri| ri.strokes.len()).sum::<usize>(), 2);
}

#[test]
fn stroke_in_overlap_is_attributed_to_both_regions() {
    // Two overlapping regions; a stroke point in the overlap must land in BOTH.
    // This is the behaviour the span-level highlight widget (Task 9) relies on.
    let m = Manifest {
        version: 1,
        regions: vec![
            Region {
                name: "a".into(),
                page: 0,
                rect: PdfRect {
                    x0: 0.0,
                    y0: 0.0,
                    x1: 20.0,
                    y1: 20.0,
                },
            },
            Region {
                name: "b".into(),
                page: 0,
                rect: PdfRect {
                    x0: 5.0,
                    y0: 5.0,
                    x1: 25.0,
                    y1: 25.0,
                },
            },
        ],
        ..Default::default()
    };
    let ink = attribute_page(&[stroke(10.0, 10.0)], &m);
    assert!(ink.iter().any(|ri| ri.region == "a"), "attributed to a");
    assert!(ink.iter().any(|ri| ri.region == "b"), "attributed to b");
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

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> PdfRect {
    PdfRect { x0, y0, x1, y1 }
}

fn dot(x: f64, y: f64) -> Stroke {
    Stroke {
        points: vec![PdfPoint { x, y }],
        highlighter: false,
    }
}

#[test]
fn split_region_stitches_across_pages() {
    let m = Manifest {
        version: 0,
        regions: vec![
            Region {
                name: "p".into(),
                page: 0,
                rect: rect(0.0, 0.0, 100.0, 100.0),
            },
            Region {
                name: "p".into(),
                page: 1,
                rect: rect(0.0, 0.0, 100.0, 100.0),
            },
        ],
        ..Default::default()
    };
    let pages = vec![vec![dot(10.0, 10.0)], vec![dot(20.0, 20.0)]];
    let out = attribute(&pages, &m);
    assert_eq!(out.len(), 1, "stitched to one RegionInk");
    assert_eq!(out[0].region, "p");
    assert_eq!(out[0].strokes.len(), 2, "ink from both pages");
}

#[test]
fn no_cross_page_attribution() {
    let m = Manifest {
        version: 0,
        regions: vec![
            Region {
                name: "a".into(),
                page: 0,
                rect: rect(0.0, 0.0, 100.0, 100.0),
            },
            Region {
                name: "b".into(),
                page: 1,
                rect: rect(0.0, 0.0, 100.0, 100.0),
            },
        ],
        ..Default::default()
    };
    let pages = vec![vec![], vec![dot(50.0, 50.0)]];
    let out = attribute(&pages, &m);
    assert_eq!(out.len(), 1);
    assert_eq!(
        out[0].region, "b",
        "page-1 ink attributes to the page-1 region only"
    );
}

#[test]
fn split_region_with_ink_on_only_one_page_still_stitches() {
    let m = Manifest {
        version: 0,
        regions: vec![
            Region {
                name: "p".into(),
                page: 0,
                rect: rect(0.0, 0.0, 100.0, 100.0),
            },
            Region {
                name: "p".into(),
                page: 1,
                rect: rect(0.0, 0.0, 100.0, 100.0),
            },
        ],
        ..Default::default()
    };
    // Page 0 empty, page 1 has the only stroke.
    let pages = vec![vec![], vec![dot(20.0, 20.0)]];
    let out = attribute(&pages, &m);
    assert_eq!(
        out.len(),
        1,
        "one stitched RegionInk even though only one frame was inked"
    );
    assert_eq!(out[0].region, "p");
    assert_eq!(out[0].strokes.len(), 1);
}

#[test]
fn attribute_page_is_single_page_wrapper() {
    let m = Manifest {
        version: 0,
        regions: vec![Region {
            name: "a".into(),
            page: 0,
            rect: rect(0.0, 0.0, 100.0, 100.0),
        }],
        ..Default::default()
    };
    let out = attribute_page(&[dot(5.0, 5.0)], &m);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].region, "a");
}

fn multi_point(points: &[(f64, f64)]) -> Stroke {
    Stroke {
        points: points
            .iter()
            .map(|(x, y)| PdfPoint { x: *x, y: *y })
            .collect(),
        highlighter: false,
    }
}

#[test]
fn multi_point_stroke_attributes_if_any_point_in_region() {
    // Region "a" = [0,0]-[10,10]. A 3-point stroke starts outside, dips into
    // the region at the midpoint, then exits. Library contract: ANY point in
    // the region → attributed.
    let m = Manifest {
        version: 1,
        regions: vec![Region {
            name: "a".into(),
            page: 0,
            rect: rect(0.0, 0.0, 10.0, 10.0),
        }],
        ..Default::default()
    };
    let s = multi_point(&[(-5.0, 5.0), (5.0, 5.0), (15.0, 5.0)]);
    let ink = attribute_page(&[s], &m);
    let a = ink
        .iter()
        .find(|ri| ri.region == "a")
        .expect("attributed to a");
    assert_eq!(a.strokes.len(), 1, "single stroke attributed once");
}

#[test]
fn multi_point_stroke_in_two_regions_attributes_to_both() {
    // Two NON-overlapping regions; a single stroke has one point in each.
    // Library contract: stroke appears in both region buckets.
    let m = Manifest {
        version: 1,
        regions: vec![
            Region {
                name: "a".into(),
                page: 0,
                rect: rect(0.0, 0.0, 10.0, 10.0),
            },
            Region {
                name: "b".into(),
                page: 0,
                rect: rect(50.0, 50.0, 60.0, 60.0),
            },
        ],
        ..Default::default()
    };
    let s = multi_point(&[(5.0, 5.0), (30.0, 30.0), (55.0, 55.0)]);
    let ink = attribute_page(&[s], &m);
    let a = ink
        .iter()
        .find(|ri| ri.region == "a")
        .expect("attributed to a");
    let b = ink
        .iter()
        .find(|ri| ri.region == "b")
        .expect("attributed to b");
    assert_eq!(a.strokes.len(), 1);
    assert_eq!(b.strokes.len(), 1);
    assert_eq!(
        ink.iter().map(|ri| ri.strokes.len()).sum::<usize>(),
        2,
        "stroke attributed to both regions (one in each bucket)"
    );
}

#[test]
fn stroke_outside_all_regions_is_dropped() {
    // Contract: a stroke that matches no region is dropped from the output —
    // there is no "unattributed" bucket in the current library design. Tests
    // pin this so any future change is a deliberate behavior change.
    let m = manifest(); // version 3, regions "a"=[0,0]-[10,10], "b"=[20,20]-[30,30]
    let strokes = vec![stroke(100.0, 100.0), stroke(200.0, 200.0)];
    let ink = attribute_page(&strokes, &m);
    assert_eq!(
        ink.iter().map(|ri| ri.strokes.len()).sum::<usize>(),
        0,
        "all strokes outside every region — dropped"
    );
}
