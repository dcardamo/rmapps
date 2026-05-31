# Spike: fulgur (HTML/CSS → PDF, no browser) — 2026-05-20

Goal: decide the Rust rendering engine for rmbujo by rendering the three hardest
elements at the Paper Pro Move's exact page size (260.18 × 462.55 pt, 9:16).

Validated spike code: `2026-05-20-fulgur-spike-main.rs` (this directory).

## Findings

| Capability | Result |
|------------|--------|
| Custom page size (`PageSize { width, height }` pts / `@page`) | ✅ |
| Flexbox layout, text, navy headers, weekday colors, per-row rules | ✅ |
| Week-start divider (border on a flagged row) | ✅ |
| Pills (`border-radius` + bg color + white text) | ✅ |
| Deterministic metadata (`producer`/`creator`/`creation_date` builder hooks) | ✅ |
| Byte-deterministic output across runs | ✅ |
| **CSS gradients** (`radial-gradient`, `linear-gradient`) | ❌ not painted by fulgur 0.6 |
| Dot grid as **tiled SVG** asset (`url(dot.svg)` + `background-repeat`) | ✅ |
| Cover gradient as **full-page SVG** asset (`url(cover.svg)`) | ✅ |
| Layout inspection (`fulgur::inspect` → `TextItem{x,y,w,h,text}`) | ✅ usable for overlap tests |
| Nix-on-macOS build | ✅ once `libiconv` is in the dev shell |

## Key decisions driven by the spike

1. **Engine = fulgur** (Blitz + krilla). No headless browser. Byte-deterministic. Single binary.
2. **Gradients are unsupported** → the dot grid and cover are generated as **SVG assets**
   (deterministic, from theme colors), registered via `AssetBundle::add_image` and
   referenced with `url(...)`.
3. **Toolchain**: the only build gap was `ld: library not found for -liconv`; fixed by
   adding `libiconv` to the Nix dev shell. The whole Stylo/Blitz/krilla tree compiles.
4. **Layout testing is feasible**: `fulgur::inspect::inspect(pdf)` returns laid-out text
   boxes, enabling geometric overlap + within-bounds assertions (catches the font-overlap
   bug observed during theme prototyping).

## fulgur API used

```rust
let mut assets = AssetBundle::new();
assets.add_image("dot.svg", dot_svg_bytes);
assets.add_image("cover.svg", cover_svg_bytes);
// assets.add_font_bytes(ttf_bytes)?;   // for the real build (vendored font)

let engine = Engine::builder()
    .page_size(PageSize { width: 260.18, height: 462.55 })
    .margin(Margin::uniform(0.0))
    .assets(assets)
    .producer("rmbujo").creator("rmbujo").creation_date("D:20000101000000Z")
    .build();
let pdf_bytes = engine.render_html(html)?;   // or render_html_to_file
```

fulgur is `0.x` (unstable API) — pin via `Cargo.lock`.
