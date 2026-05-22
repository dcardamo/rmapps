//! Post-process the rendered PDF: paint a full-bleed paper background on every
//! page (fulgur only fills the content box, leaving the margins unpainted), then
//! draw a persistent, clickable full-width nav bar (`< Prev   Home   Next >`)
//! across the top of every article page.
//!
//! fulgur cannot repeat a clickable element across the pages of a flowing
//! article (running headers render but emit zero link annotations), so we render
//! normally and then stamp the nav onto each article page here with lopdf: a
//! content stream fills the bar (theme navbg) and draws the labels (navfg) in the
//! reserved top band, and `/Link` annotations provide the real tap-targets.
use std::collections::HashMap;
use std::path::Path;

use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};

/// Paint a full-bleed paper background on every page, then draw a filled,
/// clickable full-width `< Prev   Home   Next >` bar across the top of every
/// article page, and stamp the four action-label columns (inbox/archive/later/
/// delete) in the band just below the nav bar. Fills `embedded.label_rects` and
/// each doc's `page_range`, then embeds the manifest into the PDF.
///
/// `num_articles` = index-row count; colours come from the theme
/// (`paper_hex`/`nav_bg_hex`/`nav_fg_hex`). Best-effort: on any error, log to
/// stderr and leave the rendered PDF intact (do not lose it).
#[allow(clippy::too_many_arguments)]
pub fn finalize_pdf(
    path: &Path,
    num_articles: usize,
    page_w: f32,
    page_h: f32,
    paper_hex: &str,
    nav_bg_hex: &str,
    nav_fg_hex: &str,
    embedded: &mut crate::manifest::EmbeddedManifest,
) -> anyhow::Result<()> {
    if let Err(e) = try_finalize_pdf(
        path,
        num_articles,
        page_w,
        page_h,
        paper_hex,
        nav_bg_hex,
        nav_fg_hex,
        embedded,
    ) {
        eprintln!("[rmreader] postprocess: finalize skipped ({e:#}); rendered PDF left intact");
    }
    Ok(())
}

/// Parse "#RRGGBB" into PDF rgb components (0.0..1.0).
fn hex_rgb(s: &str) -> Option<(f32, f32, f32)> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    let c = |a: usize| {
        u8::from_str_radix(&h[a..a + 2], 16)
            .ok()
            .map(|v| v as f32 / 255.0)
    };
    Some((c(0)?, c(2)?, c(4)?))
}

