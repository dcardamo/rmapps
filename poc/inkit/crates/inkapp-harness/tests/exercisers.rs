use inkapp_core::manifest::recover_regions;
use inkapp_core::render::compile_to_document;
use inkapp_core::widget::{RenderCx, Widget};
use inkapp_core::widgets::checkbox::Checkbox;
use inkapp_core::widgets::highlight_text::HighlightableText;
use inkapp_harness::simulator::{simulate, Gesture, Scenario};
use inkapp_remarkable::Remarkable;

const TOKENS: &[&str] = &["the", "quick", "brown", "fox", "lazy", "dog"];

/// Compare `png` to the committed golden at `tests/golden/<name>.png`.
/// On first run (golden absent), write it and fail with a clear message so the
/// developer reviews and commits it.
fn assert_golden(name: &str, png: &[u8]) {
    let path = format!("{}/tests/golden/{name}.png", env!("CARGO_MANIFEST_DIR"));
    match std::fs::read(&path) {
        Ok(expected) => assert_eq!(
            png,
            expected.as_slice(),
            "inspector image differs from golden {name}"
        ),
        Err(_) => {
            std::fs::create_dir_all(format!("{}/tests/golden", env!("CARGO_MANIFEST_DIR")))
                .unwrap();
            std::fs::write(&path, png).unwrap();
            panic!("golden {name} did not exist; wrote it — review and re-run");
        }
    }
}

#[test]
fn checkbox_exerciser() {
    let cb = Checkbox::new("done");
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

    let scenario = Scenario::new()
        .mark("tok-4", Gesture::Swipe)
        .mark("tok-5", Gesture::Swipe);
    let trace = simulate(&src, &manifest, &device, &scenario).unwrap();

    let mut got = w.read(&trace.readback, &manifest);
    got.sort();
    assert_eq!(got, vec!["dog".to_string(), "lazy".to_string()]);
    assert_golden("highlight_lazy_dog", &trace.inspector_png);
}
