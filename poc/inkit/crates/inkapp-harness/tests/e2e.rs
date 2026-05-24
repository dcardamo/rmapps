mod common;
use common::assert_golden;

use inkapp_core::components::checkbox::{CheckState, Checkbox};
use inkapp_core::components::highlight_text::HighlightableText;
use inkapp_core::manifest::recover_regions;
use inkapp_core::render::compile_to_document;
use inkapp_core::widget::{RenderCx, Widget};
use inkapp_harness::simulator::{simulate, Gesture, Scenario};
use inkapp_remarkable::Remarkable;

const TOKENS: &[&str] = &["the", "quick", "brown", "fox", "lazy", "dog"];

#[test]
fn checkmark_marks_checkbox() {
    let cb = Checkbox::new("done");
    let body = cb.render_at(0, 20.0, 40.0, 40.0, 40.0); // 40pt box: exercises fixture scaling into a realistic checkbox
    let src = format!("#set page(width: 200pt, height: 200pt, margin: 0pt)\n{body}");
    let doc = compile_to_document(&src).unwrap();
    let manifest = recover_regions(&doc).unwrap().with_version(1);
    let device = Remarkable::new();

    let trace = simulate(
        &src,
        &manifest,
        &device,
        &Scenario::new().mark("done", Gesture::Fixture("checkmark")),
    )
    .unwrap();
    assert_eq!(
        cb.read_state(&trace.readback, &manifest),
        CheckState::Marked
    );
    assert_golden("e2e_checkmark", &trace.inspector_png);
}

#[test]
fn scribble_out_reads_scribbled() {
    let cb = Checkbox::new("done");
    let body = cb.render_at(0, 20.0, 40.0, 40.0, 40.0); // 40pt box: exercises fixture scaling into a realistic checkbox
    let src = format!("#set page(width: 200pt, height: 200pt, margin: 0pt)\n{body}");
    let doc = compile_to_document(&src).unwrap();
    let manifest = recover_regions(&doc).unwrap().with_version(1);
    let device = Remarkable::new();

    let trace = simulate(
        &src,
        &manifest,
        &device,
        &Scenario::new().mark("done", Gesture::Fixture("scribble-out")),
    )
    .unwrap();
    assert_eq!(
        cb.read_state(&trace.readback, &manifest),
        CheckState::ScribbledOut
    );
    assert_golden("e2e_scribble_out", &trace.inspector_png);
}

#[test]
fn highlight_swipe_selects_lazy_dog() {
    let w = HighlightableText::new(TOKENS);
    let mut cx = RenderCx::new(0);
    let body = w.render(&mut cx);
    let src = format!("#set page(width: 300pt, height: 120pt, margin: 10pt)\n{body}");
    let doc = compile_to_document(&src).unwrap();
    let manifest = recover_regions(&doc).unwrap().with_version(1);
    let device = Remarkable::new();

    let scenario = Scenario::new()
        .mark("tok-4", Gesture::Fixture("highlight-swipe"))
        .mark("tok-5", Gesture::Fixture("highlight-swipe"));
    let trace = simulate(&src, &manifest, &device, &scenario).unwrap();

    let mut got = w.read(&trace.readback, &manifest);
    got.sort();
    assert_eq!(got, vec!["dog".to_string(), "lazy".to_string()]);
    assert_golden("e2e_highlight_lazy_dog", &trace.inspector_png);
}
