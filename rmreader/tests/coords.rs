use rmreader::readback::coords::Transform;

#[test]
fn maps_fixture_strokes_into_expected_pdf_regions() {
    let fx = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../rmfiles/tests/fixtures/stamped-labels.rmdoc");
    let b = rmfiles::Bundle::open(&fx).unwrap();
    let canvas = b.canvas_size(); // (1404, 1872)

    // Page size from the source PDF MediaBox.
    let pdf = b.source_pdf().unwrap().to_vec();
    let doc = lopdf::Document::load_mem(&pdf).unwrap();
    let page_id = *doc.get_pages().values().next().unwrap();
    let mb = doc
        .get_dictionary(page_id)
        .unwrap()
        .get(b"MediaBox")
        .unwrap()
        .as_array()
        .unwrap();
    let num = |o: &lopdf::Object| -> f64 {
        // lopdf uses as_float (f32) and as_i64; no as_f64 method exists.
        o.as_float()
            .map(|f| f as f64)
            .or_else(|_| o.as_i64().map(|i| i as f64))
            .unwrap()
    };
    let (page_w, page_h) = (num(&mb[2]) - num(&mb[0]), num(&mb[3]) - num(&mb[1])); // ~595 x 842
    let t = Transform::new(canvas, (page_w, page_h));

    // Collect highlighter stroke bboxes (device -> PDF).
    let mut boxes = Vec::new();
    for page in b.pages() {
        if let Some(scene) = page.scene().unwrap() {
            for s in scene.strokes() {
                if !s.is_highlighter() {
                    continue;
                }
                let pts = s.points.iter().map(|p| (p.x as f64, p.y as f64));
                boxes.push(t.pdf_bbox(pts).unwrap());
            }
        }
    }

    eprintln!("canvas: {:?}", canvas);
    eprintln!("page_w={page_w} page_h={page_h}");
    for (i, r) in boxes.iter().enumerate() {
        eprintln!(
            "  stroke {i}: x=[{:.1},{:.1}] y=[{:.1},{:.1}]",
            r.x0, r.x1, r.y0, r.y1
        );
    }

    assert!(
        boxes.len() >= 2,
        "expected at least 2 highlighter strokes, got {}",
        boxes.len()
    );

    // The label (ARCHIVE) stroke is near the TOP of the page (high PDF y, since
    // labels were stamped near y=775 on an ~842-tall page); body strokes are lower.
    // Observed values: label y1 ~777, body y0 ~706. Both sit in the upper portion of
    // the fixture page (it's a short document), but labels are distinctly above body.
    let max_y = boxes.iter().map(|r| r.y1).fold(f64::MIN, f64::max);
    let min_y = boxes.iter().map(|r| r.y0).fold(f64::MAX, f64::min);

    // Highest stroke is a label — it should be clearly in the upper region.
    assert!(
        max_y > page_h * 0.85,
        "expected a label stroke near top, max_y={max_y:.1} page_h={page_h:.1}"
    );
    // The label strokes (y1 near max_y) are higher than the body strokes (y near min_y).
    // A meaningful separation of at least 50 PDF points confirms the y-flip is right.
    assert!(
        max_y - min_y > 50.0,
        "label and body strokes should be well separated vertically; \
         max_y={max_y:.1} min_y={min_y:.1} spread={:.1}",
        max_y - min_y
    );

    // Label stroke (highest y1) sits in the left/center half.
    let label_box = boxes
        .iter()
        .max_by(|a, b| a.y1.partial_cmp(&b.y1).unwrap())
        .unwrap();
    assert!(
        label_box.x0 < page_w * 0.5,
        "label stroke should be in left half, x0={:.1} page_w={page_w:.1}",
        label_box.x0
    );
}
