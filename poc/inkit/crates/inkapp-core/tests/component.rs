use inkapp_core::component::Component;
use inkapp_core::geometry::{PdfPoint, PdfRect};
use inkapp_core::ink::{RegionInk, Stroke};
use inkapp_core::manifest::{Manifest, Region};
use inkapp_core::widget::RenderCx;

/// A minimal component: renders nothing meaningful, decodes any ink on region
/// "x" into the unit message.
struct Marker;
impl Component for Marker {
    type Msg = &'static str;
    fn render(&self, _cx: &mut RenderCx) -> String {
        String::new()
    }
    fn decode(&self, ink: &[RegionInk], _m: &Manifest) -> Vec<&'static str> {
        if ink
            .iter()
            .any(|ri| ri.region == "x" && !ri.strokes.is_empty())
        {
            vec!["marked"]
        } else {
            vec![]
        }
    }
}

#[test]
fn decode_emits_on_ink() {
    let m = Manifest {
        version: 1,
        regions: vec![Region {
            name: "x".into(),
            page: 0,
            rect: PdfRect {
                x0: 0.0,
                y0: 0.0,
                x1: 10.0,
                y1: 10.0,
            },
        }],
        ..Default::default()
    };
    let ink = vec![RegionInk {
        region: "x".into(),
        strokes: vec![Stroke {
            points: vec![PdfPoint { x: 5.0, y: 5.0 }],
            highlighter: false,
        }],
    }];
    assert_eq!(Marker.decode(&ink, &m), vec!["marked"]);
    assert!(Marker.decode(&[], &m).is_empty());
    let other = vec![RegionInk {
        region: "y".into(),
        strokes: vec![Stroke {
            points: vec![PdfPoint { x: 5.0, y: 5.0 }],
            highlighter: false,
        }],
    }];
    assert!(
        Marker.decode(&other, &m).is_empty(),
        "ink in another region is ignored"
    );
}
