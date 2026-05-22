use lopdf::dictionary;
use rmreader::embed;
use rmreader::manifest::{EmbeddedDoc, EmbeddedManifest, PageRange};

fn sample() -> EmbeddedManifest {
    EmbeddedManifest {
        v: 1,
        collection: "Library".into(),
        docs: vec![EmbeddedDoc {
            id: "abc".into(),
            title: "T".into(),
            url: "https://x/y".into(),
            author: "A".into(),
            category: "article".into(),
            page_range: PageRange { first: 1, last: 3 },
        }],
    }
}

#[test]
fn round_trips_through_save_and_reload() {
    let mut doc = lopdf::Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 200.into(), 200.into()],
    });
    let pages = dictionary! { "Type" => "Pages",
    "Kids" => vec![page_id.into()], "Count" => 1 };
    doc.objects
        .insert(pages_id, lopdf::Object::Dictionary(pages));
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);

    let m = sample();
    embed::write(&mut doc, &m).unwrap();
    let tmp = tempfile::NamedTempFile::new().unwrap();
    doc.save(tmp.path()).unwrap();

    let reloaded = lopdf::Document::load(tmp.path()).unwrap();
    let got = embed::read(&reloaded).unwrap().unwrap();
    assert_eq!(got, m);
}

#[test]
fn read_returns_none_when_absent() {
    let mut doc = lopdf::Document::with_version("1.5");
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog" });
    doc.trailer.set("Root", catalog_id);
    assert!(embed::read(&doc).unwrap().is_none());
}
