//! Render assembled HTML + image assets to PDF via fulgur (Blitz + krilla).
use std::path::Path;

use askama::Template;
use fulgur::asset::AssetBundle;
use fulgur::config::{Margin, PageSize};
use fulgur::engine::Engine;

use crate::device::Device;
use crate::theme::{css_vars, Palette};

const LORA: &[u8] = include_bytes!("../assets/fonts/Lora-Regular.ttf");
const LORA_IT: &[u8] = include_bytes!("../assets/fonts/Lora-Italic.ttf");
const LORA_SB: &[u8] = include_bytes!("../assets/fonts/Lora-SemiBold.ttf");
const HANKEN: &[u8] = include_bytes!("../assets/fonts/HankenGrotesk-Regular.ttf");
const HANKEN_MD: &[u8] = include_bytes!("../assets/fonts/HankenGrotesk-Medium.ttf");

#[derive(Template)]
#[template(path = "base.html")]
struct Base<'a> {
    css: &'a str,
    pages: &'a [String],
}

pub fn build_css(device: &Device, theme: &Palette) -> String {
    let w = device.width_pt();
    let h = device.height_pt();
    format!(
        "{vars}\n\
/* The ENGINE margin (set in render_pdf) reserves space on EVERY physical page —\n\
   top 58pt (~36pt the device toolbar overlays + the post-processed nav bar below\n\
   it), sides 16pt, bottom 30pt — so flowing articles never reach the toolbar/nav\n\
   band on any page. @page only carries size + paper background. */\n\
@page {{ size: {w}pt {h}pt; margin: 0; background: var(--paper); }}\n\
* {{ box-sizing: border-box; margin: 0; padding: 0; }}\n\
html, body {{ margin: 0; padding: 0; background: var(--paper); }}\n\
body {{ font-family: \"Lora\", serif; color: var(--ink); font-size:9.5pt; }}\n\
.article {{ break-before: page; }}\n\
.headline {{ font-weight:600; font-size:16pt; line-height:1.12; color:var(--heading); letter-spacing:-.01em; bookmark-level:1; }}\n\
.byline {{ font-family:\"Hanken Grotesk\",sans-serif; font-size:8pt; color:var(--muted); margin-top:6pt; }}\n\
.hr {{ height:0.5pt; background:var(--rule); margin:8pt 0; }}\n\
.body {{ font-size:9.5pt; line-height:1.4; color:var(--ink); }}\n\
.body p {{ margin:0 0 4.5pt; }}\n\
.body a {{ color:var(--accent); text-decoration:underline; }}\n\
.body img {{ max-width:100%; height:auto; }}\n\
.body.drop p:first-of-type::first-letter {{ font-weight:600; color:var(--accent); float:left; font-size:2.7em; line-height:.82; padding:3pt 5pt 0 0; }}\n\
.index-title {{ display:inline-block; font-weight:600; font-size:16pt; color:var(--paper); background:var(--accent); padding:3pt 10pt 4pt; border-radius:4pt; letter-spacing:-.01em; }}\n\
.index-sub {{ font-family:\"Hanken Grotesk\",sans-serif; font-size:7.5pt; letter-spacing:.12em; text-transform:uppercase; color:var(--muted); margin:10pt 0 9pt; }}\n\
.index-row {{ display:flex; gap:8pt; padding:4pt 0; border-bottom:0.5pt solid var(--rule); text-decoration:none; color:var(--ink); break-inside:avoid; }}\n\
.index-row .n {{ color:var(--accent); font-weight:600; font-size:9pt; width:14pt; }}\n\
.index-row .t {{ flex:1; font-size:9.5pt; line-height:1.3; }}\n\
.index-row .rt {{ font-family:\"Hanken Grotesk\",sans-serif; font-size:7.5pt; color:var(--muted); }}\n",
        vars = css_vars(theme),
        w = w,
        h = h,
    )
}

pub fn render_pdf(
    device: &Device,
    theme: &Palette,
    fragments: &[String],
    images: &[(String, Vec<u8>)],
    out_path: &Path,
) -> anyhow::Result<()> {
    let css = build_css(device, theme);
    let html = Base {
        css: &css,
        pages: fragments,
    }
    .render()?;

    let mut assets = AssetBundle::new();
    for (key, bytes) in images {
        assets.add_image(key, bytes.clone());
    }
    // to_vec() copies static font data; fine for once-per-run rendering.
    assets.add_font_bytes(LORA.to_vec())?;
    assets.add_font_bytes(LORA_IT.to_vec())?;
    assets.add_font_bytes(LORA_SB.to_vec())?;
    assets.add_font_bytes(HANKEN.to_vec())?;
    assets.add_font_bytes(HANKEN_MD.to_vec())?;

    let engine = Engine::builder()
        .page_size(PageSize {
            width: device.width_pt(),
            height: device.height_pt(),
        })
        .margin(Margin {
            top: 58.0,
            right: 16.0,
            bottom: 30.0,
            left: 16.0,
        })
        .assets(assets)
        .bookmarks(true)
        .producer("rmreader")
        .creator("rmreader")
        .creation_date("D:20000101000000Z")
        .build();
    engine.render_html_to_file(&html, out_path)?;
    Ok(())
}
