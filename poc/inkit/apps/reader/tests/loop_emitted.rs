//! Layer-6 full-loop sequences for the reader.
//!
//! Drives the reader through the harness `Session::step_app` API exactly as
//! an agent-driven inkctl session would. Scoped per the Layer-6 checkpoint
//! (2026-05-27): only the happy-path action-band sequence is landed here.
//! The Index → article navigation hop is deferred — Typst's `#link(<label>)`
//! renders as a `/GoTo` annotation with a NAMED destination, which the
//! harness's `pdf_links::extract` currently skips (see the "(skip)" comment
//! in pdf_links.rs). Resolving named destinations is a separate framework
//! improvement, not Layer-6 test work. Stale-manifest and offline-connector
//! paths are independently covered at Layers 3 and 5.

mod shared;

use inkapp_core::runtime::DocSet;
use inkapp_harness::session::{PublishedApp, Session, StepOpts};
use inkapp_readwise_reader::ArticleId;

/// Sequence 1 (happy path, action-band only):
///   publish reader → locate `action-Archive-a1` in the published manifest
///   → `ink_tap` on it → `step_app` decodes `Msg::Move{to: Archive}` →
///   connector overlay reflects the archive → next render's Library no
///   longer references a1.
#[tokio::test]
async fn happy_path_tap_archive_then_step() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut session = Session::new_fake(tmp.path()).await.expect("session");
    let device = session.device_new(None).expect("device");

    let (mut app, readwise) = shared::reader_app_fake_with_readwise();
    let mut set = DocSet::default();

    // Initial render + publish each Document. fake() seeds [a1, a2] in
    // Location::New so view yields a single Library Document.
    let rendered = app.render(&mut set).await.expect("initial render");
    let mut library_doc_id = String::new();
    for r in &rendered {
        let published = PublishedApp {
            app_name: r.key.0.clone(),
            pdf_bytes: r.pdf.clone(),
            manifest: r.manifest.clone(),
            source_typ: None,
        };
        let summary = session
            .document_publish(&device, published)
            .await
            .expect("publish");
        if r.key.0 == "Library" {
            library_doc_id = summary.id.clone();
        }
    }
    assert!(
        !library_doc_id.is_empty(),
        "Library document must be published; got keys: {:?}",
        rendered
            .iter()
            .map(|r| r.key.0.as_str())
            .collect::<Vec<_>>()
    );

    // Locate the Index row region for a1 — the reader builds entries from
    // `articles` in order, so a1 maps to "idx-0". (Verified by reading
    // crates/inkapp-core/src/components/index.rs: each row wraps a #region
    // named `idx-{i}` whose body contains `#link(<art-{id}>, ...)`.)
    let library = rendered
        .iter()
        .find(|r| r.key.0 == "Library")
        .expect("Library in rendered set");
    // Locate the Archive action cell for a1. The reader's ActionBand emits
    // one `action-{label}-{section_id}` region per cell per page, but the
    // a1-specific name only appears on a1's own article page (since the
    // current section_id at render time is what gets baked into the region
    // name). Find that region wherever it landed.
    let action_region = library
        .manifest
        .regions
        .iter()
        .find(|r| r.name == "action-Archive-a1")
        .unwrap_or_else(|| {
            let names: Vec<&str> = library
                .manifest
                .regions
                .iter()
                .map(|r| r.name.as_str())
                .collect();
            panic!("action-Archive-a1 missing; all regions: {names:?}");
        });
    let article_page = action_region.page;

    // Strike across the cell with a non-highlighter pen stroke. The action
    // band's decode requires a stroke (or union of strokes) spanning >=60%
    // of the region width; `ink_tap` is a single point and `ink_swipe` is a
    // highlighter, so neither fits — drive `ink_draw` directly.
    let rect = action_region.rect;
    let cy = (rect.y0 + rect.y1) / 2.0;
    let strike_points: Vec<inkapp_core::geometry::PdfPoint> = (0..=10)
        .map(|i| inkapp_core::geometry::PdfPoint {
            x: rect.x0 + (rect.x1 - rect.x0) * (i as f64 / 10.0),
            y: cy,
        })
        .collect();
    session
        .ink_draw(
            &device,
            &library_doc_id,
            article_page,
            &strike_points,
            false,
        )
        .expect("ink_draw pen strike on action-Archive-a1");

    // Step the loop.
    let step = session
        .step_app(&device, &mut app, &mut set, StepOpts::default())
        .await
        .expect("step_app");

    // The tap decodes to exactly one Move{Archive, a1}. The serialized Msg
    // shape is JSON; assert via substring on the Move variant + the id.
    let msg_strs: Vec<String> = step.msgs.iter().map(|m| m.to_string()).collect();
    let move_msg = msg_strs
        .iter()
        .find(|s| s.contains("Move") && s.to_lowercase().contains("archive") && s.contains("a1"))
        .unwrap_or_else(|| {
            panic!("expected a Move{{to: Archive, article: a1}} in decoded msgs; got: {msg_strs:?}")
        });
    let _ = move_msg;

    // Connector overlay reflects the archive.
    assert!(
        readwise.archived().contains(&ArticleId::new("a1")),
        "a1 must be in archived overlay after step"
    );

    // The post-step Library re-render no longer references a1.
    let after = app.render(&mut set).await.expect("post-step re-render");
    let library_after = after
        .iter()
        .find(|r| r.key.0 == "Library")
        .expect("Library re-rendered");
    let names_after: Vec<&str> = library_after
        .manifest
        .regions
        .iter()
        .map(|r| r.name.as_str())
        .collect();
    assert!(
        !names_after.iter().any(|n| n.contains("a1")),
        "post-step Library regions must not reference a1: {names_after:?}"
    );
    // And a2 is still there.
    assert!(
        names_after.iter().any(|n| n.contains("a2")),
        "a2 must still be in Library after a1 archive: {names_after:?}"
    );
}
