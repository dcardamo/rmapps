//! One-off: render a "type & colour tests v2" PDF for on-device review.
//! Page 1: blue/purple + rust pairings shown in-context (nav bar, Feed nameplate,
//! kicker, number, headline) so we judge the actual two-colour look on the screen.
//! Pages 2-6: a fresh batch of candidate body fonts.
//! Run: `nix develop -c cargo run --example tests`. Output: /tmp/rmreader_type_colour_tests.pdf
use fulgur::asset::AssetBundle;
use fulgur::config::{Margin, PageSize};
use fulgur::engine::Engine;

macro_rules! font {
    ($p:literal) => {
        include_bytes!(concat!("../assets/fonts/", $p)) as &[u8]
    };
}

// (label, primary, accent) — primary = nav/headline; accent = nameplate/kicker/number.
const PAIRS: &[(&str, &str, &str)] = &[
    ("1 · Cobalt + Rust", "#1F3C82", "#9C3A1B"),
    ("2 · Royal blue + Rust", "#2549B0", "#9C3A1B"),
    ("3 · Plum + Rust", "#6D2C6E", "#9C3A1B"),
    ("4 · Indigo + Tomato", "#2A2F6B", "#CF3A2B"),
    ("5 · Cobalt + Plum", "#1F3C82", "#6D2C6E"),
];

const FONTS: &[(&str, &str)] = &[
    (
        "Fraunces",
        "Characterful modern editorial serif (the most distinctive here).",
    ),
    (
        "Lora",
        "Warm contemporary editorial serif — very popular for body text.",
    ),
    (
        "Gelasio",
        "Georgia-compatible — the classic, beloved e-reader serif.",
    ),
    ("Crimson Pro", "Classic old-style book serif — elegant."),
    (
        "Roboto Serif",
        "Modern, neutral, engineered for long-form legibility.",
    ),
];

fn pairs_page(paper: &str) -> String {
    let cards: String = PAIRS
        .iter()
        .map(|(label, pri, acc)| {
            format!(
                "<div class=\"pair\">\
<div class=\"plabel\">{label}</div>\
<div class=\"pnav\" style=\"background:{pri}\"><span>‹ Prev</span><span>⌂ Home</span><span>Next ›</span></div>\
<div class=\"prow\">\
<span class=\"pplate\" style=\"background:{acc}\">Feed</span>\
<span class=\"pnum\" style=\"color:{acc}\">01</span>\
<span class=\"phead\" style=\"color:{pri}\">Architecture</span>\
</div></div>"
            )
        })
        .collect();
    format!(
        "<section class=\"page\" style=\"background:{paper}\">\
<div class=\"tt\">Colour pairings</div>\
<div class=\"note\">Blue/purple primary (nav bar + headline) with a warm accent (the \u{201c}Feed\u{201d} block, kicker, numbers). Which pairing reads best on the screen?</div>\
{cards}\
<div class=\"caption\">rmreader — colour pairings (blue/purple + warm)</div></section>"
    )
}

fn font_page(family: &str, note: &str, paper: &str) -> String {
    let cls = family.replace([' ', '4'], "").to_lowercase();
    format!(
        "<section class=\"page fp-{cls}\" style=\"background:{paper}\">\
<div class=\"fname\">{family}</div>\
<div class=\"fnote\">{note}</div>\
<h2 class=\"fhead\">The Quiet Architecture of a Reading Life</h2>\
<div class=\"fbyline\">Mara Ellison · The Marginalian · 7 min</div>\
<div class=\"frule\"></div>\
<div class=\"fbody\">\
<p>There is a difference between collecting articles and keeping company with them. The first is a logistics problem; the second is an architecture. A queue grows by accretion, indifferent to what it holds — a reading room is built: chosen, arranged, returned to. The quick brown fox jumps over the lazy dog, 0123456789.</p>\
</div>\
<div class=\"caption\">Font: {family}</div></section>"
    )
}

