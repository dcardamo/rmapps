use inkapp_core::component::Component;
use inkapp_core::components::stepper::Stepper;
use inkapp_core::geometry::{PdfPoint, PdfRect};
use inkapp_core::ink::{RegionInk, Stroke};
use inkapp_core::manifest::{DocState, Manifest, Region};
use inkapp_core::widget::Widget;
use serde_json::json;

const RECT: PdfRect = PdfRect {
    x0: 0.0,
    y0: 0.0,
    x1: 20.0,
    y1: 20.0,
};

fn manifest_with_base(base: u64) -> Manifest {
    let mut state = DocState::default();
    state.components.insert("stepper:c".into(), json!(base));
    Manifest {
        version: 1,
        regions: vec![Region {
            name: "stepper:c".into(),
            page: 0,
            rect: RECT,
        }],
        state,
    }
}

fn one_tick() -> Vec<RegionInk> {
    vec![RegionInk {
        region: "stepper:c".into(),
        strokes: vec![Stroke {
            points: vec![PdfPoint { x: 10.0, y: 10.0 }],
            highlighter: false,
        }],
    }]
}

#[test]
fn read_uses_carried_base_not_prop() {
    let s = Stepper::new("c", 9); // current prop says 9
    assert_eq!(s.read(&one_tick(), &manifest_with_base(5)), 6); // 5 + 1, NOT 10
}

#[test]
fn decode_emits_base_relative_count() {
    let s = Stepper::new("c", 9);
    assert_eq!(s.decode(&one_tick(), &manifest_with_base(5)), vec![6u64]);
}

#[test]
fn decode_empty_without_ink() {
    let s = Stepper::new("c", 5);
    assert_eq!(s.decode(&[], &manifest_with_base(5)), Vec::<u64>::new());
}

#[test]
fn read_missing_state_treats_base_as_zero() {
    // No carried state for this key -> base 0.
    let s = Stepper::new("c", 9);
    let m = Manifest {
        version: 1,
        regions: vec![Region {
            name: "stepper:c".into(),
            page: 0,
            rect: RECT,
        }],
        ..Default::default()
    };
    assert_eq!(s.read(&one_tick(), &m), 1); // 0 + 1
}

#[test]
fn read_returns_carried_base_when_no_ink() {
    let s = Stepper::new("c", 9);
    assert_eq!(s.read(&[], &manifest_with_base(5)), 5); // idle: 5 + 0
}
