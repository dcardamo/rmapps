//! Assemble HTML + assets and render to PDF via fulgur (Blitz + krilla).

use std::path::Path;

use askama::Template;
use fulgur::asset::AssetBundle;
use fulgur::config::{Margin, PageSize};
use fulgur::engine::Engine;

use crate::device::Device;
use crate::geometry::GridSpec;
use crate::svg;
use crate::templates::Base;
use crate::theme::{css_vars, Palette};

// Re-export inspection for layout tests (Task 10) without a separate dev-dep.
pub use fulgur::inspect::{inspect, InspectResult, TextItem};

const LORA_REGULAR: &[u8] = include_bytes!("../assets/fonts/Lora-Regular.ttf");
const LORA_SEMIBOLD: &[u8] = include_bytes!("../assets/fonts/Lora-SemiBold.ttf");
const FRAUNCES: &[u8] = include_bytes!("../assets/fonts/Fraunces72pt-SemiBold.ttf");
const HANKEN_REGULAR: &[u8] = include_bytes!("../assets/fonts/HankenGrotesk-Regular.ttf");
const HANKEN_MEDIUM: &[u8] = include_bytes!("../assets/fonts/HankenGrotesk-Medium.ttf");
// Lora (body/reading), Fraunces 72pt (display titles), Hanken Grotesk (meta/UI).
const BODY_FAMILY: &str = "Lora";

fn color<'a>(theme: &'a Palette, key: &str, fallback: &'a str) -> &'a str {
    theme.get(key).map(|s| s.as_str()).unwrap_or(fallback)
}

pub fn build_css(device: &Device, grid: &GridSpec, theme: &Palette) -> String {
    let w = device.width_pt();
    let h = device.height_pt();
    let m = grid.margin_pt;
    let sp = grid.spacing_pt;
    // Dot-page font sizes scale with the pitch so single-line labels always fit
    // between dot rows at any configured spacing (~15.9/10.2/7.9pt at 4.5mm).
    let head_fs = 1.25 * sp;
    let num_fs = 0.8 * sp;
    let wd_fs = 0.62 * sp;
    // The month index paints its dot grid on the day LIST, not the page. Dots and
    // rows then live in the same element's coordinate system, so they shift
    // together — alignment can't drift with the heading height, the page count, or
    // the spacing (a page-fixed grid does, because the renderer doesn't pin the
    // flowed rows' phase relative to it). A half-pitch vertical offset puts a dot
    // row on every row *boundary*, so the centred label sits in the gap between two
    // dots, never astride one (the "dot strikethrough").
    let half_sp = 0.5 * sp;
    let top = crate::geometry::TOOLBAR_SAFE_PT;
    // Align dots to the device "Dots Small" template phase so on-device inserted
    // Dots-Small pages line up with ours. Phase measured by rasterizing an exported
    // template page: dot centers at x≈2.16, y≈0.48 pt from the page corner (mod
    // pitch). The SVG paints the dot at the tile center, so offset by half a pitch.
    let dot_bx = 2.22 - half_sp;
    let dot_by = -0.04 - half_sp;
    format!(
        "{vars}\n\
@page {{ size: {w}pt {h}pt; margin: 0; }}\n\
* {{ box-sizing: border-box; margin: 0; padding: 0; }}\n\
html, body {{ margin: 0; padding: 0; }}\n\
body {{ font-family: \"{family}\", serif; font-size: 9.5pt; line-height: 1.4; color: var(--ink); }}\n\
.h-month, .h-section, .dayhead-date, .cover .title {{ font-family: \"Fraunces 72pt\", serif; }}\n\
.day, .day .wd, .cbadge, .pill, .agenda-date, .detail-meta {{ font-family: \"Hanken Grotesk\", sans-serif; }}\n\
.page {{ position: relative; width: {w}pt; height: {h}pt; padding: {top}pt {m}pt {m}pt {m}pt; overflow: hidden; background: var(--paper); break-after: page; }}\n\
.page:last-child {{ break-after: auto; }}\n\
.dotgrid {{ position: absolute; inset: 0; background-image: url(dot.svg); background-repeat: repeat; background-size: {sp}pt {sp}pt; background-position: {dot_bx}pt {dot_by}pt; }}\n\
.h-month {{ color: var(--primary); font-size: 16pt; font-weight: bold; margin-bottom: 6pt; }}\n\
.h-section {{ color: var(--primary); font-size: 14pt; font-weight: bold; }}\n\
/* Dot grid painted as the page background so headings/labels sit on top. Used by
   the month index and Tasks pages. Unlike an absolutely-positioned .dotgrid child,
   a page background resolves correctly even when rendered as a single page. */\n\
.dotpage {{ background-image: url(dot.svg); background-repeat: repeat; background-origin: border-box; background-size: {sp}pt {sp}pt; background-position: {dot_bx}pt {dot_by}pt; }}\n\
.dotpage .h-section {{ font-size: {head_fs}pt; }}\n\
.month-index .h-month {{ font-size: {head_fs}pt; margin-bottom: {half_sp}pt; }}\n\
.month-list {{ display: flex; flex-direction: column; background-image: url(dot.svg); background-repeat: repeat; background-size: {sp}pt var(--row); background-position: 0pt {half_sp}pt; }}\n\
.month-index .day {{ height: var(--row); }}\n\
.day {{ height: {sp}pt; display: flex; align-items: center; gap: 6pt; font-size: {num_fs}pt; line-height: 1; }}\n\
.day .num {{ width: 16pt; text-align: right; font-weight: bold; color: var(--accent); }}\n\
.day.weekstart .num {{ color: var(--primary); }}\n\
.day .wd {{ color: var(--muted); font-size: {wd_fs}pt; }}\n\
.daylink {{ text-decoration: none; color: inherit; display: inline-flex; gap: 6pt; align-items: center; width: 44pt; }}\n\
.cbadge {{ display: inline-block; min-width: 14pt; height: 13pt; padding-top: 2.5pt; border-radius: 6.5pt; background: var(--accent); color: var(--paper); font-size: 8pt; font-weight: bold; line-height: 1; text-align: center; text-decoration: none; }}\n\
.cover {{ position: absolute; inset: 0; display: flex; flex-direction: column; justify-content: flex-end; padding: {m}pt; color: var(--nav); background-image: url(cover.svg); background-size: 100% 100%; background-repeat: no-repeat; }}\n\
.cover .year {{ font-size: 9pt; letter-spacing: 3px; }}\n\
.cover .title {{ font-size: 24pt; font-weight: bold; }}\n\
.cover .title-blank {{ border-bottom: 1pt solid rgba(255,255,255,0.6); width: 70%; height: 22pt; }}\n\
.fl-block {{ position: relative; height: 33.33%; border-bottom: 0.6pt solid var(--rule); padding-top: 4pt; background-image: url(dot.svg); background-repeat: repeat; background-size: {sp}pt {sp}pt; }}\n\
.fl-block .h-month {{ font-size: 12pt; }}\n\
.legend {{ font-size: 9pt; line-height: 1.8; }}\n\
.legend .sym {{ display: inline-block; width: 16pt; font-weight: bold; color: var(--primary); }}\n\
.pill {{ display: inline-block; padding: 0 6pt; border-radius: 8pt; color: var(--paper); background: var(--accent); font-size: 7pt; }}\n\
.dayhead {{ display: flex; justify-content: space-between; align-items: center; background: var(--paper); padding-bottom: 4pt; }}\n\
.dayhead-date {{ font-size: 13pt; line-height: 1; text-decoration: none; color: var(--primary); border-bottom: 0.75pt solid var(--rule); padding-bottom: 2pt; }}\n\
/* Agenda + Details: ink body text, indigo underlined date headers, color swatch per event. */\n\
.h-month a {{ color: var(--primary); text-decoration: none; }}\n\
.swatch {{ display: inline-block; width: 7pt; height: 7pt; border-radius: 2pt; margin-right: 4pt; vertical-align: -0.5pt; }}\n\
.agenda-day, .detail-day {{ break-inside: avoid; margin-bottom: 8pt; }}\n\
.agenda-date {{ font-weight: bold; color: var(--primary); font-size: 11pt; text-decoration: none; border-bottom: 0.75pt solid var(--rule); padding-bottom: 1.5pt; }}\n\
.agenda-line {{ font-size: 9pt; margin: 2pt 0; color: var(--ink); }}\n\
.agenda-line a {{ color: var(--ink); text-decoration: none; }}\n\
.detail-evt {{ margin: 3pt 0 6pt 8pt; }}\n\
.detail-title {{ font-size: 10pt; color: var(--ink); }}\n\
.detail-meta {{ font-size: 9pt; color: var(--muted); }}\n",
        vars = css_vars(theme), w = w, h = h, m = m, sp = sp, half_sp = half_sp,
        top = top, dot_bx = dot_bx, dot_by = dot_by,
        head_fs = head_fs, num_fs = num_fs, wd_fs = wd_fs, family = BODY_FAMILY,
    )
}

