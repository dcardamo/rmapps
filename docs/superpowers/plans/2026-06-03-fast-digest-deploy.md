# Fast Digest Deploy Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers-extended-cc:subagent-driven-development (recommended) or superpowers-extended-cc:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make a regenerated digest appear on the reMarkable in a few seconds (not 30s+) with no transient duplicate files, by broadcasting the digest upload and reusing a stable doc UUID.

**Architecture:** Replace the digest deploy primitive (`Cloud::replace` = delete+create, non-broadcasting, fresh UUID) with a single broadcasting upsert that reuses the digest doc's UUID when it still exists. Thread that UUID through `rmdigest`'s `Backend` seam and persist it in `DocState.digest_uuids`.

**Tech Stack:** Rust, `rm-cloud` (native reMarkable client) with its `fake` test feature, `rmdigest` crate, `rmapps` binary.

**User Verification:** YES — Dan confirms on his own device that, after annotating a PDF, the digest updates within a few seconds and no duplicate digest files flash. Verified by Dan after implementation, on saturn.

---

## File Structure

- `crates/rm-cloud/src/fake/mod.rs`, `crates/rm-cloud/src/fake/handlers.rs` — test-only: record the broadcast flag from the root PUT so tests can assert a broadcast happened.
- `apps/rmapps/src/cloud.rs` — new `Cloud::deploy_digest(folder, name, pdf, prev_uuid) -> String`, the single broadcasting/stable-UUID deploy entry point. Tests live in the existing `#[cfg(test)] mod tests` here (already wired to `FakeCloud`).
- `crates/rmdigest/src/deploy.rs` — `Backend` trait: replace `put` with `deploy_digest`; update `LocalBackend`.
- `crates/rmdigest/src/generate.rs` — `process_doc` upload site: pass `prev.digest_uuids`, persist the returned UUID; update the in-crate fake backends used by tests.
- `apps/rmapps/src/cloud_adapters.rs` — `CloudBackend::deploy_digest` delegates to `Cloud::deploy_digest`.

---

### Task 1: Record broadcast flag in the rm-cloud fake

**Goal:** Let tests observe whether a commit was sent with `broadcast: true`, so Task 2 can assert the digest deploy broadcasts.

**Files:**
- Modify: `crates/rm-cloud/src/fake/mod.rs` (add `broadcast_commits` counter to `State`; add `broadcast_count()` getter on `FakeCloud`)
- Modify: `crates/rm-cloud/src/fake/handlers.rs` (`root_put` records the flag)

**Acceptance Criteria:**
- [ ] `FakeCloud::broadcast_count()` returns the number of root PUTs received with `broadcast: true`.
- [ ] A `put` (non-broadcast) leaves the count at 0; a `put_broadcast` increments it.

**Verify:** `cargo test -p rm-cloud --features fake broadcast_count` → PASS

**Steps:**

- [ ] **Step 1: Add the counter field to `State`**

In `crates/rm-cloud/src/fake/mod.rs`, add to the `State` struct (after `root_gets`):

```rust
    /// Count of root PUTs received with `broadcast: true` (test assertion of notify).
    pub broadcast_commits: u32,
```

- [ ] **Step 2: Add the getter on `FakeCloud`**

In `crates/rm-cloud/src/fake/mod.rs`, alongside `root_get_count`:

```rust
    /// Number of commits that requested a broadcast notification (test helper).
    pub fn broadcast_count(&self) -> u32 {
        self.state.lock().unwrap().broadcast_commits
    }
```

- [ ] **Step 3: Record the flag in `root_put`**

In `crates/rm-cloud/src/fake/handlers.rs`, the `RootPutReq.broadcast` field is currently `#[allow(dead_code)]`. Remove that attribute and, inside `root_put` after the generation check passes (right after `s.root_hash = req.hash.clone();`), add:

```rust
    if req.broadcast {
        s.broadcast_commits += 1;
    }
```

- [ ] **Step 4: Write the test**

