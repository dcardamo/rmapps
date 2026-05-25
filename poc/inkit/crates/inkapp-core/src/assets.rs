//! Reusable offline image pipeline: an image fetch seam, PNG normalization, and
//! a cache-backed resolver that maps `(key, url)` pairs to Typst-servable PNG
//! bytes at the virtual path `/assets/{key}.png`.
//!
//! Any image that fails to fetch or normalize is served as a 1×1 transparent
//! placeholder, so an already-emitted `#image("/assets/{key}.png")` call can
//! never dangle and compilation never fails.

use std::collections::HashMap;
use std::io::Cursor;

use image::GenericImageView;
use sha2::{Digest, Sha256};

/// Map from Typst virtual path (`/assets/{key}.png`) to PNG bytes.
pub type AssetMap = HashMap<String, Vec<u8>>;

/// A 1×1 fully-transparent PNG, served whenever an image fails to fetch or
/// normalize. A fixed byte array (never re-encoded at runtime) so renders stay
/// byte-identical across builds and `image`-crate versions.
pub const PLACEHOLDER_PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0b, 0x49, 0x44, 0x41, 0x54, 0x08, 0xd7, 0x63, 0x60, 0x00, 0x02, 0x00,
    0x00, 0x05, 0x00, 0x01, 0xe2, 0x26, 0x05, 0x9b, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44,
    0xae, 0x42, 0x60, 0x82,
];

/// The asset key for a URL: the first 16 hex chars of `sha256(url)`. Single
/// source of truth shared by components (when emitting `#image`) and the
/// resolver, so the key can never diverge between the two sides.
pub fn asset_key(url: &str) -> String {
    let hex = format!("{:x}", Sha256::digest(url.as_bytes()));
    hex[..16].to_string()
}

/// The Typst virtual path an asset key is served at.
pub fn asset_path(key: &str) -> String {
    format!("/assets/{key}.png")
}

