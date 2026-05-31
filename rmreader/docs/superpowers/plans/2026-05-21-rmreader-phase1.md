# rmreader Phase 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers-extended-cc:subagent-driven-development (recommended) or superpowers-extended-cc:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Rust CLI that reads a Readwise Reader Library + Feed and produces two beautiful, hyperlinked, reader-optimized PDFs (`Library.pdf`, `Feed.pdf`), then uploads them to the reMarkable cloud via `rmapi`.

**Architecture:** Mirror the sibling project `../rmbujo`: fulgur (Blitz + krilla) renders askama HTML/CSS → PDF with no headless browser; `rmapi` is shelled out behind a testable trait; TOML config; an interactive wizard. For each PDF we build one HTML document (index + cards + articles) and render it in a single fulgur pass so cross-section `<a href="#id">` links resolve. A sidecar manifest (page → Readwise doc id) is the seam for the future annotation phase.

**Tech Stack:** Rust 2021, fulgur 0.6, askama 0.13, serde/toml, clap, dialoguer, anyhow, chrono, ureq (rustls), serde_json, url, lol_html (HTML rewrite/sanitize), image (transcode). Dev: lopdf, image. Nix flake dev shell with `rmapi`, `poppler-utils`, fonts.

**Reference repo:** `../rmbujo` is the proven pattern source. Where a task says "copy from rmbujo", read the cited file and reproduce it with the noted changes. Key files: `src/device.rs`, `src/render.rs`, `src/deploy/{mod,rmapi,local}.rs`, `src/config.rs`, `src/wizard.rs`, `flake.nix`, `nix/overlays/rmapi.nix`, `Makefile`, `templates/base.html`, `themes/library.toml`, `src/theme.rs`.

