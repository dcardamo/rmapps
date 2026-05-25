mod common;
use common::assert_golden;

use inkapp_core::component::RenderCx;
use inkapp_core::components::checkbox::Checkbox;
use inkapp_core::components::gesture::GestureAction;
use inkapp_core::components::highlight_text::HighlightableText;
use inkapp_core::manifest::recover_regions;
use inkapp_core::render::{compile_to_document, compile_to_document_with_sources};
use inkapp_core::runtime::REGION_PRELUDE;
use inkapp_core::Component;
use inkapp_harness::simulator::{simulate, simulate_with_sources, Gesture, Scenario};
use rm_device::Remarkable;

const TOKENS: &[&str] = &["the", "quick", "brown", "fox", "lazy", "dog"];

#[test]
fn checkbox_exerciser() {
    let cb = Checkbox::new("done");
    // Use render_at for explicit placement (a checkbox lays out absolutely);
    // Checkbox default-placement render is covered by the checkbox unit tests.
    let body = cb.render_at(0, 20.0, 40.0, 16.0, 16.0);
    let src = format!("#set page(width: 200pt, height: 200pt, margin: 0pt)\n{body}");
    let doc = compile_to_document(&src).unwrap();
    let manifest = recover_regions(&doc).unwrap().with_version(1);
    let device = Remarkable::new();

    let trace = simulate(
        &src,
        &manifest,
        &device,
        &Scenario::new().mark("done", Gesture::Tap),
    )
    .unwrap();
    assert!(
        cb.read(&trace.readback, &manifest),
        "tap marks the checkbox"
    );
    assert_golden("checkbox_marked", &trace.inspector_png);

    let empty = simulate(&src, &manifest, &device, &Scenario::new()).unwrap();
    assert!(
        !cb.read(&empty.readback, &manifest),
        "no gesture leaves it unmarked"
    );
}

#[test]
fn highlight_exerciser() {
    let w = HighlightableText::new(TOKENS);
    let mut cx = RenderCx::new(0);
    let body = w.render(&mut cx);
    let src = format!("#set page(width: 300pt, height: 120pt, margin: 10pt)\n{body}");
    let doc = compile_to_document(&src).unwrap();
    let manifest = recover_regions(&doc).unwrap().with_version(1);
    let device = Remarkable::new();

    // TOKENS[4] = "lazy", TOKENS[5] = "dog"; HighlightableText mints region names tok-<i>
    // by token index, so swiping tok-4/tok-5 highlights exactly those two words.
    let scenario = Scenario::new()
        .mark("tok-4", Gesture::Swipe)
        .mark("tok-5", Gesture::Swipe);
    let trace = simulate(&src, &manifest, &device, &scenario).unwrap();

    let mut got = w.read(&trace.readback, &manifest);
    got.sort();
    assert_eq!(got, vec!["dog".to_string(), "lazy".to_string()]);
    assert_golden("highlight_lazy_dog", &trace.inspector_png);
}

#[test]
fn gesture_action_exerciser() {
    // M = &str keeps the test message trivial; we assert via `read`, not decode.
    let g = GestureAction::with_msg("title", "How CGI changed the web", "archive");
    let mut cx = RenderCx::new(0);
    let body = g.render(&mut cx);
    // `#region` is defined in the framework prelude; register it and import it so
    // the source compiles outside the runtime (which normally does this for apps).
    let prelude_sources = &[(REGION_PRELUDE.0.to_string(), REGION_PRELUDE.1.to_string())];
    let src = format!(
        "#import \"{}\": *\n#set page(width: 420pt, height: 120pt, margin: 16pt)\n{body}",
        REGION_PRELUDE.0,
    );
    let doc = compile_to_document_with_sources(&src, prelude_sources).unwrap();
    let manifest = recover_regions(&doc).unwrap().with_version(1);
    let device = Remarkable::new();

    // Helper: run one fixture against the title region and report whether it fired.
    let fires = |fixture: &'static str| -> bool {
        let trace = simulate_with_sources(
            &src,
            prelude_sources,
            &manifest,
            &device,
            &Scenario::new().mark("title", Gesture::Fixture(fixture)),
        )
        .unwrap();
        g.read(&trace.readback, &manifest)
    };

    // Real striking gestures fire.
    assert!(
        fires("strike-through"),
        "a real strike-through fires the action"
    );
    assert!(
        fires("scribble-out"),
        "a real scribble-out fires the action"
    );

    // No ink: does not fire.
    let empty =
        simulate_with_sources(&src, prelude_sources, &manifest, &device, &Scenario::new()).unwrap();
    assert!(!g.read(&empty.readback, &manifest), "no ink does not fire");

    // Wrong tool: a highlighter swipe (spans the width but is a highlighter) does not fire.
    assert!(
        !fires("highlight-swipe"),
        "a highlighter swipe must not fire"
    );

    // Wrong shape: a checkmark is a pen stroke but narrow (aspect-fit) — does not fire.
    assert!(
        !fires("checkmark"),
        "a checkmark must not fire (pen but not a strike)"
    );
}
