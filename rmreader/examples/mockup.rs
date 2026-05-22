//! One-off: render a multi-page "colour studies" PDF (real fulgur pipeline, real
//! fonts, real Paper Pro Move geometry) so options can be judged on the device,
//! not on a Mac. Run: `nix develop -c cargo run --example mockup`.
//! Output: /tmp/rmreader_colour_studies.pdf
use fulgur::asset::AssetBundle;
use fulgur::config::{Margin, PageSize};
use fulgur::engine::Engine;

const NEWS_R: &[u8] = include_bytes!("../assets/fonts/Newsreader-Regular.ttf");
const NEWS_I: &[u8] = include_bytes!("../assets/fonts/Newsreader-Italic.ttf");
const NEWS_S: &[u8] = include_bytes!("../assets/fonts/Newsreader-SemiBold.ttf");
const SS4_R: &[u8] = include_bytes!("../assets/fonts/SourceSerif4-Regular.ttf");
const SS4_I: &[u8] = include_bytes!("../assets/fonts/SourceSerif4-Italic.ttf");
const SS4_S: &[u8] = include_bytes!("../assets/fonts/SourceSerif4-SemiBold.ttf");
const HANKEN_R: &[u8] = include_bytes!("../assets/fonts/HankenGrotesk-Regular.ttf");
const HANKEN_M: &[u8] = include_bytes!("../assets/fonts/HankenGrotesk-Medium.ttf");

struct Opt {
    id: u32,
    label: &'static str,
    serif: &'static str,
    nameplate: bool,
    bg: &'static str,
    ink: &'static str,
    heading: &'static str,
    sub: &'static str,
    rule: &'static str,
    accent: &'static str,
    navbg: &'static str,
    navfg: &'static str,
    plate: &'static str,
    platefg: &'static str,
}

const FEED: &[(&str, &str, &str)] = &[
    (
        "The Quiet Architecture of a Reading Life",
        "Ellison",
        "7 min",
    ),
    ("On Slowness and the Engineered Mind", "Vohra", "11 min"),
    ("What E-Ink Knows About Patience", "Okafor", "5 min"),
    ("The Last Honest Interface", "Lindqvist", "9 min"),
    ("Notes Toward a Calmer Inbox", "Reyes", "6 min"),
    ("Paper, and the Persistence of Margins", "Tanaka", "8 min"),
    ("A Field Guide to Deep Work", "Mensah", "12 min"),
];

