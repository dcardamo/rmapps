# Sync Timing Instrumentation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers-extended-cc:subagent-driven-development (recommended) or superpowers-extended-cc:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Instrument the `rmapps sync` pipeline with `tracing` spans, gated by an on/off toggle, so a single run prints a per-stage timing breakdown that reveals whether wall-clock time goes to content building or cloud upload.

**Architecture:** Library crates (`rmreader`, `rmbujo`, `rmdigest`, `rm-cloud`) gain a `tracing` dependency and emit spans only. The `rmapps` binary owns the single global subscriber, installed via a new `timing` module and gated by a `--timings` flag / `RMAPPS_TIMINGS` env var (off by default). When off, no subscriber is installed and spans cost almost nothing.

**Tech Stack:** Rust, `tracing` (span/event macros), `tracing-subscriber` (`fmt` + `env-filter`, `FmtSpan::CLOSE` for per-span duration output), `clap` (global flag).

**User Verification:** YES — after instrumentation lands, a live `rmapps sync --timings` is run and Dan confirms the breakdown is produced and identifies which stage(s) dominate (this is the whole point of the effort: "then we do a sync, then address what's slow").

---

## File Structure

| File | Responsibility | Change |
|--------------------------------------------|------------------------------------------------------------|--------|
| `apps/rmapps/Cargo.toml`                   | Add `tracing` + `tracing-subscriber` deps                  | Modify |
| `crates/rmreader/Cargo.toml`               | Add `tracing` dep                                          | Modify |
| `crates/rmbujo/Cargo.toml`                 | Add `tracing` dep                                          | Modify |
| `crates/rmdigest/Cargo.toml`               | Add `tracing` dep                                          | Modify |
| `crates/rm-cloud/Cargo.toml`               | Add `tracing` dep                                          | Modify |
| `apps/rmapps/src/timing.rs`                | Toggle resolution, subscriber init, span helpers + tests   | Create |
| `apps/rmapps/src/main.rs`                  | `--timings` global flag; resolve toggle; init subscriber   | Modify |
| `apps/rmapps/src/sync.rs`                  | `sync.run` root span + per-`task{name}` span               | Modify |
| `apps/rmapps/src/reader.rs`                | `reader.*` build-vs-upload spans                            | Modify |
| `apps/rmapps/src/bujo.rs`                  | `bujo.*` build-vs-upload spans                             | Modify |
| `crates/rmreader/src/generate.rs`          | `reader.image_fetch`, `reader.typst_render` spans          | Modify |
| `crates/rmbujo/src/generate.rs`            | `bujo.ics_fetch`, `bujo.generate` spans                    | Modify |
| `crates/rmdigest/src/generate.rs`          | `digest.*` per-phase spans                                 | Modify |
| `crates/rm-cloud/src/client.rs`            | `cloud.commit`, `cloud.put_blob` spans                     | Modify |

---

### Task 0: Add `tracing` dependencies across the workspace

**Goal:** Every crate we will instrument depends on `tracing`, and the binary depends on `tracing-subscriber`, with the workspace still compiling and all existing tests green. No instrumentation yet.

**Files:**
- Modify: `apps/rmapps/Cargo.toml`
- Modify: `crates/rmreader/Cargo.toml`
- Modify: `crates/rmbujo/Cargo.toml`
- Modify: `crates/rmdigest/Cargo.toml`
- Modify: `crates/rm-cloud/Cargo.toml`

**Acceptance Criteria:**
- [ ] `tracing = "0.1"` is present in all five crates' `[dependencies]`.
- [ ] `tracing-subscriber = { version = "0.3", features = ["env-filter"] }` is present in `apps/rmapps`.
- [ ] `cargo build --workspace` succeeds.

**Verify:** `cargo build --workspace` → builds with no errors.

**Steps:**

- [ ] **Step 1: Add `tracing` to `apps/rmapps/Cargo.toml`**

In `apps/rmapps/Cargo.toml`, under `[dependencies]`, after the `chrono-tz = "0.10"` line, add:

