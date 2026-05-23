use inkapp_core::component::Component;
use inkapp_core::document::Document;
use inkapp_core::embed::extract_manifest;
use inkapp_core::flow;
use inkapp_core::ink::RegionInk;
use inkapp_core::manifest::Manifest;
use inkapp_core::runtime::render_document;
use inkapp_core::widget::{RenderCx, Widget};
use inkapp_core::widgets::checkbox::Checkbox;
use inkapp_core::widgets::highlight_text::HighlightableText;

#[derive(Clone, PartialEq, Eq, Debug)]
enum Msg {
    Archive,
}

/// A tiny body component wrapping HighlightableText, decoding to no messages
/// (this test only exercises render).
struct Body(HighlightableText);
impl Component for Body {
    type Msg = Msg;
    fn render(&self, cx: &mut RenderCx) -> String {
        Widget::render(&self.0, cx)
    }
    fn decode(&self, _ink: &[RegionInk], _m: &Manifest) -> Vec<Msg> {
        vec![]
    }
}

fn doc() -> Document<Msg> {
    Document::keyed(
        "article-1",
        flow![
            Body(HighlightableText::new(&["lazy", "dog"])),
            Checkbox::with_msg("done", Msg::Archive).label("Archive"),
        ],
    )
}

#[test]
fn renders_expected_regions() {
    let rd = render_document(&doc(), 1).unwrap();
    let m = extract_manifest(&rd.pdf).unwrap();
    assert_eq!(m.version, 1);
    assert!(m.regions.iter().any(|r| r.name == "tok-0"));
    assert!(m.regions.iter().any(|r| r.name == "tok-1"));
    assert!(m.regions.iter().any(|r| r.name == "done"));
}

#[test]
fn render_is_deterministic() {
    let a = render_document(&doc(), 1).unwrap();
    let b = render_document(&doc(), 1).unwrap();
    assert_eq!(a.hash, b.hash, "same doc -> same source hash");
    assert_eq!(a.manifest, b.manifest, "same doc -> same manifest");
}