pub fn render_pdf(
    device: &Device,
    grid: &GridSpec,
    theme: &Palette,
    fragments: &[String],
    out_path: &Path,
) -> anyhow::Result<()> {
    let css = build_css(device, grid, theme);
    let html = Base {
        css: &css,
        pages: fragments,
    }
    .render()?;

    let mut assets = AssetBundle::new();
    assets.add_image(
        "dot.svg",
        svg::dot_tile_svg(grid.spacing_pt, color(theme, "dot", "#55534C")).into_bytes(),
    );
    assets.add_image(
        "cover.svg",
        svg::cover_svg(
            device.width_pt(),
            device.height_pt(),
            color(theme, "primary", "#2A2F6B"),
            color(theme, "cover_to", "#1A1E48"),
        )
        .into_bytes(),
    );
    // to_vec() copies ~750 KB of static font data per call; fine for the
    // once-per-run, sequential rendering this tool does (15 PDFs/year).
    assets.add_font_bytes(LORA_REGULAR.to_vec())?;
    assets.add_font_bytes(LORA_SEMIBOLD.to_vec())?;
    assets.add_font_bytes(FRAUNCES.to_vec())?;
    assets.add_font_bytes(HANKEN_REGULAR.to_vec())?;
    assets.add_font_bytes(HANKEN_MEDIUM.to_vec())?;

    let engine = Engine::builder()
        .page_size(PageSize {
            width: device.width_pt(),
            height: device.height_pt(),
        })
        .margin(Margin::uniform(0.0))
        .assets(assets)
        .producer("rmbujo")
        .creator("rmbujo")
        .creation_date("D:20000101000000Z")
        .build();
    engine.render_html_to_file(&html, out_path)?;
    Ok(())
}