fn option_css(o: &Opt) -> String {
    let s = format!("\"{}\", serif", o.serif);
    format!(
        ".opt{id} .page{{background:{bg}}}\n\
.opt{id} .ititle,.opt{id} .headline,.opt{id} .rowtitle,.opt{id} .body,.opt{id} .feedplate{{font-family:{s};color:{ink}}}\n\
.opt{id} .ititle,.opt{id} .headline{{color:{heading}}}\n\
.opt{id} .feedplate{{background:{plate};color:{platefg}}}\n\
.opt{id} .num,.opt{id} .body.drop p:first-of-type::first-letter,.opt{id} .body a{{color:{accent}}}\n\
.opt{id} .kicker{{background:{accent};color:{platefg}}}\n\
.opt{id} .nav{{background:{navbg};color:{navfg}}}\n\
.opt{id} .sub,.opt{id} .byline,.opt{id} .irow .rt,.opt{id} .author{{color:{sub}}}\n\
.opt{id} .irow{{border-color:{rule}}}\n\
.opt{id} .arthr{{background:{rule}}}\n",
        id = o.id, bg = o.bg, ink = o.ink, heading = o.heading, sub = o.sub,
        rule = o.rule, accent = o.accent, navbg = o.navbg, navfg = o.navfg,
        plate = o.plate, platefg = o.platefg, s = s,
    )
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn feed_page(o: &Opt) -> String {
    let title = if o.nameplate {
        "<span class=\"feedplate\">Feed</span>".to_string()
    } else {
        "<h1 class=\"ititle\">Feed</h1>".to_string()
    };
    let rows: String = FEED
        .iter()
        .take(5)
        .enumerate()
        .map(|(i, (t, a, rt))| {
            format!(
                "<div class=\"irow\"><span class=\"num\">{n:02}</span>\
<span class=\"rowtitle\"><b>{t}</b> — <span class=\"author\">{a}</span></span>\
<span class=\"rt\">{rt}</span></div>",
                n = i + 1,
                t = esc(t),
                a = esc(a),
                rt = rt
            )
        })
        .collect();
    format!(
        "<section class=\"page opt{id} feedpage\"><div class=\"feedhead\">{title}</div>\
<div class=\"sub\">100 ARTICLES · NEWEST FIRST</div>{rows}\
<div class=\"caption\">Option {id} — {label}  ·  Feed list</div></section>",
        id = o.id,
        title = title,
        rows = rows,
        label = esc(o.label)
    )
}

fn article_page(o: &Opt) -> String {
    format!(
        "<section class=\"page opt{id} artpage\">\
<div class=\"nav\"><span class=\"navmut\">‹ Prev</span><span class=\"navhome\">⌂ Home</span><span>Next ›</span></div>\
<div class=\"artbody\"><span class=\"kicker\">Article</span>\
<h2 class=\"headline\">The Quiet Architecture of a Reading Life</h2>\
<div class=\"byline\">Mara Ellison · The Marginalian · 7 min</div>\
<div class=\"arthr\"></div>\
<div class=\"body drop\">\
<p>There is a difference between collecting articles and keeping company with them. The first is a logistics problem; the second is an architecture. A queue grows by accretion, indifferent to what it holds.</p>\
<p>The device on your desk is not a faster newspaper. It is, if you let it, a <a href=\"#\">place to sit</a> with one thing at a time — no banner asking you to subscribe, no related-stories rail tugging your sleeve.</p>\
</div></div>\
<div class=\"caption\">Option {id} — {label}  ·  Article first page</div></section>",
        id = o.id, label = esc(o.label)
    )
}

fn main() -> anyhow::Result<()> {
    let device = rmreader::device::get_device("paper-pro-move")?;
    let (w, h) = (device.width_pt(), device.height_pt());

    let opts = vec![
        Opt {
            id: 1,
            label: "Slate + Amber · Source Serif · Feed nameplate",
            serif: "Source Serif 4",
            nameplate: true,
            bg: "#F3F1EA",
            ink: "#1A1A18",
            heading: "#26334D",
            sub: "#5E6166",
            rule: "#E0DDD2",
            accent: "#A8590F",
            navbg: "#26334D",
            navfg: "#F4F1E8",
            plate: "#A8590F",
            platefg: "#F8F1E6",
        },
        Opt {
            id: 2,
            label: "Slate + Amber · Source Serif · plain title",
            serif: "Source Serif 4",
            nameplate: false,
            bg: "#F3F1EA",
            ink: "#1A1A18",
            heading: "#26334D",
            sub: "#5E6166",
            rule: "#E0DDD2",
            accent: "#A8590F",
            navbg: "#26334D",
            navfg: "#F4F1E8",
            plate: "#A8590F",
            platefg: "#F8F1E6",
        },
        Opt {
            id: 3,
            label: "Ink Blue + Rust · Source Serif · plain",
            serif: "Source Serif 4",
            nameplate: false,
            bg: "#F2F1EC",
            ink: "#1A1A18",
            heading: "#1F3C82",
            sub: "#5E6166",
            rule: "#DFDCD2",
            accent: "#9C3A1B",
            navbg: "#1F3C82",
            navfg: "#F2F1E8",
            plate: "#1F3C82",
            platefg: "#F4F2EC",
        },
        Opt {
            id: 4,
            label: "Forest + Ochre · Source Serif · plain",
            serif: "Source Serif 4",
            nameplate: false,
            bg: "#F1F0E8",
            ink: "#181A16",
            heading: "#1F4D39",
            sub: "#595E53",
            rule: "#DDDBCD",
            accent: "#946410",
            navbg: "#1F4D39",
            navfg: "#F1F2E8",
            plate: "#1F4D39",
            platefg: "#F2F4EC",
        },
        Opt {
            id: 5,
            label: "Minimal — black headings, one deep accent",
            serif: "Source Serif 4",
            nameplate: false,
            bg: "#F5F3EC",
            ink: "#1A1A18",
            heading: "#1A1A18",
            sub: "#5E6166",
            rule: "#E2DFD5",
            accent: "#8A2E1F",
            navbg: "#2A2A28",
            navfg: "#F2F0E9",
            plate: "#8A2E1F",
            platefg: "#F4F0E8",
        },
        Opt {
            id: 6,
            label: "Slate + Amber · Newsreader font · plain",
            serif: "Newsreader",
            nameplate: false,
            bg: "#F3F1EA",
            ink: "#1A1A18",
            heading: "#26334D",
            sub: "#5E6166",
            rule: "#E0DDD2",
            accent: "#A8590F",
            navbg: "#26334D",
            navfg: "#F4F1E8",
            plate: "#A8590F",
            platefg: "#F8F1E6",
        },
    ];

    let base_css = format!(
        "@page {{ size: {w}pt {h}pt; margin: 0; }}\n\
* {{ box-sizing:border-box; margin:0; padding:0; }}\n\
.page {{ width:{w}pt; height:{h}pt; padding:36pt 26pt 26pt; position:relative; overflow:hidden; break-after:page; background:#fff; }}\n\
.page:last-child {{ break-after:auto; }}\n\
.feedhead {{ margin-bottom:2pt; }}\n\
.feedplate {{ display:inline-block; font-weight:600; font-size:25pt; line-height:1; padding:5pt 11pt 7pt; border-radius:4pt; letter-spacing:-.01em; }}\n\
.ititle {{ font-weight:600; font-size:27pt; line-height:1; letter-spacing:-.015em; }}\n\
.sub {{ font-family:\"Hanken Grotesk\",sans-serif; font-size:7.5pt; letter-spacing:.16em; margin:10pt 0 12pt; }}\n\
.irow {{ display:flex; gap:9pt; align-items:baseline; padding:6pt 0; border-bottom:0.5pt solid #ddd; }}\n\
.num {{ font-family:\"Hanken Grotesk\",sans-serif; font-weight:700; font-size:9.5pt; width:15pt; flex:0 0 auto; }}\n\
.rowtitle {{ flex:1; font-size:11pt; line-height:1.25; }}\n\
.rowtitle b {{ font-weight:600; }}\n\
.rt {{ font-family:\"Hanken Grotesk\",sans-serif; font-size:8.5pt; white-space:nowrap; flex:0 0 auto; }}\n\
.nav {{ display:flex; align-items:center; justify-content:space-between; padding:7pt 16pt; font-family:\"Hanken Grotesk\",sans-serif; font-size:8.5pt; font-weight:600; letter-spacing:.12em; text-transform:uppercase; border-radius:4pt; }}\n\
.navhome {{ font-weight:700; }}\n\
.navmut {{ opacity:.6; }}\n\
.artbody {{ padding-top:16pt; }}\n\
.kicker {{ display:inline-block; font-family:\"Hanken Grotesk\",sans-serif; font-size:7.5pt; font-weight:700; letter-spacing:.18em; text-transform:uppercase; padding:3pt 7pt; border-radius:3pt; margin-bottom:9pt; }}\n\
.headline {{ font-weight:600; font-size:22pt; line-height:1.06; letter-spacing:-.012em; }}\n\
.byline {{ font-family:\"Hanken Grotesk\",sans-serif; font-size:8.5pt; margin-top:8pt; }}\n\
.arthr {{ height:0.5pt; margin:11pt 0; background:#ddd; }}\n\
.body {{ font-size:10.5pt; line-height:1.52; }}\n\
.body p {{ margin:0 0 8pt; }}\n\
.body.drop p:first-of-type::first-letter {{ font-weight:600; float:left; font-size:3em; line-height:.78; padding:4pt 6pt 0 0; }}\n\
.body a {{ text-decoration:underline; }}\n\
.caption {{ position:absolute; left:26pt; right:26pt; bottom:12pt; font-family:\"Hanken Grotesk\",sans-serif; font-size:7pt; letter-spacing:.04em; color:#a29e95; }}\n\
.legend {{ font-family:\"Source Serif 4\",serif; color:#1a1a18; }}\n\
.legend h1 {{ font-weight:600; font-size:26pt; margin-bottom:14pt; }}\n\
.legend p {{ font-family:\"Hanken Grotesk\",sans-serif; font-size:10pt; line-height:1.55; margin-bottom:10pt; }}\n\
.legend ol {{ margin-left:18pt; }}\n\
.legend li {{ font-family:\"Hanken Grotesk\",sans-serif; font-size:9.5pt; line-height:1.4; margin-bottom:6pt; }}\n",
    );

    let mut css = base_css;
    for o in &opts {
        css.push_str(&option_css(o));
    }

    let legend_items: String = opts
        .iter()
        .map(|o| format!("<li><b>{}</b> — {}</li>", o.id, esc(o.label)))
        .collect();
    let legend = format!(
        "<section class=\"page legend\"><h1>Colour studies</h1>\
<p>Each option below is two pages — the <b>Feed list</b>, then an <b>article's first page</b>. Same content throughout; only colour, font, and the \"Feed\" title style change. The grey line at the bottom of every page names the option.</p>\
<p>Tell me the number(s) you like — and feel free to mix (e.g. \u{201c}option 2 colours, option 6 font, plain title\u{201d}). The nav bar shown is a filled-bar proposal; today's build draws plain nav text.</p>\
<ol>{items}</ol></section>",
        items = legend_items
    );

    let mut body = legend;
    for o in &opts {
        body.push_str(&feed_page(o));
        body.push_str(&article_page(o));
    }

    let html = format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><style>{css}</style></head><body>{body}</body></html>"
    );

    let mut assets = AssetBundle::new();
    for f in [
        NEWS_R, NEWS_I, NEWS_S, SS4_R, SS4_I, SS4_S, HANKEN_R, HANKEN_M,
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

    let out = "/tmp/rmreader_colour_studies.pdf";
    engine.render_html_to_file(&html, std::path::Path::new(out))?;
    println!("wrote {out}");
    Ok(())
}