```toml
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

(`fmt` and `registry` are default features of `tracing-subscriber`; only `env-filter` must be opted in.)

- [ ] **Step 2: Add `tracing` to the four library crates**

In each of `crates/rmreader/Cargo.toml`, `crates/rmbujo/Cargo.toml`, `crates/rmdigest/Cargo.toml`, and `crates/rm-cloud/Cargo.toml`, add this line at the end of the `[dependencies]` table:

```toml
tracing = "0.1"
```

- [ ] **Step 3: Build the workspace**

Run: `cargo build --workspace`
Expected: compiles successfully (new deps download + build; no code changes yet).

- [ ] **Step 4: Commit**

```bash
git add apps/rmapps/Cargo.toml crates/rmreader/Cargo.toml crates/rmbujo/Cargo.toml crates/rmdigest/Cargo.toml crates/rm-cloud/Cargo.toml Cargo.lock
git commit -m "build(rmapps): add tracing deps for sync timing instrumentation"
```

---

### Task 1: `timing` module — toggle, subscriber init, span helpers (with tests)

**Goal:** A self-contained `timing` module exposing: a pure `timings_enabled(flag, env)` resolver, an idempotent `init(enabled)` that installs a `FmtSpan::CLOSE` subscriber only when enabled, and `sync_span()` / `task_span(name)` constructors. Wire a global `--timings` flag and `RMAPPS_TIMINGS` env var into `main.rs` and call `init` before dispatch.

**Files:**
- Create: `apps/rmapps/src/timing.rs`
- Modify: `apps/rmapps/src/main.rs`
- Test: `apps/rmapps/src/timing.rs` (inline `#[cfg(test)]` module)

**Acceptance Criteria:**
- [ ] `timings_enabled` returns true when the flag is set, or when env is one of `1`/`true`/`yes` (case-insensitive, trimmed); false otherwise.
- [ ] Entering `sync_span()` under a capturing subscriber records a span named `sync.run`; `task_span("reader")` records a span named `task`.
- [ ] `rmapps --timings sync` and `RMAPPS_TIMINGS=1 rmapps sync` both enable timing output; neither set → no timing output.
- [ ] `cargo test -p rmapps timing::` passes.

**Verify:** `cargo test -p rmapps timing::` → all timing tests pass.

**Steps:**

- [ ] **Step 1: Write the failing tests**

Create `apps/rmapps/src/timing.rs` with the test module first:

```rust
//! Sync timing instrumentation: toggle resolution, subscriber install, and the
//! span constructors used across the sync pipeline. Off by default; enabled by
//! the `--timings` flag or `RMAPPS_TIMINGS=1`.

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tracing::span::Attributes;
    use tracing::{Id, Subscriber};
    use tracing_subscriber::layer::{Context, Layer};
    use tracing_subscriber::prelude::*;
    use tracing_subscriber::registry::LookupSpan;

    /// A minimal layer that records the name of every span created while it is
    /// the active subscriber — enough to prove our helpers emit the right spans.
    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<Vec<String>>>);

    impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for Capture {
        fn on_new_span(&self, attrs: &Attributes<'_>, _id: &Id, _ctx: Context<'_, S>) {
            self.0.lock().unwrap().push(attrs.metadata().name().to_string());
        }
    }

    #[test]
    fn toggle_flag_wins() {
        assert!(timings_enabled(true, None));
        assert!(timings_enabled(true, Some("0")));
    }

    #[test]
    fn toggle_env_truthy() {
        assert!(timings_enabled(false, Some("1")));
        assert!(timings_enabled(false, Some("true")));
        assert!(timings_enabled(false, Some("YES")));
        assert!(timings_enabled(false, Some("  true  ")));
    }

    #[test]
    fn toggle_off_by_default() {
        assert!(!timings_enabled(false, None));
        assert!(!timings_enabled(false, Some("0")));
        assert!(!timings_enabled(false, Some("nope")));
    }

    #[test]
    fn helpers_emit_named_spans() {
        let cap = Capture::default();
        let names = cap.0.clone();
        let subscriber = tracing_subscriber::registry().with(cap);
        tracing::subscriber::with_default(subscriber, || {
            let _s = sync_span().entered();
            let _t = task_span("reader").entered();
        });
        let names = names.lock().unwrap();
        assert!(names.iter().any(|n| n == "sync.run"), "got {names:?}");
        assert!(names.iter().any(|n| n == "task"), "got {names:?}");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p rmapps timing::`
