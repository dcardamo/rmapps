//! Generated SVG assets (fulgur 0.6 does not paint CSS gradients).

/// A single dot-grid cell, tiled via CSS `background-repeat`.
pub fn dot_tile_svg(spacing_pt: f32, dot_color: &str) -> String {
    let c = spacing_pt / 2.0;
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{s}\" height=\"{s}\">\
         <circle cx=\"{c}\" cy=\"{c}\" r=\"0.5\" fill=\"{col}\"/></svg>",
        s = spacing_pt,
        c = c,
        col = dot_color,
    )
}

/// Full-page cover gradient.
pub fn cover_svg(width_pt: f32, height_pt: f32, from: &str, to: &str) -> String {
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\">\
         <defs><linearGradient id=\"g\" x1=\"0%\" y1=\"0%\" x2=\"100%\" y2=\"100%\">\
         <stop offset=\"0%\" stop-color=\"{from}\"/>\
         <stop offset=\"100%\" stop-color=\"{to}\"/></linearGradient></defs>\
         <rect width=\"{w}\" height=\"{h}\" fill=\"url(#g)\"/></svg>",
        w = width_pt,
        h = height_pt,
        from = from,
        to = to,
    )
}
