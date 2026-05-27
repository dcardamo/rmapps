pub mod action_band;
pub mod calendar_view;
pub mod checkbox;
pub mod gesture;
pub mod heading;
pub mod highlight_text;
pub mod index;
pub mod notice;
pub mod passage;
pub mod section;
pub mod stepper;

use crate::ink::RegionInk;
use crate::manifest::Manifest;

/// Escape a string for a Typst string literal (`"..."`): only `\` and `"` need
/// escaping — other markup chars (`[`, `]`, `#`) are literal inside a string.
pub fn esc_typst_str(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Emit one inline `#box`ed token whose laid-out rect recovers as region
/// `tok-<index>`. `t_let_expr` is the Typst expression bound to `t` and used for
/// BOTH `measure(t)` (region size) and display, so inline styling
/// (`strong("x")`, `emph("x")`, `raw("x")`, `underline("x")`, or a plain
/// `"x"`) measures and renders correctly. When `highlighted`, the token renders
/// pre-marked via `#highlight`. This is the span-level recipe proven in Typst
/// 0.14.2.
pub fn token_region(index: usize, t_let_expr: &str, highlighted: bool) -> String {
    let disp = if highlighted { "#highlight[#t]" } else { "#t" };
    format!(
        "#box[#let t = {t_let_expr}; #context [#metadata((name: \"tok-{index}\", \
           page: here().position().page - 1, x: here().position().x / 1pt, \
           y: here().position().y / 1pt, w: measure(t).width / 1pt, \
           h: measure(t).height / 1pt)) <region>]{disp}] "
    )
}

/// Indices `0..n` of tokens whose `tok-<i>` region was overlapped by a
/// highlighter stroke. Only highlighter strokes count; a stroke matches when its
/// bbox overlaps the region rect. Ascending order.
pub fn highlighted_token_indices(n: usize, ink: &[RegionInk], manifest: &Manifest) -> Vec<usize> {
    (0..n)
        .filter(|i| {
            let name = format!("tok-{i}");
            let Some(region) = manifest.regions.iter().find(|r| r.name == name) else {
                return false;
            };
            ink.iter()
                .filter(|ri| ri.region == name)
                .flat_map(|ri| &ri.strokes)
                .filter(|s| s.highlighter)
                .any(|s| s.bbox().is_some_and(|b| region.rect.overlaps(&b)))
        })
        .collect()
}