Expected: FAIL — `timings_enabled`, `sync_span`, `task_span` are not yet defined.

- [ ] **Step 3: Write the implementation**

At the **top** of `apps/rmapps/src/timing.rs` (above the test module), add:

```rust
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::EnvFilter;

/// Resolve whether timing output is enabled. The `--timings` flag wins; failing
/// that, a truthy `RMAPPS_TIMINGS` value (`1`/`true`/`yes`, case-insensitive)
/// enables it. Off otherwise. Kept pure (args injected) so it is unit-testable.
pub fn timings_enabled(flag: bool, env: Option<&str>) -> bool {
    if flag {
        return true;
    }
    matches!(
        env.map(|s| s.trim().to_ascii_lowercase()).as_deref(),
        Some("1") | Some("true") | Some("yes")
    )
}

/// Install the timing subscriber when enabled. Uses an stderr `fmt` layer that
/// logs each span's busy/idle duration on close (`FmtSpan::CLOSE`), so a run
/// prints a per-stage breakdown. Honors `RUST_LOG` if set, else defaults to
/// `info` (the level our spans use). No-op when disabled, leaving spans
/// effectively free. Safe to call once; a second install is ignored.
pub fn init(enabled: bool) {
    if !enabled {
        return;
    }
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_span_events(FmtSpan::CLOSE)
        .with_writer(std::io::stderr)
        .with_target(false)
        .try_init();
}

/// Root span for one sync invocation.
pub fn sync_span() -> tracing::Span {
    tracing::info_span!("sync.run")
}

/// Per-task span; `name` is the app key (`bujo`/`reader`/`digest`).
pub fn task_span(name: &str) -> tracing::Span {
    tracing::info_span!("task", name = name)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p rmapps timing::`
Expected: PASS (4 tests).

- [ ] **Step 5: Wire the flag + env + module into `main.rs`**

In `apps/rmapps/src/main.rs`:

Add the module declaration alongside the others (after `mod sync;`):

```rust
mod timing;
```

Add the global flag to the `Cli` struct, right after the existing `config` field (before `#[command(subcommand)]`):

```rust
    /// Print per-stage timing for the run (also enabled by `RMAPPS_TIMINGS=1`).
    #[arg(long, global = true)]
    timings: bool,
```

In `fn main`, immediately after `let cli = Cli::parse();`, resolve the toggle and init the subscriber before any dispatch:

```rust
    let timings = timing::timings_enabled(cli.timings, std::env::var("RMAPPS_TIMINGS").ok().as_deref());
    timing::init(timings);
```

- [ ] **Step 6: Build and run the full crate test suite**

Run: `cargo test -p rmapps`
Expected: PASS (existing tests + new timing tests). Confirm `cargo build -p rmapps` shows no unused-import / dead-code warnings for the new module.

- [ ] **Step 7: Commit**

```bash
git add apps/rmapps/src/timing.rs apps/rmapps/src/main.rs
git commit -m "feat(rmapps): timing module with --timings/RMAPPS_TIMINGS toggle"
```

---

### Task 2: Orchestrator + app-wrapper spans (sync / reader / bujo)

**Goal:** `sync.run` wraps the whole sync; each due task runs inside a `task{name=...}` span; and the `reader` and `bujo` one-shot wrappers split their build phase from their upload phase, so the build-vs-upload question is answered directly.

**Files:**
- Modify: `apps/rmapps/src/sync.rs:102` (`run`) and `:198` (`run_task`)
- Modify: `apps/rmapps/src/reader.rs:45-78`
- Modify: `apps/rmapps/src/bujo.rs:129-159`

