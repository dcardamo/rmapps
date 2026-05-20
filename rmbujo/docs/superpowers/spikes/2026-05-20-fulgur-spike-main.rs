use fulgur::asset::AssetBundle;
use fulgur::config::{Margin, PageSize};
use fulgur::engine::Engine;

// Single-cell dot tile, tiled via background-repeat.
const DOT_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="14.17" height="14.17"><circle cx="7.08" cy="7.08" r="0.7" fill="#CFCDC4"/></svg>"##;

// Full-page cover gradient.
const COVER_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="260.18" height="462.55"><defs><linearGradient id="g" x1="0%" y1="0%" x2="100%" y2="100%"><stop offset="0%" stop-color="#1B365D"/><stop offset="100%" stop-color="#0F2444"/></linearGradient></defs><rect width="260.18" height="462.55" fill="url(#g)"/></svg>"##;

const HTML: &str = r#"<!doctype html><html><head><meta charset="utf-8"><style>
  * { box-sizing: border-box; margin: 0; padding: 0; }
  .page { width: 260.18pt; height: 462.55pt; position: relative; overflow: hidden;
          break-after: page; background: #fff; }
  .cover { width: 100%; height: 100%; display: flex; flex-direction: column;
           justify-content: flex-end; padding: 16pt; color: #fff;
           background-image: url(cover.svg); background-size: 100% 100%;
           background-repeat: no-repeat; }
  .cover .year { font-size: 9pt; letter-spacing: 3px; }
  .cover .title { font-size: 24pt; font-weight: bold; }
  .dotgrid { width: 100%; height: 100%;
             background-image: url(dot.svg); background-repeat: repeat;
             background-size: 14.17pt 14.17pt; }
  .daylist { padding: 14pt; }
  .h-month { color: #1B365D; font-size: 16pt; font-weight: bold; margin-bottom: 6pt; }
  .day { display: flex; align-items: center; gap: 8pt; height: 13pt;
         border-bottom: 0.25pt solid #eeeeee; }
  .day.ws { border-top: 0.6pt solid #D9D6CC; }
  .num { width: 16pt; text-align: right; font-weight: bold; }
  .wd { color: #1B365D; font-size: 8pt; width: 26pt; }
  .pill { background: #8B2E1F; color: #fff; border-radius: 8pt; padding: 0 6pt; font-size: 7pt; }
</style></head><body>
  <section class="page"><div class="cover">
    <div class="year">2026</div><div class="title">Future Log</div>
  </div></section>
  <section class="page"><div class="dotgrid"></div></section>
  <section class="page"><div class="daylist">
    <div class="h-month">May 2026</div>
    <div class="day"><span class="num">1</span><span class="wd">FRI</span></div>
    <div class="day"><span class="num">2</span><span class="wd">SAT</span></div>
    <div class="day ws"><span class="num">3</span><span class="wd">SUN</span></div>
    <div class="day"><span class="num">10</span><span class="wd">SUN</span></div>
    <div class="day"><span class="num">18</span><span class="wd">MON</span><span class="pill">Victoria Day</span></div>
  </div></section>
</body></html>"#;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut assets = AssetBundle::new();
    assets.add_image("dot.svg", DOT_SVG.as_bytes().to_vec());
    assets.add_image("cover.svg", COVER_SVG.as_bytes().to_vec());

    let engine = Engine::builder()
        .page_size(PageSize { width: 260.18, height: 462.55 })
        .margin(Margin::uniform(0.0))
        .assets(assets)
        .producer("rmbujo-spike")
        .creator("rmbujo-spike")
        .creation_date("D:20000101000000Z")
        .build();
    engine.render_html_to_file(HTML, "/tmp/rmbujo-spike/out2.pdf")?;
    println!("wrote /tmp/rmbujo-spike/out2.pdf");
    Ok(())
}
