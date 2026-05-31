# Offline Image Pipeline Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers-extended-cc:subagent-driven-development (recommended) or superpowers-extended-cc:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a reusable, deterministic, offline image pipeline to `inkapp-core` so a Typst `#image("/assets/{key}.png")` call resolves to fetched-and-normalized PNG bytes served from `InkWorld`, falling back to a 1×1 transparent placeholder on any failure.

**Architecture:** A new `assets.rs` module holds the fetch seam (`ImageFetcher` trait + fake/offline/HTTP impls), a `normalize_to_png` step, and `resolve_assets` which maps `(key, url)` pairs through a durable `Cache` into an `AssetMap` (`/assets/{key}.png` → PNG bytes), substituting `PLACEHOLDER_PNG` for any failure. `InkWorld` gains an asset table its `file()` serves. The compile path gets additive `*_with_assets` variants (originals delegate, so every existing caller is untouched), and `App` auto-collects each component's declared `image_urls()`, resolves them, and threads the map into rendering.

**Tech Stack:** Rust, Typst 0.14, the `image` crate 0.25 (with `avif-native`/libdav1d for AVIF decode), `reqwest`/`reqwest-middleware`/`reqwest-retry` (mirroring the readwise connector), `foyer`-backed `inkapp_core::cache::Cache`, `async-trait`, `futures`.

**Repo conventions (apply to EVERY task):**
- Commit with the hook-workaround form so the open-native-task pre-commit hook does not block, while `.githooks` keeps `cargo fmt --check` active:
  `git -c core.hooksPath=.githooks commit -m "..."`
- **Do NOT `git add Cargo.lock`** in implementer commits. The controller sweeps and commits the lockfile separately at the end (Task 7).
- **Do NOT use any Task tools / do not touch the native task list** (implementer subagents share the parent list).
- Run `cargo fmt` before committing (the hook checks formatting).
- This worktree is on branch `imgfetch`; the working directory is the repo root.

---

### Task 1: Image primitives — `asset_key`, `normalize_to_png`, `PLACEHOLDER_PNG`

**Goal:** Create the `assets` module with the pure, synchronous image primitives and wire up the `image` crate dependency (incl. AVIF decode via dav1d).

**Files:**
- Create: `crates/inkapp-core/src/assets.rs`
- Modify: `crates/inkapp-core/src/lib.rs` (add `pub mod assets;`)
- Modify: `crates/inkapp-core/Cargo.toml` (add `image` dep)
- Modify: `flake.nix` (add `pkgs.dav1d` to `buildInputs`)

**Acceptance Criteria:**
- [ ] `asset_key(url)` returns the first 16 hex chars of `sha256(url)`; stable across calls.
- [ ] `asset_path(key)` returns `/assets/{key}.png`.
- [ ] `normalize_to_png` drops images ≤2px on either side (returns `None`), and transcodes jpeg/webp/avif/png inputs to PNG bytes (output starts with the PNG magic).
- [ ] `PLACEHOLDER_PNG` is a fixed 1×1 transparent PNG byte slice.
- [ ] `cargo test -p inkapp-core assets::` passes inside the nix devshell.

**Verify:** `nix develop --command cargo test -p inkapp-core assets::tests` → all pass

**Steps:**

- [ ] **Step 1: Add the `image` dependency**

In `crates/inkapp-core/Cargo.toml`, under `[dependencies]`, add (after the `async-trait = "0.1"` line):

```toml
image = { version = "0.25", features = ["avif-native"] }
```

(Default features supply webp/jpeg/gif/png decode; `avif-native` adds AVIF decode through libdav1d.)

- [ ] **Step 2: Add libdav1d to the nix devshell**

In `flake.nix`, add `pkgs.dav1d` to the `buildInputs` list. The list becomes:

```nix
          buildInputs = [
            pkgs.libiconv pkgs.fontconfig pkgs.dejavu_fonts pkgs.noto-fonts
            pkgs.poppler-utils pkgs.rmapi pkgs.dav1d
          ];
```

`pkg-config` is already a `nativeBuildInput`; `mkShell` exports `dav1d`'s dev-output `PKG_CONFIG_PATH` automatically, so `dav1d-sys` (pulled by `avif-native`) links. (Verified: a spike built `image` with `avif-native` against this dav1d and decoded an AVIF to PNG.)

- [ ] **Step 3: Register the module**

In `crates/inkapp-core/src/lib.rs`, add the module declaration in alphabetical position — immediately before the existing `pub mod cache;` line (there is no `app` module; `assets` is the new first entry):

```rust
pub mod assets;
```

- [ ] **Step 4: Write the module with primitives + failing tests**

Create `crates/inkapp-core/src/assets.rs`:

```rust
//! Reusable offline image pipeline: an image fetch seam, PNG normalization, and
//! a cache-backed resolver that maps `(key, url)` pairs to Typst-servable PNG
//! bytes at the virtual path `/assets/{key}.png`.
//!
//! Any image that fails to fetch or normalize is served as a 1×1 transparent
//! placeholder, so an already-emitted `#image("/assets/{key}.png")` call can
//! never dangle and compilation never fails.

