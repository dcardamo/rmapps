use std::io::Read;

use rm_files::{block_structure, write_scene, Scene, SceneItem, Stroke};

/// reMarkable v6 block type for a scene line item (ink stroke).
const BLOCK_TYPE_LINE: u8 = 0x05;

fn fixture_rm_bytes() -> Vec<u8> {
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
fn writer_output_is_all_line_items() {
    let real = fixture_rm_bytes();
    let strokes: Vec<Stroke> = Scene::parse(&real)
        .unwrap()
        .strokes()
        .into_iter()
        .cloned()
        .collect();
    assert!(!strokes.is_empty(), "fixture has strokes");

    let items: Vec<SceneItem> = strokes.iter().cloned().map(SceneItem::Line).collect();
    let written = write_scene(6, &items);

    let ours = block_structure(&written).unwrap();
    assert_eq!(ours.len(), strokes.len(), "one block per stroke");
    assert!(
        ours.iter()
            .all(|b| b.block_type == BLOCK_TYPE_LINE && b.current_version == 2),
        "writer emits only v2 line-item blocks"
    );
}

#[test]
fn real_file_carries_scaffolding_we_omit() {
    let real = fixture_rm_bytes();
    let real_struct = block_structure(&real).unwrap();

    let line_blocks = real_struct
        .iter()
        .filter(|b| b.block_type == BLOCK_TYPE_LINE)
        .count();
    let non_line_blocks = real_struct
        .iter()
        .filter(|b| b.block_type != BLOCK_TYPE_LINE)
        .count();

    assert!(line_blocks > 0, "real file has line items");
    // The minimal writer omits the CRDT/scaffolding blocks (author ids, migration
    // info, page/scene tree, group items) a device file carries. This documents
    // that gap; device-acceptance (Task 9) is the render gate, not byte-identity.
    assert!(
        non_line_blocks > 0,
        "real device file carries scaffolding blocks our writer intentionally omits"
    );
}
