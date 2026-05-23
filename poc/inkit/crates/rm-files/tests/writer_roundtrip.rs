use std::io::Read;

use rm_files::{Pen, PenColor, Point, Scene, SceneItem, Stroke};

fn pt(x: f32, y: f32) -> Point {
    Point {
        x,
        y,
        speed: Some(0.0),
        direction: Some(0.0),
        width: Some(2.0),
        pressure: Some(0.0),
    }
}

#[test]
fn synthetic_strokes_round_trip() {
    let original = Stroke {
        tool: Pen::Highlighter2,
        color: PenColor::Highlight,
        points: vec![pt(-100.0, 50.0), pt(100.0, 50.0)],
    };
    let bytes = rm_files::write_scene(6, &[SceneItem::Line(original.clone())]);

    let scene = Scene::parse(&bytes).expect("parse written scene");
    assert_eq!(scene.version(), 6);
    let strokes = scene.strokes();
    assert_eq!(strokes.len(), 1);
    let got = strokes[0];
    assert_eq!(got.tool, Pen::Highlighter2);
    assert_eq!(got.color, PenColor::Highlight);
    let xs: Vec<f32> = got.points.iter().map(|p| p.x).collect();
    let ys: Vec<f32> = got.points.iter().map(|p| p.y).collect();
    assert_eq!(xs, vec![-100.0, 100.0]);
    assert_eq!(ys, vec![50.0, 50.0]);
}

fn load_fixture_bytes() -> Vec<u8> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/stamped-labels.rmdoc"
    );
    let file = std::fs::File::open(path).expect("open rmdoc");
    let mut archive = zip::ZipArchive::new(file).expect("read zip");
    let rm_name = (0..archive.len())
        .map(|i| archive.by_index(i).unwrap().name().to_string())
        .find(|n| n.ends_with(".rm"))
        .expect(".rm entry");
    let mut e = archive.by_name(&rm_name).unwrap();
    let mut b = Vec::new();
    e.read_to_end(&mut b).unwrap();
    b
}

#[test]
fn real_fixture_strokes_round_trip() {
    let bytes = load_fixture_bytes();
    let original: Vec<Stroke> = Scene::parse(&bytes)
        .unwrap()
        .strokes()
        .into_iter()
        .cloned()
        .collect();
    assert_eq!(original.len(), 4, "fixture has 4 strokes");

    let items: Vec<SceneItem> = original.iter().cloned().map(SceneItem::Line).collect();
    let rewritten = rm_files::write_scene(6, &items);

    let reparsed: Vec<Stroke> = Scene::parse(&rewritten)
        .unwrap()
        .strokes()
        .into_iter()
        .cloned()
        .collect();

    assert_eq!(reparsed.len(), original.len());
    for (stroke_idx, (a, b)) in original.iter().zip(&reparsed).enumerate() {
        assert_eq!(a.tool, b.tool, "stroke {stroke_idx}: tool preserved");
        assert_eq!(a.color, b.color, "stroke {stroke_idx}: color preserved");
        let ax: Vec<i32> = a.points.iter().map(|p| p.x.round() as i32).collect();
        let bx: Vec<i32> = b.points.iter().map(|p| p.x.round() as i32).collect();
        let ay: Vec<i32> = a.points.iter().map(|p| p.y.round() as i32).collect();
        let by: Vec<i32> = b.points.iter().map(|p| p.y.round() as i32).collect();
        assert_eq!(ax, bx, "stroke {stroke_idx}: x geometry preserved");
        assert_eq!(ay, by, "stroke {stroke_idx}: y geometry preserved");

        // v2 points encode telemetry as integer-valued u16/u8 decoded to f32,
        // so write→read is lossless; assert exact Option<f32> equality.
        assert_eq!(
            a.points.len(),
            b.points.len(),
            "stroke {stroke_idx}: point count preserved"
        );
        for (i, (pa, pb)) in a.points.iter().zip(&b.points).enumerate() {
            assert_eq!(
                pa.speed, pb.speed,
                "stroke {stroke_idx} point {i}: speed preserved (original={:?}, reparsed={:?})",
                pa.speed, pb.speed
            );
            assert_eq!(
                pa.width, pb.width,
                "stroke {stroke_idx} point {i}: width preserved (original={:?}, reparsed={:?})",
                pa.width, pb.width
            );
            assert_eq!(
                pa.direction, pb.direction,
                "stroke {stroke_idx} point {i}: direction preserved (original={:?}, reparsed={:?})",
                pa.direction, pb.direction
            );
            assert_eq!(
                pa.pressure, pb.pressure,
                "stroke {stroke_idx} point {i}: pressure preserved (original={:?}, reparsed={:?})",
                pa.pressure, pb.pressure
            );
        }
    }
}