use std::collections::HashMap;

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
    use image::GenericImageView;
    let img = image::load_from_memory(bytes).ok()?;
    let (w, h) = img.dimensions();
    if w <= 2 || h <= 2 {
        return None; // tracking pixel
    }
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
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
        assert_eq!(asset_path("deadbeefdeadbeef"), "/assets/deadbeefdeadbeef.png");
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
}
```

- [ ] **Step 5: Run the tests**

Run: `nix develop --command cargo test -p inkapp-core assets::tests`
Expected: all six tests PASS (notably `keeps_and_transcodes_real_images`, which exercises the AVIF/dav1d path).

- [ ] **Step 6: Commit**

```bash
cargo fmt
git add crates/inkapp-core/src/assets.rs crates/inkapp-core/src/lib.rs crates/inkapp-core/Cargo.toml flake.nix
git -c core.hooksPath=.githooks commit -m "assets: image primitives (asset_key, normalize_to_png, placeholder) + image/dav1d deps"
```

---

### Task 2: Fetch seam — `ImageFetcher` trait + fake/offline/HTTP impls

**Goal:** Add the pluggable fetch seam to `assets.rs`: an async `ImageFetcher` trait with a concurrent `fetch_many` default, a `FakeFetcher` for tests, an `OfflineFetcher` default, and a real retrying `HttpImageFetcher`.

**Files:**
- Modify: `crates/inkapp-core/src/assets.rs`
- Modify: `crates/inkapp-core/Cargo.toml` (add `reqwest`, `reqwest-middleware`, `reqwest-retry`)

**Acceptance Criteria:**
- [ ] `ImageFetcher::fetch_many` returns results in input order and is concurrent by default.
- [ ] `FakeFetcher` returns canned bytes for known URLs and `None` otherwise.
- [ ] `OfflineFetcher::fetch` always returns `None`.
- [ ] `HttpImageFetcher::new()` builds a retrying middleware client (5 retries) mirroring the readwise connector; non-2xx or transport error yields `None`.
- [ ] `cargo test -p inkapp-core assets::` passes.

**Verify:** `nix develop --command cargo test -p inkapp-core assets::tests` → all pass

**Steps:**

- [ ] **Step 1: Add the HTTP dependencies**

In `crates/inkapp-core/Cargo.toml`, under `[dependencies]`, add (after the `image = ...` line from Task 1):

```toml
reqwest = { version = "0.12", features = ["json"] }
reqwest-middleware = { version = "0.4", features = ["json"] }
reqwest-retry = "0.7"
```

- [ ] **Step 2: Write the fetch seam + a failing test**

In `crates/inkapp-core/src/assets.rs`, change the top import block to add `async_trait`:

```rust
use std::collections::HashMap;

use async_trait::async_trait;
use sha2::{Digest, Sha256};
```

Then add the seam below `normalize_to_png` (before the `#[cfg(test)]` module):

```rust
// ── fetch seam ───────────────────────────────────────────────────────────────

/// How the pipeline fetches image bytes for a URL. Mirrors the readwise
/// connector's `FetchTransport` seam: a trait with a fake for tests and a real
/// concurrent retrying HTTP implementation.
#[async_trait]
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

#[async_trait]
impl ImageFetcher for FakeFetcher {
    async fn fetch(&self, url: &str) -> Option<Vec<u8>> {
        self.responses.get(url).cloned()
    }
}

/// Offline fetcher: always `None`. The `App`'s default, so nothing hits the
/// network unless a live build injects `HttpImageFetcher`.
pub struct OfflineFetcher;

#[async_trait]
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

#[async_trait]
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
```

Add these tests inside the existing `#[cfg(test)] mod tests` block (append after `placeholder_is_a_1x1_png`):

```rust
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
        assert_eq!(
            got,
            vec![Some(b"A".to_vec()), None, Some(b"C".to_vec())]
        );
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
```

- [ ] **Step 3: Run the tests**

Run: `nix develop --command cargo test -p inkapp-core assets::tests`
Expected: all PASS (the new fetch-seam tests plus the Task 1 primitives).

- [ ] **Step 4: Commit**

```bash
cargo fmt
git add crates/inkapp-core/src/assets.rs crates/inkapp-core/Cargo.toml
git -c core.hooksPath=.githooks commit -m "assets: ImageFetcher seam (fake/offline/retrying-HTTP)"
```

---

### Task 3: `resolve_assets` — cache-backed resolver with placeholder fallback

**Goal:** Add `resolve_assets`, which turns `(key, url)` pairs into an `AssetMap` using the durable `Cache` for warm/offline hits, fetching misses concurrently, normalizing to PNG, and substituting `PLACEHOLDER_PNG` for any failure.

**Files:**
- Modify: `crates/inkapp-core/src/assets.rs`

**Acceptance Criteria:**
- [ ] A cache miss is fetched, normalized, inserted into the map at `/assets/{key}.png`, and written back to the cache under `assets/{key}`.
- [ ] A second `resolve_assets` over the same cache with an `OfflineFetcher` still returns the bytes (warm-cache / offline path).
- [ ] A failing fetch yields `PLACEHOLDER_PNG` at the key (map entry always present).
- [ ] Duplicate keys are resolved once (first occurrence wins).
- [ ] `cargo test -p inkapp-core assets::` passes.

**Verify:** `nix develop --command cargo test -p inkapp-core assets::tests` → all pass

**Steps:**

- [ ] **Step 1: Add the resolver + failing tests**

In `crates/inkapp-core/src/assets.rs`, add `use crate::cache::Cache;` to the import block:

```rust
use std::collections::HashMap;

use async_trait::async_trait;
use sha2::{Digest, Sha256};

use crate::cache::Cache;
```

