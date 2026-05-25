use std::collections::HashMap;

mod common;

use common::{device_ink, fixture, region_rect};

use inkapp_core::document::DocKey;
use inkapp_core::geometry::PdfRect;
use inkapp_core::ink::Stroke;
use inkapp_core::manifest::Manifest;
use inkapp_core::reconcile::DocOp;
use inkapp_core::runtime::{app, document_source, DocSet};
use inkapp_remarkable::Remarkable;
use reading_queue::{update, view, App, Connectors, Msg};

/// Union rect of the named regions (for placing a swipe across several tokens).
/// App-loop-specific (multi-token swipe); the single-region/fixture/device-ink
/// helpers it shares with the agenda e2e live in `common`.
fn union_rect(m: &Manifest, names: &[&str]) -> PdfRect {
    let mut it = m
        .regions
        .iter()
        .filter(|r| names.contains(&r.name.as_str()));
    let first = it
        .next()
        .unwrap_or_else(|| panic!("union_rect: none of {names:?} found in manifest"))
        .rect;
    let mut u = first;
    for r in it {
        u.x0 = u.x0.min(r.rect.x0);
        u.y0 = u.y0.min(r.rect.y0);
        u.x1 = u.x1.max(r.rect.x1);
        u.y1 = u.y1.max(r.rect.y1);
    }
    u
}

#[tokio::test]
async fn reading_queue_loop_highlight_archive_preserve() {
    let device = Remarkable::new();
    let mut application = app(App)
        .connector(Connectors::fake())
        .update(update)
        .view(view)
        .key(common::test_key())
        .build();
    let mut set = DocSet::default();

    // Cycle 0: render the queue. fake() cassette bodies tokenise to:
    //   a1 = ["the","slow","web","rewards","patience"] (tok-0..tok-4)
    //   a2 = ["ink","survives","the","round","trip"]    (tok-0..tok-4)
    // The highlight-swipe below lands on union(tok-0, tok-1); at least one of
    // those token regions must exist for the Highlighted message to fire, so if
    // fake()'s a1 body is shortened this test must be updated in step.
    let rendered = application.render(&mut set).await.unwrap();
    assert_eq!(
        rendered.len(),
        2,
        "fake() cassette has exactly two articles: a1, a2"
    );

    // Article X = a1: highlight its first two tokens. Article Y = a2: archive.
    let x = DocKey::new("a1");
    let y = DocKey::new("a2");
    let mx = set.manifest(&x).unwrap().clone();
    let my = set.manifest(&y).unwrap().clone();
    let ph_x = set.page_h(&x).unwrap();
    let ph_y = set.page_h(&y).unwrap();

    let swipe = fixture("highlight-swipe");
    let check = fixture("checkmark");

    let mut ink: HashMap<String, Vec<Stroke>> = HashMap::new();
    ink.insert(
        x.0.clone(),
        device_ink(&device, &swipe, union_rect(&mx, &["tok-0", "tok-1"]), ph_x),
    );
    ink.insert(
        y.0.clone(),
        device_ink(&device, &check, region_rect(&my, "done"), ph_y),
    );

    // Cycle 1: step.
    let cycle = application.step(&mut set, &ink).await.unwrap();

    // Decoded a highlight on a1 and an archive on a2.
    assert!(
        cycle
            .decoded
            .iter()
            .any(|m| matches!(m, Msg::Highlighted { article, .. } if article.0 == "a1")),
        "decoded a highlight on a1: {:?}",
        cycle.decoded
    );
    assert!(
        cycle.decoded.contains(&Msg::Archived {
            article: inkapp_readwise_reader::ArticleId::new("a2")
        }),
        "decoded an archive on a2: {:?}",
        cycle.decoded
    );

    // Connector recorded both; a2 archived -> Delete(a2).
    assert_eq!(
        application.connectors.readwise.archived(),
        vec![inkapp_readwise_reader::ArticleId::new("a2")]
    );
    assert!(!application
        .connectors
        .readwise
        .highlights(&inkapp_readwise_reader::ArticleId::new("a1"))
        .is_empty());
    assert!(cycle.ops.contains(&DocOp::Delete(y.clone())));

    // a1 survives, re-rendered with the highlight in the body.
    assert!(set.manifest(&x).is_some(), "a1 survives");
    // DocSet stores rendered PDFs + manifests, not re-renderable source, so we
    // re-run the (pure) view to inspect what a1 would now render as.
    let docs = view(&App, &application.connectors);
    let a1_doc = docs.0.iter().find(|d| d.key == x).unwrap();
    assert!(
        document_source(a1_doc).contains("#highlight"),
        "highlight rendered into a1's body"
    );

    // a1's prior ink is preserved across the re-render.
    assert!(!set.ink(&x).is_empty(), "a1 ink preserved");

    // Cycle 2: empty ink -> stable (no new archives, no create/delete).
    let cycle2 = application.step(&mut set, &HashMap::new()).await.unwrap();
    assert!(cycle2.decoded.is_empty());
    assert!(!cycle2
        .ops
        .iter()
        .any(|o| matches!(o, DocOp::Create(_) | DocOp::Delete(_))));
}
