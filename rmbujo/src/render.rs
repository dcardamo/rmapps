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

const FONT_REGULAR: &[u8] = include_bytes!("../assets/fonts/DejaVuSerif.ttf");
const FONT_BOLD: &[u8] = include_bytes!("../assets/fonts/DejaVuSerif-Bold.ttf");
const FONT_FAMILY: &str = "DejaVu Serif";

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
    // Month-list max-height: page height minus top reserve, bottom margin, and the
    // heading block (head_fs + half_sp margin-bottom). Clips overflow without calc().
    let ml_max_h = h - top - m - 1.75 * sp;
    format!(
        "{vars}\n\
@page {{ size: {w}pt {h}pt; margin: 0; }}\n\
* {{ box-sizing: border-box; margin: 0; padding: 0; }}\n\
html, body {{ margin: 0; padding: 0; }}\n\
body {{ font-family: \"{family}\", serif; color: #1a1a1a; }}\n\
.page {{ position: relative; width: {w}pt; height: {h}pt; padding: {top}pt {m}pt {m}pt {m}pt; overflow: hidden; background: #fff; break-after: page; }}\n\
.page:last-child {{ break-after: auto; }}\n\
.dotgrid {{ position: absolute; inset: 0; background-image: url(dot.svg); background-repeat: repeat; background-size: {sp}pt {sp}pt; background-position: {m}pt {top}pt; }}\n\
.h-month {{ color: var(--navy); font-size: 16pt; font-weight: bold; margin-bottom: 6pt; }}\n\
.h-section {{ color: var(--navy); font-size: 14pt; font-weight: bold; }}\n\
/* Dot grid painted as the page background so headings/labels sit on top. Used by
   the month index and Tasks pages. Unlike an absolutely-positioned .dotgrid child,
   a page background resolves correctly even when rendered as a single page. */\n\
.dotpage {{ background-image: url(dot.svg); background-repeat: repeat; background-size: {sp}pt {sp}pt; background-position: {m}pt {top}pt; }}\n\
.dotpage .h-section {{ font-size: {head_fs}pt; }}\n\
.month-index .h-month {{ font-size: {head_fs}pt; margin-bottom: {half_sp}pt; }}\n\
.month-list {{ display: flex; flex-direction: column; overflow: hidden; max-height: {ml_max_h}pt; background-image: url(dot.svg); background-repeat: repeat; background-size: {sp}pt {sp}pt; background-position: 0pt {half_sp}pt; }}\n\
.day {{ height: {sp}pt; display: flex; align-items: center; gap: 6pt; font-size: {num_fs}pt; line-height: 1; }}\n\
.day .num {{ width: 16pt; text-align: right; font-weight: bold; }}\n\
.day.weekstart .num {{ color: var(--navy); }}\n\
.day .wd {{ color: var(--navy); font-size: {wd_fs}pt; }}\n\
.cover {{ position: absolute; inset: 0; display: flex; flex-direction: column; justify-content: flex-end; padding: {m}pt; color: #fff; background-image: url(cover.svg); background-size: 100% 100%; background-repeat: no-repeat; }}\n\
.cover .year {{ font-size: 9pt; letter-spacing: 3px; }}\n\
.cover .title {{ font-size: 24pt; font-weight: bold; }}\n\
.cover .title-blank {{ border-bottom: 1pt solid rgba(255,255,255,0.6); width: 70%; height: 22pt; }}\n\
.fl-block {{ position: relative; height: 33.33%; border-bottom: 0.6pt solid var(--rule); padding-top: 4pt; background-image: url(dot.svg); background-repeat: repeat; background-size: {sp}pt {sp}pt; }}\n\
.fl-block .h-month {{ font-size: 12pt; }}\n\
.legend {{ font-size: 9pt; line-height: 1.8; }}\n\
.legend .sym {{ display: inline-block; width: 16pt; font-weight: bold; color: var(--navy); }}\n\
.pill {{ display: inline-block; padding: 0 6pt; border-radius: 8pt; color: #fff; background: var(--brick); font-size: 7pt; }}\n",
        vars = css_vars(theme), w = w, h = h, m = m, sp = sp, half_sp = half_sp,
        top = top, ml_max_h = ml_max_h,
        head_fs = head_fs, num_fs = num_fs, wd_fs = wd_fs, family = FONT_FAMILY,
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
        svg::dot_tile_svg(grid.spacing_pt, color(theme, "dot", "#CFCDC4")).into_bytes(),
    );
    assets.add_image(
        "cover.svg",
        svg::cover_svg(
            device.width_pt(),
            device.height_pt(),
            color(theme, "navy", "#1B365D"),
            color(theme, "cover_to", "#0F2444"),
        )
        .into_bytes(),
    );
    // to_vec() copies ~750 KB of static font data per call; fine for the
    // once-per-run, sequential rendering this tool does (15 PDFs/year).
    assets.add_font_bytes(FONT_REGULAR.to_vec())?;
    assets.add_font_bytes(FONT_BOLD.to_vec())?;

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