Add the resolver below the fetch seam (before the `#[cfg(test)]` module):

```rust
// ── resolver ─────────────────────────────────────────────────────────────────

/// The cache key under which an asset's PNG bytes are stored.
fn cache_key(key: &str) -> String {
    format!("assets/{key}")
}

/// Resolve `(key, url)` pairs into an `AssetMap` (`/assets/{key}.png` -> PNG).
///
/// For each unique key: a cache hit short-circuits; otherwise the URL is fetched
/// (all misses concurrently), normalized to PNG, and — on ANY failure (fetch
/// `None`, decode error, or tracking-pixel drop) — replaced by `PLACEHOLDER_PNG`.
/// Every key always yields an entry, so an emitted `#image()` can never dangle.
/// Results are written back to `cache` (when present) so the next run is
/// warm/offline. The cache is NOT closed here (that would shut the foyer engine
/// mid-app); durability flush happens once at app shutdown via `App::close`.
pub async fn resolve_assets(
    pairs: &[(String, String)],
    cache: Option<&Cache>,
    fetcher: &dyn ImageFetcher,
) -> AssetMap {
    // Dedup by key (first occurrence wins), preserving order.
    let mut seen = std::collections::HashSet::new();
    let mut unique: Vec<(String, String)> = Vec::new();
    for (k, u) in pairs {
        if seen.insert(k.clone()) {
            unique.push((k.clone(), u.clone()));
        }
    }

    let mut map = AssetMap::new();
    let mut miss_slots: Vec<usize> = Vec::new();
    let mut miss_urls: Vec<String> = Vec::new();

    // Cache pass.
    for (i, (key, url)) in unique.iter().enumerate() {
        if let Some(c) = cache {
            if let Ok(Some(bytes)) = c.get_bytes(&cache_key(key)).await {
                map.insert(asset_path(key), bytes);
                continue;
            }
        }
        miss_slots.push(i);
        miss_urls.push(url.clone());
    }

    // Fetch all misses concurrently, normalize, fall back to placeholder.
    let fetched = fetcher.fetch_many(&miss_urls).await;
    for (slot, body) in miss_slots.into_iter().zip(fetched) {
        let key = &unique[slot].0;
        let png = body
            .as_deref()
            .and_then(normalize_to_png)
            .unwrap_or_else(|| PLACEHOLDER_PNG.to_vec());
        if let Some(c) = cache {
            let _ = c.put_bytes(&cache_key(key), &png).await;
        }
        map.insert(asset_path(key), png);
    }

    map
}
```

Add these tests inside `#[cfg(test)] mod tests` (append after `http_fetcher_builds`):

```rust
    fn fetcher_for(url: &str, bytes: Vec<u8>) -> FakeFetcher {
        let mut responses = HashMap::new();
        responses.insert(url.to_string(), bytes);
        FakeFetcher::new(responses)
    }

    #[tokio::test]
    async fn resolve_fetches_normalizes_and_caches() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::open(dir.path(), 1 << 20, 8 << 20).await.unwrap();
        let url = "https://example.com/pic.webp";
        let key = asset_key(url);
        let fetcher = fetcher_for(url, b64(WEBP_16));

        let map = resolve_assets(&[(key.clone(), url.to_string())], Some(&cache), &fetcher).await;

        let bytes = map.get(&asset_path(&key)).expect("asset present");
        assert!(is_png(bytes), "fetched webp normalized to png");
        // Written back to the cache under the bare key.
        let cached = cache.get_bytes(&cache_key(&key)).await.unwrap().unwrap();
        assert_eq!(&cached, bytes);
    }

    #[tokio::test]
    async fn resolve_serves_warm_cache_offline() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::open(dir.path(), 1 << 20, 8 << 20).await.unwrap();
        let url = "https://example.com/pic.webp";
        let key = asset_key(url);

        // Warm the cache via a real fetch.
        let online = fetcher_for(url, b64(WEBP_16));
        let first = resolve_assets(&[(key.clone(), url.to_string())], Some(&cache), &online).await;
        let warm_bytes = first.get(&asset_path(&key)).unwrap().clone();

        // Second pass with an OfflineFetcher still returns the bytes from cache.
        let offline = OfflineFetcher;
        let second = resolve_assets(&[(key.clone(), url.to_string())], Some(&cache), &offline).await;
        assert_eq!(second.get(&asset_path(&key)).unwrap(), &warm_bytes);
    }

    #[tokio::test]
    async fn resolve_falls_back_to_placeholder_on_failure() {
        // No cache, and the fetcher has nothing for this url -> placeholder.
        let url = "https://example.com/missing.png";
        let key = asset_key(url);
        let fetcher = FakeFetcher::new(HashMap::new());
        let map = resolve_assets(&[(key.clone(), url.to_string())], None, &fetcher).await;
        assert_eq!(map.get(&asset_path(&key)).unwrap(), PLACEHOLDER_PNG);
    }

    #[tokio::test]
    async fn resolve_dedups_repeated_keys() {
        let url = "https://example.com/pic.webp";
        let key = asset_key(url);
        let fetcher = fetcher_for(url, b64(WEBP_16));
        let pairs = vec![
            (key.clone(), url.to_string()),
            (key.clone(), url.to_string()),
        ];
        let map = resolve_assets(&pairs, None, &fetcher).await;
        assert_eq!(map.len(), 1);
        assert!(map.contains_key(&asset_path(&key)));
    }
```

- [ ] **Step 2: Run the tests**

