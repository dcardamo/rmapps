//! ActionBand: render emits per-cell regions keyed by section, decode classifies
//! a pen strike on the right cell into the right Msg.

use std::sync::{Arc, Mutex};

use inkapp_core::components::action_band::ActionBand;
use inkapp_core::components::notice::Notice;
use inkapp_core::components::section::Section;
use inkapp_core::document::Document;
use inkapp_core::flow;
use inkapp_core::geometry::{PageGeom, PdfRect};
use inkapp_core::ink::{RegionInk, Stroke};
use inkapp_core::manifest::{recover_regions, Manifest};
use inkapp_core::render::compile_to_document_with_sources;
use inkapp_core::runtime::{collect_typst_sources, document_source_in};
use inkapp_core::theme::Theme;

#[derive(Debug, Clone, PartialEq, Eq)]
enum TestMsg {
    Inbox(String),
    Archive(String),
}

fn band_with_recorder() -> (ActionBand<TestMsg>, Arc<Mutex<Vec<TestMsg>>>) {
    let log = Arc::new(Mutex::new(Vec::new()));
    let log_a = log.clone();
    let log_i = log.clone();
    let band = ActionBand::new([
        (
            "Inbox".to_string(),
            Box::new(move |id: &str| {
                let m = TestMsg::Inbox(id.into());
                log_i.lock().unwrap().push(m.clone());
                m
            }) as Box<dyn Fn(&str) -> TestMsg + Send + Sync>,
        ),
        (
            "Archive".to_string(),
            Box::new(move |id: &str| {
                let m = TestMsg::Archive(id.into());
                log_a.lock().unwrap().push(m.clone());
                m
            }),
        ),
    ]);
    (band, log)
}

fn compile_doc_with_band(band: ActionBand<TestMsg>) -> (Document<TestMsg>, Manifest, Vec<usize>) {
    // Two short sections so the band sees two distinct section ids on different pages.
    let s1: Section<TestMsg> = Section::new("art-1", vec![Box::new(Notice::line("first"))]);
    let s2: Section<TestMsg> = Section::new("art-2", vec![Box::new(Notice::line("second"))]);
    let doc: Document<TestMsg> = Document::keyed("library", flow![s1, s2]).page_header(band);
    let geom = PageGeom {
        w: 200.0,
        h: 120.0,
        margin: 6.0,
    };
    let src = document_source_in(&doc, geom, &Theme::reader());
    let sources = collect_typst_sources(&doc);
    let compiled = compile_to_document_with_sources(&src, &sources).unwrap();
    let manifest = recover_regions(&compiled).unwrap();
    let page_count = compiled.pages.len();
    (doc, manifest, (0..page_count).collect())
}

#[test]
fn render_produces_per_section_action_regions() {
    let (band, _log) = band_with_recorder();
    let (_doc, manifest, _) = compile_doc_with_band(band);
    let names: Vec<_> = manifest.regions.iter().map(|r| r.name.clone()).collect();
    assert!(
        names.iter().any(|n| n == "action-Inbox-art-1"),
        "Inbox/art-1 region: {names:?}"
    );
    assert!(
        names.iter().any(|n| n == "action-Archive-art-1"),
        "Archive/art-1 region: {names:?}"
    );
    assert!(
        names.iter().any(|n| n == "action-Inbox-art-2"),
        "Inbox/art-2 region: {names:?}"
    );
}

fn strike_across(rect: &PdfRect) -> Stroke {
    let y_mid = (rect.y0 + rect.y1) / 2.0;
    Stroke {
        points: (0..=10)
            .map(|i| inkapp_core::geometry::PdfPoint {
                x: rect.x0 + (rect.x1 - rect.x0) * (i as f64 / 10.0),
                y: y_mid,
            })
            .collect(),
        highlighter: false,
    }
}

#[test]
fn pen_strike_on_archive_art1_fires_the_archive_closure() {
    let (band, _log) = band_with_recorder();
    let (doc, manifest, _) = compile_doc_with_band(band);
    let header = doc.page_header.as_ref().unwrap();

    let target = manifest
        .regions
        .iter()
        .find(|r| r.name == "action-Archive-art-1")
        .unwrap();
    let region_ink = vec![RegionInk {
        region: "action-Archive-art-1".into(),
        strokes: vec![strike_across(&target.rect)],
    }];
    let msgs = header.decode(&region_ink, &manifest);
    assert_eq!(msgs, vec![TestMsg::Archive("art-1".into())]);
}

#[test]
fn highlighter_stroke_does_not_fire() {
    let (band, _log) = band_with_recorder();
    let (doc, manifest, _) = compile_doc_with_band(band);
    let header = doc.page_header.as_ref().unwrap();

    let target = manifest
        .regions
        .iter()
        .find(|r| r.name == "action-Archive-art-1")
        .unwrap();
    let mut s = strike_across(&target.rect);
    s.highlighter = true;
    let region_ink = vec![RegionInk {
        region: "action-Archive-art-1".into(),
        strokes: vec![s],
    }];
    let msgs = header.decode(&region_ink, &manifest);
    assert!(
        msgs.is_empty(),
        "highlighter must not fire actions: {msgs:?}"
    );
}

#[test]
fn empty_ink_fires_nothing() {
    let (band, _log) = band_with_recorder();
    let (doc, manifest, _) = compile_doc_with_band(band);
    let header = doc.page_header.as_ref().unwrap();
    let msgs = header.decode(&[], &manifest);
    assert!(msgs.is_empty());
}