Add to `crates/rm-cloud/src/fake/mod.rs` (or the nearest `#[cfg(test)]` module that has access to `Client`; mirror an existing fake test's setup):

```rust
#[cfg(test)]
mod broadcast_count_tests {
    use super::*;
    use crate::client::Client;
    use crate::config::Config;
    use crate::porcelain::docfiles::DocFiles;

    #[tokio::test]
    async fn broadcast_count_tracks_only_broadcasting_commits() {
        let fake = FakeCloud::spawn().await;
        let client = Client::from_user_token(Config::single_host(&fake.base), "user-token");

        // Non-broadcasting put: count stays 0.
        client.put(DocFiles::new_pdf("A", "", b"%PDF\n".to_vec())).await.unwrap();
        assert_eq!(fake.broadcast_count(), 0, "put must not broadcast");

        // Broadcasting put: count increments.
        client.put_broadcast(DocFiles::new_pdf("B", "", b"%PDF\n".to_vec())).await.unwrap();
        assert_eq!(fake.broadcast_count(), 1, "put_broadcast must broadcast once");
    }
}
```

- [ ] **Step 5: Run the test**

Run: `cargo test -p rm-cloud --features fake broadcast_count`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/rm-cloud/src/fake/mod.rs crates/rm-cloud/src/fake/handlers.rs
git commit -m "test(rm-cloud): fake records broadcast flag for deploy assertions"
```

```json:metadata
{"files": ["crates/rm-cloud/src/fake/mod.rs", "crates/rm-cloud/src/fake/handlers.rs"], "verifyCommand": "cargo test -p rm-cloud --features fake broadcast_count", "acceptanceCriteria": ["broadcast_count() reports broadcasting commits", "put=0, put_broadcast=1"], "requiresUserVerification": false}
```

---

### Task 2: `Cloud::deploy_digest` — broadcasting, stable-UUID upsert

**Goal:** One method that deploys the digest PDF as a single broadcasting commit, reusing the prior UUID when it still exists (in place, no duplicate), else creating fresh and sweeping any same-named duplicates. Returns the UUID used.

**Files:**
- Modify: `apps/rmapps/src/cloud.rs` (add `deploy_digest`; add tests in the existing `mod tests`)

**Acceptance Criteria:**
- [ ] First deploy (no prior UUID) creates one doc, returns its UUID, and broadcasts.
- [ ] Second deploy passing that UUID reuses the same doc id (still exactly one doc, same id) and broadcasts.
- [ ] Create-branch sweeps pre-existing same-named duplicates down to exactly one doc.
- [ ] A stale prior UUID (absent from cloud) falls back to create + sweep and returns a new UUID.

**Verify:** `cargo test -p rmapps deploy_digest` → PASS

**Steps:**

- [ ] **Step 1: Write the failing tests**

Add to the `#[cfg(test)] mod tests` in `apps/rmapps/src/cloud.rs` (it already imports `FakeCloud`, `Client`, `CloudConfig`, and has `cloud_from_client` + `doc_with_pdf` helpers; `DocFiles::new_pdf` mints the UUID):

```rust
    /// First deploy creates one doc and broadcasts; reusing the returned UUID
    /// updates that same doc in place (no new id, still one doc) and broadcasts.
    #[test]
    fn deploy_digest_reuses_uuid_and_broadcasts() {
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        let fake = rt.block_on(FakeCloud::spawn());
        let cloud = cloud_from_client(Client::from_user_token(
            CloudConfig::single_host(&fake.base), "user-token",
        ));

        let folder = cloud.ensure_folder("/Books").unwrap();

        // First deploy: no prior UUID → create.
        let uuid1 = cloud
            .deploy_digest("/Books", "Book.digest", b"%PDF-v1".to_vec(), None)
            .unwrap();
        assert!(!uuid1.is_empty(), "first deploy returns a UUID");
        assert_eq!(cloud.doc_ids_in(&folder, "Book.digest").unwrap().len(), 1);
        assert_eq!(fake.broadcast_count(), 1, "first deploy must broadcast");

        // Second deploy: reuse uuid1 → same doc id, still one doc, broadcasts again.
        let uuid2 = cloud
            .deploy_digest("/Books", "Book.digest", b"%PDF-v2".to_vec(), Some(&uuid1))
            .unwrap();
        assert_eq!(uuid2, uuid1, "reused UUID must be returned unchanged");
        let ids = cloud.doc_ids_in(&folder, "Book.digest").unwrap();
        assert_eq!(ids.len(), 1, "reuse must not create a second doc");
        assert_eq!(ids[0], uuid1, "the one doc keeps the original UUID");
        assert_eq!(fake.broadcast_count(), 2, "second deploy must broadcast");
    }

    /// Create-branch (no/stale prior UUID) sweeps pre-existing duplicates to one.
    #[test]
    fn deploy_digest_create_branch_sweeps_duplicates() {
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        let fake = rt.block_on(FakeCloud::spawn());
        let seed = Client::from_user_token(CloudConfig::single_host(&fake.base), "user-token");

        let folder = rt.block_on(seed.mkdir("Books", "")).unwrap();
        for id in [
            "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
            "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
        ] {
            rt.block_on(seed.put(doc_with_pdf(id, "Book.digest", &folder, b"%PDF-old")))
                .unwrap();
        }

        let cloud = cloud_from_client(Client::from_user_token(
            CloudConfig::single_host(&fake.base), "user-token",
        ));
        assert_eq!(cloud.doc_ids_in(&folder, "Book.digest").unwrap().len(), 2);

        // Stale prior UUID (never existed) → create branch + sweep.
        let uuid = cloud
            .deploy_digest("/Books", "Book.digest", b"%PDF-new".to_vec(), Some("not-a-real-uuid"))
            .unwrap();
        let ids = cloud.doc_ids_in(&folder, "Book.digest").unwrap();
        assert_eq!(ids.len(), 1, "create branch must sweep to exactly one doc");
        assert_eq!(ids[0], uuid, "the surviving doc is the freshly created one");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p rmapps deploy_digest`
Expected: FAIL — `no method named deploy_digest`.

- [ ] **Step 3: Implement `deploy_digest`**

In `apps/rmapps/src/cloud.rs`, add this method to the `impl Cloud` block (near `replace`/`upsert`). It uses existing helpers `ensure_folder`, `doc_ids_in`, `block_on`, `self.client`, and `DocFiles::new_pdf` (already imported in this module, as `upsert_in`/`replace_in` use `DocFiles::new_pdf`):

```rust
    /// Deploy the digest PDF as a single broadcasting commit. When `prev_uuid` still
    /// exists in `folder`, the digest doc is upserted in place under that same UUID
    /// (rebuilding `.content`/`.metadata` for the new PDF, so a grown digest's page
    /// count stays correct) — no duplicate flashing, one commit. Otherwise a fresh doc
    /// is created and any pre-existing same-named docs are swept (converging older
    /// `replace`-minted duplicates). Broadcasts so the device pulls the update promptly.
    /// Returns the UUID the digest now lives under.
    pub fn deploy_digest(
        &self,
        folder: &str,
        name: &str,
        pdf: Vec<u8>,
        prev_uuid: Option<&str>,
    ) -> Result<String> {
        let folder_id = self.ensure_folder(folder)?;
        let existing = self.doc_ids_in(&folder_id, name)?;

        // Reuse the prior UUID only if that exact doc still exists in this folder.
        let reuse = prev_uuid.filter(|u| existing.iter().any(|e| e == u));

        // Build a fresh doc for the new PDF (correct .content page list), then pin the id.
        let mut df = DocFiles::new_pdf(name, &folder_id, pdf);
        if let Some(u) = reuse {
            df.id = u.to_string();
        } else {
            // Create branch: sweep every pre-existing same-named doc so we converge
            // away from any duplicate state before creating the canonical one.
            for id in &existing {
                let _ = self.rt.block_on(self.client.rm(id));
            }
        }
        let id = df.id.clone();
        self.rt
            .block_on(self.client.put_broadcast(df))
            .map_err(|e| anyhow!("deploy digest {name}: {e}"))?;
        Ok(id)
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p rmapps deploy_digest`
Expected: PASS (both tests).

- [ ] **Step 5: Commit**

```bash
git add apps/rmapps/src/cloud.rs
git commit -m "feat(rmapps): Cloud::deploy_digest — broadcasting stable-UUID digest upsert"
```

```json:metadata
{"files": ["apps/rmapps/src/cloud.rs"], "verifyCommand": "cargo test -p rmapps deploy_digest", "acceptanceCriteria": ["create returns UUID + broadcasts", "reuse keeps same id, one doc, broadcasts", "create branch sweeps duplicates", "stale UUID falls back to create"], "requiresUserVerification": false}
```

---

### Task 3: Wire the stable-UUID deploy seam through rmdigest

**Goal:** Replace the `Backend::put` seam with `deploy_digest`, persist the returned UUID in `DocState.digest_uuids`, and route the cloud backend through `Cloud::deploy_digest`. All deploy paths (cloud + local + test fakes) move to the new signature in one commit so the crates compile.

**Files:**
- Modify: `crates/rmdigest/src/deploy.rs` (trait method + `LocalBackend`)
- Modify: `crates/rmdigest/src/generate.rs` (upload site + the two in-crate fake backends in tests)
- Modify: `apps/rmapps/src/cloud_adapters.rs` (`CloudBackend`)

**Acceptance Criteria:**
- [ ] `Backend` exposes `deploy_digest(&self, pdf, folder, name, prev_uuid) -> Result<String>` (no `put`).
- [ ] `process_doc` passes the prior UUID and persists the returned one into `digest_uuids`.
- [ ] `LocalBackend::deploy_digest` writes `<folder>/<name>.pdf` and returns `String::new()`.
- [ ] `CloudBackend::deploy_digest` delegates to `Cloud::deploy_digest`.
- [ ] A second run over a changed fixture passes the first run's UUID back in (stable-UUID round-trip), asserted by an updated `generate.rs` test.

**Verify:** `cargo test -p rmdigest && cargo build -p rmapps` → PASS / builds clean

**Steps:**

- [ ] **Step 1: Change the trait + `LocalBackend` (deploy.rs)**

In `crates/rmdigest/src/deploy.rs`, replace the `put` trait method:

```rust
    /// Deploy the digest `pdf` named `name` in `folder`. `prev_uuid` is the UUID a
    /// prior run recorded for this digest (if any); a backend that has stable doc
    /// identity should reuse it (update in place) and return the UUID the digest now
    /// lives under. Backends without a UUID concept return an empty string.
    fn deploy_digest(
        &self,
        pdf: &Path,
        folder: &str,
        name: &str,
        prev_uuid: Option<&str>,
    ) -> Result<String>;
```

Replace the `LocalBackend` impl of `put` with:

```rust
    fn deploy_digest(
        &self,
        pdf: &Path,
        folder: &str,
        name: &str,
        _prev_uuid: Option<&str>,
    ) -> Result<String> {
        let dest_dir = Path::new(folder);
        std::fs::create_dir_all(dest_dir)?;
        let dest = dest_dir.join(format!("{name}.pdf"));
        std::fs::copy(pdf, &dest)?;
        Ok(String::new())
    }
```

Update the local-backend test `local_backend_put_copies_pdf` to call the new method (keep its assertion):

```rust
    #[test]
    fn local_backend_deploy_writes_pdf() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.pdf");
        std::fs::write(&src, b"pdf").unwrap();
        let backend = LocalBackend;
        let uuid = backend
            .deploy_digest(&src, &dir.path().to_string_lossy(), "out", None)
            .unwrap();
        assert!(dir.path().join("out.pdf").exists());
        assert_eq!(uuid, "", "local backend has no UUID");
    }
```

- [ ] **Step 2: Update the upload site in `process_doc` (generate.rs)**

In `crates/rmdigest/src/generate.rs`, replace the staging/upload block (currently writes `digest_file`, then `backend.put(...)`, then persists state) so it threads the UUID. The block that currently reads:

```rust
    {
        let _s = tracing::info_span!("digest.upload", doc = %doc.path).entered();
        backend.put(&digest_file, &doc.folder, &digest_name)?;
    }

    // Persist state only after the upload succeeds, so a crash re-processes.
    prev.cloud_version = doc.version.clone();
    prev.page_hashes = ing.new_hashes;
    prev.skipped = false; // a previously-skipped doc that is now a supported kind re-engages
    state.save(state_path)?;
```

becomes:

```rust
    let uuid = {
        let _s = tracing::info_span!("digest.upload", doc = %doc.path).entered();
        let prev_uuid = prev.digest_uuids.first().map(String::as_str);
        backend.deploy_digest(&digest_file, &doc.folder, &digest_name, prev_uuid)?
    };

    // Persist state only after the upload succeeds, so a crash re-processes.
    prev.cloud_version = doc.version.clone();
    prev.page_hashes = ing.new_hashes;
    prev.skipped = false; // a previously-skipped doc that is now a supported kind re-engages
    prev.digest_uuids = if uuid.is_empty() { vec![] } else { vec![uuid] };
    state.save(state_path)?;
```

- [ ] **Step 3: Update the in-crate fake backends (generate.rs tests)**

In `crates/rmdigest/src/generate.rs`, both `CountingBackend` and `FakeBackend` implement `Backend`. Replace each `put` impl with `deploy_digest`.

For `CountingBackend` (in `mod cheap_skip_tests`), replace its `put`:

```rust
        fn deploy_digest(
            &self,
            _pdf: &std::path::Path,
            _folder: &str,
            _name: &str,
            _prev_uuid: Option<&str>,
        ) -> anyhow::Result<String> {
            Ok(String::new())
        }
```

For `FakeBackend` (in `mod tests`), which records puts, change its `PutLog` to also capture the prior UUID and return a stable per-deploy UUID so the round-trip can be asserted. Replace the `put` impl:

```rust
        fn deploy_digest(
            &self,
            pdf: &Path,
            folder: &str,
            name: &str,
            prev_uuid: Option<&str>,
        ) -> anyhow::Result<String> {
            let bytes = std::fs::read(pdf)?;
            self.puts.lock().unwrap().push((
                folder.to_string(),
                name.to_string(),
                bytes,
                prev_uuid.map(str::to_string),
            ));
            // Stable fake UUID keyed by name so a re-deploy of the same digest reuses it.
            Ok(format!("uuid-{name}"))
        }
```

Update the `PutLog` type alias to carry the extra field:

```rust
    type PutLog = Arc<Mutex<Vec<(String, String, Vec<u8>, Option<String>)>>>;
```

Then fix the existing put-inspecting assertions in this module to the 4-tuple. The destructuring sites are:
- `integration_two_puts_then_skip`: `for (_, name, bytes) in &first_puts` → `for (_, name, bytes, _) in &first_puts`; and `first_puts.iter().any(|(_, name, _)| ...)` → `|(_, name, _, _)|`.

(Leave `integration_two_puts_then_skip`'s "second run uploads 0" assertion as-is; the deploy seam is still only called when a doc changes.)

- [ ] **Step 4: Add a stable-UUID round-trip test (generate.rs)**

Add to `mod tests` in `crates/rmdigest/src/generate.rs`. It runs the fixture twice through `run`, mutating state between runs by clearing page hashes so the second run re-deploys, and asserts the second deploy received the first run's UUID:

```rust
    #[test]
    fn second_deploy_passes_back_prior_uuid() {
        let fixture = fixture_path();
        let puts = Arc::new(Mutex::new(Vec::new()));
        let doc = CloudDoc {
            path: "/Books/StampedLabels".to_string(),
            name: "stamped-labels".to_string(),
            folder: "/Books".to_string(),
            version: None, // None → always re-processes (no cheap-skip)
        };
        let backend = FakeBackend {
            fixture: doc,
            fixture_path: fixture,
            puts: puts.clone(),
        };
        let cfg = fake_cfg();
        let state_dir = tempfile::tempdir().expect("tempdir");
        let state_path = state_dir.path().join("state.json");
        let opts = Opts { dry_run: false, local_output: None };

        // First run: prev_uuid is None, fake returns a stable "uuid-<name>".
        run(&cfg, &backend, &state_path, &opts).expect("first run");
        // Force re-deploy on the next run: drop the recorded page hashes.
        {
            let mut st = State::load(&state_path).unwrap();
            let d = st.docs.get_mut("/Books/StampedLabels").unwrap();
            // Keep the recorded digest UUID, clear hashes so the doc looks changed.
            d.page_hashes.clear();
            st.save(&state_path).unwrap();
        }
        // Second run: must pass the persisted UUID back into deploy_digest.
        run(&cfg, &backend, &state_path, &opts).expect("second run");

        let log = puts.lock().unwrap();
        let last = log.last().expect("at least one deploy on the second run");
        let digest_name = format!("stamped-labels{}", cfg.output.digest_suffix);
        assert_eq!(last.3.as_deref(), Some(format!("uuid-{digest_name}").as_str()),
            "second deploy must receive the UUID the first run recorded");
    }
```

- [ ] **Step 5: Update the `CloudBackend` adapter (cloud_adapters.rs)**

In `apps/rmapps/src/cloud_adapters.rs`, replace `CloudBackend`'s `put` impl:

```rust
    fn deploy_digest(
        &self,
        pdf: &Path,
        folder: &str,
        name: &str,
        prev_uuid: Option<&str>,
    ) -> Result<String> {
        self.cloud.deploy_digest(folder, name, std::fs::read(pdf)?, prev_uuid)
    }
```

- [ ] **Step 6: Run tests + build**

Run: `cargo test -p rmdigest`
Expected: PASS (including the new round-trip test and updated local-backend test).

Run: `cargo build -p rmapps`
Expected: builds clean (the adapter now satisfies the new trait).

- [ ] **Step 7: Commit**

```bash
git add crates/rmdigest/src/deploy.rs crates/rmdigest/src/generate.rs apps/rmapps/src/cloud_adapters.rs
git commit -m "feat(rmdigest): deploy via stable-UUID broadcasting upsert"
```

```json:metadata
{"files": ["crates/rmdigest/src/deploy.rs", "crates/rmdigest/src/generate.rs", "apps/rmapps/src/cloud_adapters.rs"], "verifyCommand": "cargo test -p rmdigest && cargo build -p rmapps", "acceptanceCriteria": ["Backend::deploy_digest replaces put", "process_doc threads + persists UUID", "LocalBackend returns empty UUID", "CloudBackend delegates", "round-trip test passes prior UUID"], "requiresUserVerification": false}
```

---

### Task 4: Full suite + on-device verification with Dan

**Goal:** Confirm the whole workspace is green, then have Dan verify on his device that the digest now updates within seconds and no duplicates flash.

**Files:**
- None (verification only)

**Acceptance Criteria:**
- [ ] `cargo test` across the workspace passes.
- [ ] After deploying the new binary on saturn, annotating a watched PDF makes the digest update on-device within a few seconds.
- [ ] No transient duplicate digest files appear.

**Verify:** `cargo test` (workspace) → PASS, then the user-verification prompt below.

**Steps:**

- [ ] **Step 1: Run the full workspace test suite**

Run: `cargo test`
Expected: all crates PASS.

- [ ] **Step 2: Deploy on saturn**

Committing the code already advanced `HEAD`; per CLAUDE.md, force the rebuild so the daemon restarts with the new binary instead of racing the timer:

```bash
export PATH=/run/wrappers/bin:$PATH
sudo systemctl start remarkable-update   # synchronous rebuild; daemon restarts when done
```

Confirm the daemon is back up:

Run: `systemctl is-active rmapps-watch`
Expected: `active`.

- [ ] **Step 3: User Verification Required**

Before marking this task complete, you MUST call AskUserQuestion:

```yaml
AskUserQuestion:
  question: "Annotate a PDF in a watched folder (e.g. Getting to Zero). Does the digest now update on your reMarkable within a few seconds, with no duplicate digest files flashing?"
  header: "Verification"
  options:
    - label: "Fast, no duplicates"
      description: "Digest appears within a few seconds and only one digest file — fix confirmed"
    - label: "Still slow / duplicates"
      description: "Latency still high or duplicates still appear — needs rework"
```

If the user selects the negative option: the task is NOT complete. Diagnose (re-run the `--timings` measurement; check the daemon picked up the new binary; confirm the broadcast reaches the device), rework, then re-verify with AskUserQuestion again.

```json:metadata
{"files": [], "verifyCommand": "cargo test", "acceptanceCriteria": ["workspace tests pass", "digest updates on-device within seconds", "no duplicate digest files"], "requiresUserVerification": true, "userVerificationPrompt": "Annotate a PDF in a watched folder. Does the digest update on your reMarkable within a few seconds, with no duplicate digest files?"}
```

---

## Self-Review

**Spec coverage:** Broadcast (Tasks 1+2+3), stable-UUID upsert (Task 2+3), persist UUID in `digest_uuids` (Task 3), correct `.content` via full upsert (Task 2 impl), duplicate sweep on create (Task 2), no blob-PUT parallelization (honored — not in any task), reader untouched (honored). Covered.

**Placeholder scan:** No TBD/TODO; every code step shows full code.

**Type consistency:** `deploy_digest(&self, pdf: &Path, folder: &str, name: &str, prev_uuid: Option<&str>) -> Result<String>` is identical across the trait, `LocalBackend`, `CloudBackend`, and both test fakes. `Cloud::deploy_digest(folder, name, pdf: Vec<u8>, prev_uuid) -> Result<String>` is the wrapper signature the adapter calls. `digest_uuids: Vec<String>` matches `DocState`.

**Verification requirement scan:** YES — the user wants the speedup verified on his device. Task 4 carries `requiresUserVerification: true` with the standard block.