Run: `nix develop --command cargo test -p inkapp-core assets::tests`
Expected: all PASS.

- [ ] **Step 3: Commit**

```bash
cargo fmt
git add crates/inkapp-core/src/assets.rs
git -c core.hooksPath=.githooks commit -m "assets: resolve_assets (cache-backed, concurrent, placeholder fallback)"
```

---

### Task 4: `InkWorld` serves asset bytes

**Goal:** Give `InkWorld` an asset table populated at construction and make `World::file()` return registered asset bytes instead of always `NotFound`.

**Files:**
- Modify: `crates/inkapp-core/src/world.rs`

**Acceptance Criteria:**
- [ ] `InkWorld::with_sources_and_assets(src, sources, assets)` registers `(virtual_path, bytes)` assets.
- [ ] `with_sources` and `new` still work (delegate with no assets).
- [ ] `file()` returns the registered `Bytes` for a known path, `NotFound` for an unknown one.
- [ ] `cargo test -p inkapp-core world::` passes.

**Verify:** `nix develop --command cargo test -p inkapp-core world::tests` → all pass

**Steps:**

- [ ] **Step 1: Add the asset field and the new constructor**

In `crates/inkapp-core/src/world.rs`, add `assets` to the struct (after `sources`):

```rust
pub struct InkWorld {
    library: LazyHash<Library>,
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
    main: Source,
    sources: HashMap<FileId, Source>,
    assets: HashMap<FileId, Bytes>,
}
```

Replace the existing `with_sources` method with a delegating wrapper plus the new constructor (the old body moves into `with_sources_and_assets`):

```rust
    /// Like `new`, but registers additional named Typst sources (e.g. component
    /// `.typ` files) so the main source can `#import` them. `sources` is a list of
    /// `(virtual_path, source_text)`; paths are root-absolute (leading `/`) to
    /// match `#import "/path.typ"`.
    pub fn with_sources(src: &str, sources: &[(String, String)]) -> Self {
        Self::with_sources_and_assets(src, sources, &[])
    }

    /// Like `with_sources`, but also registers image assets served by `file()`.
    /// `assets` is a list of `(virtual_path, bytes)`; paths are root-absolute
    /// (e.g. `/assets/{key}.png`) to match `#image("/assets/{key}.png")`.
    pub fn with_sources_and_assets(
        src: &str,
        sources: &[(String, String)],
        assets: &[(String, Vec<u8>)],
    ) -> Self {
        let mut fonts = Vec::new();
        for data in typst_assets::fonts() {
            let bytes = Bytes::new(data.to_vec());
            // A single TTF/OTF file may contain multiple faces.
            for face in Font::iter(bytes) {
                fonts.push(face);
            }
        }
        let book = FontBook::from_fonts(&fonts);
        let main_id = FileId::new(None, VirtualPath::new("main.typ"));
        let main = Source::new(main_id, src.into());
        let sources = sources
            .iter()
            .map(|(path, text)| {
                let id = FileId::new(None, VirtualPath::new(path));
                (id, Source::new(id, text.clone()))
            })
            .collect();
        let assets = assets
            .iter()
            .map(|(path, bytes)| {
                let id = FileId::new(None, VirtualPath::new(path));
                (id, Bytes::new(bytes.clone()))
            })
            .collect();
        Self {
            library: LazyHash::new(Library::default()),
            book: LazyHash::new(book),
            fonts,
            main,
            sources,
            assets,
        }
    }
```

- [ ] **Step 2: Serve assets from `file()`**

Replace the `file` method body:

```rust
    fn file(&self, id: FileId) -> FileResult<Bytes> {
        match self.assets.get(&id) {
            Some(bytes) => Ok(bytes.clone()),
            None => Err(FileError::NotFound(
                id.vpath().as_rootless_path().to_owned(),
            )),
        }
    }
```

- [ ] **Step 3: Add unit tests**

At the bottom of `crates/inkapp-core/src/world.rs`, add:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_serves_registered_assets() {
        let assets = vec![("/assets/abc.png".to_string(), vec![1u8, 2, 3, 4])];
        let world = InkWorld::with_sources_and_assets("hello", &[], &assets);

        let id = FileId::new(None, VirtualPath::new("/assets/abc.png"));
        assert_eq!(world.file(id).unwrap().as_ref(), &[1u8, 2, 3, 4]);

        let missing = FileId::new(None, VirtualPath::new("/assets/zzz.png"));
        assert!(world.file(missing).is_err());
    }

    #[test]
    fn plain_world_serves_no_files() {
        let world = InkWorld::new("hello");
        let id = FileId::new(None, VirtualPath::new("/assets/abc.png"));
        assert!(world.file(id).is_err());
    }
}
```

- [ ] **Step 4: Run the tests**

Run: `nix develop --command cargo test -p inkapp-core world::tests`
Expected: both PASS.

- [ ] **Step 5: Commit**

```bash
cargo fmt
git add crates/inkapp-core/src/world.rs
git -c core.hooksPath=.githooks commit -m "world: InkWorld serves registered image assets via file()"
```

---

### Task 5: Thread `AssetMap` through the compile path (free functions)

**Goal:** Add additive `*_with_assets` compile/render entry points so a document with `#image("/assets/{key}.png")` compiles against a registered `AssetMap`; existing functions delegate with an empty map (every current caller is untouched). Prove the compile, placeholder, and determinism behaviors.

