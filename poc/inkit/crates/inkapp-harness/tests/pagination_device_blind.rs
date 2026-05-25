mod common;

use std::collections::BTreeSet;

use inkapp_core::component::{Component, RenderCx};
use inkapp_core::components::checkbox::Checkbox;
use inkapp_core::components::highlight_text::HighlightableText;
use inkapp_core::components::passage::Passage;
use inkapp_core::device::Device;
use inkapp_core::geometry::{PageGeom, PdfPoint};
use inkapp_core::ink::{RegionInk, Stroke};
use inkapp_core::manifest::Manifest;
use inkapp_core::readback::attribute;
use inkapp_core::runtime::render_document_in;
use rm_device::Remarkable;

/// The test app's messages. `Ord` so we can compare sets independent of order.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Msg {
    Hi(String),
    Note,
    Done,
}

/// A bespoke content component: per-token highlightable body that emits Hi(token).
struct Body {
    text: HighlightableText,
}
impl Component for Body {
    type Msg = Msg;
    fn render(&self, cx: &mut RenderCx) -> String {
        self.text.render(cx)
    }
    fn decode(&self, ink: &[RegionInk], manifest: &Manifest) -> Vec<Msg> {
        self.text
            .read(ink, manifest)
            .into_iter()
            .map(Msg::Hi)
            .collect()
    }
}

/// Build the same logical document each time (tall, so geometry changes page count).
fn doc() -> inkapp_core::document::Document<Msg> {
    let tokens: Vec<String> = (0..30).map(|i| format!("word{i:02}")).collect();
    let tok_refs: Vec<&str> = tokens.iter().map(|s| s.as_str()).collect();
    let body = Body {
        text: HighlightableText::new(&tok_refs),
    };

    let lines: Vec<String> = (0..30)
        .map(|i| format!("passage line number {i}"))
        .collect();
    let line_refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
    let passage = Passage::with_msg("notes", &line_refs, Msg::Note);

    let check = Checkbox::with_msg("done", Msg::Done).label("Archive");

    let mut flow: Vec<Box<dyn Component<Msg = Msg>>> = Vec::new();
    flow.push(Box::new(body));
    flow.push(Box::new(passage));
    flow.push(Box::new(check));
    inkapp_core::document::Document::keyed("d", flow)
}

/// Build per-page device-round-tripped ink for the given targets. Each target is a
/// (region name, is_highlighter) pair; for every recovered region with that name we
/// synthesize a swipe across its rect on that region's page, then write+read it
/// through the real .rm path so the test exercises the byte path.
fn device_pages(
    manifest: &Manifest,
    device: &Remarkable,
    page_h: f64,
    page_count: usize,
    targets: &[(&str, bool)],
) -> Vec<Vec<Stroke>> {
    let mut per_page: Vec<Vec<Stroke>> = vec![Vec::new(); page_count];
    for (name, hl) in targets {
        for r in manifest.regions.iter().filter(|r| &r.name == name) {
            // Sample multiple interior points across the rect width to ensure
            // at least one point lands inside the region after f32 round-trip
            // quantization. The attribute check requires ANY point inside the rect.
            const SAMPLES: usize = 8;
            let cy = (r.rect.y0 + r.rect.y1) / 2.0;
            let points: Vec<PdfPoint> = (0..=SAMPLES)
                .map(|k| {
                    let t = k as f64 / SAMPLES as f64;
                    PdfPoint {
                        x: r.rect.x0 + t * (r.rect.x1 - r.rect.x0),
                        y: cy,
                    }
                })
                .collect();
            let stroke = Stroke {
                points,
                highlighter: *hl,
            };
            per_page[r.page].push(stroke);
        }
    }
    per_page
        .into_iter()
        .map(|strokes| {
            // page_h is uniform across all pages (Typst's #set page applies globally; see
            // render_document_in's single-page_h comment), so one height transforms every page.
            let bytes = device.write_ink(&strokes, page_h).unwrap();
            device.read_ink(&bytes, page_h).unwrap()
        })
        .collect()
}

/// Render + ink + attribute + decode for one profile; returns (page_count, msgs).
fn run_profile(geom: PageGeom) -> (usize, BTreeSet<Msg>) {
    let key = common::test_key();
    let device = Remarkable::new();
    let d = doc();
    let rd = render_document_in(&d, 1, &key, geom).unwrap();

    // The Passage must actually split across a page break on each profile, so this
    // test genuinely exercises cross-page ink stitching (not just page distribution).
    let notes_frames = rd
        .manifest
        .regions
        .iter()
        .filter(|r| r.name == "notes")
        .count();
    assert!(
        notes_frames >= 2,
        "the 'notes' passage must span >1 page to exercise stitching, got {notes_frames} frame(s)"
    );

    // tok-7 is the region name for token index 7; doc() builds tokens as "word{i:02}",
    // so index 7 → region "tok-7" → decoded as Msg::Hi("word07"). These must agree.
    let targets: &[(&str, bool)] = &[("tok-7", true), ("notes", true), ("done", false)];
    let pages = device_pages(&rd.manifest, &device, rd.page_h, rd.page_count, targets);

    let region_ink = attribute(&pages, &rd.manifest);
    let mut msgs = BTreeSet::new();
    for c in &d.flow {
        for m in c.decode(&region_ink, &rd.manifest) {
            msgs.insert(m);
        }
    }
    (rd.page_count, msgs)
}

#[test]
fn same_content_two_profiles_decode_identically() {
    // Profile A: default (420×560pt) — standard e-ink profile.
    // Profile B: shorter (420×240pt) — same width, ~43% the height, so it
    // paginates to strictly more pages than A for the same content.
    let (pages_a, msgs_a) = run_profile(PageGeom::default());
    let (pages_b, msgs_b) = run_profile(PageGeom {
        w: 420.0,
        h: 240.0,
        margin: 16.0,
    });

    assert_ne!(
        pages_a, pages_b,
        "the two profiles must paginate to different page counts (A={pages_a}, B={pages_b})"
    );
    let expected: BTreeSet<Msg> = [Msg::Hi("word07".into()), Msg::Note, Msg::Done]
        .into_iter()
        .collect();
    assert_eq!(msgs_a, expected, "profile A decoded the expected messages");
    assert_eq!(
        msgs_a, msgs_b,
        "decoded messages are identical across profiles (page-/device-blind)"
    );
}
