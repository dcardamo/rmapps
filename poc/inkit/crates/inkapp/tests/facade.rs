use inkapp::components::checkbox::Checkbox;
use inkapp::Remarkable;

#[derive(Clone)]
enum Msg {
    Mark,
}

#[test]
fn surface_resolves() {
    let _cb = Checkbox::with_msg("done", Msg::Mark);
    let _dev = Remarkable::new();
    // `app` is callable (builder entry point).
    let _ = inkapp::app(()); // model = unit
}
