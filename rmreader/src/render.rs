//! Render assembled HTML + image assets to PDF via fulgur (Blitz + krilla).
use std::path::Path;

use askama::Template;
use fulgur::asset::AssetBundle;
use fulgur::config::{Margin, PageSize};
use fulgur::engine::Engine;

use crate::device::Device;
use crate::theme::{css_vars, Palette};

const NEWSREADER: &[u8] = include_bytes!("../assets/fonts/Newsreader-Regular.ttf");
const NEWSREADER_IT: &[u8] = include_bytes!("../assets/fonts/Newsreader-Italic.ttf");
const NEWSREADER_SB: &[u8] = include_bytes!("../assets/fonts/Newsreader-SemiBold.ttf");
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
@page {{ size: {w}pt {h}pt; margin: 0; }}\n\
* {{ box-sizing: border-box; margin: 0; padding: 0; }}\n\
html, body {{ margin: 0; padding: 0; }}\n\
body {{ font-family: \"Newsreader\", serif; color: var(--ink); }}\n\
/* top padding reserves the top ~44pt the reMarkable toolbar overlays; bottom 44pt reserves the post-processed nav bar */\n\
.page {{ position: relative; width: {w}pt; height: {h}pt; padding: 50pt 26pt 44pt; overflow: hidden; background: var(--paper); break-after: page; }}\n\
.page:last-child {{ break-after: auto; }}\n\
.article {{ break-before: page; }}\n\
.kicker {{ font-family:\"Hanken Grotesk\",sans-serif; font-size:7.5pt; font-weight:600; letter-spacing:.2em; text-transform:uppercase; color:var(--accent); margin-bottom:9pt; }}\n\
.headline {{ font-weight:600; font-size:24pt; line-height:1.05; color:var(--heading); letter-spacing:-.01em; bookmark-level:1; }}\n\
.byline {{ font-family:\"Hanken Grotesk\",sans-serif; font-size:8.5pt; color:var(--muted); margin-top:10pt; }}\n\
.hr {{ height:0.5pt; background:var(--rule); margin:14pt 0; }}\n\
.body {{ font-size:11pt; line-height:1.62; color:var(--ink); }}\n\
.body p {{ margin:0 0 9pt; }}\n\
.body a {{ color:var(--accent); text-decoration:underline; }}\n\
.body img {{ max-width:100%; height:auto; }}\n\
.body.drop p:first-of-type::first-letter {{ font-weight:600; color:var(--accent); float:left; font-size:3em; line-height:.8; padding:4pt 6pt 0 0; }}\n\
.index-title {{ font-weight:600; font-size:22pt; color:var(--heading); }}\n\
.index-sub {{ font-family:\"Hanken Grotesk\",sans-serif; font-size:8pt; letter-spacing:.12em; text-transform:uppercase; color:var(--muted); margin-bottom:12pt; }}\n\
.index-row {{ display:flex; gap:8pt; padding:6pt 0; border-bottom:0.5pt solid var(--rule); text-decoration:none; color:var(--ink); }}\n\
.index-row .n {{ color:var(--accent); font-weight:600; width:16pt; }}\n\
.index-row .t {{ flex:1; }}\n\
.index-row .rt {{ font-family:\"Hanken Grotesk\",sans-serif; font-size:8pt; color:var(--muted); }}\n",
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
    assets.add_font_bytes(NEWSREADER.to_vec())?;
    assets.add_font_bytes(NEWSREADER_IT.to_vec())?;
    assets.add_font_bytes(NEWSREADER_SB.to_vec())?;
    assets.add_font_bytes(HANKEN.to_vec())?;
    assets.add_font_bytes(HANKEN_MD.to_vec())?;

    let engine = Engine::builder()
        .page_size(PageSize {
            width: device.width_pt(),
            height: device.height_pt(),
        })
        .margin(Margin::uniform(0.0))
        .assets(assets)
        .bookmarks(true)
        .producer("rmreader")
        .creator("rmreader")
        .creation_date("D:20000101000000Z")
        .build();
    engine.render_html_to_file(&html, out_path)?;
    Ok(())
}
