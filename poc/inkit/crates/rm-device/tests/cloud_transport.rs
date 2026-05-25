//! `CloudTransport` against the in-process fake cloud: push creates a doc then
//! content-only-updates it by key, delete removes it by key, and pull decodes the
//! device ink back under its key. No real device, no network — the fake cloud is
//! the same one rm-cloud's own suite uses.

use std::collections::HashMap;

use inkapp_core::device::Device;
use inkapp_core::geometry::PdfPoint;
use inkapp_core::ink::Stroke;
use inkapp_core::sync::DeviceTransport;
use rm_cloud::fake::FakeCloud;
use rm_cloud::{Client, Config};
use rm_device::{CloudTransport, Remarkable};

fn client(cloud: &FakeCloud) -> Client {
    Client::from_user_token(Config::single_host(&cloud.base), "user-token")
}

#[tokio::test]
async fn push_creates_then_content_only_updates_by_key() {
    let cloud = FakeCloud::spawn().await;
    let client = client(&cloud);
    let t = CloudTransport::with_client(client.clone(), "/ReadingQueue");

    t.push("article-7", b"%PDF first").await.unwrap();

    // One document under the folder, named by the key.
    let folder = client.mkdir_p("/ReadingQueue").await.unwrap();
    let listing = client.ls(&folder).await.unwrap();
    assert_eq!(listing.len(), 1);
    assert_eq!(listing[0].name, "article-7");
    assert!(!listing[0].is_folder);
    let id_before = listing[0].id.clone();

    // Second push reuses the same doc (content-only swap), not a fresh one.
    t.push("article-7", b"%PDF second").await.unwrap();
    let listing = client.ls(&folder).await.unwrap();
    assert_eq!(listing.len(), 1, "no duplicate doc on re-push");
    assert_eq!(listing[0].id, id_before, "same doc id (content-only swap)");
    let got = client.get(&id_before).await.unwrap();
    assert_eq!(
        got.get(&format!("{id_before}.pdf")).unwrap(),
        b"%PDF second",
        "PDF blob was replaced"
    );
}

#[tokio::test]
async fn delete_removes_doc_by_key() {
    let cloud = FakeCloud::spawn().await;
    let client = client(&cloud);
    let t = CloudTransport::with_client(client.clone(), "/Agenda");

    t.push("event-3", b"%PDF e").await.unwrap();
    let folder = client.mkdir_p("/Agenda").await.unwrap();
    assert_eq!(client.ls(&folder).await.unwrap().len(), 1);

    t.delete("event-3").await;
    assert!(
        client.ls(&folder).await.unwrap().is_empty(),
        "the keyed doc is gone"
    );
    // Deleting a missing key is a no-op, not a panic/error.
    t.delete("event-3").await;
}

#[tokio::test]
async fn pull_decodes_ink_back_under_its_key() {
    let cloud = FakeCloud::spawn().await;
    let client = client(&cloud);
    let t = CloudTransport::with_client(client.clone(), "/ReadingQueue");
    let device = Remarkable::new();
    let page_h = 560.0;

    // Create the doc, then simulate the device adding an ink layer: a `.content`
    // page list naming one page plus that page's `<id>/<page>.rm` scene.
    t.push("article-7", b"%PDF body").await.unwrap();
    let folder = client.mkdir_p("/ReadingQueue").await.unwrap();
    let id = client.ls(&folder).await.unwrap()[0].id.clone();

    let strokes = vec![Stroke {
        points: vec![
            PdfPoint { x: 100.0, y: 200.0 },
            PdfPoint { x: 150.0, y: 220.0 },
        ],
        highlighter: false,
    }];
    let rm = device.write_ink(&strokes, page_h).unwrap();

    let mut df = client.get(&id).await.unwrap();
    df.files.retain(|(n, _)| !n.ends_with(".content"));
    df.files.push((
        format!("{id}.content"),
        br#"{"cPages":{"pages":[{"id":"p0"}]}}"#.to_vec(),
    ));
    df.files.push((format!("{id}/p0.rm"), rm));
    client.put(df).await.unwrap();

    let mut page_h_by_key = HashMap::new();
    page_h_by_key.insert("article-7".to_string(), page_h);
    let ink = t.pull(&page_h_by_key).await;

    assert!(ink.contains_key("article-7"), "ink mapped back to its key");
    assert_eq!(ink["article-7"].len(), 1, "single page");
    assert_eq!(ink["article-7"][0].len(), 1, "one stroke round-tripped");
}
