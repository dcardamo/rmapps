use rmbujo::svg::{cover_svg, dot_tile_svg};

#[test]
fn dot_tile() {
    let s = dot_tile_svg(14.17, "#CFCDC4");
    assert!(s.contains("<circle"));
    assert!(s.contains("#CFCDC4"));
    assert!(s.contains("14.17"));
}

#[test]
fn cover() {
    let s = cover_svg(260.18, 462.55, "#1B365D", "#0F2444");
    assert!(s.contains("linearGradient"));
    assert!(s.contains("#1B365D"));
    assert!(s.contains("#0F2444"));
    assert!(s.contains("260.18"));
}
