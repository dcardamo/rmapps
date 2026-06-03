# Digest: skip non-pdf/epub documents — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers-extended-cc:subagent-driven-development (recommended) or superpowers-extended-cc:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `digest` process only `pdf` and `epub` source documents, skipping native reMarkable notebooks (e.g. *Notes-Getting to Zero*) without re-fetching them on every run.

**Architecture:** Expose the `.content` `fileType` field through `rmfiles::Bundle`, then gate `rmdigest::generate::process_doc` on a fixed allow-list `["pdf","epub"]`. A `skipped` sentinel in per-doc state lets later runs cheap-skip an unsupported doc with no fetch.

**Tech Stack:** Rust, serde, the existing `rmfiles` bundle parser and `rmdigest` pipeline.

**User Verification:** NO — no user verification required (pure code change, covered by automated tests).

**Spec:** `docs/superpowers/specs/2026-06-03-digest-skip-notebooks-design.md`

---

## File Structure

- `crates/rmfiles/src/bundle/content.rs` — add `file_type` to the `Content` deserializer.
- `crates/rmfiles/src/bundle/mod.rs` — store `file_type` on `Bundle`, add `file_type()` accessor.
- `crates/rmdigest/src/state.rs` — add the `skipped` sentinel to `DocState`.
- `crates/rmdigest/src/generate.rs` — allow-list gate + early skip-sentinel cheap-skip + tests.

---

### Task 1: Expose `fileType` through `rmfiles::Bundle`

**Goal:** `Bundle::file_type()` returns the `.content` `fileType` string (`"pdf"`, `"epub"`, `"notebook"`, or `""` when absent).

**Files:**
- Modify: `crates/rmfiles/src/bundle/content.rs`
- Modify: `crates/rmfiles/src/bundle/mod.rs`

**Acceptance Criteria:**
- [ ] `Content` deserializes a `fileType` field (default `""`).
- [ ] `Bundle::file_type()` returns the parsed value.
- [ ] Missing `fileType` yields `""`.

**Verify:** `cargo test -p rmfiles` → all pass

**Steps:**

- [ ] **Step 1: Add the `file_type` field to `Content`**

In `crates/rmfiles/src/bundle/content.rs`, add this field to the `Content` struct (place it right after the `pages` field):

```rust
    /// Source document kind: `"pdf"`, `"epub"`, `"notebook"`, or `""` (absent).
    #[serde(default, rename = "fileType")]
    pub file_type: String,
```

- [ ] **Step 2: Add a failing test for the accessor**

In `crates/rmfiles/src/bundle/mod.rs`, find the `#[cfg(test)] mod tests` block (or add one at the end of the file if none exists) and add:

```rust
    #[test]
    fn file_type_read_from_content() {
        use std::fs;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let uuid = "doc";
        fs::write(
            root.join(format!("{uuid}.content")),
            r#"{"fileType":"notebook","cPages":{"pages":[{"id":"p1"}]}}"#,
        )
        .unwrap();
        fs::write(
            root.join(format!("{uuid}.metadata")),
            r#"{"visibleName":"N","type":"DocumentType"}"#,
        )
        .unwrap();
        fs::create_dir_all(root.join(uuid)).unwrap();
        fs::write(root.join(uuid).join("p1.rm"), b"x").unwrap();

        let b = Bundle::open(root).unwrap();
        assert_eq!(b.file_type(), "notebook");
    }

    #[test]
    fn file_type_defaults_empty_when_absent() {
        use std::fs;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let uuid = "doc";
        fs::write(
            root.join(format!("{uuid}.content")),
            r#"{"cPages":{"pages":[{"id":"p1"}]}}"#,
        )
        .unwrap();
        fs::write(
            root.join(format!("{uuid}.metadata")),
            r#"{"visibleName":"N","type":"DocumentType"}"#,
        )
        .unwrap();
        fs::create_dir_all(root.join(uuid)).unwrap();
        fs::write(root.join(uuid).join("p1.rm"), b"x").unwrap();

        let b = Bundle::open(root).unwrap();
        assert_eq!(b.file_type(), "");
    }
```