fn main() -> anyhow::Result<()> {
    let device = rmreader::device::get_device("paper-pro-move")?;
    let (w, h) = (device.width_pt(), device.height_pt());
    let paper = "#F3F1EA";

    let mut css = format!(
        "@page {{ size:{w}pt {h}pt; margin:0; }}\n\
* {{ box-sizing:border-box; margin:0; padding:0; }}\n\
.page {{ width:{w}pt; height:{h}pt; padding:34pt 26pt 26pt; position:relative; overflow:hidden; break-after:page; color:#1a1a18; }}\n\
.page:last-child {{ break-after:auto; }}\n\
.caption {{ position:absolute; left:26pt; right:26pt; bottom:12pt; font-family:\"Hanken Grotesk\",sans-serif; font-size:7pt; letter-spacing:.04em; color:#a29e95; }}\n\
.tt {{ font-family:\"Hanken Grotesk\",sans-serif; font-weight:700; font-size:15pt; }}\n\
.note {{ font-family:\"Hanken Grotesk\",sans-serif; font-size:8pt; color:#5b5f66; margin:6pt 0 12pt; line-height:1.4; }}\n\
.pair {{ margin-bottom:8pt; }}\n\
.plabel {{ font-family:\"Hanken Grotesk\",sans-serif; font-weight:700; font-size:9pt; margin-bottom:3pt; }}\n\
.pnav {{ display:flex; justify-content:space-between; padding:4pt 12pt; font-family:\"Hanken Grotesk\",sans-serif; font-size:7.5pt; font-weight:600; letter-spacing:.1em; text-transform:uppercase; border-radius:3pt; color:#f4f1e8; }}\n\
.prow {{ display:flex; align-items:center; gap:8pt; margin-top:5pt; }}\n\
.pplate {{ font-family:\"Source Serif 4\",serif; font-weight:600; font-size:14pt; padding:2pt 8pt 3pt; border-radius:3pt; color:#f6f1e6; flex:0 0 auto; }}\n\
.pkick {{ font-family:\"Hanken Grotesk\",sans-serif; font-size:6.5pt; font-weight:700; letter-spacing:.14em; text-transform:uppercase; padding:2.5pt 6pt; border-radius:3pt; color:#f6f1e6; flex:0 0 auto; }}\n\
.pnum {{ font-family:\"Hanken Grotesk\",sans-serif; font-weight:700; font-size:11pt; flex:0 0 auto; }}\n\
.phead {{ font-family:\"Source Serif 4\",serif; font-weight:600; font-size:12pt; flex:1; line-height:1.1; white-space:nowrap; overflow:hidden; }}\n\
.fname {{ font-family:\"Hanken Grotesk\",sans-serif; font-weight:700; font-size:13pt; }}\n\
.fnote {{ font-family:\"Hanken Grotesk\",sans-serif; font-size:8pt; color:#5b5f66; margin:5pt 0 16pt; line-height:1.4; }}\n\
.fhead {{ font-weight:600; font-size:21pt; line-height:1.07; color:#23262b; letter-spacing:-.01em; }}\n\
.fbyline {{ font-family:\"Hanken Grotesk\",sans-serif; font-size:8.5pt; color:#5b5f66; margin-top:8pt; }}\n\
.frule {{ height:0.5pt; background:#ddd9cf; margin:12pt 0; }}\n\
.fbody {{ font-size:11.5pt; line-height:1.6; }}\n\
.fbody p {{ margin:0 0 9pt; }}\n",
    );
    for (family, _) in FONTS {
        let cls = family.replace([' ', '4'], "").to_lowercase();
        css.push_str(&format!(
            ".fp-{cls} .fhead, .fp-{cls} .fbody {{ font-family:\"{family}\", serif; }}\n"
        ));
    }

    let mut body = pairs_page(paper);
    for (family, note) in FONTS {
        body.push_str(&font_page(family, note, paper));
    }

    let html = format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><style>{css}</style></head><body>{body}</body></html>"
    );

    let mut assets = AssetBundle::new();
    for f in [
        font!("HankenGrotesk-Regular.ttf"),
        font!("HankenGrotesk-Medium.ttf"),
        font!("SourceSerif4-Regular.ttf"),
        font!("SourceSerif4-SemiBold.ttf"),
        font!("Fraunces-Regular.ttf"),
        font!("Fraunces-SemiBold.ttf"),
        font!("Lora-Regular.ttf"),
        font!("Lora-SemiBold.ttf"),
        font!("Gelasio-Regular.ttf"),
        font!("Gelasio-SemiBold.ttf"),
        font!("CrimsonPro-Regular.ttf"),
        font!("CrimsonPro-SemiBold.ttf"),
        font!("RobotoSerif-Regular.ttf"),
        font!("RobotoSerif-SemiBold.ttf"),
    ] {
        assets.add_font_bytes(f.to_vec())?;
    }

    let engine = Engine::builder()
        .page_size(PageSize {
            width: w,
            height: h,
        })
        .margin(Margin::uniform(0.0))
        .assets(assets)
        .producer("rmreader")
        .creator("rmreader")
        .creation_date("D:20000101000000Z")
        .build();
    let out = "/tmp/rmreader_type_colour_tests.pdf";
    engine.render_html_to_file(&html, std::path::Path::new(out))?;
    println!("wrote {out}");
    Ok(())
}
