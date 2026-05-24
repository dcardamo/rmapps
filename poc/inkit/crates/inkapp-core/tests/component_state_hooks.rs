use inkapp_core::component::Component;
use inkapp_core::components::notice::Notice;
use inkapp_core::ink::RegionInk;
use inkapp_core::manifest::Manifest;
use inkapp_core::component::RenderCx;
use serde_json::json;

// A minimal stateful component used only to exercise the new hooks.
struct Stateful;
impl Component for Stateful {
    type Msg = ();
    fn render(&self, _cx: &mut RenderCx) -> String {
        String::new()
    }
    fn decode(&self, _ink: &[RegionInk], _m: &Manifest) -> Vec<()> {
        vec![]
    }
    fn state_key(&self) -> Option<String> {
        Some("k".into())
    }
    fn render_state(&self) -> Option<serde_json::Value> {
        Some(json!(42))
    }
}

#[test]
fn stateless_component_has_no_state() {
    let n: Notice<()> = Notice::line("hi");
    assert_eq!(n.state_key(), None);
    assert_eq!(n.render_state(), None);
}

#[test]
fn stateful_component_reports_state() {
    let s = Stateful;
    assert_eq!(s.state_key(), Some("k".to_string()));
    assert_eq!(s.render_state(), Some(json!(42)));
}
