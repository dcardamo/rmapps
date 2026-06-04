//! Build the Typst preamble (colours, fonts, dot-grid background, cover
//! gradient, page wrappers, badge/swatch helpers) shared by every page, plus the
//! markup-escaping helpers the per-page emitters in `crate::templates` use.
//!
//! All sizing lives here as `#let`s derived from the device + grid, so the page
//! emitters stay context-free (they just call the helpers by name) — the same
//! split the old fulgur CSS had (one stylesheet, context-free HTML fragments).

use crate::device::Device;
use crate::geometry::{GridSpec, TOOLBAR_SAFE_PT};
use crate::theme::Palette;

/// Escape arbitrary text for Typst *markup* (content mode). Backslash and the
/// markup-significant characters are escaped so titles/locations/descriptions
/// render verbatim.
pub fn esc_markup(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' | '#' | '$' | '*' | '_' | '`' | '<' | '>' | '@' | '=' | '~' | '"' | '\'' | '['
            | ']' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

/// Escape a string for a Typst double-quoted string literal (label names, etc.).
pub fn esc_str(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn hex(theme: &Palette, key: &str, fallback: &str) -> String {
    theme
        .get(key)
        .cloned()
        .unwrap_or_else(|| fallback.to_string())
}

/// The complete Typst preamble: `#let` bindings + `#set` rules. The page emitters
/// concatenated after it reference these helpers.
pub fn build_preamble(device: &Device, grid: &GridSpec, theme: &Palette) -> String {
    let w = device.width_pt();
    let h = device.height_pt();
    let sp = grid.spacing_pt;
    let m = grid.margin_pt;
    let top = TOOLBAR_SAFE_PT;

    // Dot-page font sizes scale with the pitch so single-line labels always fit
    // between dot rows at any configured spacing (matches the old CSS).
    let head_fs = 1.25 * sp;
    let num_fs = 0.8 * sp;
    let wd_fs = 0.62 * sp;
    let half_sp = 0.5 * sp;

    // Dot phase: align to the device "Dots Small" template so on-device inserted
    // Dots-Small pages line up with ours. The tile draws the dot at its centre, so
    // offset the whole field by (target_phase - sp/2). Target phase (2.22, -0.04)
    // pt from the page corner mod pitch is the same value the fulgur CSS used.
    let dot_dx = 2.22 - sp / 2.0;
    let dot_dy = -0.04 - sp / 2.0;

    // Cover gradient ran corner-to-corner (CSS 0,0 -> 100%,100%); the matching
    // Typst angle (0deg = east, 90deg = south, y down) is atan2(h, w).
    let cover_angle = (h as f64).atan2(w as f64).to_degrees();

    // Theme colours as a dict (swatch lookup by name) + named bindings with the
    // library defaults as fallback.
    let mut theme_dict = String::from("(");
    for (k, v) in theme {
        theme_dict.push_str(&format!("\"{}\": rgb(\"{}\"), ", esc_str(k), esc_str(v)));
    }
    theme_dict.push(')');

    let paper = hex(theme, "paper", "#F3F1EA");
    let ink = hex(theme, "ink", "#1A1A18");
    let primary = hex(theme, "primary", "#2A2F6B");
    let accent = hex(theme, "accent", "#CF3A2B");
    let muted = hex(theme, "muted", "#5E6166");
    let rule = hex(theme, "rule", "#E0DDD2");
    let nav = hex(theme, "nav", "#F4F1E8");
    let dot = hex(theme, "dot", "#55534C");
    let cover_to = hex(theme, "cover_to", "#1A1E48");

    format!(
        r#"#let theme-col = {theme_dict}
#let paper = rgb("{paper}")
#let ink = rgb("{ink}")
#let primary = rgb("{primary}")
#let accent = rgb("{accent}")
#let muted = rgb("{muted}")
#let rule-col = rgb("{rule}")
#let nav = rgb("{nav}")
#let dotcol = rgb("{dot}")
#let cover-to = rgb("{cover_to}")

#let page-w = {w}pt
#let page-h = {h}pt
#let sp = {sp}pt
#let margin-pt = {m}pt
#let toolbar-pt = {top}pt
#let head-fs = {head_fs}pt
#let num-fs = {num_fs}pt
#let wd-fs = {wd_fs}pt
#let half-sp = {half_sp}pt

// A block-relative dot tiling. Used only for the Future Log's month blocks,
// whose dot field should restart at each block's top edge (block-relative phase)
// — there's no row content there to misalign, so a tiling's pixel-quantised step
// is harmless.
#let dot-tile = tiling(size: (sp, sp))[
  #place(dx: sp / 2 - 0.5pt, dy: sp / 2 - 0.5pt,
    circle(radius: 0.5pt, fill: dotcol, stroke: none))
]

// Page dot-grid background, device-aligned. Dots are placed as explicit vector
// circles (NOT a `tiling`): a tiling pattern's tile period gets quantised to
// whole device pixels by PDF rasterisers (poppler, and the reMarkable), so its
// effective pitch drifts from `sp` — vector content placed at the true `sp`
// pitch (the monthly day rows) then walks off the dots down the page. Placing
// each dot individually keeps every dot at its true coordinate, so the grid and
// any sp-pitched content stay aligned in every renderer. Dot centres sit at
// (2.22, -0.04) + (i, j)·sp pt from the page corner — the device "Dots Small"
// phase the old tiling used.
#let dot-cols = calc.ceil((page-w + sp) / sp)
#let dot-rows = calc.ceil((page-h + sp) / sp)
#let dot-bg = place(top + left, dx: {dot_dx}pt, dy: {dot_dy}pt, {{
  for j in range(dot-rows) {{
    for i in range(dot-cols) {{
      place(dx: i * sp + sp / 2 - 0.5pt, dy: j * sp + sp / 2 - 0.5pt,
        circle(radius: 0.5pt, fill: dotcol, stroke: none))
    }}
  }}
}})

