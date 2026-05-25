# Offline image pipeline for inkapp-core

**Date:** 2026-05-25
**Status:** Approved — ready for implementation plan

## Goal

A reusable image pipeline so a Typst `#image("/assets/{key}.png")` call resolves
**offline and deterministically**. Images are fetched, normalized to PNG, cached
durably (warm restart / offline), and served from `InkWorld` at the virtual path
`/assets/{key}.png` — where today `World::file()` always returns `NotFound`.

**Input contract** (shared with the parallel content worktree): the resolver
receives `Vec<(String key, String url)>` where `key = first 16 hex chars of
sha256(url)`, and registers `/assets/{key}.png`. **Critical:** for any URL that
fails to fetch or normalize, register a 1×1 transparent PNG at that key so the
content crate's already-emitted `#image()` call never dangles and compilation
never fails.

## Boundaries

Five well-bounded, independently testable units:

| Unit              | File                     | Responsibility                                            |
|-------------------|--------------------------|-----------------------------------------------------------|
| Fetch seam        | `assets.rs`              | `ImageFetcher` trait + `FakeFetcher` + `HttpImageFetcher`  |
| Normalize         | `assets.rs`              | bytes → deterministic PNG (drop ≤2px, transcode any→PNG)   |
| Resolve           | `assets.rs`              | `(key,url)` pairs + `Cache` → `AssetMap`, placeholder on fail |
| World serving     | `world.rs`               | `InkWorld` holds assets; `file()` returns them             |
| Compile threading | `render.rs`/`runtime.rs` | thread `AssetMap` into the world; App auto-collects needs  |

`asset_key(url) -> String` lives in `assets.rs` and is the **single source of
truth** for the key (first 16 hex chars of `sha256(url)`). Both components (when
emitting `#image`) and the resolver use it, so the content worktree and this one
can never disagree on the key.

## 1. Fetch seam (`assets.rs`)

```rust
#[async_trait::async_trait]
pub trait ImageFetcher: Send + Sync {
    async fn fetch(&self, url: &str) -> Option<Vec<u8>>;
    /// Fetch many concurrently; results in input order. Default is concurrent.
    async fn fetch_many(&self, urls: &[String]) -> Vec<Option<Vec<u8>>> {
        futures::future::join_all(urls.iter().map(|u| self.fetch(u))).await
    }
}
```

- **`FakeFetcher`** — wraps a `HashMap<String, Vec<u8>>` of canned responses; any
  URL not present returns `None`. Used by tests.
- **`HttpImageFetcher`** — a real concurrent retrying HTTP impl, mirroring
  readwise's `retrying_http_client`: a `reqwest-middleware` client with
  `ExponentialBackoff` (5 retries) over a plain `reqwest::Client`. Concurrency
  comes free from the default `fetch_many`. A non-2xx status or transport error
  yields `None` (the resolver turns that into a placeholder).
- **`OfflineFetcher`** — always returns `None`. This is the `App`'s default
  fetcher, so nothing hits the network unless a live build injects
  `HttpImageFetcher`.

Dependencies added to `inkapp-core`: `reqwest`, `reqwest-middleware`,
`reqwest-retry` (already in the workspace lockfile via the readwise crate).

## 2. Normalize — always PNG

Ported from rmreader's `normalize_image`, **with one deliberate deviation**:
rmreader keeps jpg/gif as-is, but here the virtual path is always
`/assets/{key}.png`, so we transcode *everything* (jpg/gif/webp/avif/png) to PNG.
This guarantees the served bytes match the `.png` path regardless of how Typst
detects format, and keeps output deterministic.

```rust
fn normalize_to_png(bytes: &[u8]) -> Option<Vec<u8>> {
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
```

Cargo: `image = { version = "0.25", features = ["avif-native"] }`. Default
features cover webp/jpeg/gif/png decode; `avif-native` adds AVIF decode via
libdav1d (C dependency — supplied through `flake.nix`, see §8).

`PLACEHOLDER_PNG` is a hardcoded `const [u8; N]` 1×1 transparent PNG (NOT
re-encoded at runtime — a fixed byte array guarantees byte-identical determinism
across builds and image-crate versions).

## 3. Resolve (`assets.rs`)

```rust
pub type AssetMap = std::collections::HashMap<String, Vec<u8>>; // "/assets/{key}.png" -> PNG

pub async fn resolve_assets(
    pairs: &[(String, String)],   // (key, url) — the documented contract boundary
    cache: Option<&Cache>,
    fetcher: &dyn ImageFetcher,
) -> AssetMap
```

Algorithm:

1. Dedup `pairs` by key (first occurrence wins).
2. For each key, if `cache` is present, try `cache.get_bytes("assets/{key}")`; a
   hit short-circuits to the cached bytes.
3. Fetch all misses concurrently via `fetcher.fetch_many`.
4. Normalize each fetched body to PNG. On **any** failure — fetch returned
   `None`, decode error, or tracking-pixel drop — use `PLACEHOLDER_PNG`.
5. If `cache` is present, write the result back (`put_bytes("assets/{key}", png)`),
   so the next run is warm/offline. Placeholders are cached too (a known-bad URL
   stays cheap; cache eviction will re-attempt eventually under a future policy).
6. Insert into the map under `"/assets/{key}.png"`.

Every key always yields an entry, so an emitted `#image()` can never dangle.
`cache.close()` is **not** called here — closing shuts the foyer engine and would
break subsequent renders. Durability flush happens once at app shutdown (§6).

Cache-key note: assets are stored under the bare `assets/{key}` (key already a
sha256-derived digest); we do not use `Cache::derived_key`, since the key is
already content/URL-stable.

## 4. World serving (`world.rs`)

