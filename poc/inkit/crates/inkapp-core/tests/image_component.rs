use std::collections::HashMap;
use std::sync::Arc;

use inkapp_core::assets::{asset_key, asset_path, FakeFetcher, ImageFetcher, OfflineFetcher};
use inkapp_core::component::{Component, RenderCx};
use inkapp_core::connector::{Connector, ConnectorSet};
use inkapp_core::crypto::Key;
use inkapp_core::document::{Document, Documents};
use inkapp_core::flow;
use inkapp_core::ink::RegionInk;
use inkapp_core::manifest::Manifest;
use inkapp_core::runtime::{app, DocSet};

/// A 16×16 lossless WebP (generated offline; base64-encoded here).
const WEBP_16: &str =
    "UklGRjYAAABXRUJQVlA4ICoAAADQAQCdASoQABAAAgA0JaACdLoB+AADsAD+7L2P/PTNeYP8nP+3Jl5tsAA=";

fn webp_bytes() -> Vec<u8> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(WEBP_16)
        .unwrap()
}

const IMG_URL: &str = "https://example.com/pic.webp";

/// A component that renders an `#image` for IMG_URL and declares it via image_urls.
struct ImageCard;

impl Component for ImageCard {
    type Msg = ();
    fn render(&self, _cx: &mut RenderCx) -> String {
        format!("#image(\"{}\")\n", asset_path(&asset_key(IMG_URL)))
    }
    fn decode(&self, _ink: &[RegionInk], _manifest: &Manifest) -> Vec<()> {
        Vec::new()
    }
    fn image_urls(&self) -> Vec<String> {
        vec![IMG_URL.to_string()]
    }
}

struct Cx;
impl ConnectorSet for Cx {
    fn connectors(&self) -> Vec<Arc<dyn Connector>> {
        vec![]
    }
}

struct Model;
fn update(_m: (), _model: &mut Model, _cx: &Cx) {}
fn view(_m: &Model, _cx: &Cx) -> Documents<()> {
    Documents(vec![Document::keyed("card", flow![ImageCard])])
}

#[tokio::test]
async fn app_resolves_and_embeds_declared_image() {
    let mut responses = HashMap::new();
    responses.insert(IMG_URL.to_string(), webp_bytes());
    let fetcher: Arc<dyn ImageFetcher> = Arc::new(FakeFetcher::new(responses));

    let mut application = app(Model)
        .connector(Cx)
        .update(update)
        .view(view)
        .key(Key::from_bytes([5u8; 32]))
        .fetcher(fetcher)
        .build();

    let mut set = DocSet::default();
    let rendered = application.render(&mut set).await.expect("render succeeds");
    assert_eq!(rendered.len(), 1);
    assert!(
        rendered[0].pdf.len() > 100,
        "image embedded -> non-empty pdf"
    );
}

#[tokio::test]
async fn app_uses_placeholder_when_offline() {
    // Default fetcher is OfflineFetcher; the declared image fails to fetch but the
    // placeholder is registered, so compilation still succeeds.
    let mut application = app(Model)
        .connector(Cx)
        .update(update)
        .view(view)
        .key(Key::from_bytes([5u8; 32]))
        .fetcher(Arc::new(OfflineFetcher))
        .build();

    let mut set = DocSet::default();
    let rendered = application
        .render(&mut set)
        .await
        .expect("offline render still succeeds via placeholder");
    assert_eq!(rendered.len(), 1);
    assert!(rendered[0].pdf.len() > 100);
}