If the test module doesn't already `use super::*;`, ensure it does. `tempfile` is already a dev-dependency in this crate (the bundle tests use it elsewhere); if `cargo test` reports it missing, add `tempfile` under `[dev-dependencies]` in `crates/rmfiles/Cargo.toml`.

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p rmfiles file_type -- --nocapture`
Expected: compile error — no method `file_type` on `Bundle`.

- [ ] **Step 4: Store `file_type` on `Bundle` and add the accessor**

In `crates/rmfiles/src/bundle/mod.rs`, add a field to the `Bundle` struct (next to `canvas`):

```rust
    /// Source document kind from `.content` `fileType` (`""` when absent).
    file_type: String,
```

In `Bundle::open`, after the `let canvas = (...)` binding and before `Ok(Bundle { ... })`, capture the value, then add it to the struct literal:

```rust
        let file_type = content.file_type.clone();

        Ok(Bundle {
            files,
            uuid,
            meta,
            page_ids,
            source_pages,
            canvas,
            file_type,
        })
```

Add the accessor in the `impl Bundle` block, next to `canvas_size`:

```rust
    /// Source document kind: `"pdf"`, `"epub"`, `"notebook"`, or `""` (absent).
    pub fn file_type(&self) -> &str {
        &self.file_type
    }
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p rmfiles`
Expected: PASS (including the two new tests and all existing bundle tests).

- [ ] **Step 6: Commit**

```bash
git add crates/rmfiles/src/bundle/content.rs crates/rmfiles/src/bundle/mod.rs
git commit -m "feat(rmfiles): expose .content fileType via Bundle::file_type"
```

---

### Task 2: Gate digest on the pdf/epub allow-list with a skip sentinel

**Goal:** `process_doc` skips any source whose `file_type` is not `pdf`/`epub`, records a `skipped` sentinel so later runs avoid re-fetching, and re-engages if the doc later becomes a supported kind.

**Files:**
- Modify: `crates/rmdigest/src/state.rs`
- Modify: `crates/rmdigest/src/generate.rs`

**Acceptance Criteria:**
- [ ] `DocState` has a `skipped: bool` field (`#[serde(default)]`, backward-compatible).
- [ ] A notebook bundle produces **0 puts** and saves `skipped == true`, `page_hashes` empty.
- [ ] A second run over the same unchanged notebook performs **no second fetch**.
- [ ] Existing pdf-fixture tests still upload exactly one digest.

**Verify:** `cargo test -p rmdigest` → all pass

**Steps:**

- [ ] **Step 1: Add the `skipped` field to `DocState`**

In `crates/rmdigest/src/state.rs`, add to the `DocState` struct (after `digest_uuids`):

```rust
    /// True if this source was skipped as an unsupported kind (e.g. a native
    /// notebook). Lets a later run cheap-skip without re-fetching the bundle.
    #[serde(default)]
    pub skipped: bool,
```

Any test in this repo that constructs `DocState { ... }` with explicit fields will now fail to compile (missing field). Update those literals to add `skipped: false`. Known sites to fix: `crates/rmdigest/src/ingest.rs` (three `DocState { ... }` literals in its tests), `crates/rmdigest/src/generate.rs` (the `backfills_cloud_version...` test and the `skip_when_unchanged` test), and the `save_and_load_round_trips` test in `state.rs` itself. After editing, `cargo build -p rmdigest --tests` will name any remaining ones.

- [ ] **Step 2: Add the allow-list constant and failing tests**

In `crates/rmdigest/src/generate.rs`, add near the top (after the `use` lines):

```rust
/// Source document kinds the digest pipeline processes. Anything else
/// (notebooks, empty/unknown `fileType`) is skipped.
const SUPPORTED_FILE_TYPES: [&str; 2] = ["pdf", "epub"];
```

Add these tests inside the existing `mod cheap_skip_tests` block (it already has `CountingBackend`, `test_cfg`, `test_opts`, `test_doc`, and imports `ingest`, `State`, `Ordering`, `Arc`, `AtomicU32`). The notebook fixture is built inline as a temp dir so no new fixture file is needed:

