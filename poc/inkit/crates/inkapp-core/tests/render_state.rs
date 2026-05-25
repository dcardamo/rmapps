use inkapp_core::component::Component;
use inkapp_core::component::RenderCx;
use inkapp_core::components::notice::Notice;
use inkapp_core::crypto::Key;
use inkapp_core::document::Document;
use inkapp_core::flow;
use inkapp_core::ink::RegionInk;
use inkapp_core::manifest::Manifest;
use inkapp_core::runtime::render_document;
use serde_json::json;

// Minimal stateful component: emits no regions, just carries state.
struct Carrier {
    key: String,
    value: u64,
}
impl Component for Carrier {
    type Msg = ();
    fn render(&self, _cx: &mut RenderCx) -> String {
        // A trivial visible glyph so the page is non-empty.
        format!("#text[{}]\n", self.value)
    }
    fn decode(&self, _ink: &[RegionInk], _m: &Manifest) -> Vec<()> {
        vec![]
    }
    fn state_key(&self) -> Option<String> {
        Some(self.key.clone())
    }
    fn render_state(&self) -> Option<serde_json::Value> {
        Some(json!(self.value))
    }
}

#[test]
fn render_collects_doc_and_component_state() {
    let doc: Document<()> = Document::keyed_with_state(
        "d",
        flow![
            Carrier {
                key: "carrier:1".into(),
                value: 5
            },
            Notice::line("note")
        ],
        json!({"cursor": 3}),
    );
    let rd = render_document(&doc, 1, &Key::from_bytes([7u8; 32])).unwrap();
    assert_eq!(rd.manifest.state.doc, Some(json!({"cursor": 3})));
    assert_eq!(
        rd.manifest.state.components.get("carrier:1"),
        Some(&json!(5u64))
    );
    assert_eq!(
        rd.manifest.state.components.len(),
        1,
        "stateless components contribute nothing"
    );
}