#let cover-grad = gradient.linear(angle: {cover_angle}deg, primary, cover-to)

// Page wrappers — one per fragment. Defaults (size, paper fill, toolbar-reserve
// top margin) come from the #set page below; each wrapper overrides only what it
// needs.
#let plain-page(body) = page(body)
// Dot-grid pages render on pure white (not the warm editorial `paper`): the grey
// dots read with much more contrast on white, so the grid stays easy to follow.
#let dot-page(body) = page(fill: white, background: dot-bg, body)
#let cover-page(body) = page(fill: cover-grad, margin: margin-pt, body)

// The month index drops the top/bottom margins so its rows can be `place`d on the
// real dot-row centres (absolute page Y) — the day list aligns to the grid instead
// of flowing. The masthead is placed by hand into the top toolbar-reserve band.
#let month-page(body) = page(
  fill: white,
  background: dot-bg,
  margin: (top: 0pt, bottom: 0pt, left: margin-pt, right: margin-pt),
  body,
)

// Red count badge: a circle for one digit, a pill for two. Links to its target.
// `h` is the badge height (the monthly index passes a smaller value so a badge
// fits inside one dot cell); the digit scales with it (13pt → 8pt by default).
#let cbadge(n, target, h: 13pt) = link(target, box(
  fill: accent, height: h, radius: h / 2, inset: (x: 4pt), outset: (y: 0pt),
  align(center + horizon,
    text(font: "Hanken Grotesk", size: h * 8 / 13, weight: 700, fill: paper, [#n]))))

// 7pt rounded colour swatch for a calendar event, keyed by theme colour name.
#let swatch(name) = box(
  width: 7pt, height: 7pt, radius: 2pt, baseline: 0.5pt,
  fill: theme-col.at(name, default: accent))

#set page(
  width: page-w, height: page-h,
  margin: (top: toolbar-pt, left: margin-pt, right: margin-pt, bottom: margin-pt),
  fill: paper,
)
#set text(font: "Lora", size: 9.5pt, fill: ink, lang: "en", hyphenate: false)
#set par(leading: 0.5em, spacing: 0.6em, justify: false)
"#,
    )
}
