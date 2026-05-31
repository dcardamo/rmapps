# Faster PDF Generation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers-extended-cc:subagent-driven-development (recommended) or superpowers-extended-cc:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Cut a full rmreader run from ~41s to ~8–15s by caching per-document processed output, fetching all cache-miss images in one concurrent pool, and building the Library and Feed PDFs in parallel — with output bytes unchanged.

**Architecture:** Image fetch + normalize is ~75% of runtime and is currently serialized per document. We add an on-disk per-doc content cache (sanitized HTML + normalized image blobs) keyed by a content hash; cache misses have their images fetched in one global concurrent pool; the two independent collection pipelines run on separate threads. Readwise docs are always fetched fresh. fulgur rendering is byte-deterministic (verified), so cache and parallelism are validated by byte-equality tests.

**Tech Stack:** Rust 2021 (toolchain 1.94 via `nix develop`), fulgur 0.6 (Blitz + krilla), lol_html, image 0.25, serde/serde_json, lopdf 0.36. No new dependencies (`tempfile` is dev-only; cache uses `std::fs` + `serde_json`).

**Spec:** `docs/superpowers/specs/2026-05-21-faster-pdf-generation-design.md`

**Conventions:**
- All cargo commands run in the dev shell: `nix develop -c cargo ...` (or `make test` / `make clippy` / `make fmt-check`).
- Run `nix develop -c cargo fmt` before every commit (the repo's pre-commit hook is `cargo fmt --check`).
- No `Co-Authored-By` lines in commits (per repo owner preference).
- There is **no** golden/visual test in this repo (the Makefile `update-goldens` target references a non-existent `--test visual`). Byte-stability is guarded by the new tests in Tasks 3 and 4. Do not add or rely on a visual golden suite.

---

### Task 0: Add `[cache]` config section

**Goal:** Add a `CacheConfig` (`enabled`, `dir`, `expiry_days`) to `Config` with serde defaults, and update every `Config` struct literal so the project compiles.

**Files:**
- Modify: `src/config.rs` (add `CacheConfig`, add field to `Config`)
- Modify: `src/wizard.rs:22-46` (`assemble()` builds a full `Config` literal)
- Modify: `tests/generate.rs:31-55` (test builds a full `Config` literal)
- Test: `tests/config.rs`

**Acceptance Criteria:**
- [ ] `CacheConfig` parses from TOML and fills defaults when absent (`enabled=true`, `dir=None`, `expiry_days=7`).
- [ ] `Config` has a `cache: CacheConfig` field with `#[serde(default)]`.
- [ ] Whole workspace compiles; all existing tests still pass.

**Verify:** `nix develop -c cargo test --test config` → passes, including a new defaults test.

**Steps:**

- [ ] **Step 1: Write the failing test** — append to `tests/config.rs`:

```rust
#[test]
fn cache_config_defaults_when_absent() {
    // A config with no [cache] section gets cache defaults.
    let toml = r#"
device = "paper-pro-move"
output_dir = "."
theme = "reader"
[readwise]
token = "t"
"#;
    let cfg: rmreader::config::Config = toml::from_str(toml).unwrap();
    assert!(cfg.cache.enabled);
    assert_eq!(cfg.cache.dir, None);
    assert_eq!(cfg.cache.expiry_days, 7);
}

#[test]
fn cache_config_parses_explicit_values() {
    let toml = r#"
device = "paper-pro-move"
output_dir = "."
theme = "reader"
[readwise]
token = "t"
[cache]
enabled = false
dir = "/tmp/rmcache"
expiry_days = 30
"#;
    let cfg: rmreader::config::Config = toml::from_str(toml).unwrap();
    assert!(!cfg.cache.enabled);
    assert_eq!(cfg.cache.dir.as_deref(), Some("/tmp/rmcache"));
    assert_eq!(cfg.cache.expiry_days, 30);
}
```

Note: `tests/config.rs` already uses `toml`. If `toml` is not imported there, reference the crate directly as `toml::from_str` (it is a dependency).

- [ ] **Step 2: Run test to verify it fails**

Run: `nix develop -c cargo test --test config cache_config`
Expected: compile error — `Config` has no field `cache`.

- [ ] **Step 3: Add `CacheConfig` and the `Config` field** — in `src/config.rs`, add after `ContentConfig` (near line 90):

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CacheConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Cache directory. `None` → resolved at runtime to
    /// `$XDG_CACHE_HOME/rmreader` (else `~/.cache/rmreader`).
    #[serde(default)]
    pub dir: Option<String>,
    #[serde(default = "default_expiry_days")]
    pub expiry_days: u64,
}
fn default_expiry_days() -> u64 {
    7
}
impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            dir: None,
            expiry_days: 7,
        }
    }
}
```

In the `Config` struct (near line 114), add a field alongside the others:

```rust
    #[serde(default)]
    pub cache: CacheConfig,
```

(`default_true` already exists in this file and is reused.)

- [ ] **Step 4: Update the two `Config` struct literals**

In `src/wizard.rs`, the `assemble()` function builds a full `Config { ... }` (lines 23-46). Add the field before the closing brace (after `deploy: DeployConfig { ... },`):

```rust
        cache: crate::config::CacheConfig::default(),
```

Also add `CacheConfig` to the `use crate::config::{...}` import at the top of `src/wizard.rs`.

In `tests/generate.rs`, the test builds a full `Config { ... }` (lines 31-55). Add a field that points the cache at the test's temp dir so tests never touch the real cache:

```rust
        cache: CacheConfig {
            enabled: true,
            dir: Some(dir.path().join("cache").to_str().unwrap().to_string()),
            expiry_days: 7,
        },