Add `assets: HashMap<FileId, Bytes>` to `InkWorld`.

```rust
pub fn with_sources_and_assets(
    src: &str,
    sources: &[(String, String)],
    assets: &[(String, Vec<u8>)],   // (virtual path "/assets/{key}.png", bytes)
) -> Self
```

`with_sources` delegates with an empty asset slice. Assets are keyed by
`FileId::new(None, VirtualPath::new(path))`, matching how root-absolute Typst
sources already register. `file()` becomes:

```rust
fn file(&self, id: FileId) -> FileResult<Bytes> {
    match self.assets.get(&id) {
        Some(bytes) => Ok(bytes.clone()),
        None => Err(FileError::NotFound(id.vpath().as_rootless_path().to_owned())),
    }
}
```

## 5. Compile-path threading (`render.rs` / `runtime.rs`)

**Additive variants, originals preserved** — so every existing caller
(`tests/loop_driver.rs` and the rest) compiles unchanged. Editing all signatures
is more churn for no benefit.

- `render.rs`: add `compile_to_document_with_sources_and_assets(src, sources, assets)`;
  the existing `compile_to_document_with_sources` delegates with empty assets.
- `runtime.rs`: add `compile_document_in_with_assets(doc, geom, assets)` and
  `render_document_in_with_assets(doc, version, key, geom, assets)`; existing
  `compile_document_in` / `render_document_in` / `render_document` delegate with
  an empty `AssetMap`.

The `AssetMap` is converted to the `&[(String, Vec<u8>)]` slice form
`with_sources_and_assets` expects at the call site that builds the world.

## 6. Component collection + App wiring (`component.rs` / `runtime.rs`)

**Component API** — add to the `Component` trait with a default empty impl (no
existing component changes):

```rust
/// URLs this component's render references via `#image("/assets/{asset_key(url)}.png")`.
fn image_urls(&self) -> Vec<String> { Vec::new() }
```

Components declare their image URLs here, and in `render` they build the path with
the shared `assets::asset_key(url)` helper — single source of truth for the key.

**App wiring** — `App` gains:

- `fetcher: Arc<dyn ImageFetcher>` (default `Arc<OfflineFetcher>`)
- `asset_cache: Option<Arc<Cache>>` (default `None`)

set via builder methods `.fetcher(..)` and `.asset_cache(..)` on `BuilderReady`
(alongside the existing `.page(..)`).

In `render()` and in `step()`'s post-fold render, the App:

1. collects `image_urls()` across every doc's flow,
2. maps each URL to `(asset_key(url), url)` and dedups,
3. `resolve_assets(pairs, asset_cache.as_deref(), &*fetcher).await`,
4. passes the resulting `AssetMap` to `render_document_in_with_assets`.

`App::close()` flushes the asset cache (`cache.close()`) for warm-restart
durability; live binaries call it on shutdown.

The low-level `resolve_assets` (pairs in) remains available for the content
worktree to call directly without going through components.

## 7. Tests (TDD)

1. **Fetch seam:** `FakeFetcher` returns canned bytes for a known URL, `None`
   otherwise; `fetch_many` preserves input order.
2. **Normalize:** 1×1 and 2×2 inputs → `None` (tracking-pixel drop); webp→PNG
   (output starts with PNG magic); avif→PNG; jpeg→PNG.
3. **Resolve:** miss → fetch → map contains `/assets/{key}.png` with PNG bytes; a
   second `resolve_assets` over the same cache with an `OfflineFetcher` still
   returns the bytes (warm cache); a failing fetch → `PLACEHOLDER_PNG` at the key.
4. **World:** `with_sources_and_assets` then `file()` returns the registered
   bytes; `NotFound` for an unregistered path.
5. **Compile:** a document containing `#image("/assets/<key>.png")` with the asset
   registered renders to a non-empty PDF; the missing-asset (placeholder) path
   also renders to a non-empty PDF.
6. **Determinism:** two `compile_to_document_with_sources_and_assets` +
   `document_to_pdf` runs over identical src + assets produce byte-identical PDFs.
7. **App:** a component declaring `image_urls()` → `App::render()` resolves and the
   rendered doc embeds the image; a failed fetch → placeholder and compilation
   still succeeds.
8. `cargo test --workspace` is green.

The determinism test (6) deliberately exercises the bare compile→PDF path (no
sealed manifest) so the assertion is about the image pipeline's determinism, not
the manifest seal.

## 8. Build & repo conventions

- **`flake.nix`:** add `pkgs.dav1d` to `buildInputs` (libdav1d for image's
  `avif-native` decode; `pkg-config` is already a `nativeBuildInput`). We only
  decode AVIF (encode target is PNG), so the dav1d decoder suffices.
- **`Cargo.lock`:** dep additions go in `Cargo.toml`; the lockfile is **not**
  staged by implementer subagents — the controller sweeps and commits it
  separately at the end (avoids an unbuildable `--locked` tree without making
  implementers fight that blind spot).
- **Commits:** use the `git -c core.hooksPath=.githooks commit -m "..."` form so
  the open-native-task pre-commit hook is bypassed while `cargo fmt --check`
  stays active.
- **Native tasks:** cleared before committing.
- **Docs:** the final step records the new capability in `docs/appdx.md`.

## Out of scope

- The content-side HTML→Typst conversion and the production of `(key,url)` pairs
  from article bodies (that is the parallel content worktree's job; this worktree
  provides the `resolve_assets` boundary and the `image_urls` component hook it
  plugs into).
- Honoring HTTP `Retry-After` (the shared retry policy is pure exponential
  backoff, same limitation as the readwise client).
- Image resizing / DPI adaptation for the device — assets are served at their
  source resolution.
