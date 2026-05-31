use inkapp_core::components::checkbox::Checkbox;
use inkapp_core::document::{DocKey, Document, Documents};
use inkapp_core::flow;

#[derive(Clone, PartialEq, Eq, Debug)]
enum Msg {
    A,
    B,
}

#[test]
fn build_heterogeneous_flow() {
    // Two checkboxes carrying different messages, same Msg type -> one flow.
    let doc: Document<Msg> = Document::keyed(
        "k1",
        flow![
            Checkbox::with_msg("a", Msg::A),
            Checkbox::with_msg("b", Msg::B),
        ],
    );
    assert_eq!(doc.key, DocKey::new("k1"));
    assert_eq!(doc.flow.len(), 2);

    let docs: Documents<Msg> = Documents(vec![doc]);
    assert_eq!(docs.0.len(), 1);
}

#[test]
fn keyed_has_no_state_keyed_with_state_does() {
    use inkapp_core::flow;
    use serde_json::json;
    let plain: inkapp_core::document::Document<()> = Document::keyed("k", flow![]);
    assert_eq!(plain.state, None);
    let stateful: inkapp_core::document::Document<()> =
        Document::keyed_with_state("k", flow![], json!({"cursor": 1}));
    assert_eq!(stateful.state, Some(json!({"cursor": 1})));
}