```

(`tests/generate.rs` already does `use rmreader::config::*;`, so `CacheConfig` is in scope.)

- [ ] **Step 5: Run tests to verify they pass**

Run: `nix develop -c cargo test --test config && nix develop -c cargo build`
Expected: PASS; workspace compiles.

- [ ] **Step 6: Commit**

```bash
nix develop -c cargo fmt
git add src/config.rs src/wizard.rs tests/config.rs tests/generate.rs
git commit -m "Add [cache] config (enabled/dir/expiry_days) with serde defaults"
```

---

### Task 1: Cache module — key hashing, atomic read/write, touch-on-hit, sweep

**Goal:** A self-contained `src/cache.rs` providing a content-hash key, an on-disk per-entry store (atomic writes), touch-on-hit, and an mtime-based expiry sweep. Not yet wired into generation.

**Files:**
- Create: `src/cache.rs`
- Modify: `src/lib.rs` (add `pub mod cache;`)
- Test: `tests/cache.rs` (new)

**Acceptance Criteria:**
- [ ] `cache::key` changes when any of `doc_id`, html, `max_bytes`, or `images_enabled` changes.
- [ ] `put` then `get` round-trips HTML + image blobs exactly.
- [ ] `get` returns `None` when disabled, on version mismatch, or for a missing/partial entry.
- [ ] `get` on a hit refreshes the entry's `meta.json` mtime (touch).
- [ ] `sweep` deletes entries older than `expiry`, keeps fresh ones, and removes `.tmp-*` and partial dirs.

**Verify:** `nix develop -c cargo test --test cache` → all pass.

**Steps:**

- [ ] **Step 1: Write the failing tests** — create `tests/cache.rs`:

```rust
use rmreader::cache::{key, Cache};
use std::time::{Duration, SystemTime};

fn tmp_cache() -> (tempfile::TempDir, Cache) {
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::new(dir.path().to_path_buf(), true, 7);
    (dir, cache)
}

#[test]
fn key_is_sensitive_to_each_input() {
    let base = key("d1", "<p>hi</p>", 80_000, true);
    assert_ne!(base, key("d2", "<p>hi</p>", 80_000, true), "doc_id");
    assert_ne!(base, key("d1", "<p>HI</p>", 80_000, true), "html");
    assert_ne!(base, key("d1", "<p>hi</p>", 40_000, true), "max_bytes");
    assert_ne!(base, key("d1", "<p>hi</p>", 80_000, false), "images_enabled");
    assert_eq!(base, key("d1", "<p>hi</p>", 80_000, true), "stable");
}

#[test]
fn put_then_get_roundtrips_html_and_assets() {
    let (_d, cache) = tmp_cache();
    let assets = vec![
        ("img-d1-0.png".to_string(), vec![1u8, 2, 3]),
        ("img-d1-1.jpg".to_string(), vec![9u8, 8, 7, 6]),
    ];
    cache.put("k1", "<p>body</p>", &assets);
    let got = cache.get("k1").expect("hit");
    assert_eq!(got.html, "<p>body</p>");
    assert_eq!(got.assets, assets);
}

#[test]
fn get_misses_when_disabled_or_absent() {
    let (_d, cache) = tmp_cache();
    assert!(cache.get("nope").is_none());

    let dir = tempfile::tempdir().unwrap();
    let disabled = Cache::new(dir.path().to_path_buf(), false, 7);
    disabled.put("k", "<p>x</p>", &[]);
    assert!(disabled.get("k").is_none(), "disabled cache never hits");
}

#[test]
fn get_touches_mtime() {
    let (d, cache) = tmp_cache();
    cache.put("k", "<p>x</p>", &[]);
    let meta = d.path().join("k").join("meta.json");
    // Backdate the entry well past any test runtime.
    let old = SystemTime::now() - Duration::from_secs(60 * 60 * 24 * 10);
    std::fs::File::open(&meta).unwrap().set_modified(old).unwrap();
    assert!(cache.get("k").is_some());
    let mtime = std::fs::metadata(&meta).unwrap().modified().unwrap();
    assert!(
        SystemTime::now().duration_since(mtime).unwrap() < Duration::from_secs(60),
        "get should have refreshed the mtime to ~now"
    );
}

