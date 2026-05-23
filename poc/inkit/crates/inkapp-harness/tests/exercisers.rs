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
///
/// Byte equality is only meaningful when the rendering stack (typst-render,
/// tiny-skia, image/zlib) is pinned to the same versions — which the Nix devshell
/// enforces. Running outside `nix develop` on a different platform may produce a
/// false mismatch; regenerate the golden inside the devshell.
fn assert_golden(name: &str, png: &[u8]) {
    let path = format!("{}/tests/golden/{name}.png", env!("CARGO_MANIFEST_DIR"));
    match std::fs::read(&path) {
        Ok(expected) => assert_eq!(
            png,
            expected.as_slice(),
            "inspector image differs from golden {name}"
        ),
        // Only "file not found" triggers bootstrap; any other I/O error (e.g.
        // unreadable file) is a real failure and must not silently rewrite the
        // golden — that would compare against freshly-written bytes and pass.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir_all(format!("{}/tests/golden", env!("CARGO_MANIFEST_DIR")))
                .unwrap();
            std::fs::write(&path, png).unwrap();
            // Must panic: if we returned, the (now-equal) file written above would
            // make a same-run comparison pass with no human review.
            panic!("golden {name} did not exist; wrote it — review and re-run");
        }
        Err(e) => panic!("could not read golden {name}: {e}"),
    }
}

#[test]
fn checkbox_exerciser() {
    let cb = Checkbox::new("done");
    // Use render_at for explicit placement (a checkbox lays out absolutely); the
    // Widget::render default-placement path is covered by the checkbox unit tests.
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

    // TOKENS[4] = "lazy", TOKENS[5] = "dog"; the widget mints region names tok-<i>
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
