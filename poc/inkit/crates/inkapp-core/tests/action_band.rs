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

#[cfg(test)]
#[allow(dead_code)]
fn _debug_minimal_typst() {
    // Kept for reference (debugging `state.at` API in Typst 0.14.2 headers).
    let region_typ = include_str!("../typst/region.typ");
    let section_typ = include_str!("../typst/section.typ");

    // Step 1: context+if alone
    let v1 = r#"
#import "/inkapp/section.typ": section-state
#let action-band(labels) = context {
  let sid = section-state.at(here().position())
  if sid == "" { block(height: 18pt, []) } else { [#sid] }
}
#set page(width: 200pt, height: 120pt, margin: 6pt)
#set page(header: [#action-band(("Inbox", "Archive", ))])
#section-state.update("art-1")
hello
"#;
    let srcs1 = vec![
        ("/inkapp/section.typ".into(), section_typ.into()),
    ];
    let r1 = compile_to_document_with_sources(v1, &srcs1);
    println!("v1 (context+if): {:?}", r1.as_ref().map(|d| d.pages.len()).map_err(|e| e.to_string()));

    // Step 2: add region import
    let v2 = r#"
#import "/inkapp/region.typ": region
#import "/inkapp/section.typ": section-state
#let action-band(labels) = context {
  let sid = section-state.at(here().position())
  if sid == "" { block(height: 18pt, []) } else {
    region("test-region", box(height: 18pt, [x]))
  }
}
#set page(width: 200pt, height: 120pt, margin: 6pt)
#set page(header: [#action-band(("Inbox", "Archive", ))])
#section-state.update("art-1")
hello
"#;
    let srcs2 = vec![
        ("/inkapp/region.typ".into(), region_typ.into()),
        ("/inkapp/section.typ".into(), section_typ.into()),
    ];
    let r2 = compile_to_document_with_sources(v2, &srcs2);
    println!("v2 (region in header): {:?}", r2.as_ref().map(|d| d.pages.len()).map_err(|e| e.to_string()));

    // Step 3: context alone without state.at
    let v3 = r#"
#import "/inkapp/section.typ": section-state
#let action-band(labels) = context {
  [hello from context]
}
#set page(width: 200pt, height: 120pt, margin: 6pt)
#set page(header: [#action-band(("Inbox", ))])
#section-state.update("art-1")
hello
"#;
    let r3 = compile_to_document_with_sources(v3, &srcs1);
    println!("v3 (context alone): {:?}", r3.as_ref().map(|d| d.pages.len()).map_err(|e| e.to_string()));

    // Step 4a: state.at(here().position()) - wrong
    let v4 = r#"
#import "/inkapp/section.typ": section-state
#let action-band(labels) = context {
  let sid = section-state.at(here().position())
  [#sid]
}
#set page(width: 200pt, height: 120pt, margin: 6pt)
#set page(header: [#action-band(("Inbox", ))])
#section-state.update("art-1")
hello
"#;
    let r4 = compile_to_document_with_sources(v4, &srcs1);
    println!("v4a (state.at(here().position())): {:?}", r4.as_ref().map(|d| d.pages.len()).map_err(|e| e.to_string()));

    // Step 4b: state.at(here()) - correct?
    let v4b = r#"
#import "/inkapp/section.typ": section-state
#let action-band(labels) = context {
  let sid = section-state.at(here())
  [#sid]
}
#set page(width: 200pt, height: 120pt, margin: 6pt)
#set page(header: [#action-band(("Inbox", ))])
#section-state.update("art-1")
hello
"#;
    let r4b = compile_to_document_with_sources(v4b, &srcs1);
    println!("v4b (state.at(here())): {:?}", r4b.as_ref().map(|d| d.pages.len()).map_err(|e| e.to_string()));

    // Step 5: metadata dict in header context
    let v5 = r#"
#let my-fn() = context {
  [#metadata((name: "test")) <region>]
}
#set page(width: 200pt, height: 120pt, margin: 6pt)
#set page(header: [#my-fn()])
hello
"#;
    let r5 = compile_to_document_with_sources(v5, &[]);
    println!("v5 (metadata in header context): {:?}", r5.as_ref().map(|d| d.pages.len()).map_err(|e| e.to_string()));

    // Step 6: state update BEFORE pagebreak in section
    let section_v2 = r#"
#let section-state = state("inkapp.section", "")
#let section(id, body) = {
  section-state.update(id)   // update first
  pagebreak(weak: true)       // then break
  body
}
"#;
    let action_band_typ = r#"
#import "/inkapp/section.typ": section-state
#let action-band(labels) = context {
  let sid = section-state.at(here())
  if sid == "" { block(height: 18pt, []) } else { [#sid] }
}
"#;
    let v6 = r#"
#import "/inkapp/section.typ": *
#import "/inkapp/action_band.typ": *
#set page(width: 200pt, height: 120pt, margin: 6pt)
#set page(header: [#action-band(("Inbox", ))])
#section("art-1", [hello first])
#section("art-2", [hello second])
"#;
    let srcs6 = vec![
        ("/inkapp/section.typ".into(), section_v2.into()),
        ("/inkapp/action_band.typ".into(), action_band_typ.into()),
    ];
    let r6 = compile_to_document_with_sources(v6, &srcs6);
    println!("v6 (state before break): {:?}", r6.as_ref().map(|d| d.pages.len()).map_err(|e| e.to_string()));
    if let Ok(doc) = r6 {
        use inkapp_core::manifest::recover_regions;
        // no regions here, just pages
        println!("  pages: {}", doc.pages.len());
    }

    // Step 7: test with the page-first approach and regions
    let action_band_full = include_str!("../typst/action_band.typ");
    let region_typ = include_str!("../typst/region.typ");
    let v7 = r#"
#import "/inkapp/region.typ": region
#import "/inkapp/section.typ": *
#import "/inkapp/action_band.typ": *
#set page(width: 200pt, height: 120pt, margin: 6pt)
#set page(header: [#action-band(("Inbox", "Archive", ))])
#section("art-1", [hello first])
#section("art-2", [hello second])
"#;
    let srcs7 = vec![
        ("/inkapp/region.typ".into(), region_typ.into()),
        ("/inkapp/section.typ".into(), section_v2.into()),
        ("/inkapp/action_band.typ".into(), action_band_full.into()),
    ];
    let r7 = compile_to_document_with_sources(v7, &srcs7);
    println!("v7 (full with state-before-break): {:?}", r7.as_ref().map(|d| d.pages.len()).map_err(|e| e.to_string()));
    if let Ok(compiled) = &r7 {
        use inkapp_core::manifest::recover_regions;
        if let Ok(manifest) = recover_regions(compiled) {
            println!("  regions: {:?}", manifest.regions.iter().map(|r| &r.name).collect::<Vec<_>>());
        }
    }
}