```rust
    /// Build a minimal notebook bundle dir (fileType "notebook", one page, no
    /// source PDF) and return its path. The tempdir guard must outlive the run.
    fn notebook_bundle(dir: &std::path::Path) -> PathBuf {
        use std::fs;
        let uuid = "nb";
        fs::write(
            dir.join(format!("{uuid}.content")),
            r#"{"fileType":"notebook","cPages":{"pages":[{"id":"p1"}]},"customZoomPageWidth":1404,"customZoomPageHeight":1872}"#,
        )
        .unwrap();
        fs::write(
            dir.join(format!("{uuid}.metadata")),
            r#"{"visibleName":"Notes-Getting to Zero","type":"DocumentType"}"#,
        )
        .unwrap();
        fs::create_dir_all(dir.join(uuid)).unwrap();
        fs::write(dir.join(uuid).join("p1.rm"), b"ink-bytes").unwrap();
        dir.to_path_buf()
    }

    #[test]
    fn notebook_is_skipped_and_not_refetched() {
        let bundle_dir = tempfile::tempdir().expect("bundle tempdir");
        let bundle = notebook_bundle(bundle_dir.path());

        let fetches = Arc::new(AtomicU32::new(0));
        let backend = CountingBackend {
            bundle,
            fetches: fetches.clone(),
        };
        let cfg = test_cfg();
        let state_dir = tempfile::tempdir().expect("state tempdir");
        let state_path = state_dir.path().join("state.json");
        let opts = test_opts();
        let doc = test_doc(Some("nb-hash-1"));

        // First run: fetches once, detects notebook, records skip sentinel.
        run_one(&cfg, &backend, &state_path, &opts, &doc).expect("first run");
        assert_eq!(fetches.load(Ordering::Relaxed), 1, "notebook fetched once to inspect");

        let state = State::load(&state_path).expect("load state");
        let ds = state.docs.get(&doc.path).expect("doc state present");
        assert!(ds.skipped, "notebook must be marked skipped");
        assert!(ds.page_hashes.is_empty(), "skipped notebook stores no page hashes");

        // Second run: same version + skipped → no fetch.
        run_one(&cfg, &backend, &state_path, &opts, &doc).expect("second run");
        assert_eq!(
            fetches.load(Ordering::Relaxed),
            1,
            "unchanged skipped notebook must not be re-fetched"
        );
    }
```

Note: `CountingBackend::put` already records nothing observable, and its `list` returns empty, so "0 puts" is asserted via the `run` integration test below. Add this to the `mod tests` block (the one with `FakeBackend`/`PutLog`), which can observe puts:

```rust
    #[test]
    fn notebook_produces_no_digest_put() {
        use std::fs;
        let bundle_dir = tempfile::tempdir().expect("bundle tempdir");
        let root = bundle_dir.path();
        let uuid = "nb";
        fs::write(
            root.join(format!("{uuid}.content")),
            r#"{"fileType":"notebook","cPages":{"pages":[{"id":"p1"}]},"customZoomPageWidth":1404,"customZoomPageHeight":1872}"#,
        )
        .unwrap();
        fs::write(
            root.join(format!("{uuid}.metadata")),
            r#"{"visibleName":"Notes-Getting to Zero","type":"DocumentType"}"#,
        )
        .unwrap();
        fs::create_dir_all(root.join(uuid)).unwrap();
        fs::write(root.join(uuid).join("p1.rm"), b"ink").unwrap();

        let puts = Arc::new(Mutex::new(Vec::new()));
        let doc = CloudDoc {
            path: "/Books/Notes-Getting to Zero".to_string(),
            name: "Notes-Getting to Zero".to_string(),
            folder: "/Books".to_string(),
            version: None,
        };
        let backend = FakeBackend {
            fixture: doc,
            fixture_path: root.to_path_buf(),
            puts: puts.clone(),
        };
        let cfg = fake_cfg();
        let state_dir = tempfile::tempdir().expect("state tempdir");
        let state_path = state_dir.path().join("state.json");
        let opts = Opts { dry_run: false, local_output: None };

        run(&cfg, &backend, &state_path, &opts).expect("run over notebook");
        assert_eq!(puts.lock().unwrap().len(), 0, "notebook must produce no digest put");
    }
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p rmdigest notebook -- --nocapture`
Expected: FAIL — the notebook is currently processed: `notebook_produces_no_digest_put` sees 1 put, and `notebook_is_skipped_and_not_refetched` finds `skipped == false` / a second fetch.