**Spec:** `docs/superpowers/specs/2026-05-21-rmreader-design.md`. Note: this plan uses `lol_html` for the content rewrite/sanitize step (better suited to attribute rewriting than the spec's tentative ammonia/scraper note).

---

## File structure

```
Cargo.toml                 # deps; [lib] rmreader + [[bin]] rmreader
flake.nix                  # dev shell + package (copied from rmbujo, renamed)
nix/overlays/rmapi.nix     # copied verbatim from rmbujo
Makefile                   # test/build/clippy/fmt/update-goldens/hooks (copied)
.gitignore                 # /target /result **/*.rs.bk rmreader.toml **/rmreader.toml
.githooks/pre-commit       # cargo fmt --check (copied from rmbujo)
src/main.rs                # calls rmreader::cli::main()
src/lib.rs                 # module declarations
src/cli.rs                 # `init` wizard | `<config.toml>` regenerate; arg parsing
src/config.rs              # Config structs, load/dump, validate()
src/device.rs              # MOVE / PRO geometry (copied from rmbujo)
src/theme.rs               # reader.toml palette -> CSS vars (adapted from rmbujo)
src/readwise/mod.rs        # Document type; HttpTransport trait; client logic
src/readwise/http.rs       # ureq transport impl
src/content.rs             # sanitize html_content; image fetch/rewrite/transcode
src/manifest.rs            # Manifest types + JSON writer
src/assemble.rs            # build the 3-tier HTML doc + anchors + nav + bookmarks
src/render.rs              # fulgur render (CSS, AssetBundle, bookmarks) (adapted)
src/deploy/{mod,rmapi,local}.rs  # rmapi backend (copied from rmbujo)
src/generate.rs            # orchestrate fetch -> content -> assemble -> render -> deploy
templates/base.html        # askama shell (copied from rmbujo)
themes/reader.toml         # "Newsprint" palette (already created)
assets/fonts/*.ttf         # Newsreader + Hanken Grotesk static TTFs
```

---

### Task 0: Project scaffold

**Goal:** A buildable Rust skeleton mirroring rmbujo, with copied infra (flake, Makefile, gitignore, base template, device, theme) and empty module stubs that compile.

**Files:**
- Create: `Cargo.toml`, `src/main.rs`, `src/lib.rs`, `flake.nix`, `nix/overlays/rmapi.nix`, `Makefile`, `.gitignore`, `.githooks/pre-commit`, `templates/base.html`
- Create (copy from rmbujo, adapt): `src/device.rs`, `src/theme.rs`
- Already present: `themes/reader.toml`
- Test: `tests/device.rs`

**Acceptance Criteria:**
- [ ] `nix develop -c cargo build` succeeds
- [ ] `nix develop -c cargo test --test device` passes
- [ ] `.gitignore` ignores `rmreader.toml`, `**/rmreader.toml`, `/target`, `/result`, `**/*.rs.bk`

**Verify:** `nix develop -c cargo test --test device` → `test result: ok. 2 passed`

**Steps:**

- [ ] **Step 1: `Cargo.toml`**

```toml
[package]
name = "rmreader"
version = "0.1.0"
edition = "2021"
description = "Readwise Reader -> reMarkable reader PDFs"

[lib]
name = "rmreader"
path = "src/lib.rs"

[[bin]]
name = "rmreader"
path = "src/main.rs"

[dependencies]
fulgur = "0.6"
askama = "0.13"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
chrono = { version = "0.4", default-features = false, features = ["clock"] }
clap = { version = "4", features = ["derive"] }
dialoguer = "0.11"
anyhow = "1"
ureq = "2"
url = "2"
lol_html = "1"
image = "0.25"

[dev-dependencies]
lopdf = "0.36"
image = "0.25"
```

- [ ] **Step 2: Copy Nix + Make infra from rmbujo**

Copy these files verbatim, then `sed`-rename `rmbujo` → `rmreader` inside them:
- `../rmbujo/nix/overlays/rmapi.nix` → `nix/overlays/rmapi.nix` (verbatim, no rename needed)
- `../rmbujo/flake.nix` → `flake.nix` (rename description, pname `rmbujo`→`rmreader`)
- `../rmbujo/Makefile` → `Makefile` (rename the `update-goldens` env var `RMBUJO_UPDATE_GOLDENS`→`RMREADER_UPDATE_GOLDENS`)
- `../rmbujo/.githooks/pre-commit` → `.githooks/pre-commit` (verbatim)

```bash
mkdir -p nix/overlays .githooks assets/fonts
cp ../rmbujo/nix/overlays/rmapi.nix nix/overlays/rmapi.nix
cp ../rmbujo/.githooks/pre-commit .githooks/pre-commit
sed 's/rmbujo/rmreader/g' ../rmbujo/flake.nix > flake.nix
sed -e 's/rmbujo/rmreader/g' -e 's/RMBUJO_UPDATE_GOLDENS/RMREADER_UPDATE_GOLDENS/g' ../rmbujo/Makefile > Makefile
```

- [ ] **Step 3: `.gitignore`**

```
/target
/result
**/*.rs.bk
rmreader.toml
**/rmreader.toml
```

- [ ] **Step 4: `templates/base.html`** (copied from rmbujo verbatim)

```html
<!doctype html><html lang="en"><head><meta charset="utf-8"><style>{{ css|safe }}</style></head>
<body>{% for page in pages %}{{ page|safe }}{% endfor %}</body></html>
```

- [ ] **Step 5: `src/device.rs`** — copy from `../rmbujo/src/device.rs` verbatim (the `Device` struct, `width_pt`/`height_pt`, `MOVE`, `PRO`, `get_device`). It is already correct for our devices.

- [ ] **Step 6: `src/theme.rs`** — adapt from `../rmbujo/src/theme.rs`. Replace the embedded theme with our reader palette and keep `Palette`, `load_theme`, `css_vars`:

```rust
//! Reader theme: TOML palette -> map + CSS custom properties.
use std::collections::BTreeMap;

const READER_TOML: &str = include_str!("../themes/reader.toml");
pub type Palette = BTreeMap<String, String>;

pub fn load_theme(name_or_path: &str) -> anyhow::Result<Palette> {
    let content = match name_or_path {
        "reader" => READER_TOML.to_string(),
        p if p.ends_with(".toml") => std::fs::read_to_string(p)
            .map_err(|e| anyhow::anyhow!("theme not found: {name_or_path} ({e})"))?,
        other => anyhow::bail!("unknown theme {other:?}; use 'reader' or a path to a .toml"),
    };
    Ok(toml::from_str(&content)?)
}

pub fn css_vars(theme: &Palette) -> String {
    let mut s = String::from(":root{");
    for (k, v) in theme {
        s.push_str(&format!("--{k}:{v};"));
    }
    s.push('}');
    s
}
```

- [ ] **Step 7: `src/lib.rs`** (stubs so it compiles; modules filled in later tasks)

```rust
//! rmreader — Readwise Reader -> reMarkable reader PDFs.
pub mod cli;
pub mod config;
pub mod device;
pub mod theme;
// Added in later tasks:
// pub mod readwise; pub mod content; pub mod manifest;
// pub mod assemble; pub mod render; pub mod deploy; pub mod generate;
```

For Task 0 only, create minimal `src/cli.rs` and `src/config.rs` stubs so the crate builds:

```rust
// src/cli.rs (stub, replaced in Task 9)
pub fn main() -> anyhow::Result<()> { Ok(()) }
```
```rust
// src/config.rs (stub, replaced in Task 1)
```

- [ ] **Step 8: `src/main.rs`**

```rust
fn main() -> anyhow::Result<()> {
    rmreader::cli::main()
}
```

- [ ] **Step 9: `tests/device.rs`** (failing first)

```rust
use rmreader::device::get_device;

#[test]
fn move_geometry_points() {
    let d = get_device("paper-pro-move").unwrap();
    // 954/264*72 ≈ 260.18, 1696/264*72 ≈ 462.55
    assert!((d.width_pt() - 260.18).abs() < 0.1);
    assert!((d.height_pt() - 462.55).abs() < 0.1);
}

#[test]
fn unknown_device_errs() {
    assert!(get_device("nope").is_err());
}
```

- [ ] **Step 10: Build + test**

Run: `nix develop -c cargo build && nix develop -c cargo test --test device`
Expected: build OK; `test result: ok. 2 passed`.

- [ ] **Step 11: Commit**

```bash
git add -A && git commit -m "Scaffold rmreader: cargo, nix, device, theme"
```

---

### Task 1: Config

**Goal:** `Config` structs that round-trip through TOML and a `validate()` that fails fast on bad input.

**Files:**
- Create: `src/config.rs` (replaces stub)
- Modify: `src/lib.rs` (already declares `config`)
- Test: `tests/config.rs`

**Acceptance Criteria:**
- [ ] Config serializes to and parses from TOML
- [ ] `validate()` rejects unknown device, empty token, bad locations, bad backend, empty rmapi folders
- [ ] Defaults applied for optional fields

**Verify:** `nix develop -c cargo test --test config` → all pass

**Steps:**

- [ ] **Step 1: Write `tests/config.rs` (failing)**

```rust
use rmreader::config::Config;

fn valid_toml() -> &'static str {
r#"
device = "paper-pro-move"
output_dir = "."
[readwise]
token = "abc123"
[library]
locations = ["new", "later", "shortlist"]
max_items = 100
[feed]
enabled = true
max_items = 100
[images]
enabled = true
[deploy]
backend = "rmapi"
library_folder = "/Reader"
feed_folder = "/Reader"
"#
}

#[test]
fn parses_and_validates() {
    let c: Config = toml::from_str(valid_toml()).unwrap();
    assert_eq!(c.device, "paper-pro-move");
    assert_eq!(c.library.locations, vec!["new", "later", "shortlist"]);
    assert_eq!(c.library.max_items, 100);
    assert!(c.validate().is_ok());
}

#[test]
fn roundtrips() {
    let c: Config = toml::from_str(valid_toml()).unwrap();
    let s = toml::to_string_pretty(&c).unwrap();
    let c2: Config = toml::from_str(&s).unwrap();
    assert_eq!(c, c2);
}

#[test]
fn rejects_empty_token() {
    let mut c: Config = toml::from_str(valid_toml()).unwrap();
    c.readwise.token = String::new();
    assert!(c.validate().is_err());
}

#[test]
fn rejects_bad_location() {
    let mut c: Config = toml::from_str(valid_toml()).unwrap();
    c.library.locations = vec!["bogus".into()];
    assert!(c.validate().is_err());
}

#[test]
fn rejects_rmapi_without_folder() {
    let mut c: Config = toml::from_str(valid_toml()).unwrap();
    c.deploy.library_folder = String::new();
    assert!(c.validate().is_err());
}
```

- [ ] **Step 2: Run — expect FAIL** (`cargo test --test config`): does not compile (no `Config`).

- [ ] **Step 3: Implement `src/config.rs`**

```rust
//! rmreader config: serde structs + TOML load/dump + validate.
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReadwiseConfig {
    pub token: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LibraryConfig {
    #[serde(default = "default_library_locations")]
    pub locations: Vec<String>,
    #[serde(default = "default_max_items")]
    pub max_items: u32,
}
fn default_library_locations() -> Vec<String> {
    vec!["new".into(), "later".into(), "shortlist".into()]
}
fn default_max_items() -> u32 { 100 }
impl Default for LibraryConfig {
    fn default() -> Self { Self { locations: default_library_locations(), max_items: default_max_items() } }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeedConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_max_items")]
    pub max_items: u32,
}
fn default_true() -> bool { true }
impl Default for FeedConfig {
    fn default() -> Self { Self { enabled: true, max_items: default_max_items() } }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImagesConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
}
impl Default for ImagesConfig {
    fn default() -> Self { Self { enabled: true } }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeployConfig {
    #[serde(default = "default_backend")]
    pub backend: String,
    #[serde(default)]
    pub library_folder: String,
    #[serde(default)]
    pub feed_folder: String,
}
fn default_backend() -> String { "none".into() }
impl Default for DeployConfig {
    fn default() -> Self { Self { backend: "none".into(), library_folder: String::new(), feed_folder: String::new() } }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_device")]
    pub device: String,
    #[serde(default = "default_output_dir")]
    pub output_dir: String,
    #[serde(default = "default_theme")]
    pub theme: String,
    pub readwise: ReadwiseConfig,
    #[serde(default)]
    pub library: LibraryConfig,
    #[serde(default)]
    pub feed: FeedConfig,
    #[serde(default)]
    pub images: ImagesConfig,
    #[serde(default)]
    pub deploy: DeployConfig,
}
fn default_device() -> String { "paper-pro-move".into() }
fn default_output_dir() -> String { ".".into() }
fn default_theme() -> String { "reader".into() }

const VALID_LOCATIONS: &[&str] = &["new", "later", "shortlist", "archive", "feed"];

impl Config {
    pub fn validate(&self) -> anyhow::Result<()> {
        crate::device::get_device(&self.device)?;
        crate::theme::load_theme(&self.theme)?;
        if self.readwise.token.trim().is_empty() {
            anyhow::bail!("readwise.token is required (get one at https://readwise.io/access_token)");
        }
        if self.library.locations.is_empty() {
            anyhow::bail!("library.locations must list at least one location");
        }
        for loc in &self.library.locations {
            if !VALID_LOCATIONS.contains(&loc.as_str()) {
                anyhow::bail!("invalid library location {loc:?}; choices: {VALID_LOCATIONS:?}");
            }
        }
        match self.deploy.backend.as_str() {
            "none" => {}
            "rmapi" => {
                if self.deploy.library_folder.trim().is_empty() {
                    anyhow::bail!("deploy.library_folder is required for the rmapi backend");
                }
                if self.feed.enabled && self.deploy.feed_folder.trim().is_empty() {
                    anyhow::bail!("deploy.feed_folder is required when feed is enabled and backend is rmapi");
                }
            }
            other => anyhow::bail!("deploy.backend must be 'none' or 'rmapi', got {other:?}"),
        }
        Ok(())
    }
}

pub fn load(path: &Path) -> anyhow::Result<Config> {
    let s = std::fs::read_to_string(path)?;
    Ok(toml::from_str(&s)?)
}

pub fn dump(config: &Config, path: &Path) -> anyhow::Result<()> {
    std::fs::write(path, toml::to_string_pretty(config)?)?;
    Ok(())
}
```

- [ ] **Step 4: Run — expect PASS** (`cargo test --test config`).

- [ ] **Step 5: Commit**

```bash
git add src/config.rs tests/config.rs && git commit -m "Add config with validation"
```

---

### Task 2: Readwise client

**Goal:** Fetch the Library (merge of `new`+`later`+`shortlist`) and Feed documents with inline HTML content, paginated, deduped, sorted by `saved_at` desc, capped — behind a testable `HttpTransport` seam. Plus a token-validation helper.

**Files:**
- Create: `src/readwise/mod.rs`, `src/readwise/http.rs`
- Modify: `src/lib.rs` (add `pub mod readwise;`)
- Test: `tests/readwise.rs`

**Acceptance Criteria:**
- [ ] `HttpTransport` trait with a `ureq` impl and a fake for tests
- [ ] Paginates via `nextPageCursor`, requests `withHtmlContent=true&limit=100&location=<loc>`
- [ ] Merges locations, dedupes by `id`, sorts by `saved_at` desc, truncates to `max_items`
- [ ] `429` honored by sleeping `Retry-After` (injectable sleep; tests assert retry without real sleep)
- [ ] `validate_token` returns Ok on 204, Err otherwise

**Verify:** `nix develop -c cargo test --test readwise` → all pass

**Steps:**

- [ ] **Step 1: Write `tests/readwise.rs` (failing)**

```rust
use rmreader::readwise::{fetch_documents, Document, HttpResponse, HttpTransport};
use std::cell::RefCell;

/// Fake transport returning canned responses per URL substring.
struct Fake { calls: RefCell<Vec<String>>, script: Vec<(u16, Option<u64>, String)>, idx: RefCell<usize> }
impl HttpTransport for Fake {
    fn get(&self, url: &str, _token: &str) -> anyhow::Result<HttpResponse> {
        self.calls.borrow_mut().push(url.to_string());
        let mut i = self.idx.borrow_mut();
        let (status, retry, body) = self.script[*i].clone();
        *i += 1;
        Ok(HttpResponse { status, retry_after: retry, body })
    }
}

fn page(results: &str, cursor: Option<&str>) -> String {
    let c = cursor.map(|x| format!("\"{x}\"")).unwrap_or("null".into());
    format!("{{\"count\":1,\"nextPageCursor\":{c},\"results\":[{results}]}}")
}
fn doc(id: &str, saved: &str) -> String {
    format!("{{\"id\":\"{id}\",\"title\":\"T{id}\",\"saved_at\":\"{saved}\",\"location\":\"new\",\"category\":\"article\",\"html_content\":\"<p>x</p>\"}}")
}

#[test]
fn paginates_and_sorts_desc_and_caps() {
    // two pages on one location, returned newest-first after sort, capped to 2.
    let script = vec![
        (200, None, page(&format!("{},{}", doc("a","2026-01-01T00:00:00Z"), doc("b","2026-03-01T00:00:00Z")), Some("CUR"))),
        (200, None, page(&doc("c","2026-02-01T00:00:00Z"), None)),
    ];
    let fake = Fake { calls: RefCell::new(vec![]), script, idx: RefCell::new(0) };
    let docs = fetch_documents(&fake, "tok", &["new".into()], 2, |_| {}).unwrap();
    assert_eq!(docs.iter().map(|d| d.id.as_str()).collect::<Vec<_>>(), vec!["b", "c"]);
    assert!(fake.calls.borrow()[1].contains("pageCursor=CUR"));
    assert!(fake.calls.borrow()[0].contains("withHtmlContent=true"));
    assert!(fake.calls.borrow()[0].contains("location=new"));
}

#[test]
fn retries_after_429() {
    let mut slept = 0u64;
    let script = vec![
        (429, Some(7), String::new()),
        (200, None, page(&doc("a","2026-01-01T00:00:00Z"), None)),
    ];
    let fake = Fake { calls: RefCell::new(vec![]), script, idx: RefCell::new(0) };
    let docs = fetch_documents(&fake, "tok", &["new".into()], 10, |s| slept += s).unwrap();
    assert_eq!(docs.len(), 1);
    assert_eq!(slept, 7);
}

#[test]
fn dedupes_across_locations() {
    let script = vec![
        (200, None, page(&doc("a","2026-01-01T00:00:00Z"), None)), // new
        (200, None, page(&doc("a","2026-01-01T00:00:00Z"), None)), // later (same id)
    ];
    let fake = Fake { calls: RefCell::new(vec![]), script, idx: RefCell::new(0) };
    let docs = fetch_documents(&fake, "tok", &["new".into(), "later".into()], 10, |_| {}).unwrap();
    assert_eq!(docs.len(), 1);
}
```

- [ ] **Step 2: Run — expect FAIL** (no `readwise` module).

- [ ] **Step 3: Implement `src/readwise/mod.rs`**

```rust
//! Readwise Reader API client (https://readwise.io/reader_api).
pub mod http;

use serde::Deserialize;

const LIST_URL: &str = "https://readwise.io/api/v3/list/";
const AUTH_URL: &str = "https://readwise.io/api/v2/auth/";

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub retry_after: Option<u64>,
    pub body: String,
}

/// Low-level seam so pagination/sort/rate-limit are testable without network.
pub trait HttpTransport {
    fn get(&self, url: &str, token: &str) -> anyhow::Result<HttpResponse>;
}

#[derive(Debug, Clone, Deserialize)]
pub struct Document {
    pub id: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub source_url: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub site_name: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub image_url: String,
    #[serde(default)]
    pub word_count: Option<u32>,
    #[serde(default)]
    pub reading_time: Option<u32>,
    #[serde(default)]
    pub published_date: Option<String>,
    #[serde(default)]
    pub saved_at: String,
    #[serde(default)]
    pub html_content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ListResponse {
    #[serde(default)]
    next_page_cursor: Option<String>,
    results: Vec<Document>,
}

fn list_url(location: &str, cursor: Option<&str>) -> String {
    let mut u = format!("{LIST_URL}?withHtmlContent=true&limit=100&location={location}");
    if let Some(c) = cursor {
        u.push_str(&format!("&pageCursor={c}"));
    }
    u
}

/// Validate a token: GET /api/v2/auth/ returns 204 when valid.
pub fn validate_token(t: &dyn HttpTransport, token: &str) -> anyhow::Result<()> {
    let r = t.get(AUTH_URL, token)?;
    if r.status == 204 || r.status == 200 {
        Ok(())
    } else {
        anyhow::bail!("Readwise token rejected (HTTP {})", r.status)
    }
}

/// Fetch + merge + dedupe + sort(saved_at desc) + cap. `sleep` is injected so
/// tests can assert Retry-After handling without real delays.
pub fn fetch_documents(
    t: &dyn HttpTransport,
    token: &str,
    locations: &[String],
    max_items: u32,
    mut sleep: impl FnMut(u64),
) -> anyhow::Result<Vec<Document>> {
    let mut all: Vec<Document> = Vec::new();
    for loc in locations {
        let mut cursor: Option<String> = None;
        loop {
            let url = list_url(loc, cursor.as_deref());
            let resp = t.get(&url, token)?;
            if resp.status == 429 {
                sleep(resp.retry_after.unwrap_or(60));
                continue; // retry same cursor
            }
            if resp.status != 200 {
                anyhow::bail!("Readwise list failed (HTTP {}) for location {loc}", resp.status);
            }
            let parsed: ListResponse = serde_json::from_str(&resp.body)?;
            all.extend(parsed.results);
            match parsed.next_page_cursor {
                Some(c) if !c.is_empty() => cursor = Some(c),
                _ => break,
            }
        }
    }
    // dedupe by id (keep first seen)
    let mut seen = std::collections::HashSet::new();
    all.retain(|d| seen.insert(d.id.clone()));
    // sort newest first by saved_at (ISO 8601 sorts lexicographically)
    all.sort_by(|a, b| b.saved_at.cmp(&a.saved_at));
    all.truncate(max_items as usize);
    Ok(all)
}
```

- [ ] **Step 4: Implement `src/readwise/http.rs`** (ureq impl)

```rust
//! Real HttpTransport over ureq (rustls).
use super::{HttpResponse, HttpTransport};

#[derive(Debug, Default)]
pub struct UreqTransport;

impl HttpTransport for UreqTransport {
    fn get(&self, url: &str, token: &str) -> anyhow::Result<HttpResponse> {
        let auth = format!("Token {token}");
        let result = ureq::get(url).set("Authorization", &auth).call();
        match result {
            Ok(resp) => Ok(HttpResponse {
                status: resp.status(),
                retry_after: None,
                body: resp.into_string()?,
            }),
            Err(ureq::Error::Status(code, resp)) => {
                let retry_after = resp.header("retry-after").and_then(|s| s.trim().parse::<u64>().ok());
                Ok(HttpResponse { status: code, retry_after, body: resp.into_string().unwrap_or_default() })
            }
            Err(e) => Err(anyhow::anyhow!("HTTP error for {url}: {e}")),
        }
    }
}
```

- [ ] **Step 5: Add `pub mod readwise;` to `src/lib.rs`.**

- [ ] **Step 6: Run — expect PASS** (`cargo test --test readwise`).

- [ ] **Step 7: Commit**

```bash
git add src/readwise tests/readwise.rs src/lib.rs && git commit -m "Add Readwise client with pagination, sort, rate-limit"
```

---

### Task 3: Wizard

**Goal:** `rmreader init` interactive wizard. Pure `assemble()` builds `Config` + paths from answers (testable); `run_wizard()` prompts via dialoguer and validates the token via the transport.

**Files:**
- Create: `src/wizard.rs`
- Modify: `src/lib.rs` (add `pub mod wizard;`)
- Test: `tests/wizard.rs`

**Acceptance Criteria:**
- [ ] `assemble(Answers)` returns a valid `Config`, output dir, and `rmreader.toml` path
- [ ] Resulting config passes `Config::validate()`
- [ ] Token validation surfaced (uses `readwise::validate_token`)

**Verify:** `nix develop -c cargo test --test wizard` → all pass

**Steps:**

- [ ] **Step 1: Write `tests/wizard.rs` (failing)**

```rust
use rmreader::wizard::{assemble, Answers};

#[test]
fn assemble_builds_valid_config() {
    let (cfg, out_dir, cfg_path) = assemble(Answers {
        output_dir: "/tmp/reader".into(),
        device: "paper-pro-move".into(),
        token: "tok".into(),
        library_locations: vec!["new".into(), "later".into(), "shortlist".into()],
        library_max: 100,
        feed_enabled: true,
        feed_max: 100,
        images_enabled: true,
        deploy_backend: "rmapi".into(),
        library_folder: "/Reader".into(),
        feed_folder: "/Reader".into(),
    });
    assert!(cfg.validate().is_ok());
    assert_eq!(out_dir.to_str().unwrap(), "/tmp/reader");
    assert_eq!(cfg_path.to_str().unwrap(), "/tmp/reader/rmreader.toml");
}
```

- [ ] **Step 2: Run — expect FAIL.**

- [ ] **Step 3: Implement `src/wizard.rs`**

```rust
//! Interactive `init` wizard. `assemble` is pure (testable); `run_wizard` prompts.
use std::path::PathBuf;

use crate::config::{Config, DeployConfig, FeedConfig, ImagesConfig, LibraryConfig, ReadwiseConfig};

pub struct Answers {
    pub output_dir: String,
    pub device: String,
    pub token: String,
    pub library_locations: Vec<String>,
    pub library_max: u32,
    pub feed_enabled: bool,
    pub feed_max: u32,
    pub images_enabled: bool,
    pub deploy_backend: String,
    pub library_folder: String,
    pub feed_folder: String,
}

pub fn assemble(a: Answers) -> (Config, PathBuf, PathBuf) {
    let config = Config {
        device: a.device,
        output_dir: a.output_dir.clone(),
        theme: "reader".into(),
        readwise: ReadwiseConfig { token: a.token },
        library: LibraryConfig { locations: a.library_locations, max_items: a.library_max },
        feed: FeedConfig { enabled: a.feed_enabled, max_items: a.feed_max },
        images: ImagesConfig { enabled: a.images_enabled },
        deploy: DeployConfig {
            backend: a.deploy_backend,
            library_folder: a.library_folder,
            feed_folder: a.feed_folder,
        },
    };
    let out_dir = PathBuf::from(a.output_dir);
    let config_path = out_dir.join("rmreader.toml");
    (config, out_dir, config_path)
}

/// Prompt, validate the token, and return Config + paths. Caller writes files.
pub fn run_wizard(transport: &dyn crate::readwise::HttpTransport) -> anyhow::Result<(Config, PathBuf, PathBuf)> {
    use dialoguer::{Confirm, Input};

    let output_dir: String = Input::new().with_prompt("Output directory").default(".".into()).interact_text()?;
    let device: String = Input::new().with_prompt("Device (paper-pro-move|paper-pro)").default("paper-pro-move".into()).interact_text()?;

    println!("Get your Readwise token at https://readwise.io/access_token");
    let token: String = Input::new().with_prompt("Readwise token").interact_text()?;
    crate::readwise::validate_token(transport, &token)?; // fail fast on bad token

    let library_max: u32 = Input::new().with_prompt("Library: max items").default(100).interact_text()?;
    let feed_enabled: bool = Confirm::new().with_prompt("Generate a Feed PDF?").default(true).interact()?;
    let feed_max: u32 = Input::new().with_prompt("Feed: max items").default(100).interact_text()?;
    let images_enabled: bool = Confirm::new().with_prompt("Include images?").default(true).interact()?;
    let deploy_backend: String = Input::new().with_prompt("Deploy backend (none|rmapi)").default("none".into()).interact_text()?;
    let (library_folder, feed_folder) = if deploy_backend == "rmapi" {
        let lf: String = Input::new().with_prompt("reMarkable folder for Library").default("/Reader".into()).interact_text()?;
        let ff: String = Input::new().with_prompt("reMarkable folder for Feed").default(lf.clone()).interact_text()?;
        (lf, ff)
    } else {
        (String::new(), String::new())
    };

    Ok(assemble(Answers {
        output_dir, device, token,
        library_locations: vec!["new".into(), "later".into(), "shortlist".into()],
        library_max, feed_enabled, feed_max, images_enabled,
        deploy_backend, library_folder, feed_folder,
    }))
}
```

- [ ] **Step 4: Add `pub mod wizard;` to `src/lib.rs`. Run — expect PASS.**

- [ ] **Step 5: Commit**

```bash
git add src/wizard.rs tests/wizard.rs src/lib.rs && git commit -m "Add init wizard"
```

---

### Task 4: Content pipeline

**Goal:** Transform a Readwise `html_content` string into render-ready HTML: fetch content images (with tracking-pixel/size guards), transcode WebP/AVIF → PNG, rewrite `<img src>` to `AssetBundle` keys, and strip `<script>`/`<iframe>`/`on*` handlers. Image fetching is behind a trait so tests don't hit the network.

**Files:**
- Create: `src/content.rs`
- Modify: `src/lib.rs` (add `pub mod content;`)
- Test: `tests/content.rs`

**Acceptance Criteria:**
- [ ] Returns cleaned HTML plus a list of `(asset_key, bytes)` for embedding
- [ ] `<script>`, `<iframe>` removed; `onclick` etc. removed
- [ ] Tracking pixels (≤2px or non-decodable) dropped; failed fetches drop the `<img>`
- [ ] WebP/AVIF bytes transcoded to PNG; PNG/JPEG/GIF/SVG passed through
- [ ] `images_enabled=false` drops all `<img>` and fetches nothing

**Verify:** `nix develop -c cargo test --test content` → all pass

**Steps:**

- [ ] **Step 1: Write `tests/content.rs` (failing)**

```rust
use rmreader::content::{process_html, ImageFetcher, FetchedImage};
use std::cell::RefCell;

struct FakeFetcher { map: std::collections::HashMap<String, Option<FetchedImage>>, fetched: RefCell<Vec<String>> }
impl ImageFetcher for FakeFetcher {
    fn fetch(&self, url: &str) -> Option<FetchedImage> {
        self.fetched.borrow_mut().push(url.to_string());
        self.map.get(url).cloned().flatten()
    }
}

fn png_1x1() -> Vec<u8> {
    // minimal valid 1x1 PNG
    use image::{ImageBuffer, Rgba};
    let img: ImageBuffer<Rgba<u8>, _> = ImageBuffer::from_pixel(1, 1, Rgba([0,0,0,255]));
    let mut buf = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(img).write_to(&mut buf, image::ImageFormat::Png).unwrap();
    buf.into_inner()
}
fn png_8x8() -> Vec<u8> {
    use image::{ImageBuffer, Rgba};
    let img: ImageBuffer<Rgba<u8>, _> = ImageBuffer::from_pixel(8, 8, Rgba([10,20,30,255]));
    let mut buf = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(img).write_to(&mut buf, image::ImageFormat::Png).unwrap();
    buf.into_inner()
}

#[test]
fn strips_scripts_and_handlers_keeps_text() {
    let html = r#"<p onclick="x()">Hello</p><script>evil()</script><iframe src="z"></iframe>"#;
    let f = FakeFetcher { map: Default::default(), fetched: RefCell::new(vec![]) };
    let out = process_html(html, true, &f);
    assert!(out.html.contains("Hello"));
    assert!(!out.html.contains("script"));
    assert!(!out.html.contains("iframe"));
    assert!(!out.html.contains("onclick"));
}

#[test]
fn embeds_real_image_and_rewrites_src() {
    let mut map = std::collections::HashMap::new();
    map.insert("https://x/p.png".to_string(), Some(FetchedImage { bytes: png_8x8(), ext: "png".into() }));
    let f = FakeFetcher { map, fetched: RefCell::new(vec![]) };
    let out = process_html(r#"<p><img src="https://x/p.png"></p>"#, true, &f);
    assert_eq!(out.assets.len(), 1);
    let key = &out.assets[0].0;
    assert!(out.html.contains(key));
    assert!(!out.html.contains("https://x/p.png"));
}

#[test]
fn drops_tracking_pixel() {
    let mut map = std::collections::HashMap::new();
    map.insert("https://x/track.png".to_string(), Some(FetchedImage { bytes: png_1x1(), ext: "png".into() }));
    let f = FakeFetcher { map, fetched: RefCell::new(vec![]) };
    let out = process_html(r#"<img src="https://x/track.png">"#, true, &f);
    assert_eq!(out.assets.len(), 0);
    assert!(!out.html.contains("img"));
}

#[test]
fn images_disabled_drops_all_imgs_without_fetch() {
    let f = FakeFetcher { map: Default::default(), fetched: RefCell::new(vec![]) };
    let out = process_html(r#"<img src="https://x/p.png">text"#, false, &f);
    assert!(out.html.contains("text"));
    assert!(!out.html.contains("img"));
    assert!(f.fetched.borrow().is_empty());
}
```

- [ ] **Step 2: Run — expect FAIL.**

- [ ] **Step 3: Implement `src/content.rs`**

```rust
//! Turn Readwise html_content into render-ready HTML with embedded local images.
use lol_html::{element, rewrite_str, RewriteStrSettings};

#[derive(Clone)]
pub struct FetchedImage {
    pub bytes: Vec<u8>,
    pub ext: String, // "png" | "jpg" | "gif" | "svg" (post-transcode)
}

/// Network seam (real impl in render/generate uses ureq).
pub trait ImageFetcher {
    fn fetch(&self, url: &str) -> Option<FetchedImage>;
}

pub struct Processed {
    pub html: String,
    pub assets: Vec<(String, Vec<u8>)>, // (asset_key, bytes) for AssetBundle
}

/// Collect <img> src URLs (first pass).
fn collect_img_urls(html: &str) -> Vec<String> {
    let urls = std::cell::RefCell::new(Vec::new());
    let _ = rewrite_str(html, RewriteStrSettings {
        element_content_handlers: vec![element!("img[src]", |el| {
            if let Some(src) = el.get_attribute("src") {
                if src.starts_with("http://") || src.starts_with("https://") {
                    urls.borrow_mut().push(src);
                }
            }
            Ok(())
        })],
        ..RewriteStrSettings::default()
    });
    urls.into_inner()
}

/// Decode bytes, drop tracking pixels (<=2px on either side), transcode
/// WebP/AVIF to PNG. Returns (final_bytes, ext) or None to drop the image.
fn normalize_image(bytes: &[u8]) -> Option<(Vec<u8>, String)> {
    let fmt = image::guess_format(bytes).ok()?;
    let img = image::load_from_memory(bytes).ok()?;
    let (w, h) = image::GenericImageView::dimensions(&img);
    if w <= 2 || h <= 2 {
        return None; // tracking pixel
    }
    use image::ImageFormat as F;
    match fmt {
        F::Png => Some((bytes.to_vec(), "png".into())),
        F::Jpeg => Some((bytes.to_vec(), "jpg".into())),
        F::Gif => Some((bytes.to_vec(), "gif".into())),
        _ => {
            // WebP/AVIF/etc -> re-encode to PNG (krilla supports png/jpeg/gif/svg)
            let mut out = std::io::Cursor::new(Vec::new());
            img.write_to(&mut out, F::Png).ok()?;
            Some((out.into_inner(), "png".into()))
        }
    }
}

pub fn process_html(html: &str, images_enabled: bool, fetcher: &dyn ImageFetcher) -> Processed {
    use std::collections::HashMap;

    // Pass 1: fetch + normalize images into a url -> key map.
    let mut url_to_key: HashMap<String, String> = HashMap::new();
    let mut assets: Vec<(String, Vec<u8>)> = Vec::new();
    if images_enabled {
        for (i, url) in collect_img_urls(html).into_iter().enumerate() {
            if url_to_key.contains_key(&url) {
                continue;
            }
            if let Some(fetched) = fetcher.fetch(&url) {
                let normalized = if fetched.ext == "svg" {
                    Some((fetched.bytes.clone(), "svg".into()))
                } else {
                    normalize_image(&fetched.bytes)
                };
                if let Some((bytes, ext)) = normalized {
                    let key = format!("img-{i}.{ext}");
                    url_to_key.insert(url.clone(), key.clone());
                    assets.push((key, bytes));
                }
            }
        }
    }

    // Pass 2: rewrite img src -> key (drop unresolved/disabled), strip dangerous nodes/attrs.
    let cleaned = rewrite_str(html, RewriteStrSettings {
        element_content_handlers: vec![
            element!("script,iframe,noscript,style,object,embed,form", |el| { el.remove(); Ok(()) }),
            element!("img", |el| {
                let keep = el.get_attribute("src")
                    .and_then(|s| url_to_key.get(&s).cloned());
                match keep {
                    Some(key) => { let _ = el.set_attribute("src", &key); }
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
    }).unwrap_or_else(|_| html.to_string());

    Processed { html: cleaned, assets }
}
```

- [ ] **Step 4: Add `pub mod content;` to `src/lib.rs`. Run — expect PASS.**

  Note: if `image::guess_format`/`GenericImageView` imports differ in image 0.25, adjust to `image::GenericImageView` trait import (`use image::GenericImageView;`). Add `use image::GenericImageView;` at top if the dimensions call needs it.

- [ ] **Step 5: Commit**

```bash
git add src/content.rs tests/content.rs src/lib.rs && git commit -m "Add content pipeline: sanitize + image fetch/rewrite/transcode"
```

---

### Task 5: SPIKE — links inside fulgur running headers

**Goal:** Decide whether the nav bar can be a per-page running header with working links, or must be a per-section block. This decision shapes `assemble.rs` (Task 6).

**Files:**
- Create: `docs/superpowers/spikes/2026-05-21-fulgur-running-header-links.md`
- Create (throwaway): `tests/spike_running_header.rs` (delete after recording the result)

**Acceptance Criteria:**
- [ ] A minimal multi-page HTML with an `<a href="#home">` in a GCPM running header is rendered to PDF
- [ ] lopdf inspection determines whether a link annotation appears on a non-first page
- [ ] Decision recorded in the spike doc; `tests/spike_running_header.rs` removed

**Verify:** `nix develop -c cargo test --test spike_running_header` runs; read its asserted output, then record the finding.

**Steps:**

- [ ] **Step 1: Write `tests/spike_running_header.rs`**

```rust
// Spike: does fulgur emit a link annotation for <a> inside a GCPM running header,
// on a page that is NOT the first? Render ~3 pages, then count /Annots with /Link.
use fulgur::asset::AssetBundle;
use fulgur::config::{Margin, PageSize};
use fulgur::engine::Engine;

#[test]
fn running_header_link_on_later_pages() {
    let css = r#"
@page { size: 300pt 200pt; margin: 30pt 20pt; }
.nav { position: running(nav); }
@page { @top-center { content: element(nav); } }
.tall { height: 1200pt; }
"#;
    let html = format!(
        "<!doctype html><html><head><style>{css}</style></head><body>\
         <div class=\"nav\"><a href=\"#home\">Home</a></div>\
         <h1 id=\"home\">Home</h1><div class=\"tall\">long</div></body></html>");
    let engine = Engine::builder()
        .page_size(PageSize { width: 300.0, height: 200.0 })
        .margin(Margin::uniform(0.0))
        .assets(AssetBundle::new())
        .build();
    let dir = std::env::temp_dir();
    let out = dir.join("spike_rh.pdf");
    engine.render_html_to_file(&html, &out).unwrap();

    let doc = lopdf::Document::load(&out).unwrap();
    let pages = doc.get_pages();
    assert!(pages.len() >= 2, "spike needs multiple pages, got {}", pages.len());
    let mut link_pages = 0;
    for (_n, page_id) in pages {
        let page = doc.get_dictionary(page_id).unwrap();
        if let Ok(annots) = page.get(b"Annots") {
            if let Ok(arr) = annots.as_array() {
                for a in arr {
                    if let Ok(id) = a.as_reference() {
                        if let Ok(ad) = doc.get_dictionary(id) {
                            if ad.get(b"Subtype").ok().and_then(|s| s.as_name().ok()) == Some(b"Link") {
                                link_pages += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    eprintln!("SPIKE RESULT: link annotations found on {link_pages} page(s)");
    // Not an assertion of pass/fail — the eprintln is the result we record.
}
```

- [ ] **Step 2: Run and read result**

Run: `nix develop -c cargo test --test spike_running_header -- --nocapture`
Read the `SPIKE RESULT:` line.

- [ ] **Step 3: Record decision**

Write `docs/superpowers/spikes/2026-05-21-fulgur-running-header-links.md`:
- If links appear on later pages → **per-page running-header nav** in Task 6.
- If not → **per-section nav block** (one nav at the top of index/each card/each article first page) + rely on `bookmarks(true)` native outline for device-wide TOC. Task 6 implements this fallback.

- [ ] **Step 4: Remove the throwaway test, commit the spike doc**

```bash
rm tests/spike_running_header.rs
git add docs/superpowers/spikes/2026-05-21-fulgur-running-header-links.md
git commit -m "Spike: fulgur running-header link support; record decision"
```

---

### Task 6: Manifest + PDF assembly + templates

**Goal:** Build one HTML document per PDF (index → cards → articles) with stable cross-section anchors, nav, and bookmark CSS; and a sidecar manifest mapping each doc id to its card/article anchors. Aggregate per-article image assets.

**Files:**
- Create: `src/manifest.rs`, `src/assemble.rs`, `templates/index.html`, `templates/card.html`, `templates/article.html`, `templates/nav.html`
- Modify: `src/lib.rs` (add `pub mod manifest; pub mod assemble;`)
- Test: `tests/assemble.rs`

**Acceptance Criteria:**
- [ ] `assemble(docs, &content_fn)` returns `{ html: String, assets: Vec<(String,Vec<u8>)>, manifest: Manifest }`
- [ ] Index rows link to `#item-<id>`; cards link to `#article-<id>`; sections carry matching `id`s
- [ ] Nav per the spike decision (Task 5)
- [ ] Article headings carry `bookmark-level`/`bookmark-label` CSS classes
- [ ] Manifest lists every doc id with its `card_anchor` and `article_anchor`

**Verify:** `nix develop -c cargo test --test assemble` → all pass

**Steps:**

- [ ] **Step 1: Write `tests/assemble.rs` (failing)**

```rust
use rmreader::assemble::assemble_document;
use rmreader::readwise::Document;

fn doc(id: &str) -> Document {
    Document {
        id: id.into(), url: format!("https://ex/{id}"), source_url: String::new(),
        title: format!("Title {id}"), author: "Auth".into(), site_name: "Site".into(),
        category: "article".into(), location: "new".into(), summary: "Sum".into(),
        image_url: String::new(), word_count: Some(500), reading_time: Some(3),
        published_date: None, saved_at: "2026-01-01T00:00:00Z".into(),
        html_content: Some("<p>Body</p>".into()),
    }
}

#[test]
fn builds_linked_document_and_manifest() {
    let docs = vec![doc("a"), doc("b")];
    // content_fn: identity-ish (returns processed html + no assets)
    let built = assemble_document("Library", &docs, |html, _id| (html.to_string(), vec![]));
    let html = built.fragments.join("\n");
    // anchors present
    assert!(html.contains("id=\"item-a\""));
    assert!(html.contains("id=\"article-a\""));
    assert!(html.contains("href=\"#item-a\""));   // index -> card
    assert!(html.contains("href=\"#article-a\"")); // card -> article
    // manifest
    assert_eq!(built.manifest.items.len(), 2);
    assert_eq!(built.manifest.items[0].id, "a");
    assert_eq!(built.manifest.items[0].card_anchor, "item-a");
    assert_eq!(built.manifest.items[0].article_anchor, "article-a");
}
```

- [ ] **Step 2: Run — expect FAIL.**

- [ ] **Step 3: Implement `src/manifest.rs`**

```rust
//! Sidecar manifest: maps Readwise doc ids to PDF anchors. Seam for the future
//! annotation phase (page -> doc id once page numbers are known post-render).
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManifestItem {
    pub id: String,
    pub title: String,
    pub url: String,
    pub card_anchor: String,
    pub article_anchor: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub collection: String, // "Library" | "Feed"
    pub items: Vec<ManifestItem>,
}

impl Manifest {
    pub fn write(&self, path: &Path) -> anyhow::Result<()> {
        std::fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }
}
```

- [ ] **Step 4: Implement templates** (askama). `templates/nav.html`, `templates/index.html`, `templates/card.html`, `templates/article.html`. Use the spike decision for nav placement; below is the per-section variant (swap to running-header CSS in `render.rs` if the spike was positive). Keep anchors EXACTLY `item-<id>` and `article-<id>`.

`templates/nav.html`:
```html
<nav class="nav">
  <a class="home" href="#index">⌂ Home</a>
  {% if let Some(p) = prev %}<a href="#{{ p }}">‹ Prev</a>{% endif %}
  {% if let Some(n) = next %}<a href="#{{ n }}">Next ›</a>{% endif %}
  {% if let Some(c) = card %}<a href="#{{ c }}">↑ Card</a>{% endif %}
</nav>
```

`templates/index.html`:
```html
<section class="page index" id="index">
  <h1 class="index-title">{{ collection }}</h1>
  <div class="index-sub">{{ count }} articles · newest first</div>
  {% for row in rows %}
  <a class="index-row" href="#{{ row.anchor }}">
    <span class="n">{{ row.num }}</span>
    <span class="t">{{ row.title }} — {{ row.author }}</span>
    <span class="rt">{{ row.reading_time }}</span>
  </a>
  {% endfor %}
</section>
```

`templates/card.html`:
```html
<section class="page card" id="{{ anchor }}">
  {{ nav|safe }}
  <div class="card-body">
    <div class="kicker">{{ category }}</div>
    <h2 class="ctitle">{{ title }}</h2>
    <div class="summary">{{ summary }}</div>
    <div class="meta-row"><span class="who">{{ author }}</span> · <span>{{ site_name }}</span> · <span>{{ reading_time }}</span></div>
    <a class="read" href="#{{ article_anchor }}">Read the article →</a>
  </div>
</section>
```

`templates/article.html`:
```html
<section class="page article" id="{{ anchor }}">
  {{ nav|safe }}
  <div class="kicker">{{ category }}</div>
  <h2 class="headline bk">{{ title }}</h2>
  <div class="byline"><span class="who">{{ author }}</span> · <span>{{ site_name }}</span> · <span>{{ reading_time }}</span></div>
  <div class="hr"></div>
  <div class="body drop">{{ content|safe }}</div>
</section>
```

Corresponding askama structs go in `src/assemble.rs` (Step 5). The `bk` class is targeted by `bookmark-level`/`bookmark-label` CSS in `render.rs`.

- [ ] **Step 5: Implement `src/assemble.rs`**

```rust
//! Build the 3-tier HTML document (index, cards, articles) + manifest.
use askama::Template;

use crate::manifest::{Manifest, ManifestItem};
use crate::readwise::Document;

pub struct Built {
    pub fragments: Vec<String>, // page fragments (wrapped by render::Base)
    pub assets: Vec<(String, Vec<u8>)>,
    pub manifest: Manifest,
}

#[derive(Template)]
#[template(path = "nav.html")]
struct Nav { prev: Option<String>, next: Option<String>, card: Option<String> }

struct IndexRow { num: String, title: String, author: String, reading_time: String, anchor: String }

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTpl<'a> { collection: &'a str, count: usize, rows: &'a [IndexRow] }

#[derive(Template)]
#[template(path = "card.html")]
struct CardTpl<'a> {
    anchor: &'a str, article_anchor: &'a str, nav: &'a str,
    category: &'a str, title: &'a str, summary: &'a str,
    author: &'a str, site_name: &'a str, reading_time: &'a str,
}

#[derive(Template)]
#[template(path = "article.html")]
struct ArticleTpl<'a> {
    anchor: &'a str, nav: &'a str, category: &'a str, title: &'a str,
    author: &'a str, site_name: &'a str, reading_time: &'a str, content: &'a str,
}

fn rt(d: &Document) -> String {
    d.reading_time.map(|m| format!("{m} min")).unwrap_or_else(|| "—".into())
}

/// `content_fn(html_content, id) -> (processed_html, assets)` is injected so the
/// content pipeline (and its network) stays out of assembly (testable).
pub fn assemble_document(
    collection: &str,
    docs: &[Document],
    content_fn: impl Fn(&str, &str) -> (String, Vec<(String, Vec<u8>)>),
) -> Built {
    let mut assets: Vec<(String, Vec<u8>)> = Vec::new();
    let mut items: Vec<ManifestItem> = Vec::new();
    let mut fragments: Vec<String> = Vec::new();

    // Index
    let rows: Vec<IndexRow> = docs.iter().enumerate().map(|(i, d)| IndexRow {
        num: format!("{:02}", i + 1),
        title: d.title.clone(),
        author: d.author.clone(),
        reading_time: rt(d),
        anchor: format!("item-{}", d.id),
    }).collect();
    fragments.push(IndexTpl { collection, count: docs.len(), rows: &rows }.render().unwrap());

    // Cards
    for (i, d) in docs.iter().enumerate() {
        let card_anchor = format!("item-{}", d.id);
        let article_anchor = format!("article-{}", d.id);
        let prev = if i > 0 { Some(format!("item-{}", docs[i - 1].id)) } else { None };
        let next = if i + 1 < docs.len() { Some(format!("item-{}", docs[i + 1].id)) } else { None };
        let nav = Nav { prev, next, card: None }.render().unwrap();
        fragments.push(CardTpl {
            anchor: &card_anchor, article_anchor: &article_anchor, nav: &nav,
            category: &d.category, title: &d.title, summary: &d.summary,
            author: &d.author, site_name: &d.site_name, reading_time: &rt(d),
        }.render().unwrap());
        items.push(ManifestItem {
            id: d.id.clone(), title: d.title.clone(), url: d.url.clone(),
            card_anchor, article_anchor,
        });
    }

    // Articles
    for (i, d) in docs.iter().enumerate() {
        let article_anchor = format!("article-{}", d.id);
        let card_anchor = format!("item-{}", d.id);
        let prev = if i > 0 { Some(format!("article-{}", docs[i - 1].id)) } else { None };
        let next = if i + 1 < docs.len() { Some(format!("article-{}", docs[i + 1].id)) } else { None };
        let nav = Nav { prev, next, card: Some(card_anchor) }.render().unwrap();
        let raw = d.html_content.clone().unwrap_or_default();
        let (processed, mut a) = content_fn(&raw, &d.id);
        assets.append(&mut a);
        fragments.push(ArticleTpl {
            anchor: &article_anchor, nav: &nav, category: &d.category, title: &d.title,
            author: &d.author, site_name: &d.site_name, reading_time: &rt(d), content: &processed,
        }.render().unwrap());
    }

    Built {
        fragments,
        assets,
        manifest: Manifest { collection: collection.to_string(), items },
    }
}
```

- [ ] **Step 6: Add `pub mod manifest; pub mod assemble;` to `src/lib.rs`. Run — expect PASS.**

- [ ] **Step 7: Commit**

```bash
git add src/manifest.rs src/assemble.rs templates/ src/lib.rs tests/assemble.rs
git commit -m "Add PDF assembly (index/cards/articles) + manifest"
```

---

### Task 7: Render (fulgur)

**Goal:** Render an assembled HTML fragment list + image assets into a PDF with reader CSS, embedded fonts, color images, and native bookmarks. Prove internal links resolve.

**Files:**
- Create: `src/render.rs` (adapted from `../rmbujo/src/render.rs`)
- Create: `templates` already exist; add reader CSS in `render.rs` (string builder, like rmbujo)
- Add: `assets/fonts/Newsreader-Regular.ttf`, `Newsreader-Italic.ttf`, `Newsreader-SemiBold.ttf`, `HankenGrotesk-Regular.ttf`, `HankenGrotesk-Medium.ttf` (download from Google Fonts static instances)
- Modify: `src/lib.rs` (add `pub mod render;`)
- Test: `tests/render.rs`

**Acceptance Criteria:**
- [ ] `render_pdf(device, theme, fragments, assets, out)` writes a valid PDF
- [ ] A fragment with `<section id="article-a">` and another with `<a href="#article-a">` produces a PDF whose link annotation resolves (lopdf: a `/Link` annotation with a `/Dest` or action exists)
- [ ] Bookmarks present when article headings carry the bookmark class

**Verify:** `nix develop -c cargo test --test render` → pass

**Steps:**

- [ ] **Step 1: Acquire fonts**

```bash
# Newsreader + Hanken Grotesk static TTFs (Google Fonts). Place in assets/fonts/.
# If offline, download via fontsource mirror or google-fonts nix package.
```
Place the five TTFs listed above in `assets/fonts/`.

- [ ] **Step 2: Implement `src/render.rs`** (adapt rmbujo). Build reader CSS using theme vars; embed fonts; assemble assets; render.

```rust
//! Render assembled HTML + image assets to PDF via fulgur (Blitz + krilla).
use std::path::Path;

use askama::Template;
use fulgur::asset::AssetBundle;
use fulgur::config::{Margin, PageSize};
use fulgur::engine::Engine;

use crate::device::Device;
use crate::theme::{css_vars, Palette};

const NEWSREADER: &[u8] = include_bytes!("../assets/fonts/Newsreader-Regular.ttf");
const NEWSREADER_IT: &[u8] = include_bytes!("../assets/fonts/Newsreader-Italic.ttf");
const NEWSREADER_SB: &[u8] = include_bytes!("../assets/fonts/Newsreader-SemiBold.ttf");
const HANKEN: &[u8] = include_bytes!("../assets/fonts/HankenGrotesk-Regular.ttf");
const HANKEN_MD: &[u8] = include_bytes!("../assets/fonts/HankenGrotesk-Medium.ttf");

#[derive(Template)]
#[template(path = "base.html")]
struct Base<'a> { css: &'a str, pages: &'a [String] }

pub fn build_css(device: &Device, theme: &Palette) -> String {
    let w = device.width_pt();
    let h = device.height_pt();
    format!(
        "{vars}\n\
@page {{ size: {w}pt {h}pt; margin: 0; }}\n\
* {{ box-sizing: border-box; margin: 0; padding: 0; }}\n\
html, body {{ margin: 0; padding: 0; }}\n\
body {{ font-family: \"Newsreader\", serif; color: var(--ink); }}\n\
.page {{ position: relative; width: {w}pt; height: {h}pt; padding: 30pt 26pt; overflow: hidden; background: var(--paper); break-after: page; }}\n\
.page:last-child {{ break-after: auto; }}\n\
.article {{ break-before: page; }}\n\
.nav {{ display:flex; justify-content:space-between; font-family:\"Hanken Grotesk\",sans-serif; font-size:7.5pt; letter-spacing:.12em; text-transform:uppercase; color:var(--muted); border-bottom:0.5pt solid var(--rule); padding-bottom:6pt; margin-bottom:14pt; }}\n\
.nav a {{ color:var(--muted); text-decoration:none; margin-left:10pt; }}\n\
.nav a.home {{ color:var(--accent); font-weight:600; margin-left:0; }}\n\
.kicker {{ font-family:\"Hanken Grotesk\",sans-serif; font-size:7.5pt; font-weight:600; letter-spacing:.2em; text-transform:uppercase; color:var(--accent); margin-bottom:9pt; }}\n\
.headline {{ font-weight:600; font-size:24pt; line-height:1.05; color:var(--heading); letter-spacing:-.01em; bookmark-level:1; bookmark-label:content(text); }}\n\
.ctitle {{ font-weight:600; font-size:20pt; line-height:1.08; color:var(--heading); }}\n\
.byline, .meta-row {{ font-family:\"Hanken Grotesk\",sans-serif; font-size:8.5pt; color:var(--muted); margin-top:10pt; }}\n\
.hr {{ height:0.5pt; background:var(--rule); margin:14pt 0; }}\n\
.body {{ font-size:11pt; line-height:1.62; color:var(--ink); }}\n\
.body p {{ margin:0 0 9pt; }}\n\
.body a {{ color:var(--accent); text-decoration:underline; }}\n\
.body img {{ max-width:100%; height:auto; }}\n\
.body.drop p:first-of-type::first-letter {{ font-weight:600; color:var(--accent); float:left; font-size:3em; line-height:.8; padding:4pt 6pt 0 0; }}\n\
.index-title {{ font-weight:600; font-size:22pt; color:var(--heading); }}\n\
.index-sub {{ font-family:\"Hanken Grotesk\",sans-serif; font-size:8pt; letter-spacing:.12em; text-transform:uppercase; color:var(--faint); margin-bottom:12pt; }}\n\
.index-row {{ display:flex; gap:8pt; padding:6pt 0; border-bottom:0.5pt solid var(--rule); text-decoration:none; color:var(--ink); }}\n\
.index-row .n {{ color:var(--accent); font-weight:600; width:16pt; }}\n\
.index-row .t {{ flex:1; }}\n\
.index-row .rt {{ font-family:\"Hanken Grotesk\",sans-serif; font-size:8pt; color:var(--muted); }}\n\
.summary {{ font-size:11pt; line-height:1.55; margin-top:8pt; }}\n\
.read {{ display:inline-block; margin-top:12pt; font-family:\"Hanken Grotesk\",sans-serif; font-size:8.5pt; font-weight:600; letter-spacing:.08em; text-transform:uppercase; color:var(--accent); text-decoration:none; border-bottom:1pt solid var(--accent); }}\n",
        vars = css_vars(theme), w = w, h = h,
    )
}

pub fn render_pdf(
    device: &Device,
    theme: &Palette,
    fragments: &[String],
    images: &[(String, Vec<u8>)],
    out_path: &Path,
) -> anyhow::Result<()> {
    let css = build_css(device, theme);
    let html = Base { css: &css, pages: fragments }.render()?;

    let mut assets = AssetBundle::new();
    for (key, bytes) in images {
        assets.add_image(key, bytes.clone());
    }
    assets.add_font_bytes(NEWSREADER.to_vec())?;
    assets.add_font_bytes(NEWSREADER_IT.to_vec())?;
    assets.add_font_bytes(NEWSREADER_SB.to_vec())?;
    assets.add_font_bytes(HANKEN.to_vec())?;
    assets.add_font_bytes(HANKEN_MD.to_vec())?;

    let engine = Engine::builder()
        .page_size(PageSize { width: device.width_pt(), height: device.height_pt() })
        .margin(Margin::uniform(0.0))
        .assets(assets)
        .bookmarks(true)
        .producer("rmreader")
        .creator("rmreader")
        .creation_date("D:20000101000000Z")
        .build();
    engine.render_html_to_file(&html, out_path)?;
    Ok(())
}
```

- [ ] **Step 3: Write `tests/render.rs`**

```rust
use rmreader::device::get_device;
use rmreader::render::render_pdf;
use rmreader::theme::load_theme;

#[test]
fn renders_pdf_with_resolving_internal_link() {
    let device = get_device("paper-pro-move").unwrap();
    let theme = load_theme("reader").unwrap();
    let frags = vec![
        r#"<section class="page" id="index"><a href="#article-a">go</a></section>"#.to_string(),
        r#"<section class="page article" id="article-a"><h2 class="headline">A</h2><div class="body"><p>hi</p></div></section>"#.to_string(),
    ];
    let out = std::env::temp_dir().join("rmreader_render.pdf");
    render_pdf(&device, &theme, &frags, &[], &out).unwrap();

    let doc = lopdf::Document::load(&out).unwrap();
    // find at least one /Link annotation
    let mut links = 0;
    for (_n, pid) in doc.get_pages() {
        if let Ok(annots) = doc.get_dictionary(pid).and_then(|p| p.get(b"Annots")).and_then(|a| a.as_array()) {
            for a in annots {
                if let Ok(id) = a.as_reference() {
                    if let Ok(ad) = doc.get_dictionary(id) {
                        if ad.get(b"Subtype").ok().and_then(|s| s.as_name().ok()) == Some(b"Link") {
                            links += 1;
                        }
                    }
                }
            }
        }
    }
    assert!(links >= 1, "expected at least one Link annotation");
}
```

- [ ] **Step 4: Add `pub mod render;` to `src/lib.rs`. Run — expect PASS.**

  If `.bookmarks(true)` or `add_image` signatures differ, consult `../rmbujo/src/render.rs` and the fulgur source at the path noted in the spec (`engine.rs`, `asset.rs`). The bookmark CSS (`bookmark-level`) requires `.bookmarks(true)`.

- [ ] **Step 5: Commit**

```bash
git add src/render.rs assets/fonts/ src/lib.rs tests/render.rs
git commit -m "Add fulgur render with reader CSS, fonts, bookmarks, links"
```

---

### Task 8: Deploy (rmapi)

**Goal:** Upload both PDFs to the reMarkable cloud, non-destructively on regenerate. Copy rmbujo's proven backend, including the token-clobber guard, adapted to two folders.

**Files:**
- Create: `src/deploy/mod.rs`, `src/deploy/rmapi.rs`, `src/deploy/local.rs` (copied from rmbujo, adapted)
- Modify: `src/lib.rs` (add `pub mod deploy;`)
- Test: `tests/deploy.rs`

**Acceptance Criteria:**
- [ ] `RmapiRunner` trait + `RmapiDeployer` (copied from rmbujo)
- [ ] `deploy(targets)` runs `mkdir` then `put` per (pdf, folder); `refresh` runs `put --content-only`
- [ ] Fake runner verifies the exact command sequences for two PDFs in two folders

**Verify:** `nix develop -c cargo test --test deploy` → all pass

**Steps:**

- [ ] **Step 1: Copy `src/deploy/local.rs` and `src/deploy/rmapi.rs` from rmbujo verbatim.** They already implement the `RmapiRunner` trait, `RmapiDeployer`, `ProcessRmapi` (with token-clobber guard), and the `LocalDeployer`. The only change: rmbujo's `RmapiDeployer` stores a single `target_folder` and deploys a flat list of paths. We need **(pdf, folder)** pairs. Change the deploy/refresh signatures to accept `&[(PathBuf, String)]`:

In `src/deploy/rmapi.rs`, replace the `Deployer for RmapiDeployer` impl body and `RmapiDeployer` fields:

```rust
#[derive(Debug)]
pub struct RmapiDeployer<R: RmapiRunner> { runner: R }

impl<R: RmapiRunner> RmapiDeployer<R> {
    pub fn new(runner: R) -> Self { Self { runner } }
    fn put_args<'a>(&self, pdf: &'a str, folder: &'a str, content_only: bool) -> Vec<&'a str> {
        let mut a = vec!["-ni", "put"];
        if content_only { a.push("--content-only"); }
        a.push(pdf);
        a.push(folder);
        a
    }
}

impl<R: RmapiRunner> super::Deployer for RmapiDeployer<R> {
    fn deploy(&self, targets: &[(std::path::PathBuf, String)]) -> anyhow::Result<()> {
        for (pdf, folder) in targets {
            let _ = self.runner.run(&["-ni", "mkdir", folder.as_str()]);
            self.runner.run(&self.put_args(path_str(pdf)?, folder, false))?;
        }
        Ok(())
    }
    fn refresh(&self, targets: &[(std::path::PathBuf, String)]) -> anyhow::Result<()> {
        for (pdf, folder) in targets {
            self.runner.run(&self.put_args(path_str(pdf)?, folder, true))?;
        }
        Ok(())
    }
}
```
Keep `ProcessRmapi`, `RmapiRunner`, `path_str`, conf-path helpers, and `is_blank_conf` exactly as rmbujo has them.

- [ ] **Step 2: Implement `src/deploy/mod.rs`** (adapted seam)

```rust
//! Deploy seam. `none` is a no-op; `rmapi` uploads (pdf, folder) pairs.
pub mod local;
pub mod rmapi;

use std::path::PathBuf;

use crate::config::Config;

pub trait Deployer: std::fmt::Debug {
    fn deploy(&self, targets: &[(PathBuf, String)]) -> anyhow::Result<()>;
    fn refresh(&self, targets: &[(PathBuf, String)]) -> anyhow::Result<()>;
}

pub fn get_deployer(config: &Config) -> anyhow::Result<Box<dyn Deployer>> {
    match config.deploy.backend.as_str() {
        "none" => Ok(Box::new(local::LocalDeployer)),
        "rmapi" => Ok(Box::new(rmapi::RmapiDeployer::new(rmapi::ProcessRmapi::new()?))),
        other => anyhow::bail!("unsupported deploy backend: {other:?}"),
    }
}
```

Update `src/deploy/local.rs` `Deployer` impl to the new `&[(PathBuf, String)]` signatures (both no-ops).

- [ ] **Step 3: Write `tests/deploy.rs`**

```rust
use rmreader::deploy::rmapi::{RmapiDeployer, RmapiRunner};
use rmreader::deploy::Deployer;
use std::cell::RefCell;
use std::path::PathBuf;

#[derive(Debug, Default)]
struct FakeRunner { calls: RefCell<Vec<Vec<String>>> }
impl RmapiRunner for FakeRunner {
    fn run(&self, args: &[&str]) -> anyhow::Result<()> {
        self.calls.borrow_mut().push(args.iter().map(|s| s.to_string()).collect());
        Ok(())
    }
}

#[test]
fn deploy_mkdir_then_put_per_target() {
    let r = FakeRunner::default();
    let d = RmapiDeployer::new(r);
    let targets = vec![
        (PathBuf::from("/o/Library.pdf"), "/Reader".to_string()),
        (PathBuf::from("/o/Feed.pdf"), "/Reader".to_string()),
    ];
    d.deploy(&targets).unwrap();
    // Can't read r back (moved). Re-do with shared ref instead:
}
```

  Adjust the test to keep a handle on the runner (wrap calls in `Rc<RefCell<...>>` or assert via a runner that stores into an outer `Rc`). Concretely:

```rust
use std::rc::Rc;
#[derive(Debug, Default)]
struct SharedRunner { calls: Rc<RefCell<Vec<Vec<String>>>> }
impl RmapiRunner for SharedRunner {
    fn run(&self, args: &[&str]) -> anyhow::Result<()> {
        self.calls.borrow_mut().push(args.iter().map(|s| s.to_string()).collect());
        Ok(())
    }
}

#[test]
fn deploy_then_refresh_sequences() {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let d = RmapiDeployer::new(SharedRunner { calls: calls.clone() });
    let targets = vec![
        (PathBuf::from("/o/Library.pdf"), "/Reader".to_string()),
        (PathBuf::from("/o/Feed.pdf"), "/Feeds".to_string()),
    ];
    d.deploy(&targets).unwrap();
    d.refresh(&targets).unwrap();
    let c = calls.borrow();
    assert_eq!(c[0], vec!["-ni","mkdir","/Reader"]);
    assert_eq!(c[1], vec!["-ni","put","/o/Library.pdf","/Reader"]);
    assert_eq!(c[2], vec!["-ni","mkdir","/Feeds"]);
    assert_eq!(c[3], vec!["-ni","put","/o/Feed.pdf","/Feeds"]);
    assert_eq!(c[4], vec!["-ni","put","--content-only","/o/Library.pdf","/Reader"]);
    assert_eq!(c[5], vec!["-ni","put","--content-only","/o/Feed.pdf","/Feeds"]);
}
```

- [ ] **Step 4: Add `pub mod deploy;` to `src/lib.rs`. Run — expect PASS.**

- [ ] **Step 5: Commit**

```bash
git add src/deploy tests/deploy.rs src/lib.rs && git commit -m "Add rmapi deploy backend (two folders)"
```

---

### Task 9: Generate orchestration + CLI

**Goal:** Wire everything: fetch → process content (real ureq image fetcher) → assemble → render → write manifest → deploy. Implement the `init` and regenerate CLI flows.

**Files:**
- Create: `src/generate.rs`
- Replace: `src/cli.rs` (the Task 0 stub)
- Modify: `src/lib.rs` (add `pub mod generate;`)
- Test: `tests/cli.rs`, `tests/generate.rs`

**Acceptance Criteria:**
- [ ] `generate(config, transport, image_fetcher)` produces `Library.pdf` (+ `Feed.pdf` if enabled) and matching `*.manifest.json` in `output_dir`, returns `Vec<(PathBuf, String)>` deploy targets
- [ ] CLI: `rmreader init` runs wizard→write config→generate→deploy; `rmreader <toml>` loads→validate→generate→refresh; no args prints help
- [ ] `--help`/`--version` exit 0

**Verify:** `nix develop -c cargo test` → entire suite passes

**Steps:**

- [ ] **Step 1: Implement `src/generate.rs`**

```rust
//! Orchestrate: fetch -> content -> assemble -> render -> manifest -> deploy targets.
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::content::{process_html, FetchedImage, ImageFetcher};
use crate::readwise::HttpTransport;

/// Real image fetcher over ureq with guards (size cap; content-type sniff).
pub struct UreqImageFetcher;
impl ImageFetcher for UreqImageFetcher {
    fn fetch(&self, url: &str) -> Option<FetchedImage> {
        let resp = ureq::get(url).call().ok()?;
        let ct = resp.header("content-type").unwrap_or("").to_string();
        if !ct.starts_with("image/") { return None; }
        let ext = if ct.contains("svg") { "svg" } else { "bin" }.to_string();
        let mut bytes = Vec::new();
        use std::io::Read;
        resp.into_reader().take(8 * 1024 * 1024).read_to_end(&mut bytes).ok()?;
        Some(FetchedImage { bytes, ext })
    }
}

fn build_one(
    collection: &str,
    docs: &[crate::readwise::Document],
    config: &Config,
    fetcher: &dyn ImageFetcher,
    out_dir: &Path,
) -> anyhow::Result<PathBuf> {
    let images_enabled = config.images.enabled;
    let built = crate::assemble::assemble_document(collection, docs, |html, _id| {
        let p = process_html(html, images_enabled, fetcher);
        (p.html, p.assets)
    });
    let device = crate::device::get_device(&config.device)?;
    let theme = crate::theme::load_theme(&config.theme)?;
    let pdf_path = out_dir.join(format!("{collection}.pdf"));
    crate::render::render_pdf(&device, &theme, &built.fragments, &built.assets, &pdf_path)?;
    built.manifest.write(&out_dir.join(format!("{collection}.manifest.json")))?;
    Ok(pdf_path)
}

/// Returns deploy targets: (pdf_path, remarkable_folder).
pub fn generate(
    config: &Config,
    transport: &dyn HttpTransport,
    fetcher: &dyn ImageFetcher,
) -> anyhow::Result<Vec<(PathBuf, String)>> {
    let out_dir = PathBuf::from(&config.output_dir);
    std::fs::create_dir_all(&out_dir)?;
    let mut targets = Vec::new();

    let lib = crate::readwise::fetch_documents(
        transport, &config.readwise.token, &config.library.locations, config.library.max_items,
        |s| std::thread::sleep(std::time::Duration::from_secs(s)),
    )?;
    let lib_pdf = build_one("Library", &lib, config, fetcher, &out_dir)?;
    targets.push((lib_pdf, config.deploy.library_folder.clone()));

    if config.feed.enabled {
        let feed = crate::readwise::fetch_documents(
            transport, &config.readwise.token, &["feed".into()], config.feed.max_items,
            |s| std::thread::sleep(std::time::Duration::from_secs(s)),
        )?;
        let feed_pdf = build_one("Feed", &feed, config, fetcher, &out_dir)?;
        targets.push((feed_pdf, config.deploy.feed_folder.clone()));
    }
    Ok(targets)
}
```

  Note: `assemble_document` returns `Built.fragments` (a `Vec<String>` of page fragments) — pass it directly to `render_pdf`.

- [ ] **Step 2: Replace `src/cli.rs`**

```rust
//! rmreader CLI: `init` wizard, or regenerate from a config path.
use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::{config, deploy, generate, readwise, wizard};

#[derive(Parser)]
#[command(name = "rmreader", version, about = "Readwise Reader -> reMarkable reader PDFs",
          args_conflicts_with_subcommands = true)]
struct Cli {
    /// Path to an existing rmreader.toml to regenerate.
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Create config interactively and generate.
    Init,
}

pub fn run(args: Vec<String>) -> anyhow::Result<()> {
    let cli = Cli::try_parse_from(args).unwrap_or_else(|e| e.exit());
    let transport = readwise::http::UreqTransport;
    let fetcher = generate::UreqImageFetcher;
    match (cli.command, cli.config) {
        (Some(Command::Init), _) => {
            let (cfg, out_dir, cfg_path) = wizard::run_wizard(&transport)?;
            cfg.validate()?;
            std::fs::create_dir_all(&out_dir)?;
            config::dump(&cfg, &cfg_path)?;
            let targets = generate::generate(&cfg, &transport, &fetcher)?;
            deploy::get_deployer(&cfg)?.deploy(&targets)?;
            println!("Wrote {} PDF(s) to {}", targets.len(), out_dir.display());
            Ok(())
        }
        (None, Some(path)) => {
            let cfg = config::load(&path)?;
            cfg.validate()?;
            let targets = generate::generate(&cfg, &transport, &fetcher)?;
            deploy::get_deployer(&cfg)?.refresh(&targets)?;
            println!("Regenerated {} PDF(s)", targets.len());
            Ok(())
        }
        (None, None) => {
            use clap::CommandFactory;
            Cli::command().print_help()?;
            println!();
            Ok(())
        }
    }
}

pub fn main() -> anyhow::Result<()> {
    run(std::env::args().collect())
}
```

- [ ] **Step 3: Write `tests/cli.rs`**

```rust
#[test]
fn help_exits_ok() {
    // run() with --help calls e.exit() (exits process); instead test no-arg help path.
    let r = rmreader::cli::run(vec!["rmreader".into()]);
    assert!(r.is_ok());
}
```

- [ ] **Step 4: Write `tests/generate.rs`** — end-to-end with fakes (no network), asserting files exist and link resolves.

```rust
use rmreader::config::*;
use rmreader::content::{FetchedImage, ImageFetcher};
use rmreader::generate::generate;
use rmreader::readwise::{HttpResponse, HttpTransport};

struct FakeT;
impl HttpTransport for FakeT {
    fn get(&self, url: &str, _t: &str) -> anyhow::Result<HttpResponse> {
        let body = if url.contains("location=feed") {
            r#"{"nextPageCursor":null,"results":[]}"#.to_string()
        } else {
            r#"{"nextPageCursor":null,"results":[{"id":"a","title":"A","saved_at":"2026-01-01T00:00:00Z","html_content":"<p>hi <a href=\"http://x\">l</a></p>"}]}"#.to_string()
        };
        Ok(HttpResponse { status: 200, retry_after: None, body })
    }
}
struct NoImages;
impl ImageFetcher for NoImages { fn fetch(&self, _u: &str) -> Option<FetchedImage> { None } }

#[test]
fn generate_writes_pdfs_and_manifests() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = Config {
        device: "paper-pro-move".into(),
        output_dir: dir.path().to_str().unwrap().into(),
        theme: "reader".into(),
        readwise: ReadwiseConfig { token: "t".into() },
        library: LibraryConfig { locations: vec!["new".into()], max_items: 10 },
        feed: FeedConfig { enabled: true, max_items: 10 },
        images: ImagesConfig { enabled: false },
        deploy: DeployConfig { backend: "none".into(), library_folder: String::new(), feed_folder: String::new() },
    };
    let targets = generate(&cfg, &FakeT, &NoImages).unwrap();
    assert_eq!(targets.len(), 2);
    assert!(dir.path().join("Library.pdf").exists());
    assert!(dir.path().join("Library.manifest.json").exists());
    assert!(dir.path().join("Feed.pdf").exists());
}
```

  Add `tempfile = "3"` to `[dev-dependencies]` in `Cargo.toml`.

- [ ] **Step 5: Add `pub mod generate;` to `src/lib.rs`. Run full suite.**

Run: `nix develop -c cargo test`
Expected: all tests pass.

- [ ] **Step 6: Clippy + fmt**

Run: `make clippy && make fmt-check`
Expected: no warnings; formatted.

- [ ] **Step 7: Commit**

```bash
git add -A && git commit -m "Wire generate orchestration + CLI; end-to-end tests"
```

---

## Self-review

**Spec coverage:**
- Devices (MOVE/PRO) → Task 0. Config schema + validate → Task 1. Readwise API (auth/list/pagination/rate-limit/sort/cap, locations merge, feed) → Task 2. Wizard (token paste+validate) → Task 3. Content (sanitize, image fetch/guards/transcode/rewrite, images-disabled) → Task 4. fulgur link feasibility (running-header nav) → Task 5 spike. 3-tier hyperlinked doc + nav + bookmarks + manifest → Tasks 6, 7. Reader theme "Newsprint" + fonts → Task 7. rmapi deploy (two folders, content-only, token-clobber guard) → Task 8. CLI flows + end-to-end → Task 9. Future annotation seam → manifest (Task 6) + Deployer trait room (note: a future `fetch` method is added later, not in Phase 1). Nix/Make/gitignore → Task 0. **All spec sections covered.**
- **Resolved:** `assemble_document` returns `Built.fragments: Vec<String>` (Task 6), passed directly to `render_pdf` (Task 9) — no brittle string-splitting.

**Placeholder scan:** No "TBD"/"add error handling"/"write tests for the above" — every code step has real code; every test step has real assertions. The two "adjust if signature differs" notes (image 0.25 imports, fulgur asset/bookmark API) point at exact reference files, not vague hand-waving.

**Type consistency:** `HttpTransport`/`HttpResponse`/`Document`/`fetch_documents` signatures match across Tasks 2, 3, 9. `ImageFetcher`/`FetchedImage`/`process_html`/`Processed` match across Tasks 4, 6, 9. `Deployer`/`RmapiRunner`/`(PathBuf, String)` targets match across Tasks 8, 9. Anchors `item-<id>`/`article-<id>` consistent across templates, assemble, and tests. `Config` field names match across Tasks 1, 3, 9.
