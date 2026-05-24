use inkapp_core::component::Component;
use inkapp_core::geometry::{PdfPoint, PdfRect};
use inkapp_core::ink::{RegionInk, Stroke};
use inkapp_core::manifest::{Manifest, Region};
use inkapp_core::components::checkbox::Checkbox;

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
fn component_render_region_recovers() {
    use inkapp_core::manifest::recover_regions;
    use inkapp_core::render::compile_to_document;
    use inkapp_core::widget::RenderCx;
    let cb = Checkbox::with_msg("done", Msg::Archived(1)).label("Archive");
    let mut cx = RenderCx::new(0);
    let body = cb.render(&mut cx);
    let src = format!("#set page(width: 200pt, height: 80pt, margin: 10pt)\n{body}");
    let doc = compile_to_document(&src).unwrap();
    let m = recover_regions(&doc).unwrap();
    assert!(
        m.regions.iter().any(|r| r.name == "done"),
        "inline checkbox region recovers"
    );
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
