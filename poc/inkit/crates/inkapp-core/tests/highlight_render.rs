use inkapp_core::component::RenderCx;
use inkapp_core::components::highlight_text::HighlightableText;

#[test]
fn highlighted_token_is_marked_in_render() {
    let w = HighlightableText::with_highlights(&["alpha", "beta", "gamma"], &["beta".to_string()]);
    let mut cx = RenderCx::new(0);
    let src = w.render(&mut cx);
    assert!(
        src.contains("#highlight"),
        "a highlighted token renders with #highlight"
    );
    assert_eq!(
        src.matches("#highlight[").count(),
        1,
        "exactly one token highlighted, not all"
    );
    // Plain render (no highlights) must NOT contain #highlight.
    let plain = HighlightableText::new(&["alpha", "beta"]).render(&mut RenderCx::new(0));
    assert!(
        !plain.contains("#highlight"),
        "plain text has no highlight markup"
    );
}