#[allow(clippy::too_many_arguments)]
fn try_finalize_pdf(
    path: &Path,
    num_articles: usize,
    page_w: f32,
    page_h: f32,
    paper_hex: &str,
    nav_bg_hex: &str,
    nav_fg_hex: &str,
    embedded: &mut crate::manifest::EmbeddedManifest,
) -> anyhow::Result<()> {
    use crate::manifest::{LabelRect, ManifestRect, PageRange};

    let mut doc = Document::load(path)?;

    // BTreeMap is ordered by page number, so into_values() yields page order.
    let pages: Vec<ObjectId> = doc.get_pages().into_values().collect();
    if pages.is_empty() {
        return Ok(());
    }

    // Full-bleed paper background, behind everything, on EVERY page. fulgur only
    // fills the content box (leaving the margins the default colour, so the page
    // looks like a tinted box in a neutral frame); prepend a page-sized fill in
    // the theme's paper colour so the whole sheet is one colour.
    let (pr, pg, pb) = hex_rgb(paper_hex).unwrap_or((0.95, 0.95, 0.93));
    let bg_fill = format!("q {pr:.3} {pg:.3} {pb:.3} rg 0 0 {page_w:.2} {page_h:.2} re f Q\n");
    for &page_id in &pages {
        let sid = doc.add_object(Object::Stream(Stream::new(
            dictionary! {},
            bg_fill.clone().into_bytes(),
        )));
        prepend_content_stream(&mut doc, page_id, sid)?;
    }

    // The nav bar only applies when there are article pages; save the background
    // work either way.
    if num_articles == 0 {
        crate::embed::write(&mut doc, embedded)?;
        doc.save(path)?;
        return Ok(());
    }
    // ref -> page index, for resolving a /Dest target page to its order.
    let page_index: HashMap<ObjectId, usize> =
        pages.iter().enumerate().map(|(i, &id)| (id, i)).collect();

    // Article start page indices: the first `num_articles` /Link annotations in
    // page order are the index rows, each /Dest pointing at an article's first
    // page. Walk pages, resolve those dests to page indices.
    let mut starts: Vec<usize> = Vec::with_capacity(num_articles);
    'outer: for &pid in &pages {
        for annot in page_link_annots(&doc, pid) {
            if let Some(target) = link_dest_page(&doc, annot) {
                if let Some(&idx) = page_index.get(&target) {
                    starts.push(idx);
                    if starts.len() == num_articles {
                        break 'outer;
                    }
                }
            }
        }
    }
    if starts.is_empty() {
        crate::embed::write(&mut doc, embedded)?;
        doc.save(path)?;
        return Ok(());
    }
    let first_article = starts[0];

    // Fill per-doc page ranges: doc i spans [starts[i], starts[i+1]-1] (or last page).
    let total_pages = pages.len();
    for (i, &first) in starts.iter().enumerate() {
        let last = starts.get(i + 1).map(|&s| s - 1).unwrap_or(total_pages - 1);
        if let Some(doc_entry) = embedded.docs.get_mut(i) {
            doc_entry.page_range = PageRange { first, last };
        }
    }

    // Compute the 4 label column rects (PDF points, bottom-left origin).
    // The label band sits just below the nav bar: y ∈ [page_h-86, page_h-62].
    // Page width is split into 4 equal columns.
    let label_kinds = ["inbox", "archive", "later", "delete"];
    let label_band_y0 = f64::from(page_h) - 86.0;
    let label_band_y1 = f64::from(page_h) - 62.0;
    let pw = f64::from(page_w);
    embedded.label_rects = label_kinds
        .iter()
        .enumerate()
        .map(|(i, &kind)| LabelRect {
            kind: kind.to_string(),
            rect: ManifestRect {
                x0: pw * i as f64 / 4.0,
                y0: label_band_y0,
                x1: pw * (i + 1) as f64 / 4.0,
                y1: label_band_y1,
            },
        })
        .collect();

    // One shared Helvetica font for all nav labels.
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });

    let home = pages[0];
    let home_x = page_w * 0.5 - 14.0; // ~"Home" centered at midpage
    let next_x = page_w * 0.80;
    // A filled nav bar in the top band — below the ~36pt the device toolbar overlays
    // and above the content (which starts at the @page top margin, 92pt). The device's
    // transient page-indicator toolbar covers the BOTTOM, so the nav goes up top.
    let (br, bg, bb) = hex_rgb(nav_bg_hex).unwrap_or((0.16, 0.18, 0.40));
    let (fr, fg, fb) = hex_rgb(nav_fg_hex).unwrap_or((0.96, 0.95, 0.91));
    let bar_x = 0.0_f32; // full-bleed bar (100% width)
    let bar_w = page_w;
    let bar_y = page_h - 58.0; // bottom edge of the nav bar
    let bar_h = 21.0_f32;
    let baseline_y = bar_y + 6.5; // nav text baseline, centred in the nav bar

    // Label band geometry (PDF bottom-left origin; y increases upward).
    // The band is just below the nav bar: bottom at page_h-86, top at page_h-62.
    let label_bar_y = page_h - 86.0; // bottom edge of label band
    let label_bar_h = 24.0_f32; // height of label band (86-62 = 24)
    let label_baseline_y = page_h - 80.0; // text baseline ~6pt above label band bottom
    let label_font_size = 7.5_f32;

    for pi in first_article..pages.len() {
        let page_id = pages[pi];
        // Which article does this page belong to? rposition: last start <= pi.
        let ai = starts.iter().rposition(|&s| s <= pi).unwrap_or(0);
        let prev = (ai > 0).then(|| pages[starts[ai - 1]]);
        let next = (ai + 1 < starts.len()).then(|| pages[starts[ai + 1]]);

        ensure_nav_font(&mut doc, page_id, font_id)?;

        // --- draw the nav bar and label band (after existing content, on top) ---
        let mut content = String::new();

        // Full-width filled nav bar.
        content.push_str(&format!(
            "q {br:.3} {bg:.3} {bb:.3} rg {bar_x:.2} {bar_y:.2} {bar_w:.2} {bar_h:.2} re f Q\n"
        ));

        // Nav text labels.
        if prev.is_some() {
            content.push_str(&format!(
                "q {fr:.3} {fg:.3} {fb:.3} rg BT /NAVF 8.5 Tf 16 {baseline_y:.2} Td (< Prev) Tj ET Q\n"
            ));
        }
        content.push_str(&format!(
            "q {fr:.3} {fg:.3} {fb:.3} rg BT /NAVF 8.5 Tf {home_x:.2} {baseline_y:.2} Td (Home) Tj ET Q\n"
        ));
        if next.is_some() {
            content.push_str(&format!(
                "q {fr:.3} {fg:.3} {fb:.3} rg BT /NAVF 8.5 Tf {next_x:.2} {baseline_y:.2} Td (Next >) Tj ET Q\n"
            ));
        }

        // Label band background (same nav colour so it reads as a contiguous header).
        content.push_str(&format!(
            "q {br:.3} {bg:.3} {bb:.3} rg {bar_x:.2} {label_bar_y:.2} {bar_w:.2} {label_bar_h:.2} re f Q\n"
        ));

        // Four label words, one per column, centred horizontally in each column.
        // Helvetica width ≈ 0.5 * font_size * char_count (good enough for centering).
        for (i, &kind) in label_kinds.iter().enumerate() {
            let col_w = page_w / 4.0;
            let col_x = col_w * i as f32;
            // Capitalise the label so it reads clearly on a small device.
            let label = kind.to_uppercase();
            let text_w = 0.5 * label_font_size * label.len() as f32;
            let text_x = col_x + (col_w - text_w) / 2.0;
            content.push_str(&format!(
                "q {fr:.3} {fg:.3} {fb:.3} rg BT /NAVF {label_font_size:.1} Tf {text_x:.2} {label_baseline_y:.2} Td ({label}) Tj ET Q\n"
            ));
        }

        let stream_id = doc.add_object(Object::Stream(Stream::new(
            dictionary! {},
            content.into_bytes(),
        )));
        append_content_stream(&mut doc, page_id, stream_id)?;

        // --- clickable tap-targets (Home always; Prev/Next only when present) ---
        let mut new_annots: Vec<Object> = Vec::new();
        if let Some(target) = prev {
            new_annots.push(Object::Reference(doc.add_object(link_annot(
                target,
                page_w,
                page_h,
                NavSlot::Prev,
            ))));
        }
        new_annots.push(Object::Reference(doc.add_object(link_annot(
            home,
            page_w,
            page_h,
            NavSlot::Home,
        ))));
        if let Some(target) = next {
            new_annots.push(Object::Reference(doc.add_object(link_annot(
                target,
                page_w,
                page_h,
                NavSlot::Next,
            ))));
        }
        append_annots(&mut doc, page_id, new_annots)?;
    }

    // Embed the populated manifest before saving.
    crate::embed::write(&mut doc, embedded)?;
    doc.save(path)?;
    Ok(())
}