**Acceptance Criteria:**
- [ ] `sync::run` enters `timing::sync_span()` for the duration of the task loop.
- [ ] `run_task` enters `timing::task_span(&task.app)` around the per-app dispatch.
- [ ] `reader::run` wraps the `generate(...)` call in a `reader.generate` span and the upload loop in a `reader.upload` span.
- [ ] `bujo::run` wraps `generate_year(...)` in a `bujo.generate_year` span and each upload loop in a `bujo.upload` span.
- [ ] `cargo test -p rmapps` passes; `cargo build --workspace` is clean.

**Verify:** `cargo build --workspace && cargo test -p rmapps` → builds clean, tests pass.

**Steps:**

- [ ] **Step 1: Wrap the sync loop in `sync.run`**

In `apps/rmapps/src/sync.rs`, inside `pub fn run`, after `let mut state = load_state();` (line 108), enter the root span so it covers the whole loop:

```rust
    let _sync_span = crate::timing::sync_span().entered();
```

- [ ] **Step 2: Wrap per-task dispatch in a `task` span**

In `apps/rmapps/src/sync.rs`, in `run_task`, make the span cover the dispatch. Change the body so the first line is:

```rust
pub(crate) fn run_task(task: &crate::config::SyncTask, key: &str, cfg: &Config) -> Result<()> {
    let _task_span = crate::timing::task_span(&task.app).entered();
    match task.app.as_str() {
```

(Everything else in `run_task` is unchanged.)

- [ ] **Step 3: Split build vs upload in `reader::run`**

In `apps/rmapps/src/reader.rs`, in the `if upload { ... }` branch, replace the generate+deploy section (currently lines 70-74) with spanned phases:

```rust
        let targets = {
            let _s = tracing::info_span!("reader.generate").entered();
            rmreader::generate::generate(&reader, &transport, &fetcher)?
        };
        {
            let _s = tracing::info_span!("reader.upload", docs = targets.len()).entered();
            for (pdf, folder) in &targets {
                cl.replace(folder, &cloud::doc_name(pdf)?, std::fs::read(pdf)?)?;
            }
        }
        println!("Deployed {} reader PDF(s)", targets.len());
```

In the `else` branch (generate-only, line 76), wrap the generate call too:

```rust
        let targets = {
            let _s = tracing::info_span!("reader.generate").entered();
            rmreader::generate::generate(&reader, &transport, &fetcher)?
        };
        println!("Generated {} reader PDF(s) (upload skipped)", targets.len());
```

- [ ] **Step 4: Split build vs upload in `bujo::run`**

In `apps/rmapps/src/bujo.rs`, wrap the whole-year generate call (line 112):

```rust
    let mut paths = {
        let _s = tracing::info_span!("bujo.generate_year").entered();
        rmbujo::generate::generate_year(bujo, &out_dir, args.refresh_feeds)?
    };
```

Then wrap each upload loop in a `bujo.upload` span. For the `only_month` branch, surround the two loops (lines 143-148):

```rust
        {
            let _s = tracing::info_span!("bujo.upload", mode = "only_month").entered();
            for pdf in &month_pdfs {
                cl.upsert(&target, &cloud::doc_name(pdf)?, std::fs::read(pdf)?)?;
            }
            for pdf in &extras {
                cl.create_if_missing(&target, &cloud::doc_name(pdf)?, std::fs::read(pdf)?)?;
            }
        }
```

For the default branch (lines 155-157):

```rust
        {
            let _s = tracing::info_span!("bujo.upload", docs = paths.len()).entered();
            for pdf in &paths {
                cl.upsert(&target, &cloud::doc_name(pdf)?, std::fs::read(pdf)?)?;
            }
        }
```

(The single-`--month` early-return path at lines 95-108 is not exercised by sync — leave it unspanned to keep the change focused.)

- [ ] **Step 5: Build and test**

Run: `cargo build --workspace && cargo test -p rmapps`
Expected: clean build, tests pass.

- [ ] **Step 6: Commit**

```bash
git add apps/rmapps/src/sync.rs apps/rmapps/src/reader.rs apps/rmapps/src/bujo.rs
git commit -m "feat(rmapps): sync.run/task spans + reader/bujo build-vs-upload spans"
```

---

### Task 3: Deep library spans (rmreader / rmbujo / rmdigest)