- [ ] **Step 4: Add the early skip-sentinel cheap-skip in `process_doc`**

In `crates/rmdigest/src/generate.rs`, in `process_doc`, immediately AFTER the existing cheap-skip block (the one ending `return Ok(());` for `prev.cloud_version == doc.version && !prev.page_hashes.is_empty()`) and BEFORE the `let fetched = { ... }` block, insert:

```rust
    // Skip-sentinel cheap-skip: an unsupported kind (e.g. notebook) recorded on a
    // prior run. Same cloud version → nothing to reconsider, avoid the fetch.
    if doc.version.is_some() && prev.skipped && prev.cloud_version == doc.version {
        eprintln!("rmdigest: {} unsupported kind (skip sentinel, no fetch), skipping", doc.path);
        return Ok(());
    }
```

- [ ] **Step 5: Add the allow-list gate after ingest**

Still in `process_doc`, immediately AFTER `let ing = ingest(&bundle_path, prev)?;` and BEFORE the "Skip if nothing changed" block, insert:

```rust
    // Allow-list gate: only real source documents get digested. A native
    // notebook (or any unknown/empty kind) is recorded as skipped — with the
    // cloud version so the skip-sentinel cheap-skip engages next run — and
    // produces no digest.
    let kind = ing.bundle.file_type();
    if !SUPPORTED_FILE_TYPES.contains(&kind) {
        prev.cloud_version = doc.version.clone();
        prev.skipped = true;
        prev.page_hashes.clear();
        state.save(state_path)?;
        eprintln!("rmdigest: {} is '{}', not pdf/epub — skipping", doc.path, kind);
        return Ok(());
    }
```

- [ ] **Step 6: Clear the sentinel on the successful processing path**

Still in `process_doc`, in the final persist block after a successful upload, add the `skipped = false` reset. Change:

```rust
    // Persist state only after the upload succeeds, so a crash re-processes.
    prev.cloud_version = doc.version.clone();
    prev.page_hashes = ing.new_hashes;
    state.save(state_path)?;
```

to:

```rust
    // Persist state only after the upload succeeds, so a crash re-processes.
    prev.cloud_version = doc.version.clone();
    prev.page_hashes = ing.new_hashes;
    prev.skipped = false; // a previously-skipped doc that is now a supported kind re-engages
    state.save(state_path)?;
```

- [ ] **Step 7: Run the full crate test suite**

Run: `cargo test -p rmdigest`
Expected: PASS — new notebook tests pass; existing `integration_two_puts_then_skip`, `dry_run_does_not_poison_state`, cheap-skip tests, and ingest tests all still pass.

- [ ] **Step 8: Workspace check**

Run: `cargo test --workspace`
Expected: PASS — no other crate regressed by the `DocState`/`Content` changes.

- [ ] **Step 9: Commit**

```bash
git add crates/rmdigest/src/state.rs crates/rmdigest/src/generate.rs crates/rmdigest/src/ingest.rs
git commit -m "feat(rmdigest): digest only pdf/epub sources, skip notebooks"
```

---

## Notes for the implementer

- Run `cargo build -p rmdigest --tests` after Task 2 Step 1 to surface every `DocState { ... }` literal that needs `skipped: false`; don't guess the list.
- There is no `Makefile` in this repo — `cargo test` (per-crate `-p`, then `--workspace`) is the test runner.
- Do not touch the cloud sync index or `list_recursive`; the gate is intentionally inside `process_doc` so both the reactive watch path and the scheduled sweep are covered by one change.
