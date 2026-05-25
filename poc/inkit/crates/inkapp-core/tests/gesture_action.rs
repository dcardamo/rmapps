use inkapp_core::component::{Component, RenderCx};
use inkapp_core::components::gesture::GestureAction;
use inkapp_core::geometry::{PdfPoint, PdfRect};
use inkapp_core::ink::{RegionInk, Stroke};
use inkapp_core::manifest::{Manifest, Region};

#[derive(Debug, Clone, PartialEq)]
enum M {
    Archived,
}

fn manifest_with(name: &str, rect: PdfRect) -> Manifest {
    Manifest {
        version: 1,
        regions: vec![Region {
            name: name.into(),
            page: 0,
            rect,
        }],
        ..Default::default()
    }
}

// A title region 100pt wide, 16pt tall.
const TITLE_RECT: PdfRect = PdfRect {
    x0: 10.0,
    y0: 10.0,
    x1: 110.0,
    y1: 26.0,
};

fn region_ink(highlighter: bool, points: Vec<PdfPoint>) -> Vec<RegionInk> {
    vec![RegionInk {
        region: "title".into(),
        strokes: vec![Stroke {
            points,
            highlighter,
        }],
    }]
}

#[test]
fn fires_on_wide_pen_strike() {
    let g = GestureAction::with_msg("title", "How CGI changed the web", M::Archived);
    let manifest = manifest_with("title", TITLE_RECT);
    // A horizontal pen stroke spanning ~96% of the region width.
    let ink = region_ink(
        false,
        vec![
            PdfPoint { x: 12.0, y: 18.0 },
            PdfPoint { x: 108.0, y: 18.0 },
        ],
    );
    assert_eq!(g.decode(&ink, &manifest), vec![M::Archived]);
}

#[test]
fn no_fire_on_tap() {
    let g = GestureAction::with_msg("title", "How CGI changed the web", M::Archived);
    let manifest = manifest_with("title", TITLE_RECT);
    let ink = region_ink(false, vec![PdfPoint { x: 60.0, y: 18.0 }]);
    assert!(
        g.decode(&ink, &manifest).is_empty(),
        "a single dot is not a strike"
    );
}

#[test]
fn no_fire_on_narrow_pen_stroke() {
    let g = GestureAction::with_msg("title", "How CGI changed the web", M::Archived);
    let manifest = manifest_with("title", TITLE_RECT);
    // A short pen stroke spanning only ~30% of the 100pt-wide region — below the
    // 0.6 strike threshold, so it must not fire.
    let ink = region_ink(
        false,
        vec![PdfPoint { x: 40.0, y: 18.0 }, PdfPoint { x: 70.0, y: 18.0 }],
    );
    assert!(
        g.decode(&ink, &manifest).is_empty(),
        "a narrow pen stroke is not a strike"
    );
}

#[test]
fn no_fire_on_highlighter_swipe() {
    let g = GestureAction::with_msg("title", "How CGI changed the web", M::Archived);
    let manifest = manifest_with("title", TITLE_RECT);
    // Same wide geometry as the firing case, but a highlighter — wrong tool.
    let ink = region_ink(
        true,
        vec![
            PdfPoint { x: 12.0, y: 18.0 },
            PdfPoint { x: 108.0, y: 18.0 },
        ],
    );
    assert!(
        g.decode(&ink, &manifest).is_empty(),
        "a highlighter swipe must not fire the action"
    );
}

#[test]
fn no_fire_when_empty() {
    let g = GestureAction::with_msg("title", "How CGI changed the web", M::Archived);
    let manifest = manifest_with("title", TITLE_RECT);
    assert!(g.decode(&[], &manifest).is_empty());
}

#[test]
fn new_presence_only_fires_on_strike() {
    // The M=() convenience constructor: decode yields one unit message on a strike.
    let g = GestureAction::new("title", "How CGI changed the web");
    let manifest = manifest_with("title", TITLE_RECT);
    let ink = region_ink(
        false,
        vec![
            PdfPoint { x: 12.0, y: 18.0 },
            PdfPoint { x: 108.0, y: 18.0 },
        ],
    );
    assert!(
        g.read(&ink, &manifest),
        "presence-only control detects the strike"
    );
    assert_eq!(g.decode(&ink, &manifest), vec![()]);
}

#[test]
fn render_declares_region_and_content() {
    let g = GestureAction::with_msg("title", "Hello", M::Archived);
    let markup = g.render(&mut RenderCx::new(0));
    assert!(
        markup.contains("#region(\"title\""),
        "calls the region prelude: {markup}"
    );
    assert!(markup.contains("Hello"), "content present: {markup}");
}

// Integration: render → recover → attribute → decode, single page.
use inkapp_core::document::Document;
use inkapp_core::flow;
use inkapp_core::geometry::PageGeom;
use inkapp_core::manifest::recover_regions;
use inkapp_core::readback::attribute_page;
use inkapp_core::runtime::compile_document_in;
use inkapp_core::Theme;

#[test]
fn gesture_action_decodes_strike_end_to_end() {
    let doc: Document<M> = Document::keyed(
        "d",
        flow![GestureAction::with_msg(
            "title",
            "How CGI changed the web",
            M::Archived
        )],
    );
    let compiled = compile_document_in(&doc, PageGeom::default(), &Theme::reader()).unwrap();
    let manifest = recover_regions(&compiled).unwrap();

    let region = manifest
        .regions
        .iter()
        .find(|r| r.name == "title")
        .expect("title region recovered");
    // A pen strike spanning the full recovered region width.
    let cy = (region.rect.y0 + region.rect.y1) / 2.0;
    let stroke = Stroke {
        points: vec![
            PdfPoint {
                x: region.rect.x0,
                y: cy,
            },
            PdfPoint {
                x: region.rect.x1,
                y: cy,
            },
        ],
        highlighter: false,
    };
    let ink = attribute_page(&[stroke], &manifest);
    let decoded = doc.flow[0].decode(&ink, &manifest);
    assert_eq!(
        decoded,
        vec![M::Archived],
        "a region-spanning pen strike decodes to one Archived"
    );
}