**Goal:** Sub-phase spans inside the library builders so we see, within a slow task, exactly which stage dominates: Reader image fetch vs Typst render; Bujo ICS fetch vs PDF generation; and the Digest per-doc phases (bundle fetch, extract, render, upload).

**Files:**
- Modify: `crates/rmreader/src/generate.rs:177` and `:217`
- Modify: `crates/rmbujo/src/generate.rs:19` and `:22-41`
- Modify: `crates/rmdigest/src/generate.rs:95`, `:114`, `:120-121`, `:136`

**Acceptance Criteria:**
- [ ] Reader: the image-fetch region and the Typst-render region are each wrapped in `reader.image_fetch` / `reader.typst_render` spans carrying the collection name.
- [ ] Bujo: `build_event_map` is wrapped in `bujo.ics_fetch`; the future-log + month loop + collection + reference generation is wrapped in `bujo.generate`.
- [ ] Digest: `backend.fetch` → `digest.bundle_fetch`; `extract` → `digest.extract`; `build_linked` + `compile` → `digest.render`; `backend.put` → `digest.upload`. Each carries the doc path.
- [ ] Existing `eprintln!` timing lines are left intact (additive change).
- [ ] `cargo build --workspace && cargo test --workspace` passes.

**Verify:** `cargo build --workspace && cargo test --workspace` → builds clean, all tests pass.

**Steps:**

- [ ] **Step 1: Reader image-fetch + render spans**

In `crates/rmreader/src/generate.rs`, wrap the image-fetch region. Replace the existing `let t = Instant::now();` / `fetcher.fetch_many` block (lines 177-188) so the fetch happens inside a span:

```rust
    let t = Instant::now();
    let _img_span = tracing::info_span!("reader.image_fetch", collection, urls = union.len()).entered();
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
    drop(_img_span);
```

Wrap the Typst render. Replace the `let t = Instant::now();` + `render_collection(...)` block (lines 217-235) so the render runs inside a span:

```rust
    let t = Instant::now();
    let _render_span = tracing::info_span!("reader.typst_render", collection, articles = built.typst_articles.len()).entered();
    // Typst references images at /assets/{key}; serve them there.
    let assets: Vec<(String, Vec<u8>)> = built
        .assets
        .iter()
        .map(|(k, b)| (format!("/assets/{k}"), b.clone()))
        .collect();
    let rendered = crate::render::render_collection(
        &device,
        &theme,
        collection,
        &built.typst_rows,
        &built.typst_articles,
        &assets,
    )?;
    eprintln!(
        "[rmreader] {collection}: rendered in {:.1}s",
        t.elapsed().as_secs_f32()
    );
    drop(_render_span);
```

