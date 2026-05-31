use inkapp_core::components::checkbox::Checkbox;
use inkapp_core::geometry::{PdfPoint, PdfRect};
use inkapp_core::ink::{RegionInk, Stroke};
use inkapp_core::manifest::{Manifest, Region};

fn manifest_with(name: &str, rect: PdfRect) -> Manifest {
    Manifest {
        version: 1,
        regions: vec![Region {
            name: name.into(),
            page: 0,
            rect,
        }],
        ..Default::default()
    }
}

#[test]
fn checkbox_reads_true_when_marked() {
    let cb = Checkbox::new("done");
    let rect = PdfRect {
        x0: 10.0,
        y0: 10.0,
        x1: 30.0,
        y1: 30.0,
    };
    let manifest = manifest_with("done", rect);
    let ink = RegionInk {
        region: "done".into(),
        strokes: vec![Stroke {
            points: vec![PdfPoint { x: 20.0, y: 20.0 }],
            highlighter: false,
        }],
    };
    assert!(cb.read(&[ink], &manifest));
}

#[test]
fn checkbox_reads_false_when_empty() {
    let cb = Checkbox::new("done");
    let rect = PdfRect {
        x0: 10.0,
        y0: 10.0,
        x1: 30.0,
        y1: 30.0,
    };
    let manifest = manifest_with("done", rect);
    assert!(!cb.read(&[], &manifest));
}

#[test]
fn checkbox_render_declares_its_region() {
    let cb = Checkbox::new("done");
    let markup = cb.render_at(0, 10.0, 10.0, 20.0, 20.0);
    assert!(markup.contains("<region>"), "declares a region label");
    assert!(markup.contains("name: \"done\""), "names the region");
}

#[test]
fn checkbox_region_recovers_after_render() {
    use inkapp_core::manifest::recover_regions;
    use inkapp_core::render::compile_to_document;
    let cb = Checkbox::new("done");
    let body = cb.render_at(0, 20.0, 40.0, 16.0, 16.0);
    let src = format!("#set page(width: 200pt, height: 200pt, margin: 0pt)\n{body}");
    let doc = compile_to_document(&src).unwrap();
    let m = recover_regions(&doc).unwrap();
    assert!(
        m.regions.iter().any(|r| r.name == "done"),
        "rendered region recovers"
    );
}
