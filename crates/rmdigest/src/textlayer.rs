//! NOTE: deliberate twin of rmreader/src/readback/textlayer.rs (leaf utility; see plan sharing decision).
//! Extract the PDF text layer (per-page word boxes) so a highlight stroke's region
//! can be turned back into the words it covers. Coordinates are PDF points,
//! origin BOTTOM-LEFT (matching rmfiles::coords).
use std::io::Write as IoWrite;

use rmfiles::coords::PdfRect;

#[derive(Debug, Clone)]
pub struct Word {
    pub page: usize,
    pub text: String,
    pub bbox: PdfRect,
}

#[derive(Debug, Default)]
pub struct TextLayer {
    pub words: Vec<Word>,
}

impl TextLayer {
    /// Run `pdftotext -bbox` on the given PDF bytes and parse word boxes.
    /// The returned coordinates are PDF points, origin BOTTOM-LEFT (y up),
    /// matching `rmfiles::coords::PdfRect`.
    pub fn extract(pdf: &[u8]) -> anyhow::Result<TextLayer> {
        // Write to a temp file with a .pdf extension so pdftotext recognises it.
        let mut tmp = tempfile::Builder::new()
            .prefix("rmdigest-tl-")
            .suffix(".pdf")
            .tempfile()?;
        tmp.write_all(pdf)?;
        let path = tmp.path().to_owned();

        // pdftotext -bbox <file> - writes XHTML to stdout.
        let out = std::process::Command::new("pdftotext")
            .args(["-bbox", path.to_str().unwrap_or(""), "-"])
            .output()
            .map_err(|e| anyhow::anyhow!("pdftotext not found: {e}"))?;

        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            anyhow::bail!("pdftotext failed ({}): {stderr}", out.status);
        }

        let xhtml = String::from_utf8_lossy(&out.stdout);
        parse_xhtml(&xhtml)
    }

    /// Return the text whose word boxes overlap `rect` on `page` (0-based),
    /// joined in reading order (top-to-bottom, left-to-right).
    pub fn words_under(&self, page: usize, rect: &PdfRect) -> String {
        let mut matched: Vec<&Word> = self
            .words
            .iter()
            .filter(|w| w.page == page && w.bbox.intersects(rect))
            .collect();

        // Reading order: descending y (top of page first, since y is bottom-left),
        // then ascending x for words on the same line.
        matched.sort_by(|a, b| {
            b.bbox
                .y0
                .partial_cmp(&a.bbox.y0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(
                    a.bbox
                        .x0
                        .partial_cmp(&b.bbox.x0)
                        .unwrap_or(std::cmp::Ordering::Equal),
                )
        });

        // fulgur emits whole sentences as single text runs, so pdftotext can report
        // the same run text for each wrapped visual line; collapse consecutive
        // identical texts so a single highlight doesn't repeat the sentence.
        let mut texts: Vec<&str> = matched.iter().map(|w| w.text.as_str()).collect();
        texts.dedup();
        texts.join(" ").trim().to_string()
    }
}

/// Parse the XHTML output of `pdftotext -bbox`.
///
/// Format (one page per `<page>` element, words inside):
/// ```xml
/// <page width="595.0" height="842.0">
///   <word xMin="50.0" yMin="40.5" xMax="99.8" yMax="55.3">INBOX</word>
/// ```
/// pdftotext uses TOP-LEFT origin (y increases downward). We flip to BOTTOM-LEFT:
///   y0_bl = height - yMax_tl
///   y1_bl = height - yMin_tl
fn parse_xhtml(xhtml: &str) -> anyhow::Result<TextLayer> {
    let mut words = Vec::new();
    let mut rest = xhtml;
    let mut page_index: usize = 0;

    while let Some(p) = rest.find("<page ") {
        rest = &rest[p..];

        // Extract the page tag's attribute region (everything up to '>').
        let tag_end = rest.find('>').unwrap_or(rest.len());
        let page_tag = &rest[..tag_end];

        let page_h = parse_attr_f64(page_tag, "height").unwrap_or(0.0);

        // Find the closing </page> to bound word scanning.
        let close = rest.find("</page>").unwrap_or(rest.len());
        let page_body = &rest[tag_end..close];

        parse_words(page_body, page_index, page_h, &mut words);

        rest = &rest[close..];
        page_index += 1;
    }

    Ok(TextLayer { words })
}

