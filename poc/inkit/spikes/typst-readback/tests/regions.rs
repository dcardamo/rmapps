use typst_readback::regions::{typst_to_pdf_rect, PdfRect};

const DOC: &str = include_str!("fixtures/regions.typ");

#[test]
fn recovers_region_rects_in_pdf_coords() {
    let (pdf, regions, page_h) = typst_readback::regions::compile_with_regions(DOC).unwrap();

    assert!(pdf.starts_with(b"%PDF"), "output is not a PDF");

    // Region "a": top-left (20, 40) in Typst coords, size 60x24.
    let a = regions
        .iter()
        .find(|r| r.name == "a")
        .expect("region 'a' not found");
    let got = typst_to_pdf_rect(a, page_h);

    // page height 300pt:
    //   pdf y0 = 300 - (40 + 24) = 236
    //   pdf y1 = 300 - 40        = 260
    let want = PdfRect {
        x0: 20.0,
        y0: 300.0 - 64.0,
        x1: 80.0,
        y1: 300.0 - 40.0,
    };

    let close = |x: f64, y: f64| (x - y).abs() < 1.0;
    assert!(
        close(got.x0, want.x0)
            && close(got.y0, want.y0)
            && close(got.x1, want.x1)
            && close(got.y1, want.y1),
        "got {got:?}  want {want:?}\n  a region raw: {a:?}  page_h: {page_h}"
    );
}

#[test]
fn rendered_border_falls_inside_recovered_rect() {
    let (pdf, regions, page_h) = typst_readback::regions::compile_with_regions(DOC).unwrap();

    // Write PDF to a temp dir and rasterise at 72 dpi (1 px == 1 pt).
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("p.pdf"), &pdf).unwrap();
    let st = std::process::Command::new("pdftoppm")
        .args(["-r", "72", "-png", "p.pdf", "p"])
        .current_dir(dir.path())
        .status()
        .unwrap();
    assert!(st.success(), "pdftoppm failed");

    let img = image::open(dir.path().join("p-1.png")).unwrap().to_luma8();

    let a = regions
        .iter()
        .find(|r| r.name == "a")
        .expect("region 'a' not found");
    let r = typst_to_pdf_rect(a, page_h);

    // At 72 dpi, 1 px == 1 pt.
    // PDF y-up -> image y-down: img_y = page_h - pdf_y.
    // The top border of the box in image coords is at img_y = page_h - r.y1.
    // We sample the midpoint of the top edge.
    let px = (((r.x0 + r.x1) / 2.0) as u32).min(img.width() - 1);
    let py_center = (page_h - r.y1) as u32;

    // Sample a 3-wide, 3-tall neighbourhood around the expected border pixel
    // to guard against sub-pixel rounding differences.
    let min_luma = (-1i32..=1)
        .flat_map(|dy| (-1i32..=1).map(move |dx| (dx, dy)))
        .map(|(dx, dy)| {
            let x = (px as i32 + dx).clamp(0, img.width() as i32 - 1) as u32;
            let y = (py_center as i32 + dy).clamp(0, img.height() as i32 - 1) as u32;
            img.get_pixel(x, y).0[0]
        })
        .min()
        .unwrap();

    assert!(
        min_luma < 200,
        "expected dark border pixel near box top edge at img ({px},{py_center}), \
         min luma in 3×3 neighbourhood = {min_luma}"
    );
}