enum NavSlot {
    Prev,
    Home,
    Next,
}

/// Build a /Link annotation covering one nav slot's tap band (`y0=8 .. y1=34`);
/// rects are derived from the slot + page_w so geometry lives in one place.
fn link_annot(target: ObjectId, page_w: f32, page_h: f32, slot: NavSlot) -> Dictionary {
    let (x0, x1) = match slot {
        NavSlot::Prev => (0.0, page_w * 0.34),
        NavSlot::Home => (page_w * 0.36, page_w * 0.64),
        NavSlot::Next => (page_w * 0.66, page_w),
    };
    // Top nav band: just below the toolbar, above the content margin.
    let (y0, y1) = (page_h - 58.0, page_h - 38.0);
    dictionary! {
        "Type" => "Annot",
        "Subtype" => "Link",
        "Rect" => vec![x0.into(), y0.into(), x1.into(), y1.into()],
        "Border" => vec![0.into(), 0.into(), 0.into()],
        "Dest" => vec![
            Object::Reference(target),
            "XYZ".into(),
            Object::Null,
            Object::Null,
            Object::Null,
        ],
    }
}

/// All /Link annotation dictionaries on a page (resolving the /Annots ref/array).
fn page_link_annots(doc: &Document, page_id: ObjectId) -> Vec<&Dictionary> {
    let mut out = Vec::new();
    let Ok(page) = doc.get_dictionary(page_id) else {
        return out;
    };
    let Ok(annots_obj) = page.get(b"Annots") else {
        return out;
    };
    let arr = match resolve_array(doc, annots_obj) {
        Some(a) => a,
        None => return out,
    };
    for item in arr {
        let annot = match item {
            Object::Reference(id) => doc.get_dictionary(*id).ok(),
            Object::Dictionary(d) => Some(d),
            _ => None,
        };
        if let Some(d) = annot {
            if d.get(b"Subtype").ok().and_then(|s| s.as_name().ok()) == Some(b"Link") {
                out.push(d);
            }
        }
    }
    out
}