#[test]
fn sweep_removes_stale_keeps_fresh_and_cleans_junk() {
    let (d, cache) = tmp_cache();
    cache.put("fresh", "<p>f</p>", &[]);
    cache.put("stale", "<p>s</p>", &[]);
    // Backdate "stale" past the 7-day expiry.
    let old = SystemTime::now() - Duration::from_secs(60 * 60 * 24 * 8);
    std::fs::File::open(d.path().join("stale").join("meta.json"))
        .unwrap()
        .set_modified(old)
        .unwrap();
    // A leftover temp dir and a partial entry (no meta.json).
    std::fs::create_dir_all(d.path().join(".tmp-junk")).unwrap();
    std::fs::create_dir_all(d.path().join("partial")).unwrap();

    cache.sweep();

    assert!(cache.get("fresh").is_some(), "fresh kept");
    assert!(!d.path().join("stale").exists(), "stale removed");
    assert!(!d.path().join(".tmp-junk").exists(), "temp removed");
    assert!(!d.path().join("partial").exists(), "partial removed");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `nix develop -c cargo test --test cache`
Expected: compile error — no crate `rmreader::cache`.

- [ ] **Step 3: Create `src/cache.rs`**

```rust
//! On-disk per-document content cache: sanitized HTML + normalized image blobs,
//! keyed by a content hash. A hit skips both the network image fetch and the
//! image decode/transcode. Touch-on-hit plus an mtime-based expiry sweep reclaim
//! entries for documents that have rolled out of the library/feed.
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::config::CacheConfig;

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Bump to invalidate every cache entry when the on-disk format or the
/// processing pipeline changes in a way that affects rendered output.
pub const CACHE_FORMAT_VERSION: u32 = 1;

#[inline]
fn mix(h: &mut u64, bytes: &[u8]) {
    for &b in bytes {
        *h ^= b as u64;
        *h = h.wrapping_mul(FNV_PRIME);
    }
}

/// Content-addressed cache key (FNV-1a, 64-bit, hex). Any change to the document
/// id, its HTML, the byte cap, the images flag, or the format version yields a
/// different key — i.e. a "new or changed document".
pub fn key(doc_id: &str, html_content: &str, max_bytes: usize, images_enabled: bool) -> String {
    let mut h = FNV_OFFSET;
    mix(&mut h, &CACHE_FORMAT_VERSION.to_le_bytes());
    mix(&mut h, doc_id.as_bytes());
    mix(&mut h, &[0]); // field separator
    mix(&mut h, &(max_bytes as u64).to_le_bytes());
    mix(&mut h, &[images_enabled as u8]);
    mix(&mut h, html_content.as_bytes());
    format!("{h:016x}")
}

#[derive(Serialize, Deserialize)]
struct Meta {
    version: u32,
    assets: Vec<String>,
}

/// A cached processed document: render-ready HTML + (asset_key, bytes) blobs.
pub struct Cached {
    pub html: String,
    pub assets: Vec<(String, Vec<u8>)>,
}

pub struct Cache {
    dir: PathBuf,
    enabled: bool,
    expiry: Duration,
}

fn default_cache_dir() -> PathBuf {
    if let Some(x) = std::env::var_os("XDG_CACHE_HOME") {
        if !x.is_empty() {
            return PathBuf::from(x).join("rmreader");
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".cache").join("rmreader");
    }
    std::env::temp_dir().join("rmreader-cache")
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn unique_suffix() -> String {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{}-{}-{}", std::process::id(), nanos, n)
}

fn touch(meta_path: &Path) {
    if let Ok(f) = fs::OpenOptions::new().write(true).open(meta_path) {
        let _ = f.set_modified(SystemTime::now());
    }
}

impl Cache {
    pub fn new(dir: PathBuf, enabled: bool, expiry_days: u64) -> Self {
        Self {
            dir,
            enabled,
            expiry: Duration::from_secs(expiry_days.saturating_mul(86_400)),
        }
    }

    pub fn from_config(c: &CacheConfig) -> Self {
        let dir = c
            .dir
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(default_cache_dir);
        Self::new(dir, c.enabled, c.expiry_days)
    }

    /// Read a cached entry, touching its mtime so the sweep treats it as fresh.
    /// Returns `None` on miss, when disabled, on version mismatch, or any IO/parse error.
    pub fn get(&self, key: &str) -> Option<Cached> {
        if !self.enabled {
            return None;
        }
        let entry = self.dir.join(key);
        let meta: Meta = serde_json::from_slice(&fs::read(entry.join("meta.json")).ok()?).ok()?;
        if meta.version != CACHE_FORMAT_VERSION {
            return None;
        }
        let html = fs::read_to_string(entry.join("html")).ok()?;
        let mut assets = Vec::with_capacity(meta.assets.len());
        for k in &meta.assets {
            assets.push((k.clone(), fs::read(entry.join(k)).ok()?));
        }
        touch(&entry.join("meta.json"));
        Some(Cached { html, assets })
    }

    /// Write an entry atomically (temp dir + rename). Best-effort: never fails the
    /// build. A concurrent writer of the same key wins; we just drop our temp.
    pub fn put(&self, key: &str, html: &str, assets: &[(String, Vec<u8>)]) {
        if !self.enabled {
            return;
        }
        let _ = self.try_put(key, html, assets);
    }

    fn try_put(&self, key: &str, html: &str, assets: &[(String, Vec<u8>)]) -> std::io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let tmp = self.dir.join(format!(".tmp-{}", unique_suffix()));
        fs::create_dir_all(&tmp)?;
        let meta = Meta {
            version: CACHE_FORMAT_VERSION,
            assets: assets.iter().map(|(k, _)| k.clone()).collect(),
        };
        fs::write(tmp.join("meta.json"), serde_json::to_vec(&meta)?)?;
        fs::write(tmp.join("html"), html)?;
        for (k, bytes) in assets {
            fs::write(tmp.join(k), bytes)?;
        }
        let dest = self.dir.join(key);
        if dest.exists() || fs::rename(&tmp, &dest).is_err() {
            let _ = fs::remove_dir_all(&tmp);
        }
        Ok(())
    }

    /// Remove entries not used within `expiry` (mtime of `meta.json`), plus any
    /// leftover temp dirs and partial entries (missing/unreadable `meta.json`).
    pub fn sweep(&self) {
        if !self.enabled {
            return;
        }
        let Ok(rd) = fs::read_dir(&self.dir) else {
            return;
        };
        let now = SystemTime::now();
        for ent in rd.flatten() {
            let p = ent.path();
            if !p.is_dir() {
                continue;
            }
            if ent.file_name().to_string_lossy().starts_with(".tmp-") {
                let _ = fs::remove_dir_all(&p);
                continue;
            }
            let fresh = fs::metadata(p.join("meta.json"))
                .and_then(|m| m.modified())
                .ok()
                .and_then(|mtime| now.duration_since(mtime).ok())
                .map(|age| age <= self.expiry)
                .unwrap_or(false);
            if !fresh {
                let _ = fs::remove_dir_all(&p);
            }
        }
    }
}
```

- [ ] **Step 4: Register the module** — in `src/lib.rs`, add in alphabetical position (after `pub mod assemble;`):

```rust
pub mod cache;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `nix develop -c cargo test --test cache`
Expected: 5 tests PASS.

- [ ] **Step 6: Commit**

```bash
nix develop -c cargo fmt
git add src/cache.rs src/lib.rs tests/cache.rs
git commit -m "Add content cache module (FNV-1a key, atomic store, touch-on-hit, mtime sweep)"
```

---

### Task 2: Split the content pipeline (collect URLs / assemble from fetched bytes)

**Goal:** Split `process_html` into `collect_doc_urls` (truncate + dedup URLs) and `assemble_processed` (normalize from already-fetched bytes + sanitize), with `process_html` kept as a thin wrapper. This lets the orchestrator fetch all miss images globally. Output is unchanged.

**Files:**
- Modify: `src/content.rs`
- Test: `tests/content.rs` (existing tests must still pass; add new ones)

**Acceptance Criteria:**
- [ ] `collect_doc_urls` truncates at the byte cap (UTF-8 boundary), returns the truncated flag, and returns deduped `<img>` URLs in first-seen order (empty when images disabled).
- [ ] `assemble_processed` reproduces the legacy asset keys (`img-{safe_id}-{i}`) and sanitization exactly.
- [ ] `process_html` (wrapper) produces identical output to the pre-split version — all existing `tests/content.rs` tests pass unchanged.

**Verify:** `nix develop -c cargo test --test content` → all pass (existing + new).

**Steps:**

- [ ] **Step 1: Write the new failing tests** — append to `tests/content.rs`:

```rust
use rmreader::content::{assemble_processed, collect_doc_urls};
use std::collections::HashMap;

#[test]
fn collect_doc_urls_dedups_and_respects_images_flag() {
    let html = r#"<img src="https://x/a.png"><img src="https://x/a.png"><img src="https://x/b.png">"#;
    let (out_html, truncated, urls) = collect_doc_urls(html, 80_000, true);
    assert!(!truncated);
    assert_eq!(out_html, html);
    assert_eq!(urls, vec!["https://x/a.png".to_string(), "https://x/b.png".to_string()]);

    let (_h, _t, none) = collect_doc_urls(html, 80_000, false);
    assert!(none.is_empty(), "images disabled => no urls collected");
}

#[test]
fn collect_doc_urls_truncates_oversize() {
    let big = format!("<p>{}</p>", "word ".repeat(40_000)); // ~200 KB
    let (out_html, truncated, _urls) = collect_doc_urls(&big, 80_000, false);
    assert!(truncated);
    assert!(out_html.len() <= 80_000);
}

#[test]
fn assemble_processed_matches_legacy_keys_and_sanitizes() {
    let url = "https://x/p.png".to_string();
    let mut fetched = HashMap::new();
    fetched.insert(
        url.clone(),
        rmreader::content::FetchedImage { bytes: png_8x8(), ext: "png".into() },
    );
    let html = r#"<p onclick="x()"><img src="https://x/p.png"></p><script>e()</script>"#;
    let (thtml, truncated, urls) = collect_doc_urls(html, 80_000, true);
    let out = assemble_processed("d1", &thtml, truncated, true, &urls, &fetched);
    assert_eq!(out.assets.len(), 1);
    assert_eq!(out.assets[0].0, "img-d1-0.png");
    assert!(out.html.contains("img-d1-0.png"));
    assert!(!out.html.contains("https://x/p.png"));
    assert!(!out.html.contains("script"));
    assert!(!out.html.contains("onclick"));
}
```

(`png_8x8` is already defined in `tests/content.rs`.)

- [ ] **Step 2: Run tests to verify they fail**

Run: `nix develop -c cargo test --test content`
Expected: compile error — `collect_doc_urls` / `assemble_processed` not found.

- [ ] **Step 3: Refactor `src/content.rs`** — replace the body of `process_html` (lines 90-203) with these three public functions. Keep all other code (imports, `FetchedImage`, `ImageFetcher`, `Processed`, `collect_img_urls`, `normalize_image`) intact.

```rust
/// Truncate at the byte cap (on a UTF-8 boundary) and collect the document's
/// deduplicated `<img>` URLs (first-seen order). When images are disabled the URL
/// list is empty. Returns `(possibly-truncated HTML, truncated flag, urls)`.
pub fn collect_doc_urls(
    html: &str,
    max_bytes: usize,
    images_enabled: bool,
) -> (String, bool, Vec<String>) {
    let (html, truncated) = if html.len() > max_bytes {
        let mut end = max_bytes;
        while end > 0 && !html.is_char_boundary(end) {
            end -= 1;
        }
        (&html[..end], true)
    } else {
        (html, false)
    };
    let urls = if images_enabled {
        let mut seen = std::collections::HashSet::new();
        collect_img_urls(html)
            .into_iter()
            .filter(|u| seen.insert(u.clone()))
            .collect()
    } else {
        Vec::new()
    };
    (html.to_string(), truncated, urls)
}

/// Build sanitized HTML + normalized image assets for one document, given the
/// already-truncated HTML, its deduped image URLs (first-seen order), and a
/// shared `url -> fetched bytes` map. Asset keys are `img-{safe_id}-{i}` over
/// `urls`, matching the legacy single-document path byte-for-byte.
pub fn assemble_processed(
    doc_id: &str,
    truncated_html: &str,
    truncated: bool,
    images_enabled: bool,
    urls: &[String],
    fetched: &std::collections::HashMap<String, FetchedImage>,
) -> Processed {
    use std::collections::HashMap;
    let safe_id: String = doc_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();

    let mut url_to_key: HashMap<String, String> = HashMap::new();
    let mut assets: Vec<(String, Vec<u8>)> = Vec::new();
    if images_enabled {
        for (i, url) in urls.iter().enumerate() {
            let Some(f) = fetched.get(url) else { continue };
            let normalized = if f.ext == "svg" {
                Some((f.bytes.clone(), "svg".to_string()))
            } else {
                normalize_image(&f.bytes)
            };
            if let Some((bytes, ext)) = normalized {
                let key = format!("img-{safe_id}-{i}.{ext}");
                url_to_key.insert(url.clone(), key.clone());
                assets.push((key, bytes));
            }
        }
    }

    let mut cleaned = rewrite_str(
        truncated_html,
        RewriteStrSettings {
            element_content_handlers: vec![
                element!("script,iframe,noscript,style,object,embed,form", |el| {
                    el.remove();
                    Ok(())
                }),
                element!("img", |el| {
                    let keep = el
                        .get_attribute("src")
                        .and_then(|s| url_to_key.get(&s).cloned());
                    match keep {
                        Some(key) => {
                            let _ = el.set_attribute("src", &key);
                        }
                        None => el.remove(),
                    }
                    Ok(())
                }),
                element!("*", |el| {
                    let names: Vec<String> = el.attributes().iter().map(|a| a.name()).collect();
                    for n in names {
                        if n.starts_with("on") {
                            el.remove_attribute(&n);
                        }
                    }
                    Ok(())
                }),
            ],
            ..RewriteStrSettings::default()
        },
    )
    .unwrap_or_else(|_| truncated_html.to_string());

    if truncated {
        cleaned.push_str(
            "<p class=\"truncated\"><em>… Article truncated for on-device reading — open it in Readwise for the full text.</em></p>",
        );
    }

    Processed {
        html: cleaned,
        assets,
    }
}

/// Sanitise `html` and embed images as local assets (single-document path used by
/// tests and any caller without a shared fetch pool). Delegates to
/// `collect_doc_urls` + `fetch_many` + `assemble_processed`.
pub fn process_html(
    html: &str,
    doc_id: &str,
    images_enabled: bool,
    max_bytes: usize,
    fetcher: &dyn ImageFetcher,
) -> Processed {
    let (truncated_html, truncated, urls) = collect_doc_urls(html, max_bytes, images_enabled);
    let results = fetcher.fetch_many(&urls);
    let fetched: std::collections::HashMap<String, FetchedImage> = urls
        .iter()
        .cloned()
        .zip(results)
        .filter_map(|(u, r)| r.map(|f| (u, f)))
        .collect();
    assemble_processed(doc_id, &truncated_html, truncated, images_enabled, &urls, &fetched)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `nix develop -c cargo test --test content`
Expected: all existing tests + 3 new tests PASS.

- [ ] **Step 5: Commit**

```bash
nix develop -c cargo fmt
git add src/content.rs tests/content.rs
git commit -m "Split content pipeline: collect_doc_urls + assemble_processed (process_html wrapper)"
```

---

### Task 3: Wire cache + global concurrent fetch into the build pipeline

**Goal:** Rewrite `build_one` to classify docs against the cache, fetch all cache-miss images in one concurrent pool, assemble from the shared bytes, and write new entries. Thread the `Cache` through `generate` and run `sweep()` once. Collections still build sequentially (parallelized in Task 4). Output bytes are unchanged.

**Files:**
- Modify: `src/assemble.rs:64-67` (change closure bound `Fn` → `FnMut`)
- Modify: `src/generate.rs` (`build_one` body; `generate` constructs cache + sweep)
- Test: `tests/generate.rs` (add cache-transparency test)

**Acceptance Criteria:**
- [ ] `build_one` produces the same PDF bytes as before for the same inputs.
- [ ] A cold run and an immediate warm run (cache populated) produce byte-identical PDFs.
- [ ] `generate` builds the `Cache` from config and calls `sweep()` exactly once.
- [ ] All existing tests pass; `cargo clippy` is clean.

**Verify:** `nix develop -c cargo test --test generate` and `make clippy` → pass.

**Steps:**

- [ ] **Step 1: Write the failing test** — append to `tests/generate.rs`:

```rust
#[test]
fn cold_and_warm_runs_produce_identical_pdfs() {
    // Same fake inputs, cache enabled at a temp dir. Run twice; the second run is
    // fully warm (cache hits). PDFs must be byte-identical (rendering is
    // deterministic), proving the cache is transparent.
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = dir.path().join("cache");
    let mk_cfg = |out: &std::path::Path| Config {
        device: "paper-pro-move".into(),
        output_dir: out.to_str().unwrap().into(),
        theme: "reader".into(),
        readwise: ReadwiseConfig { token: "t".into() },
        library: LibraryConfig { locations: vec!["new".into()], max_items: 10 },
        feed: FeedConfig { enabled: true, max_items: 10 },
        images: ImagesConfig { enabled: false, timeout_secs: 8, concurrency: 12 },
        content: ContentConfig::default(),
        deploy: DeployConfig { backend: "none".into(), library_folder: String::new(), feed_folder: String::new() },
        cache: CacheConfig { enabled: true, dir: Some(cache_dir.to_str().unwrap().into()), expiry_days: 7 },
    };

    let cold = dir.path().join("cold");
    std::fs::create_dir_all(&cold).unwrap();
    generate(&mk_cfg(&cold), &FakeT, &NoImages).unwrap();

    let warm = dir.path().join("warm");
    std::fs::create_dir_all(&warm).unwrap();
    generate(&mk_cfg(&warm), &FakeT, &NoImages).unwrap();

    for name in ["Library.pdf", "Feed.pdf"] {
        let a = std::fs::read(cold.join(name)).unwrap();
        let b = std::fs::read(warm.join(name)).unwrap();
        assert_eq!(a, b, "{name}: cold and warm output differ");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `nix develop -c cargo test --test generate cold_and_warm`
Expected: compile error — `generate` does not yet accept the cache wiring / `build_one` signature mismatch (after Step 4 it compiles and passes).

- [ ] **Step 3: Loosen the assembly closure bound** — in `src/assemble.rs`, change the `assemble_document` signature (line 64-67) from `Fn` to `FnMut`:

```rust
pub fn assemble_document(
    collection: &str,
    docs: &[Document],
    mut content_fn: impl FnMut(&str, &str) -> (String, Vec<(String, Vec<u8>)>),
) -> Built {
```

Existing closures (`tests/assemble.rs`, `tests/postprocess.rs`) coerce to `FnMut` unchanged. No other edits in this file.

- [ ] **Step 4: Rewrite `build_one`** — in `src/generate.rs`, replace the entire `build_one` function (lines 112-171) with:

```rust
fn build_one(
    collection: &str,
    docs: &[crate::readwise::Document],
    config: &Config,
    fetcher: &dyn ImageFetcher,
    out_dir: &Path,
    cache: &crate::cache::Cache,
) -> anyhow::Result<PathBuf> {
    use std::collections::{HashMap, HashSet};
    use std::time::Instant;
    let images_enabled = config.images.enabled;
    let max_bytes = config.content.max_article_bytes;

    // Pass A: classify each doc as a cache hit (reuse processed output) or a miss
    // (collect its image URLs for the shared fetch below).
    struct Miss {
        id: String,
        key: String,
        html: String,
        truncated: bool,
        urls: Vec<String>,
    }
    let mut processed: HashMap<String, (String, Vec<(String, Vec<u8>)>)> = HashMap::new();
    let mut misses: Vec<Miss> = Vec::new();
    for d in docs {
        let raw = d.html_content.clone().unwrap_or_default();
        let key = crate::cache::key(&d.id, &raw, max_bytes, images_enabled);
        if let Some(c) = cache.get(&key) {
            processed.insert(d.id.clone(), (c.html, c.assets));
        } else {
            let (html, truncated, urls) =
                crate::content::collect_doc_urls(&raw, max_bytes, images_enabled);
            misses.push(Miss { id: d.id.clone(), key, html, truncated, urls });
        }
    }
    eprintln!(
        "[rmreader] {collection}: {} docs ({} cached, {} to fetch)",
        docs.len(),
        docs.len() - misses.len(),
        misses.len()
    );

    // One concurrent fetch over the deduped union of all miss-doc image URLs.
    let mut union: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for m in &misses {
        for u in &m.urls {
            if seen.insert(u.clone()) {
                union.push(u.clone());
            }
        }
    }
    let t = Instant::now();
    let results = fetcher.fetch_many(&union);
    let fetched: HashMap<String, crate::content::FetchedImage> = union
        .into_iter()
        .zip(results)
        .filter_map(|(u, r)| r.map(|f| (u, f)))
        .collect();
    eprintln!(
        "[rmreader] {collection}: fetched {} images in {:.1}s",
        fetched.len(),
        t.elapsed().as_secs_f32()
    );

    // Pass B: normalize + sanitize each miss from the shared bytes, then cache it.
    for m in misses {
        let p = crate::content::assemble_processed(
            &m.id,
            &m.html,
            m.truncated,
            images_enabled,
            &m.urls,
            &fetched,
        );
        cache.put(&m.key, &p.html, &p.assets);
        processed.insert(m.id, (p.html, p.assets));
    }

    // Assemble: the closure just hands each doc its precomputed processed output.
    let built = crate::assemble::assemble_document(collection, docs, |_html, id| {
        processed.remove(id).unwrap_or_default()
    });

    let device = crate::device::get_device(&config.device)?;
    let theme = crate::theme::load_theme(&config.theme)?;
    let pdf_path = out_dir.join(format!("{collection}.pdf"));
    eprintln!(
        "[rmreader] {collection}: rendering {} fragments...",
        built.fragments.len()
    );
    let t = Instant::now();
    crate::render::render_pdf(&device, &theme, &built.fragments, &built.assets, &pdf_path)?;
    eprintln!(
        "[rmreader] {collection}: wrote {} in {:.1}s",
        pdf_path.display(),
        t.elapsed().as_secs_f32()
    );
    // Paint the full-page paper background and stamp the clickable nav bar.
    let paper = theme.get("paper").map(|s| s.as_str()).unwrap_or("#F3F1EA");
    let navbg = theme.get("navbg").map(|s| s.as_str()).unwrap_or("#2A2F6B");
    let navfg = theme.get("navfg").map(|s| s.as_str()).unwrap_or("#F4F1E8");
    crate::postprocess::finalize_pdf(
        &pdf_path,
        docs.len(),
        device.width_pt(),
        device.height_pt(),
        paper,
        navbg,
        navfg,
    )?;
    built
        .manifest
        .write(&out_dir.join(format!("{collection}.manifest.json")))?;
    Ok(pdf_path)
}
```

- [ ] **Step 5: Thread the cache through `generate`** — in `src/generate.rs`, update `generate` (lines 174-214). Keep it sequential for now; add cache construction + sweep, and pass `&cache` to both `build_one` calls. Insert after `std::fs::create_dir_all(&out_dir)?;`:

```rust
    let cache = crate::cache::Cache::from_config(&config.cache);
    cache.sweep();
```

Then change the two calls:
```rust
    let lib_pdf = build_one("Library", &lib, config, fetcher, &out_dir, &cache)?;
```
```rust
        let feed_pdf = build_one("Feed", &feed, config, fetcher, &out_dir, &cache)?;
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `nix develop -c cargo test --test generate && nix develop -c cargo test && make clippy`
Expected: all PASS, including `cold_and_warm_runs_produce_identical_pdfs`; clippy clean.

- [ ] **Step 7: Commit**

```bash
nix develop -c cargo fmt
git add src/assemble.rs src/generate.rs tests/generate.rs
git commit -m "Wire content cache + global concurrent image fetch into build pipeline"
```

---

### Task 4: Parallelize the Library and Feed pipelines

**Goal:** Run the two collection pipelines concurrently in a `std::thread::scope`, with the sweep done once before spawning. Verify fulgur renders safely under concurrency; if not, serialize only the render call behind a mutex.

**Files:**
- Modify: `src/generate.rs` (`generate` body + `+ Sync` bounds)
- Test: `tests/render.rs` (concurrent-render determinism)
- Modify (contingency only): `src/render.rs` (render mutex)

**Acceptance Criteria:**
- [ ] `generate` builds Library and Feed on separate threads; both PDFs + manifests are produced.
- [ ] Concurrent renders of identical input are byte-identical to a single-threaded render (verified by test, run repeatedly).
- [ ] `generate(&cfg, &FakeT, &NoImages)` and `generate(&transport, &fetcher)` in `cli.rs` still compile (fakes and `Ureq*` types are `Sync`).
- [ ] Full suite + clippy pass.

**Verify:** `nix develop -c cargo test` and `make clippy` → pass.

**Steps:**

- [ ] **Step 1: Write the failing concurrency test** — append to `tests/render.rs`:

```rust
#[test]
fn concurrent_render_is_deterministic() {
    // fulgur rendering must be byte-identical to a single-threaded reference even
    // when several renders run at once (generate() renders Library + Feed in
    // parallel). Repeat to shake out races.
    let device = get_device("paper-pro-move").unwrap();
    let theme = load_theme("reader").unwrap();
    let frags = vec![
        r##"<section class="page" id="index"><a href="#article-a">go</a></section>"##.to_string(),
        r##"<section class="page article" id="article-a"><h2 class="headline">A</h2><div class="body"><p>hi</p></div></section>"##.to_string(),
    ];
    let dir = tempfile::tempdir().unwrap();
    let reference = dir.path().join("ref.pdf");
    render_pdf(&device, &theme, &frags, &[], &reference).unwrap();
    let ref_bytes = std::fs::read(&reference).unwrap();

    for round in 0..3 {
        std::thread::scope(|s| {
            let handles: Vec<_> = (0..8)
                .map(|i| {
                    let device = &device;
                    let theme = &theme;
                    let frags = &frags;
                    let ref_bytes = &ref_bytes;
                    let out = dir.path().join(format!("c{round}-{i}.pdf"));
                    s.spawn(move || {
                        render_pdf(device, theme, frags, &[], &out).unwrap();
                        assert_eq!(
                            std::fs::read(&out).unwrap(),
                            *ref_bytes,
                            "concurrent render {round}-{i} differs from reference"
                        );
                    })
                })
                .collect();
            for h in handles {
                h.join().unwrap();
            }
        });
    }
}
```

- [ ] **Step 2: Run the test (baseline, single-threaded render still in place)**

Run: `nix develop -c cargo test --test render concurrent_render_is_deterministic`
Expected: PASS (this confirms fulgur is concurrency-safe). If it FAILS or panics, apply the **contingency** in Step 5 before proceeding.

- [ ] **Step 3: Parallelize `generate`** — in `src/generate.rs`, change the signature bounds and rewrite the body (lines 174-214):

```rust
/// Returns deploy targets: (pdf_path, remarkable_folder).
pub fn generate(
    config: &Config,
    transport: &(dyn HttpTransport + Sync),
    fetcher: &(dyn ImageFetcher + Sync),
) -> anyhow::Result<Vec<(PathBuf, String)>> {
    let out_dir = PathBuf::from(&config.output_dir);
    std::fs::create_dir_all(&out_dir)?;

    let cache = crate::cache::Cache::from_config(&config.cache);
    cache.sweep();

    // Both collections are independent: fetch + build each on its own thread.
    let (lib_res, feed_res) = std::thread::scope(|s| {
        let out_dir = &out_dir;
        let cache = &cache;
        let lib_h = s.spawn(move || -> anyhow::Result<(PathBuf, String)> {
            eprintln!("[rmreader] fetching library {:?}...", config.library.locations);
            let lib = crate::readwise::fetch_documents(
                transport,
                &config.readwise.token,
                &config.library.locations,
                config.library.max_items,
                sleep_secs,
            )?;
            let lib = drop_empty(lib);
            eprintln!("[rmreader] library: {} docs", lib.len());
            let pdf = build_one("Library", &lib, config, fetcher, out_dir, cache)?;
            Ok((pdf, config.deploy.library_folder.clone()))
        });
        let feed_h = if config.feed.enabled {
            Some(s.spawn(move || -> anyhow::Result<(PathBuf, String)> {
                eprintln!("[rmreader] fetching feed...");
                let feed = crate::readwise::fetch_documents(
                    transport,
                    &config.readwise.token,
                    &["feed".into()],
                    config.feed.max_items,
                    sleep_secs,
                )?;
                let feed = drop_empty(feed);
                eprintln!("[rmreader] feed: {} docs", feed.len());
                let pdf = build_one("Feed", &feed, config, fetcher, out_dir, cache)?;
                Ok((pdf, config.deploy.feed_folder.clone()))
            }))
        } else {
            None
        };
        let lib_res = lib_h.join().expect("library build thread panicked");
        let feed_res = feed_h.map(|h| h.join().expect("feed build thread panicked"));
        (lib_res, feed_res)
    });

    let mut targets = vec![lib_res?];
    if let Some(fr) = feed_res {
        targets.push(fr?);
    }
    Ok(targets)
}

/// Rate-limit sleep used by `fetch_documents` (a plain fn item so both build
/// threads can share it; it is `Copy` and `Sync`).
fn sleep_secs(s: u64) {
    std::thread::sleep(std::time::Duration::from_secs(s));
}
```

- [ ] **Step 4: Build + run the full suite**

Run: `nix develop -c cargo build && nix develop -c cargo test && make clippy`
Expected: compiles (cli.rs unchanged — `&UreqTransport`/`&UreqImageFetcher` coerce to `&(dyn _ + Sync)`); all tests PASS; clippy clean.

- [ ] **Step 5: CONTINGENCY (only if Step 2 or Step 4 shows a render race / non-identical bytes / panic)** — serialize just the render call. In `src/render.rs`, add at module top:

```rust
use std::sync::Mutex;

/// fulgur/krilla is not safe to run from multiple threads at once on this
/// platform; serialize the render pass. Fetch + assemble (the ~75% cost) stay
/// parallel, so the win is preserved.
static RENDER_LOCK: Mutex<()> = Mutex::new(());
```

Then wrap the render call in `render_pdf` (the `engine.render_html_to_file(&html, out_path)?;` line):

```rust
    {
        let _guard = RENDER_LOCK.lock().unwrap();
        engine.render_html_to_file(&html, out_path)?;
    }
```

Re-run Step 4 to confirm green. Commit message should note the mutex was required.

- [ ] **Step 6: Commit**

```bash
nix develop -c cargo fmt
git add src/generate.rs tests/render.rs
# include src/render.rs in the add only if the Step 5 contingency was applied
git commit -m "Build Library and Feed PDFs in parallel"
```

---

### Task 5: Verify real-run performance (no commit)

**Goal:** Confirm the end-to-end speedup against the live config: cold run ≈ ~15s, warm re-run ≈ ~8s, output still valid. This is a verification task — it produces no code or commit.

**Files:** none (uses a scratch config; never deploys to the device).

**Acceptance Criteria:**
- [ ] A cold run (cleared cache) completes in roughly ~15s (down from ~41s baseline).
- [ ] An immediate second run (warm cache) is markedly faster (~8s) and logs mostly "cached" docs.
- [ ] Both `Library.pdf` and `Feed.pdf` load in lopdf without error.

**Verify:** commands below; compare wall-clock and the `[rmreader]` cache logs.

**Steps:**

- [ ] **Step 1: Build release**

Run: `nix develop -c cargo build --release`

- [ ] **Step 2: Make a scratch, no-deploy config** (reuses the real token + locations from `danout/rmreader.toml`):

```bash
python3 - <<'PY'
import re
src = open('danout/rmreader.toml').read()
src = re.sub(r'output_dir = .*', 'output_dir = "/tmp/rmperf/out"', src)
src = re.sub(r'backend = "rmapi"', 'backend = "none"', src)
if '[cache]' not in src:
    src += '\n[cache]\nenabled = true\ndir = "/tmp/rmperf/cache"\nexpiry_days = 7\n'
open('/tmp/rmperf.toml','w').write(src)
print(src)
PY
mkdir -p /tmp/rmperf/out
```

- [ ] **Step 3: Cold run (cleared cache), timed**

```bash
rm -rf /tmp/rmperf/cache
/usr/bin/env time -v ./target/release/rmreader /tmp/rmperf.toml 2>&1 | grep -E '\[rmreader\]|Elapsed'
```
Expected: total Elapsed ≈ ~15s; logs show "(0 cached, N to fetch)".

- [ ] **Step 4: Warm run (cache populated), timed**

```bash
/usr/bin/env time -v ./target/release/rmreader /tmp/rmperf.toml 2>&1 | grep -E '\[rmreader\]|Elapsed'
```
Expected: total Elapsed ≈ ~8s; logs show mostly "(N cached, ~0 to fetch)".

- [ ] **Step 5: Validate output PDFs**

```bash
ls -l /tmp/rmperf/out/*.pdf
nix develop -c cargo run --quiet --example tests 2>/dev/null || true   # if a PDF-load helper exists; else:
python3 - <<'PY'
for n in ("Library","Feed"):
    p=f"/tmp/rmperf/out/{n}.pdf"
    head=open(p,'rb').read(5)
    assert head==b'%PDF-', (n, head)
    print(n, "ok", )
PY
```
Expected: both files exist and start with `%PDF-`.

---

## Self-Review

**1. Spec coverage:**
- Per-doc content cache (key over `doc_id+html+max_bytes+images_enabled+version`, FNV-1a, atomic store) → Task 1. ✔
- Touch-on-hit + mtime sweep (every run, default 7 days) → Task 1 (`get` touch, `sweep`), Task 3/4 (`sweep()` once). ✔
- Global concurrent image fetch across miss docs → Task 2 (split) + Task 3 (`build_one` union fetch). ✔
- Parallel Library + Feed, `+ Sync` bounds, fulgur risk + mutex fallback → Task 4. ✔
- `[cache]` config (`enabled`/`dir`/`expiry_days`, XDG default) → Task 0 + Task 1 (`from_config`/`default_cache_dir`). ✔
- Byte-identical output invariant → Task 3 (cold==warm), Task 4 (concurrent==reference); determinism pre-verified. ✔
- Tests: cache unit, content split, transparency, determinism → Tasks 1–4. ✔
- Performance targets → Task 5. ✔
- Note: spec said `assemble_document` would take a precomputed map; the plan instead keeps the closure seam and changes `Fn`→`FnMut` (less churn, existing tests unchanged) — same separation of concerns. Intentional, documented in Task 3.

**2. Placeholder scan:** No TBD/TODO; every code step shows complete code. ✔

**3. Type consistency:** `cache::key`, `Cache::{new,from_config,get,put,sweep}`, `Cached{html,assets}`, `content::{collect_doc_urls,assemble_processed,process_html}`, `build_one(.., cache)`, `generate(config, &(dyn _ + Sync), &(dyn _ + Sync))`, `finalize_pdf(path, num_articles, w, h, paper, navbg, navfg)` — names match across tasks and the current source. ✔