/// Three short non-highlighter strokes across the Archive cell, each spanning
/// ~20% of the cell width on its own (below the 60% threshold), but with a
/// total union of ~60%. The bbox-union classification fires; the old per-stroke
/// `any(…)` approach would not.
#[test]
fn multi_stroke_scribble_fires_action() {
    let (band, _log) = band_with_recorder();
    let (doc, manifest, _) = compile_doc_with_band(band);
    let header = doc.page_header.as_ref().unwrap();

    let target = manifest
        .regions
        .iter()
        .find(|r| r.name == "action-Archive-art-1")
        .unwrap();

    let w = target.rect.x1 - target.rect.x0;
    let y_mid = (target.rect.y0 + target.rect.y1) / 2.0;

    // Three strokes each covering ~22% of the cell width (< 60% threshold alone),
    // placed consecutively so their union spans ~66% (> 60% threshold together).
    // Using 22% per stroke avoids the floating-point boundary at exactly 60%.
    let make_short_stroke = |frac_start: f64, frac_end: f64| -> Stroke {
        Stroke {
            points: (0..=5)
                .map(|i| inkapp_core::geometry::PdfPoint {
                    x: target.rect.x0
                        + w * (frac_start + (frac_end - frac_start) * (i as f64 / 5.0)),
                    y: y_mid,
                })
                .collect(),
            highlighter: false,
        }
    };

    let region_ink = vec![RegionInk {
        region: "action-Archive-art-1".into(),
        strokes: vec![
            make_short_stroke(0.0, 0.22),
            make_short_stroke(0.22, 0.44),
            make_short_stroke(0.44, 0.66),
        ],
    }];

    let msgs = header.decode(&region_ink, &manifest);
    assert_eq!(
        msgs,
        vec![TestMsg::Archive("art-1".into())],
        "three short strokes spanning 60% total must fire the action via bbox union"
    );
}

#[test]
#[should_panic(expected = "must not contain '-'")]
fn label_with_dash_panics_on_construction() {
    ActionBand::new([(
        "Move-Archive".to_string(),
        Box::new(|_id: &str| ()) as Box<dyn Fn(&str) + Send + Sync>,
    )]);
}

#[test]
#[should_panic(expected = "must not be empty")]
fn empty_label_panics_on_construction() {
    ActionBand::new([(
        "".to_string(),
        Box::new(|_id: &str| ()) as Box<dyn Fn(&str) + Send + Sync>,
    )]);
}

#[test]
fn pen_strike_on_inbox_art2_fires_the_inbox_closure() {
    let (band, _log) = band_with_recorder();
    let (doc, manifest, _) = compile_doc_with_band(band);
    let header = doc.page_header.as_ref().unwrap();

    let target = manifest
        .regions
        .iter()
        .find(|r| r.name == "action-Inbox-art-2")
        .expect("Inbox/art-2 region present in recovered manifest");
    let region_ink = vec![RegionInk {
        region: "action-Inbox-art-2".into(),
        strokes: vec![strike_across(&target.rect)],
    }];
    let msgs = header.decode(&region_ink, &manifest);
    assert_eq!(msgs, vec![TestMsg::Inbox("art-2".into())]);
}

#[test]
fn each_section_has_full_action_set_on_its_own_page() {
    let (band, _log) = band_with_recorder();
    let (_doc, manifest, _) = compile_doc_with_band(band);

    let by_name: std::collections::HashMap<&str, usize> = manifest
        .regions
        .iter()
        .map(|r| (r.name.as_str(), r.page))
        .collect();

    for section in ["art-1", "art-2"] {
        for label in ["Inbox", "Archive"] {
            let name = format!("action-{label}-{section}");
            assert!(
                by_name.contains_key(name.as_str()),
                "missing region {name}; saw: {:?}",
                by_name.keys().collect::<Vec<_>>()
            );
        }
    }

    let page_art1 = by_name["action-Inbox-art-1"];
    let page_art2 = by_name["action-Inbox-art-2"];
    assert_ne!(
        page_art1, page_art2,
        "art-1 and art-2 must land on different pages; both on {page_art1}"
    );
}

#[test]
fn sub_threshold_strike_does_not_fire() {
    let (band, _log) = band_with_recorder();
    let (doc, manifest, _) = compile_doc_with_band(band);
    let header = doc.page_header.as_ref().unwrap();

    let target = manifest
        .regions
        .iter()
        .find(|r| r.name == "action-Archive-art-1")
        .unwrap();
    let w = target.rect.x1 - target.rect.x0;
    let y_mid = (target.rect.y0 + target.rect.y1) / 2.0;
    let stroke = Stroke {
        points: (0..=5)
            .map(|i| inkapp_core::geometry::PdfPoint {
                x: target.rect.x0 + w * 0.4 + w * 0.2 * (i as f64 / 5.0),
                y: y_mid,
            })
            .collect(),
        highlighter: false,
    };

    let region_ink = vec![RegionInk {
        region: "action-Archive-art-1".into(),
        strokes: vec![stroke],
    }];
    let msgs = header.decode(&region_ink, &manifest);
    assert!(
        msgs.is_empty(),
        "20%-width strike must not fire; got: {msgs:?}"
    );
}
