use rmreader::manifest::{EmbeddedDoc, EmbeddedManifest, PageRange};
use rmreader::readback::{classify, PageHighlight};
use rmreader::readwise::ActionKind;

fn manifest() -> EmbeddedManifest {
    EmbeddedManifest {
        schema_version: 1,
        collection: "Library".into(),
        docs: vec![
            EmbeddedDoc {
                id: "d1".into(),
                title: "One".into(),
                url: "https://a".into(),
                author: "A".into(),
                category: "articles".into(),
                page_range: PageRange { first: 0, last: 1 },
            },
            EmbeddedDoc {
                id: "d2".into(),
                title: "Two".into(),
                url: "https://b".into(),
                author: "B".into(),
                category: "articles".into(),
                page_range: PageRange { first: 2, last: 2 },
            },
        ],
        label_rects: Vec::new(),
    }
}

fn h(page: usize, text: &str, top: bool) -> PageHighlight {
    PageHighlight {
        page,
        text: text.into(),
        top_band: top,
    }
}

#[test]
fn label_in_top_band_becomes_action() {
    let p = classify(&manifest(), &[h(1, "ARCHIVE", true)]);
    assert_eq!(p.actions, vec![("d1".into(), ActionKind::Archive)]);
    assert!(p.highlights.is_empty());
}

#[test]
fn body_highlight_becomes_content() {
    let p = classify(&manifest(), &[h(2, "a great sentence", false)]);
    assert!(p.actions.is_empty());
    assert_eq!(p.highlights.len(), 1);
    assert_eq!(p.highlights[0].source_url, "https://b");
    assert_eq!(p.highlights[0].text, "a great sentence");
}

#[test]
fn label_word_in_body_not_top_band_is_content() {
    let p = classify(&manifest(), &[h(0, "archive", false)]);
    assert!(p.actions.is_empty());
    assert_eq!(p.highlights.len(), 1);
}

#[test]
fn two_actions_skip_with_warning() {
    let p = classify(&manifest(), &[h(0, "ARCHIVE", true), h(1, "DELETE", true)]);
    assert!(p.actions.is_empty());
    assert_eq!(p.warnings.len(), 1);
}

#[test]
fn duplicate_same_action_applies_once() {
    let p = classify(&manifest(), &[h(0, "ARCHIVE", true), h(1, "archive", true)]);
    assert_eq!(p.actions, vec![("d1".into(), ActionKind::Archive)]);
}

#[test]
fn highlight_off_manifest_warns() {
    let p = classify(&manifest(), &[h(99, "x", false)]);
    assert!(p.actions.is_empty() && p.highlights.is_empty());
    assert_eq!(p.warnings.len(), 1);
}
