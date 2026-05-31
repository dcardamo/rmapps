use std::collections::HashMap;

use inkapp_core::assets::{
    asset_key, asset_path, resolve_assets, AssetMap, FakeFetcher, OfflineFetcher, PLACEHOLDER_PNG,
};
use inkapp_core::render::{compile_to_document_with_sources_and_assets, document_to_pdf};

/// A 16×16 lossless WebP (generated offline; base64-encoded here).
const WEBP_16: &str =
    "UklGRjYAAABXRUJQVlA4ICoAAADQAQCdASoQABAAAgA0JaACdLoB+AADsAD+7L2P/PTNeYP8nP+3Jl5tsAA=";

fn webp_bytes() -> Vec<u8> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(WEBP_16)
        .unwrap()
}

fn assets_as_slice(map: &AssetMap) -> Vec<(String, Vec<u8>)> {
    map.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}

#[tokio::test]
async fn image_document_renders_with_registered_asset() {
    let url = "https://example.com/pic.webp";
    let key = asset_key(url);
    let path = asset_path(&key);

    let mut responses = HashMap::new();
    responses.insert(url.to_string(), webp_bytes());
    let fetcher = FakeFetcher::new(responses);

    let assets = resolve_assets(&[(key.clone(), url.to_string())], None, &fetcher).await;
    assert!(
        assets.contains_key(&path),
        "asset registered at virtual path"
    );

    let src = format!("#image(\"{path}\")\n");
    let doc = compile_to_document_with_sources_and_assets(&src, &[], &assets_as_slice(&assets))
        .expect("document with #image compiles");
    let pdf = document_to_pdf(&doc).expect("pdf export");
    assert!(pdf.len() > 100, "non-empty pdf");
}

#[tokio::test]
async fn missing_asset_uses_placeholder_and_still_compiles() {
    // OfflineFetcher fails every fetch -> placeholder registered at the key.
    let url = "https://example.com/missing.png";
    let key = asset_key(url);
    let path = asset_path(&key);

    let assets = resolve_assets(&[(key.clone(), url.to_string())], None, &OfflineFetcher).await;
    assert_eq!(assets.get(&path).unwrap(), PLACEHOLDER_PNG);

    let src = format!("#image(\"{path}\")\n");
    let doc = compile_to_document_with_sources_and_assets(&src, &[], &assets_as_slice(&assets))
        .expect("placeholder image still compiles");
    let pdf = document_to_pdf(&doc).expect("pdf export");
    assert!(pdf.len() > 100, "non-empty pdf for placeholder");
}

#[tokio::test]
async fn identical_inputs_produce_byte_identical_pdfs() {
    let url = "https://example.com/pic.webp";
    let key = asset_key(url);
    let path = asset_path(&key);
    let mut responses = HashMap::new();
    responses.insert(url.to_string(), webp_bytes());
    let fetcher = FakeFetcher::new(responses);

    let assets = resolve_assets(&[(key.clone(), url.to_string())], None, &fetcher).await;
    let src = format!("#image(\"{path}\")\n");

    let a = document_to_pdf(
        &compile_to_document_with_sources_and_assets(&src, &[], &assets_as_slice(&assets)).unwrap(),
    )
    .unwrap();
    let b = document_to_pdf(
        &compile_to_document_with_sources_and_assets(&src, &[], &assets_as_slice(&assets)).unwrap(),
    )
    .unwrap();
    assert_eq!(a, b, "two renders of identical inputs are byte-identical");
}