/// Parse all `<word ...>TEXT</word>` tags within a page's body.
fn parse_words(body: &str, page: usize, page_h: f64, words: &mut Vec<Word>) {
    let mut rest = body;
    while let Some(start) = rest.find("<word ") {
        rest = &rest[start..];
        let tag_end = match rest.find('>') {
            Some(i) => i,
            None => break,
        };
        let word_tag = &rest[..tag_end];

        let x_min = parse_attr_f64(word_tag, "xMin").unwrap_or(0.0);
        let y_min = parse_attr_f64(word_tag, "yMin").unwrap_or(0.0);
        let x_max = parse_attr_f64(word_tag, "xMax").unwrap_or(0.0);
        let y_max = parse_attr_f64(word_tag, "yMax").unwrap_or(0.0);

        // Flip y from top-left to bottom-left origin.
        let y0 = page_h - y_max;
        let y1 = page_h - y_min;

        // Extract text content between '>' and '</word>'.
        let after_gt = &rest[tag_end + 1..];
        let close = match after_gt.find("</word>") {
            Some(i) => i,
            None => break,
        };
        let raw_text = &after_gt[..close];
        let text = decode_xml_entities(raw_text);

        if !text.is_empty() {
            words.push(Word {
                page,
                text,
                bbox: PdfRect {
                    x0: x_min,
                    y0,
                    x1: x_max,
                    y1,
                },
            });
        }

        rest = &after_gt[close + 7..]; // 7 == len("</word>")
    }
}

/// Pull the numeric value of a named attribute from a tag string.
/// Handles both `attr="value"` and `attr='value'`.
fn parse_attr_f64(tag: &str, name: &str) -> Option<f64> {
    // Look for `name="` or `name='`.
    let search = format!("{name}=\"");
    let (inner, close_char) = if let Some(i) = tag.find(&search) {
        (&tag[i + search.len()..], '"')
    } else {
        let search2 = format!("{name}='");
        let i = tag.find(&search2)?;
        (&tag[i + search2.len()..], '\'')
    };
    let end = inner.find(close_char)?;
    inner[..end].parse().ok()
}

/// Decode the XML character entities that pdftotext may emit in word text.
fn decode_xml_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let s = s.replace("&amp;", "&");
    let s = s.replace("&lt;", "<");
    let s = s.replace("&gt;", ">");
    let s = s.replace("&quot;", "\"");
    s.replace("&#39;", "'")
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::content::{Content, Operation};
    use lopdf::{dictionary, Document, Object, Stream};

    fn one_word_pdf(word: &str) -> Vec<u8> {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let font_id = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        });
        let resources_id = doc.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font_id },
        });
        let content = Content {
            operations: vec![
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec!["F1".into(), 24.into()]),
                Operation::new("Td", vec![100.into(), 700.into()]),
                Operation::new("Tj", vec![Object::string_literal(word)]),
                Operation::new("ET", vec![]),
            ],
        };
        let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => content_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        });
        let pages = dictionary! {
            "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1,
            "Resources" => resources_id,
        };
        doc.objects.insert(pages_id, Object::Dictionary(pages));
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog", "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog_id);
        let mut buf = Vec::new();
        doc.save_to(&mut buf).unwrap();
        buf
    }

    #[test]
    fn extracts_a_known_word() {
        let pdf = one_word_pdf("HELLO");
        let tl = TextLayer::extract(&pdf).unwrap();
        assert!(
            tl.words.iter().any(|w| w.text.contains("HELLO")),
            "expected HELLO in {:?}",
            tl.words
        );
    }

    #[test]
    fn words_under_matches_overlapping_rect() {
        let pdf = one_word_pdf("HELLO");
        let tl = TextLayer::extract(&pdf).unwrap();
        // The word sits near (100,700) in a 612x792 page (bottom-left origin).
        // A generous rect over that region should capture it.
        let rect = PdfRect {
            x0: 50.0,
            y0: 650.0,
            x1: 300.0,
            y1: 760.0,
        };
        let got = tl.words_under(0, &rect);
        assert!(got.contains("HELLO"), "words_under returned {got:?}");
    }
}