/// If the annotation is a /Link with an explicit destination, return the target
/// page's ObjectId. fulgur stores `/Dest` as a *reference* to a destination
/// array `[pageRef /XYZ x y z]`, so we resolve the ref then read the array's
/// first element. An inline `/Dest [pageRef ...]` array is also handled.
fn link_dest_page(doc: &Document, annot: &Dictionary) -> Option<ObjectId> {
    let dest = annot.get(b"Dest").ok()?;
    let arr = resolve_array(doc, dest)?;
    match arr.first()? {
        Object::Reference(id) => Some(*id),
        _ => None,
    }
}

/// Resolve an object that is either an inline array or a reference to one.
fn resolve_array<'a>(doc: &'a Document, obj: &'a Object) -> Option<&'a Vec<Object>> {
    match obj {
        Object::Array(a) => Some(a),
        Object::Reference(id) => doc.get_object(*id).ok().and_then(|o| o.as_array().ok()),
        _ => None,
    }
}

/// Ensure the page's /Resources /Font dict maps /NAVF -> font_id, writing
/// /Resources back as an inline dict so we never mutate shared inherited
/// resources from the Pages tree.
fn ensure_nav_font(doc: &mut Document, page_id: ObjectId, font_id: ObjectId) -> anyhow::Result<()> {
    // Clone the page's own /Resources (inline or via ref) if present; else fresh.
    let mut resources: Dictionary = {
        let page = doc.get_dictionary(page_id)?;
        match page.get(b"Resources") {
            Ok(Object::Dictionary(d)) => d.clone(),
            Ok(Object::Reference(id)) => doc.get_dictionary(*id).cloned().unwrap_or_default(),
            _ => Dictionary::new(),
        }
    };

    // Ensure /Font subdict, resolving a referenced one to inline first.
    let mut fonts: Dictionary = match resources.get(b"Font") {
        Ok(Object::Dictionary(d)) => d.clone(),
        Ok(Object::Reference(id)) => doc.get_dictionary(*id).cloned().unwrap_or_default(),
        _ => Dictionary::new(),
    };
    fonts.set("NAVF", Object::Reference(font_id));
    resources.set("Font", Object::Dictionary(fonts));

    let page = doc.get_dictionary_mut(page_id)?;
    page.set("Resources", Object::Dictionary(resources));
    Ok(())
}

/// Append a content stream ref to the page's /Contents (single ref -> array, or
/// push onto an existing array).
fn append_content_stream(
    doc: &mut Document,
    page_id: ObjectId,
    stream_id: ObjectId,
) -> anyhow::Result<()> {
    let page = doc.get_dictionary_mut(page_id)?;
    match page.get(b"Contents").ok().cloned() {
        Some(Object::Reference(old)) => {
            page.set(
                "Contents",
                Object::Array(vec![Object::Reference(old), Object::Reference(stream_id)]),
            );
        }
        Some(Object::Array(mut arr)) => {
            arr.push(Object::Reference(stream_id));
            page.set("Contents", Object::Array(arr));
        }
        _ => {
            page.set(
                "Contents",
                Object::Array(vec![Object::Reference(stream_id)]),
            );
        }
    }
    Ok(())
}

/// Prepend a content stream ref to the page's /Contents so it draws *behind*
/// existing content (used for the full-page background fill).
fn prepend_content_stream(
    doc: &mut Document,
    page_id: ObjectId,
    stream_id: ObjectId,
) -> anyhow::Result<()> {
    let page = doc.get_dictionary_mut(page_id)?;
    let mut arr = match page.get(b"Contents").ok().cloned() {
        Some(Object::Reference(old)) => vec![Object::Reference(old)],
        Some(Object::Array(a)) => a,
        _ => Vec::new(),
    };
    arr.insert(0, Object::Reference(stream_id));
    page.set("Contents", Object::Array(arr));
    Ok(())
}

/// Normalize the page's /Annots to an inline array and append the new annots.
fn append_annots(
    doc: &mut Document,
    page_id: ObjectId,
    mut new_annots: Vec<Object>,
) -> anyhow::Result<()> {
    // Resolve any referenced array to a concrete Vec before mutably borrowing.
    let mut existing: Vec<Object> = {
        let page = doc.get_dictionary(page_id)?;
        match page.get(b"Annots") {
            Ok(Object::Array(a)) => a.clone(),
            Ok(Object::Reference(id)) => doc
                .get_object(*id)
                .ok()
                .and_then(|o| o.as_array().ok())
                .cloned()
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    };
    existing.append(&mut new_annots);
    let page = doc.get_dictionary_mut(page_id)?;
    page.set("Annots", Object::Array(existing));
    Ok(())
}