**Files:**
- Modify: `crates/inkapp-core/src/render.rs`
- Modify: `crates/inkapp-core/src/runtime.rs`
- Create: `crates/inkapp-core/tests/assets_pipeline.rs`

**Acceptance Criteria:**
- [ ] `compile_to_document_with_sources_and_assets(src, sources, assets)` exists; `compile_to_document_with_sources` delegates with `&[]`.
- [ ] `compile_document_in_with_assets` and `render_document_in_with_assets` exist; the existing `compile_document_in` / `render_document_in` delegate with an empty `AssetMap`.
- [ ] A `#image("/assets/<key>.png")` document with the asset registered renders to a non-empty PDF.
- [ ] The placeholder path (asset resolved to `PLACEHOLDER_PNG`) also renders to a non-empty PDF.
- [ ] Two compile+pdf runs over identical src + assets produce byte-identical PDFs.
- [ ] `cargo test -p inkapp-core` passes (incl. existing pagination/render tests, unchanged).

**Verify:** `nix develop --command cargo test -p inkapp-core` → all pass

**Steps:**

- [ ] **Step 1: Add the render.rs entry point**

In `crates/inkapp-core/src/render.rs`, replace the `compile_to_document_with_sources` function with a delegating wrapper plus the new assets-aware function:

```rust
/// Compile with additional registered Typst sources the main source may `#import`
/// (component render halves + the framework prelude).
pub fn compile_to_document_with_sources(
    src: &str,
    sources: &[(String, String)],
) -> Result<PagedDocument> {
    compile_to_document_with_sources_and_assets(src, sources, &[])
}

/// Like `compile_to_document_with_sources`, but also registers image assets
/// (`(virtual_path "/assets/{key}.png", bytes)`) served via `World::file()`, so
/// the document may embed `#image("/assets/{key}.png")`.
pub fn compile_to_document_with_sources_and_assets(
    src: &str,
    sources: &[(String, String)],
    assets: &[(String, Vec<u8>)],
) -> Result<PagedDocument> {
    let world = InkWorld::with_sources_and_assets(src, sources, assets);
    typst::compile::<PagedDocument>(&world)
        .output
        .map_err(|d| Error::Compile(format!("{d:?}")))
}
```

- [ ] **Step 2: Add the runtime.rs entry points**

In `crates/inkapp-core/src/runtime.rs`, add the `AssetMap` import near the top (with the other `use crate::...` lines, e.g. after `use crate::render::document_to_pdf;`):

```rust
use crate::assets::AssetMap;
```

Add this private helper just below the `RenderedDoc` struct definition (after its closing `}`):

```rust
/// Flatten an `AssetMap` into the `(path, bytes)` slice form the compile
/// functions take.
fn assets_as_slice(assets: &AssetMap) -> Vec<(String, Vec<u8>)> {
    assets.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}
```

Replace `compile_document_in` with a delegating wrapper plus an assets-aware variant:

```rust
/// Compile a document at an explicit page geometry, with all its Typst sources
/// (prelude + authored components) registered.
pub fn compile_document_in<M>(
    doc: &Document<M>,
    geom: PageGeom,
) -> Result<typst::layout::PagedDocument> {
    compile_document_in_with_assets(doc, geom, &AssetMap::new())
}

/// Like `compile_document_in`, but also registers `assets` so the document may
/// embed `#image("/assets/{key}.png")`.
pub fn compile_document_in_with_assets<M>(
    doc: &Document<M>,
    geom: PageGeom,
    assets: &AssetMap,
) -> Result<typst::layout::PagedDocument> {
    let src = document_source_in(doc, geom);
    let sources = collect_typst_sources(doc);
    let asset_vec = assets_as_slice(assets);
    crate::render::compile_to_document_with_sources_and_assets(&src, &sources, &asset_vec)
}
```

Replace `render_document_in` with a delegating wrapper plus an assets-aware variant (the original body moves into the `_with_assets` form):

```rust
/// Render one document at an explicit page geometry, sealing its manifest with `key`.
pub fn render_document_in<M>(
    doc: &Document<M>,
    version: u64,
    key: &Key,
    geom: PageGeom,
) -> Result<RenderedDoc> {
    render_document_in_with_assets(doc, version, key, geom, &AssetMap::new())
}