(`drop(_img_span)` / `drop(_render_span)` close the span before the next phase so durations don't bleed together. `collection` is a `&str` in scope here and is recorded as a field via field-init shorthand.)

- [ ] **Step 2: Bujo ICS + generate spans**

In `crates/rmbujo/src/generate.rs`, wrap the ICS fetch (line 19-20):

```rust
    // Build the per-day event map once; every month reads from it.
    let events = {
        let _s = tracing::info_span!("bujo.ics_fetch").entered();
        crate::ics::build_event_map(config, out_dir, refresh, &crate::ics::fetch::UreqFetcher)?
    };
```

Wrap the PDF generation (future log through reference, lines 22-41) in one span by entering it just before building the future log and dropping it before `Ok(paths)`:

```rust
    let _gen_span = tracing::info_span!("bujo.generate").entered();

    let fl = out_dir.join(format!("{y} Future Log.pdf"));
    future_log::build_future_log_pdf(config, &fl)?;
    paths.push(fl);

    for mo in 1..=12u32 {
        let p = out_dir.join(format!(
            "{y}.{mo:02} {name}.pdf",
            name = MONTH_NAMES[mo as usize]
        ));
        month::build_month_pdf(config, mo, &events, &p)?;
        paths.push(p);
    }

    let col = out_dir.join(format!("{y} Collection Template.pdf"));
    collection::build_collection_pdf(config, &col)?;
    paths.push(col);

    let r = out_dir.join(format!("{y} Reference.pdf"));
    reference::build_reference_pdf(config, &r)?;
    paths.push(r);

    drop(_gen_span);
    Ok(paths)
```

- [ ] **Step 3: Digest per-phase spans**

In `crates/rmdigest/src/generate.rs`, in `process_doc`, wrap each phase. Bundle fetch (line 95):

```rust
    let bundle_path = match {
        let _s = tracing::info_span!("digest.bundle_fetch", doc = %doc.path).entered();
        backend.fetch(doc)?
    } {
        Some(p) => p,
        None => {
            eprintln!("rmdigest: fetch returned None for {}, skipping", doc.path);
            return Ok(());
        }
    };
```

Extract (line 114):

```rust
    let marks = {
        let _s = tracing::info_span!("digest.extract", doc = %doc.path).entered();
        extract(&ing.bundle, &all_pages)?
    };
```

Render — the `build_linked` + `compile` pair (lines 120-121):

```rust
    let (digest_pdf, _src_assets) = {
        let _s = tracing::info_span!("digest.render", doc = %doc.path).entered();
        let (src, assets) = crate::linked_doc::build_linked(&meta, &marks, &ing.bundle, &device)?;
        let pdf = compile(&src, &assets)?;
        (pdf, ())
    };
```

(The original binds `(src, assets)` then `digest_pdf`; the rewrite keeps `digest_pdf` in scope for the existing `std::fs::write(&digest_file, &digest_pdf)` call at line 135. `_src_assets` discards the now-unneeded tuple.)

Upload (line 136):

```rust
    {
        let _s = tracing::info_span!("digest.upload", doc = %doc.path).entered();
        backend.put(&digest_file, &doc.folder, &digest_name)?;
    }
```

- [ ] **Step 4: Build and test the workspace**

Run: `cargo build --workspace && cargo test --workspace`
Expected: clean build, all crate tests pass. Watch for the `digest.render` rewrite — confirm `digest_pdf` is still in scope at the `std::fs::write` call.

- [ ] **Step 5: Commit**

```bash
git add crates/rmreader/src/generate.rs crates/rmbujo/src/generate.rs crates/rmdigest/src/generate.rs
git commit -m "feat(rm*): deep timing spans for reader/bujo/digest build phases"
```

---

### Task 4: `rm-cloud` upload spans (separate build time from governed upload)

**Goal:** Span the actual blob-upload path so the timing breakdown isolates the rate-governed network upload (150ms min interval, 4-way cap) from content building — directly testing the user's "uploading should take seconds" hypothesis.

**Files:**
- Modify: `crates/rm-cloud/src/client.rs:285` (`put_blob`) and `:316` (`commit_with`)

**Acceptance Criteria:**
- [ ] The `pub(crate) async fn put_blob` is annotated so each blob upload emits a `cloud.put_blob` span.
- [ ] `async fn commit_with` is annotated so the whole commit (blob loop + CAS root put) emits a `cloud.commit` span.
- [ ] Annotations use `skip_all` so large byte buffers are never Debug-formatted.
- [ ] `cargo test --workspace` passes (including the `rm-cloud` fake-feature suite where applicable).

**Verify:** `cargo build --workspace && cargo test -p rm-cloud` → builds clean, tests pass.

**Steps:**

- [ ] **Step 1: Annotate `put_blob`**

In `crates/rm-cloud/src/client.rs`, add an attribute directly above `pub(crate) async fn put_blob` (line 285). The `#[instrument]` macro handles async fns natively; `skip_all` avoids formatting `bytes`, and we record the small `name` field explicitly:

```rust
    /// PUT a blob under `hash` with the given logical filename.
    #[tracing::instrument(name = "cloud.put_blob", skip_all, fields(name = %name))]
    pub(crate) async fn put_blob(&self, hash: &str, name: &str, bytes: Vec<u8>) -> Result<()> {
```

- [ ] **Step 2: Annotate `commit_with`**

In `crates/rm-cloud/src/client.rs`, add an attribute directly above `async fn commit_with` (line 316):

```rust
    #[tracing::instrument(name = "cloud.commit", skip_all)]
    async fn commit_with(&self, mutation: Mutation, broadcast: bool) -> Result<Snapshot> {
```

- [ ] **Step 3: Build and test**

Run: `cargo build --workspace && cargo test -p rm-cloud`
Expected: clean build, tests pass. (Instrumentation is additive; behavior is unchanged, so the existing fake-cloud suite remains the regression guard.)

- [ ] **Step 4: Commit**

```bash
git add crates/rm-cloud/src/client.rs
git commit -m "feat(rm-cloud): cloud.commit/put_blob spans to isolate upload time"
```

---

### Task 5: Live measurement + bottleneck confirmation (user verification)

**Goal:** Run a real `rmapps sync --timings`, capture the per-stage breakdown, and confirm with Dan which stage(s) dominate — the input to the separate optimization effort.

**Files:**
- None (operational run; no code changes).

**Acceptance Criteria:**
- [ ] A live `rmapps sync --timings` run completes and prints `close time.busy=...` lines for `sync.run`, the `task{...}` spans, and the sub-phase spans.
- [ ] The breakdown is captured and presented to Dan, with the dominant stage(s) called out.
- [ ] Dan confirms the breakdown is sufficient to choose what to optimize next.

**Verify:** `RMAPPS_TIMINGS=1 cargo run -p rmapps -- sync` (or the release binary) → emits the span-close timing breakdown on stderr.

**Steps:**

- [ ] **Step 1: Build the release binary**

Run: `cargo build --release -p rmapps`
Expected: builds the optimized binary (timing measurements should reflect release performance, not debug).

- [ ] **Step 2: Free the cloud lock if held**

The saturn `rmapps-watch` daemon holds the single-instance cloud lock while running a task. If a sync errors with "another rmapps cloud op in progress", stop the daemon first (per `CLAUDE.md`):

```bash
export PATH=/run/wrappers/bin:$PATH
sudo systemctl stop rmapps-watch
```

- [ ] **Step 3: Run a timed sync and capture the breakdown**

Run (capturing stderr where the timing lines go):

```bash
./target/release/rmapps sync --timings 2>&1 | tee /tmp/rmapps-sync-timings.log
```

Expected: normal sync output interleaved with span-close lines such as:

```
reader.typst_render close time.busy=18.1s
reader.image_fetch  close time.busy=12.4s
task{name=reader}   close time.busy=42.1s
sync.run            close time.busy=48.6s
```

- [ ] **Step 4: Restart the daemon (if it was stopped)**

```bash
export PATH=/run/wrappers/bin:$PATH
sudo systemctl start rmapps-watch
```

- [ ] **Step 5: Summarize the dominant stage(s)**

From `/tmp/rmapps-sync-timings.log`, identify the largest `time.busy` contributors and which task they belong to (build vs upload).

- [ ] **Step 6: User verification**

**User Verification Required:**
Before marking this task complete, you MUST call AskUserQuestion:

```yaml
AskUserQuestion:
  question: "Sync timing breakdown captured. The dominant stage(s) are <fill in from the run>. Is this enough to decide what to optimize next?"
  header: "Verification"
  options:
    - label: "Yes — proceed to optimize"
      description: "The breakdown clearly identifies the bottleneck; open the optimization effort against it."
    - label: "No — need finer detail"
      description: "The breakdown is too coarse or a stage is missing; add/adjust spans and re-measure."
```

**If Dan selects the negative option:** the task is NOT complete — add or refine spans (extend Task 3/4), rebuild, re-run the timed sync, and re-verify with AskUserQuestion.

---

## Notes / known limitations

- **Cross-thread spans (Reader):** `rmreader::generate::generate` builds Library and Feed on separate `std::thread::scope` threads. With a global `Registry` subscriber, spans created on those threads are still recorded and their close-timings still print; only the parent/child nesting to `task{name=reader}` may not link across the thread boundary. That is fine for measurement — each stage still reports its own duration.
- **Scope of this plan is measurement only.** No parallelization, governor tuning, or caching changes. Those belong to the follow-up optimization effort the spec describes, chosen based on Task 5's output.
