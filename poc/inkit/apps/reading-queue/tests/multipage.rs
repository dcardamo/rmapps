//! The reading-queue device pull assembles per-page ink from a multi-page
//! `.rmdoc` in `.content` order: ink on page k attributes to page k, a passage
//! split across page breaks stitches to one message, and an un-inked middle page
//! stays an empty slot (no index shift). No device / no rmapi.

use std::io::Write;

use inkapp::{Document, Remarkable};
use inkapp_core::component::{Component, RenderCx};
use inkapp_core::components::checkbox::Checkbox;
use inkapp_core::components::highlight_text::HighlightableText;
use inkapp_core::components::passage::Passage;
use inkapp_core::crypto::Key;
use inkapp_core::device::Device;
use inkapp_core::geometry::{PageGeom, PdfPoint};
use inkapp_core::ink::{RegionInk, Stroke};
use inkapp_core::manifest::Manifest;
use inkapp_core::readback::attribute;
use inkapp_core::runtime::render_document_in;

use reading_queue::serve::strokes_by_page;

/// Local test messages (this test exercises the pull path, not the app's view).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Msg {
    Hi(String),
    Note,
    Done,
}

/// A per-token highlightable body that emits `Hi(token)`.
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

/// A tall document: tokens + a long breakable passage + an archive checkbox.
/// The passage forces several pages and splits across breaks.
fn doc() -> Document<Msg> {
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
    Document::keyed("d", flow)
}

/// A swipe across a region's rect, sampled at interior points so at least one
/// survives the f32 round-trip in the device transform (`attribute` needs ANY
/// point inside the rect).
fn swipe(r: &inkapp_core::manifest::Region, highlighter: bool) -> Stroke {
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
    Stroke {
        points,
        highlighter,
    }
}

/// Write a multi-page `.rmdoc` zip: `<uuid>.content` listing `page_count` pages in
/// order, and `<uuid>/<page-uuid>.rm` for every page whose `per_page` strokes are
/// non-empty (un-inked pages get no `.rm`). Returns the file path.
fn write_rmdoc(device: &Remarkable, page_h: f64, per_page: &[Vec<Stroke>]) -> std::path::PathBuf {
    let uuid = "doc-uuid";
    let page_ids: Vec<String> = (0..per_page.len()).map(|p| format!("page-{p}")).collect();

    let unique = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let path = std::env::temp_dir().join(format!("rq-multipage-{unique}.rmdoc"));
    let file = std::fs::File::create(&path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let opts: zip::write::SimpleFileOptions =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

    // .content (cPages page list, in order).
    let pages_json: Vec<String> = page_ids
        .iter()
        .map(|id| format!(r#"{{"id":"{id}"}}"#))
        .collect();
    let content = format!(r#"{{"cPages":{{"pages":[{}]}}}}"#, pages_json.join(","));
    zip.start_file(format!("{uuid}.content"), opts).unwrap();
    zip.write_all(content.as_bytes()).unwrap();

    // One `.rm` per inked page.
    for (p, strokes) in per_page.iter().enumerate() {
        if strokes.is_empty() {
            continue;
        }
        let bytes = device.write_ink(strokes, page_h).unwrap();
        zip.start_file(format!("{uuid}/{}.rm", page_ids[p]), opts)
            .unwrap();
        zip.write_all(&bytes).unwrap();
    }
    zip.finish().unwrap();
    path
}

#[test]
fn pull_assembles_per_page_ink_with_empty_middle_page() {
    let key = Key::from_bytes([7u8; 32]);
    let device = Remarkable::new();
    let d = doc();

    // A short page forces several pages and splits the passage across breaks.
    let geom = PageGeom {
        w: 420.0,
        h: 240.0,
        margin: 16.0,
    };
    let rd = render_document_in(&d, 1, &key, geom).unwrap();

    // Pages the `notes` passage occupies (one frame per page it flows through).
    let mut notes_pages: Vec<usize> = rd
        .manifest
        .regions
        .iter()
        .filter(|r| r.name == "notes")
        .map(|r| r.page)
        .collect();
    notes_pages.sort_unstable();
    notes_pages.dedup();
    assert!(
        rd.page_count >= 3 && notes_pages.len() >= 3,
        "need >=3 pages and a passage spanning >=3 frames to test a gap; \
         got page_count={}, notes frames on pages {:?}",
        rd.page_count,
        notes_pages
    );

    // Ink targets: a token, the `done` checkbox, and notes on two NON-ADJACENT
    // frames (notes_pages[0] and notes_pages[2]); leave notes_pages[1] un-inked as
    // the empty middle slot.
    let inked_notes = [notes_pages[0], notes_pages[2]];
    let empty_page = notes_pages[1];

    let mut per_page: Vec<Vec<Stroke>> = vec![Vec::new(); rd.page_count];
    for r in &rd.manifest.regions {
        let target = match r.name.as_str() {
            "tok-7" => Some(true),
            "done" => Some(false),
            "notes" if inked_notes.contains(&r.page) => Some(true),
            _ => None,
        };
        if let Some(hl) = target {
            per_page[r.page].push(swipe(r, hl));
        }
    }
    assert!(
        per_page[empty_page].is_empty(),
        "the chosen middle page must be un-inked (it carries only a skipped notes frame)"
    );

    // Synthesize the `.rmdoc` and run the real pull path.
    let path = write_rmdoc(&device, rd.page_h, &per_page);
    let pages = strokes_by_page(&device, &path, rd.page_h);
    std::fs::remove_file(&path).ok();

    // Length matches the `.content` page list; the middle page is an empty slot.
    assert_eq!(pages.len(), rd.page_count, "one slot per .content page");
    assert!(
        pages[empty_page].is_empty(),
        "un-inked middle page {empty_page} is an empty slot, not dropped"
    );

    // Attribute + decode: per-page alignment, split-region stitching, no shift.
    let region_ink = attribute(&pages, &rd.manifest);
    let mut msgs = std::collections::BTreeSet::new();
    for c in &d.flow {
        for m in c.decode(&region_ink, &rd.manifest) {
            msgs.insert(m);
        }
    }
    let expected: std::collections::BTreeSet<Msg> =
        [Msg::Hi("word07".into()), Msg::Note, Msg::Done]
            .into_iter()
            .collect();
    assert_eq!(
        msgs, expected,
        "ink attributes to the right page (tok-7=>word07, notes stitched from two \
         frames=>one Note, done on the last page=>Done) despite an empty middle page"
    );
}