/// Like `render_document_in`, but registers `assets` so the document may embed
/// `#image("/assets/{key}.png")`.
pub fn render_document_in_with_assets<M>(
    doc: &Document<M>,
    version: u64,
    key: &Key,
    geom: PageGeom,
    assets: &AssetMap,
) -> Result<RenderedDoc> {
    let src = document_source_in(doc, geom);
    let sources = collect_typst_sources(doc);
    let asset_vec = assets_as_slice(assets);
    let compiled =
        crate::render::compile_to_document_with_sources_and_assets(&src, &sources, &asset_vec)?;
    // A single page_h suffices: `#set page` fixes every page of a document to the same
    // height, so the per-page device transform uses the same height on every page.
    let page_h = compiled
        .pages
        .first()
        .map(|p| p.frame.height().to_pt())
        .unwrap_or(geom.h);
    let page_count = compiled.pages.len();
    let mut manifest = recover_regions(&compiled)?.with_version(version);
    // Collect app-defined state into the manifest before sealing: the document's
    // own blob, then each stateful component's slice keyed by state_key().
    manifest.state.doc = doc.state.clone();
    for c in &doc.flow {
        if let (Some(k), Some(v)) = (c.state_key(), c.render_state()) {
            manifest.state.components.insert(k, v);
        }
    }
    let pdf = embed_manifest(&document_to_pdf(&compiled)?, &manifest, key)?;
    Ok(RenderedDoc {
        key: doc.key.clone(),
        pdf,
        manifest,
        page_h,
        page_count,
        hash: hash_str(&src),
    })
}
```

- [ ] **Step 3: Write the integration test (compile + placeholder + determinism)**

Create `crates/inkapp-core/tests/assets_pipeline.rs`:

```rust
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
    base64::engine::general_purpose::STANDARD.decode(WEBP_16).unwrap()
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
    assert!(assets.contains_key(&path), "asset registered at virtual path");

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
```

- [ ] **Step 4: Run the tests**

Run: `nix develop --command cargo test -p inkapp-core`
Expected: the three new `assets_pipeline` tests PASS, and all pre-existing tests still PASS (delegation kept their signatures).

- [ ] **Step 5: Commit**

```bash
cargo fmt
git add crates/inkapp-core/src/render.rs crates/inkapp-core/src/runtime.rs crates/inkapp-core/tests/assets_pipeline.rs
git -c core.hooksPath=.githooks commit -m "render/runtime: thread AssetMap through compile path (additive *_with_assets)"
```

---

### Task 6: Component `image_urls` + App auto-collection & resolution

**Goal:** Add `Component::image_urls()`, give `App` an injectable fetcher + asset cache, auto-collect declared URLs each render/step, resolve them, and thread the `AssetMap` into rendering; add `App::close()` for cache durability. Re-export the asset surface from the `inkapp` facade.

**Files:**
- Modify: `crates/inkapp-core/src/component.rs`
- Modify: `crates/inkapp-core/src/runtime.rs`
- Modify: `crates/inkapp/src/lib.rs` (facade re-export)
- Create: `crates/inkapp-core/tests/image_component.rs`

**Acceptance Criteria:**
- [ ] `Component::image_urls(&self) -> Vec<String>` exists with a default empty impl (no existing component changes).
- [ ] `App` has `fetcher: Arc<dyn ImageFetcher>` (default `OfflineFetcher`) and `asset_cache: Option<Arc<Cache>>` (default `None`), settable via `.fetcher(..)` / `.asset_cache(..)` on the builder.
- [ ] `App::render` and `App::step` collect `image_urls()` across all docs, resolve via `resolve_assets`, and render with the resulting `AssetMap`.
- [ ] `App::close()` flushes the asset cache when present.
- [ ] A component that emits `#image("/assets/{key}.png")` and declares `image_urls()` renders successfully through `App::render` both with a `FakeFetcher` (real bytes) and with the default `OfflineFetcher` (placeholder).
- [ ] `cargo test --workspace` passes.

**Verify:** `nix develop --command cargo test --workspace` → all pass

**Steps:**

- [ ] **Step 1: Add the trait method**

In `crates/inkapp-core/src/component.rs`, add to the `Component` trait (after `render_state`, before the closing `}`):

```rust
    /// URLs whose images this component's `render` references via
    /// `#image("/assets/{asset_key(url)}.png")`. The framework collects these,
    /// resolves them through the image pipeline (fetch + normalize + cache, with
    /// a placeholder on failure), and registers the bytes before compiling — so
    /// the emitted `#image` always resolves. Default: none.
    fn image_urls(&self) -> Vec<String> {
        Vec::new()
    }
```

- [ ] **Step 2: Add App fields, imports, and the resolve helper**

In `crates/inkapp-core/src/runtime.rs`, extend the imports. Add near the other `use crate::...` lines:

```rust
use std::sync::Arc;

use crate::assets::{asset_key, resolve_assets, ImageFetcher, OfflineFetcher};
use crate::cache::Cache;
```

(Note: `AssetMap` is already imported from Task 5. `std::collections::HashMap` is already imported further down; keep it.)

Extend the `App` struct (add the two fields after `geom`):

```rust
pub struct App<M, Msg, Cx> {
    pub model: M,
    pub connectors: Cx,
    update: UpdateFn<M, Msg, Cx>,
    view: ViewFn<M, Msg, Cx>,
    version: u64,
    key: Key,
    geom: PageGeom,
    fetcher: Arc<dyn ImageFetcher>,
    asset_cache: Option<Arc<Cache>>,
}
```

Update `App::new` to take and store the two new fields:

```rust
impl<M, Msg, Cx> App<M, Msg, Cx> {
    pub fn new(
        model: M,
        connectors: Cx,
        update: UpdateFn<M, Msg, Cx>,
        view: ViewFn<M, Msg, Cx>,
        key: Key,
        geom: PageGeom,
        fetcher: Arc<dyn ImageFetcher>,
        asset_cache: Option<Arc<Cache>>,
    ) -> Self {
        Self {
            model,
            connectors,
            update,
            view,
            version: 1,
            key,
            geom,
            fetcher,
            asset_cache,
        }
    }
}
```

- [ ] **Step 3: Add the collection helper, wire render/step, add close()**

Still in `runtime.rs`, inside `impl<M, Msg, Cx: ConnectorSet> App<M, Msg, Cx>` (where `refresh_all`/`flush_all` live), add a helper and a `close`:

```rust
    /// Collect every component's declared image URLs across the doc set, map each
    /// to its `(asset_key, url)` pair, and resolve them through the pipeline into
    /// an `AssetMap` (fetch + normalize + cache, placeholder on failure).
    async fn resolve_doc_assets(&self, docs: &Documents<Msg>) -> AssetMap {
        let mut pairs: Vec<(String, String)> = Vec::new();
        for doc in &docs.0 {
            for c in &doc.flow {
                for url in c.image_urls() {
                    pairs.push((asset_key(&url), url));
                }
            }
        }
        resolve_assets(&pairs, self.asset_cache.as_deref(), &*self.fetcher).await
    }

    /// Flush the asset cache (if any) so resolved images survive a restart.
    /// Live binaries call this on shutdown.
    pub async fn close(&self) -> Result<()> {
        if let Some(c) = &self.asset_cache {
            c.close().await?;
        }
        Ok(())
    }