/// Decode arbitrary image bytes and re-encode to PNG, dropping ≤2px tracking
/// pixels. Returns `None` to drop the image (tracking pixel or undecodable).
///
/// Unlike the rmreader port this is taken from, EVERYTHING is transcoded to PNG
/// (not just webp/avif): the served virtual path is always `.png`, so the bytes
/// must be PNG regardless of source format. Re-encoding is deterministic for a
/// given input and `image`-crate version.
pub(crate) fn normalize_to_png(bytes: &[u8]) -> Option<Vec<u8>> {
    let img = image::load_from_memory(bytes).ok()?;
    let (w, h) = img.dimensions();
    if w <= 2 || h <= 2 {
        return None; // tracking pixel
    }
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

// ── fetch seam ───────────────────────────────────────────────────────────────

/// How the pipeline fetches image bytes for a URL. Mirrors the readwise
/// connector's `FetchTransport` seam: a trait with a fake for tests and a real
/// concurrent retrying HTTP implementation.
#[async_trait::async_trait]
pub trait ImageFetcher: Send + Sync {
    /// Fetch one URL's raw bytes, or `None` on any failure.
    async fn fetch(&self, url: &str) -> Option<Vec<u8>>;

    /// Fetch many URLs, results in input order. The default runs them
    /// concurrently; the real HTTP fetcher shares one connection pool, so this
    /// gives concurrent downloads for free.
    async fn fetch_many(&self, urls: &[String]) -> Vec<Option<Vec<u8>>> {
        futures::future::join_all(urls.iter().map(|u| self.fetch(u))).await
    }
}

/// Test fetcher: returns canned bytes for known URLs, `None` otherwise.
pub struct FakeFetcher {
    responses: HashMap<String, Vec<u8>>,
}

impl FakeFetcher {
    pub fn new(responses: HashMap<String, Vec<u8>>) -> Self {
        Self { responses }
    }
}

#[async_trait::async_trait]
impl ImageFetcher for FakeFetcher {
    async fn fetch(&self, url: &str) -> Option<Vec<u8>> {
        self.responses.get(url).cloned()
    }
}

/// Offline fetcher: always `None`. The `App`'s default, so nothing hits the
/// network unless a live build injects `HttpImageFetcher`.
pub struct OfflineFetcher;

#[async_trait::async_trait]
impl ImageFetcher for OfflineFetcher {
    async fn fetch(&self, _url: &str) -> Option<Vec<u8>> {
        None
    }
}

/// Real HTTP fetcher with exponential-backoff retry on transient failures
/// (429 / 5xx), mirroring the readwise connector's `retrying_http_client`. A
/// non-2xx status or transport error yields `None`, which the resolver turns
/// into a placeholder.
pub struct HttpImageFetcher {
    client: reqwest_middleware::ClientWithMiddleware,
}

impl HttpImageFetcher {
    pub fn new() -> Self {
        use reqwest_middleware::ClientBuilder;
        use reqwest_retry::{policies::ExponentialBackoff, RetryTransientMiddleware};

        let retry_policy = ExponentialBackoff::builder().build_with_max_retries(5);
        let client = ClientBuilder::new(reqwest::Client::new())
            .with(RetryTransientMiddleware::new_with_policy(retry_policy))
            .build();
        Self { client }
    }
}

impl Default for HttpImageFetcher {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl ImageFetcher for HttpImageFetcher {
    async fn fetch(&self, url: &str) -> Option<Vec<u8>> {
        let resp = self.client.get(url).send().await.ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let bytes = resp.bytes().await.ok()?;
        Some(bytes.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 16×16 lossless WebP (generated offline; base64-encoded here).
    const WEBP_16: &str =
        "UklGRjYAAABXRUJQVlA4ICoAAADQAQCdASoQABAAAgA0JaACdLoB+AADsAD+7L2P/PTNeYP8nP+3Jl5tsAA=";
    /// A 16×16 AVIF (generated offline; base64-encoded here).
    const AVIF_16: &str = "AAAAHGZ0eXBhdmlmAAAAAG1pZjFhdmlmbWlhZgAAANZtZXRhAAAAAAAAACFoZGxyAAAAAAAAAABwaWN0AAAAAAAAAAAAAAAAAAAAAA5waXRtAAAAAAABAAAAImlsb2MAAAAAREAAAQABAAAAAAD6AAEAAAAAAAAAIAAAACNpaW5mAAAAAAABAAAAFWluZmUCAAAAAAEAAGF2MDEAAAAAVmlwcnAAAAA4aXBjbwAAAAxhdjFDgQAMAAAAABRpc3BlAAAAAAAAABAAAAAQAAAAEHBpeGkAAAAAAwgICAAAABZpcG1hAAAAAAAAAAEAAQOBAgMAAAAobWRhdBIACgkYDP/YICGg0IAyERgACiiihAAAqY6ASKpaJbZA";

    fn b64(s: &str) -> Vec<u8> {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.decode(s).unwrap()
    }

    fn png_of(w: u32, h: u32) -> Vec<u8> {
        let img = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            w,
            h,
            image::Rgba([10, 120, 200, 255]),
        ));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    fn is_png(bytes: &[u8]) -> bool {
        bytes.starts_with(&[0x89, b'P', b'N', b'G'])
    }

    #[test]
    fn asset_key_is_16_hex_and_stable() {
        let k = asset_key("https://example.com/a.jpg");
        assert_eq!(k.len(), 16);
        assert!(k.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(k, asset_key("https://example.com/a.jpg"));
        assert_ne!(k, asset_key("https://example.com/b.jpg"));
    }

    #[test]
    fn asset_path_format() {
        assert_eq!(
            asset_path("deadbeefdeadbeef"),
            "/assets/deadbeefdeadbeef.png"
        );
    }

    #[test]
    fn drops_tracking_pixels() {
        assert!(normalize_to_png(&png_of(1, 1)).is_none());
        assert!(normalize_to_png(&png_of(2, 2)).is_none());
        assert!(normalize_to_png(&png_of(2, 50)).is_none());
        assert!(normalize_to_png(PLACEHOLDER_PNG).is_none());
    }

    #[test]
    fn keeps_and_transcodes_real_images() {
        // PNG passthrough (re-encoded, still PNG).
        assert!(is_png(&normalize_to_png(&png_of(16, 16)).unwrap()));
        // WebP -> PNG.
        assert!(is_png(&normalize_to_png(&b64(WEBP_16)).unwrap()));
        // AVIF -> PNG (requires the avif-native feature + dav1d).
        assert!(is_png(&normalize_to_png(&b64(AVIF_16)).unwrap()));
        // JPEG -> PNG (encode a jpeg inline, then normalize it).
        let jpeg = {
            let img = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
                16,
                16,
                image::Rgba([200, 80, 80, 255]),
            ));
            let mut out = std::io::Cursor::new(Vec::new());
            img.write_to(&mut out, image::ImageFormat::Jpeg).unwrap();
            out.into_inner()
        };
        assert!(is_png(&normalize_to_png(&jpeg).unwrap()));
    }

    #[test]
    fn placeholder_is_a_1x1_png() {
        use image::GenericImageView;
        assert!(is_png(PLACEHOLDER_PNG));
        let img = image::load_from_memory(PLACEHOLDER_PNG).unwrap();
        assert_eq!(img.dimensions(), (1, 1));
    }

    #[tokio::test]
    async fn fake_fetcher_hits_and_misses() {
        let mut responses = HashMap::new();
        responses.insert("u1".to_string(), b"one".to_vec());
        let f = FakeFetcher::new(responses);
        assert_eq!(f.fetch("u1").await, Some(b"one".to_vec()));
        assert_eq!(f.fetch("missing").await, None);
    }

    #[tokio::test]
    async fn fetch_many_preserves_order() {
        let mut responses = HashMap::new();
        responses.insert("a".to_string(), b"A".to_vec());
        responses.insert("c".to_string(), b"C".to_vec());
        let f = FakeFetcher::new(responses);
        let urls = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let got = f.fetch_many(&urls).await;
        assert_eq!(got, vec![Some(b"A".to_vec()), None, Some(b"C".to_vec())]);
    }

    #[tokio::test]
    async fn offline_fetcher_is_always_none() {
        assert_eq!(OfflineFetcher.fetch("anything").await, None);
    }

    #[test]
    fn http_fetcher_builds() {
        // Construction must not panic (exercises the middleware/retry wiring).
        let _ = HttpImageFetcher::new();
    }
}
