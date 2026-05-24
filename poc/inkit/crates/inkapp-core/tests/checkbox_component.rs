use inkapp_core::component::Component;
use inkapp_core::components::checkbox::Checkbox;
use inkapp_core::geometry::{PdfPoint, PdfRect};
use inkapp_core::ink::{RegionInk, Stroke};
use inkapp_core::manifest::{Manifest, Region};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Msg {
    Archived(u32),
}

fn manifest() -> Manifest {
    Manifest {
        version: 1,
        regions: vec![Region {
            name: "done".into(),
            page: 0,
            rect: PdfRect {
                x0: 0.0,
                y0: 0.0,
                x1: 20.0,
                y1: 20.0,
            },
        }],
    }
}

fn mark() -> Vec<RegionInk> {
    vec![RegionInk {
        region: "done".into(),
        strokes: vec![Stroke {
            points: vec![PdfPoint { x: 10.0, y: 10.0 }],
            highlighter: false,
        }],
    }]
}

#[test]
fn decode_emits_on_check_when_marked() {
    let cb = Checkbox::with_msg("done", Msg::Archived(42));
    assert_eq!(cb.decode(&mark(), &manifest()), vec![Msg::Archived(42)]);
}

#[test]
fn decode_empty_when_no_ink() {
    let cb = Checkbox::with_msg("done", Msg::Archived(42));
    assert!(cb.decode(&[], &manifest()).is_empty());
}

#[test]
fn authored_checkbox_round_trips_through_driver() {
    use inkapp_core::document::Document;
    use inkapp_core::flow;
    use inkapp_core::geometry::PdfPoint;
    use inkapp_core::ink::{RegionInk, Stroke};
    use inkapp_core::manifest::recover_regions;
    use inkapp_core::runtime::compile_document;

    let doc: Document<Msg> = Document::keyed(
        "k",
        flow![Checkbox::with_msg("done", Msg::Archived(7)).label("Archive")],
    );
    let compiled = compile_document(&doc).unwrap();
    let m = recover_regions(&compiled).unwrap();
    let region = m
        .regions
        .iter()
        .find(|r| r.name == "done")
        .expect("authored region recovers");

    // The region wraps the 14x14 affordance only.
    assert!(
        (region.rect.x1 - region.rect.x0 - 14.0).abs() < 0.01,
        "width ~14pt"
    );
    assert!(
        (region.rect.y1 - region.rect.y0 - 14.0).abs() < 0.01,
        "height ~14pt"
    );

    // Ink at the region centre decodes to the carried message.
    let cx_mid = (region.rect.x0 + region.rect.x1) / 2.0;
    let cy_mid = (region.rect.y0 + region.rect.y1) / 2.0;
    let ink = vec![RegionInk {
        region: "done".into(),
        strokes: vec![Stroke {
            points: vec![PdfPoint {
                x: cx_mid,
                y: cy_mid,
            }],
            highlighter: false,
        }],
    }];
    let cb = Checkbox::with_msg("done", Msg::Archived(7)).label("Archive");
    assert_eq!(cb.decode(&ink, &m), vec![Msg::Archived(7)]);
    assert!(cb.decode(&[], &m).is_empty());
}

#[test]
fn decode_fires_on_scribble_out_too() {
    // A dense scribble reads as ScribbledOut (non-Empty), so decode still emits
    // the message. (Treating a scribble as an explicit un-check is future work.)
    let cb = Checkbox::with_msg("done", Msg::Archived(42));
    let mut pts = Vec::new();
    for i in 0..12 {
        let x = 2.0 + (i as f64) * 1.4;
        let y = if i % 2 == 0 { 3.0 } else { 17.0 };
        pts.push(PdfPoint { x, y });
    }
    let ink = vec![RegionInk {
        region: "done".into(),
        strokes: vec![Stroke {
            points: pts,
            highlighter: false,
        }],
    }];
    assert_eq!(cb.decode(&ink, &manifest()), vec![Msg::Archived(42)]);
}