```

In `render`, resolve assets after computing `docs` and pass the map into rendering. Replace the `render` body's doc loop:

```rust
    pub async fn render(&mut self, set: &mut DocSet) -> Result<Vec<RenderedDoc>> {
        self.refresh_all().await;
        let docs = (self.view)(&self.model, &self.connectors);
        let assets = self.resolve_doc_assets(&docs).await;
        let mut out = Vec::new();
        let mut entries = HashMap::new();
        for doc in &docs.0 {
            let rd =
                render_document_in_with_assets(doc, self.version, &self.key, self.geom, &assets)?;
            entries.insert(
                rd.key.0.clone(),
                DocEntry {
                    manifest: rd.manifest.clone(),
                    page_h: rd.page_h,
                    page_count: rd.page_count,
                    hash: rd.hash,
                    version: self.version,
                    ink: Vec::new(),
                },
            );
            out.push(rd);
        }
        set.entries = entries;
        Ok(out)
    }
```

In `step`, phase 3 (the post-fold re-render), resolve assets for the post-fold docs and use the assets-aware render. Replace the phase-3 block:

```rust
        // 3. Re-render the post-fold view.
        let next = (self.view)(&self.model, &self.connectors);
        let assets = self.resolve_doc_assets(&next).await;
        let mut next_rendered: Vec<RenderedDoc> = Vec::new();
        for doc in &next.0 {
            next_rendered.push(render_document_in_with_assets(
                doc,
                self.version,
                &self.key,
                self.geom,
                &assets,
            )?);
        }
```

- [ ] **Step 4: Update the builder to inject fetcher/asset_cache**

Still in `runtime.rs`, extend `BuilderReady` (add the two fields after `geom`):

```rust
pub struct BuilderReady<M, Msg, Cx> {
    model: M,
    connectors: Cx,
    update: UpdateFn<M, Msg, Cx>,
    view: ViewFn<M, Msg, Cx>,
    key: Key,
    geom: PageGeom,
    fetcher: Arc<dyn ImageFetcher>,
    asset_cache: Option<Arc<Cache>>,
}
```

In `BuilderFull::key`, set the defaults when constructing `BuilderReady`:

```rust
    pub fn key(self, key: Key) -> BuilderReady<M, Msg, Cx> {
        BuilderReady {
            model: self.model,
            connectors: self.connectors,
            update: self.update,
            view: self.view,
            key,
            geom: PageGeom::default(),
            fetcher: Arc::new(OfflineFetcher),
            asset_cache: None,
        }
    }
```

In `impl<M, Msg, Cx> BuilderReady<M, Msg, Cx>`, add the two setter methods (next to `page`):

```rust
    /// Inject the image fetcher (default: `OfflineFetcher`, i.e. no network).
    #[must_use]
    pub fn fetcher(mut self, fetcher: Arc<dyn ImageFetcher>) -> Self {
        self.fetcher = fetcher;
        self
    }

    /// Inject the durable asset cache used for warm-restart / offline image serving.
    #[must_use]
    pub fn asset_cache(mut self, cache: Arc<Cache>) -> Self {
        self.asset_cache = Some(cache);
        self
    }
```

Update `BuilderReady::build` to pass the new fields:

```rust
    pub fn build(self) -> App<M, Msg, Cx> {
        App::new(
            self.model,
            self.connectors,
            self.update,
            self.view,
            self.key,
            self.geom,
            self.fetcher,
            self.asset_cache,
        )
    }
```

- [ ] **Step 5: Re-export the asset surface from the facade**

In `crates/inkapp/src/lib.rs`, add a re-export of the asset module's public API. Add this line after the existing `pub use inkapp_core::...` re-exports (match the file's existing re-export style):

```rust
pub use inkapp_core::assets::{
    asset_key, asset_path, resolve_assets, AssetMap, FakeFetcher, HttpImageFetcher, ImageFetcher,
    OfflineFetcher, PLACEHOLDER_PNG,
};
```

- [ ] **Step 6: Write the App-level integration test**

Create `crates/inkapp-core/tests/image_component.rs`:

```rust
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
    base64::engine::general_purpose::STANDARD.decode(WEBP_16).unwrap()
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
    assert!(rendered[0].pdf.len() > 100, "image embedded -> non-empty pdf");
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
```

- [ ] **Step 7: Run the tests**

Run: `nix develop --command cargo test --workspace`
Expected: the two new `image_component` tests PASS and the whole workspace is green.

- [ ] **Step 8: Commit**

```bash
cargo fmt
git add crates/inkapp-core/src/component.rs crates/inkapp-core/src/runtime.rs crates/inkapp/src/lib.rs crates/inkapp-core/tests/image_component.rs
git -c core.hooksPath=.githooks commit -m "runtime: Component::image_urls + App auto-resolves/embeds assets; facade re-export"
```

---

### Task 7: Lockfile sweep, full-workspace verification, and docs

**Goal:** Commit the accumulated `Cargo.lock` changes separately, confirm the whole workspace builds locked and tests green, and record the new capability in `docs/appdx.md`.

**Files:**
- Modify: `Cargo.lock` (committed by the controller only)
- Modify: `docs/appdx.md`

**Acceptance Criteria:**
- [ ] `cargo build --workspace --locked` succeeds (lockfile in sync with manifests).
- [ ] `cargo test --workspace` is green.
- [ ] `docs/appdx.md` records the offline image pipeline capability.

**Verify:** `nix develop --command cargo test --workspace` → all pass; `nix develop --command cargo build --workspace --locked` → succeeds

**Steps:**

- [ ] **Step 1: Verify the full workspace (tests + locked build)**

Run:
```bash
nix develop --command cargo test --workspace
nix develop --command cargo build --workspace --locked
```
Expected: all tests PASS; the locked build succeeds (the `image`/`reqwest*` additions are reflected in `Cargo.lock`).

- [ ] **Step 2: Commit the lockfile separately**

```bash
git add Cargo.lock
git -c core.hooksPath=.githooks commit -m "Cargo.lock: image (avif-native) + reqwest(-middleware,-retry) for the image pipeline"
```

- [ ] **Step 3: Record the capability in docs/appdx.md**

Read `docs/appdx.md` to match its existing structure/heading style, then append an entry for this capability. Use wording consistent with the surrounding entries; the entry must state that:
- `inkapp-core` now has a reusable offline image pipeline (`assets` module): `ImageFetcher` seam (`FakeFetcher`/`OfflineFetcher`/`HttpImageFetcher`), `normalize_to_png` (transcodes jpeg/webp/avif/png → PNG, drops ≤2px tracking pixels), and `resolve_assets` (cache-backed, concurrent, 1×1 transparent placeholder on failure).
- `InkWorld` serves registered assets at `/assets/{key}.png`; the compile path has `*_with_assets` variants; `App` auto-collects `Component::image_urls()` and resolves+embeds them each render/step, with `App::close()` flushing the asset cache.
- AVIF decode requires libdav1d (`pkgs.dav1d` added to the flake devshell).

Example entry to adapt to the file's actual format:

```markdown
- **Offline image pipeline (inkapp-core `assets`).** `#image("/assets/{key}.png")`
  now resolves offline and deterministically: `resolve_assets` fetches `(key,url)`
  pairs (pluggable `ImageFetcher`: fake / offline / retrying-HTTP), normalizes
  each to PNG (transcodes jpeg/webp/avif→PNG via the `image` crate + libdav1d,
  drops ≤2px tracking pixels), caches bytes in `inkapp_core::cache::Cache`, and
  registers them in `InkWorld` (served by `World::file()`), substituting a 1×1
  transparent placeholder on any failure so compilation never dangles. `App`
  auto-collects each `Component::image_urls()` and threads the asset map through
  the compile path; `App::close()` flushes the cache for warm restart.
```

- [ ] **Step 4: Commit the docs**

```bash
git add docs/appdx.md
git -c core.hooksPath=.githooks commit -m "appdx: record the offline image pipeline capability"
```

---

## Self-Review

**Spec coverage check:**
- Spec §1 (fetch seam: trait + fake + HTTP) → Task 2. ✓ (`OfflineFetcher` is the additional default.)
- Spec §2 (normalize → PNG, drop ≤2px, transcode incl. avif via dav1d) → Task 1. ✓
- Spec §3 (resolve_assets, cache, placeholder, no close-per-render) → Task 3. ✓
- Spec §4 (InkWorld serves assets) → Task 4. ✓
- Spec §5 (compile-path threading, additive variants) → Task 5. ✓
- Spec §6 (Component::image_urls, App fetcher/asset_cache/close, collection) → Task 6. ✓
- Spec §7 (all eight tests) → covered: fetch seam (T2), normalize (T1), resolve incl. warm/placeholder (T3), world (T4), compile + placeholder + determinism (T5), App component (T6), `cargo test --workspace` (T6/T7). ✓
- Spec §8 (flake dav1d, Cargo.lock not staged by implementers, hook-form commits, appdx last) → flake in T1; lockfile/appdx in T7; commit form in every task. ✓

**Placeholder scan:** No "TBD"/"handle edge cases"/"similar to". The one prose-only step (T7 Step 3) is a docs edit that must match an existing file's format, and includes a concrete example block to adapt.

**Type consistency:** `AssetMap` (`HashMap<String, Vec<u8>>`), `asset_key`/`asset_path`, `normalize_to_png` (pub(crate), tested via unit tests in-module), `PLACEHOLDER_PNG: &[u8]`, `ImageFetcher`/`FakeFetcher`/`OfflineFetcher`/`HttpImageFetcher`, `resolve_assets(pairs, Option<&Cache>, &dyn ImageFetcher)`, `with_sources_and_assets`, `compile_to_document_with_sources_and_assets`, `compile_document_in_with_assets`, `render_document_in_with_assets`, `Component::image_urls`, `App::{fetcher,asset_cache,close}`, builder `.fetcher`/`.asset_cache` — names are consistent across all tasks. `App::new`'s only caller (`BuilderReady::build`) is updated in the same task (Task 6) that changes its signature.
