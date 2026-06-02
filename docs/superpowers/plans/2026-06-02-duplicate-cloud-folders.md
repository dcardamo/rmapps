# Duplicate Cloud-Folder Prevention Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers-extended-cc:subagent-driven-development (recommended) or superpowers-extended-cc:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stop `rmapps` from minting duplicate same-named cloud folders by resolving each destination path once per run and reusing the folder id for every deploy.

**Architecture:** Add id-based deploy methods to `Cloud` (`*_in(folder_id, …)`) and refactor the existing path-based methods into thin wrappers. Callers (reader, bujo) resolve each distinct path once via a small run-scoped `FolderIds` memo and reuse the id, collapsing the intra-run multiplicity (reader 2×, bujo ≤14×) into a single `mkdir` decision. A new fake-cloud staleness seam (`lag_next_commit`) reproduces the eventual-consistency window so the regression is locked by tests.

**Tech Stack:** Rust workspace (`apps/rmapps`, `crates/rm-cloud`), Tokio, axum-based in-process fake cloud (`fake` feature), `tempfile`.

**User Verification:** NO — verification is automated (`cargo test --workspace`) plus an optional live `rmapps ls` check noted in the spec; the spec requires no human sign-off.

**Spec:** `docs/superpowers/specs/2026-06-02-duplicate-cloud-folders-design.md`

---

## File Structure

- `apps/rmapps/src/cloud.rs` — add `upsert_in`/`replace_in`/`create_if_missing_in`; refactor `upsert`/`replace`/`create_if_missing` into wrappers; add `FolderIds` resolver; add fix/guard tests.
- `crates/rm-cloud/src/fake/mod.rs` — `State` lag fields + `lag_next_commit` helper.
- `crates/rm-cloud/src/fake/handlers.rs` — arm-on-commit in `root_put`, serve-stale-index in `root_get`.
- `crates/rm-cloud/src/porcelain/fs.rs` — bug-lock test (`mkdir_p` duplicates under lag).
- `apps/rmapps/src/reader.rs` — upload loop uses `FolderIds` + `replace_in`.
- `apps/rmapps/src/bujo.rs` — three deploy loops use `FolderIds` + `*_in`.

---

### Task 1: Id-based deploy methods on `Cloud`

**Goal:** `Cloud` exposes `upsert_in`/`replace_in`/`create_if_missing_in` that take an already-resolved `folder_id` and never call `ensure_folder`; the path-based methods delegate to them.

**Files:**
- Modify: `apps/rmapps/src/cloud.rs` (methods around lines 167–209; tests in the `#[cfg(test)] mod tests`)

**Acceptance Criteria:**
- [ ] `replace_in`/`upsert_in`/`create_if_missing_in` exist and operate on a `folder_id`.
- [ ] `upsert`/`replace`/`create_if_missing` are wrappers: `ensure_folder` then delegate.
- [ ] Existing `replace_removes_all_same_named_docs` and `warm_replace_is_account_size_independent` tests still pass.
- [ ] New test proves `replace_in` deploys into the given folder id without resolving a path.

**Verify:** `cargo test -p rmapps cloud::` → all pass.

**Steps:**

- [ ] **Step 1: Write the failing test** (append inside `apps/rmapps/src/cloud.rs` `mod tests`)

```rust
    /// `replace_in` deploys into an already-resolved folder id and sweeps duplicates,
    /// without doing any path resolution itself.
    #[test]
    fn replace_in_targets_resolved_folder_id() {
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        let fake = rt.block_on(FakeCloud::spawn());
        let client = Client::from_user_token(CloudConfig::single_host(&fake.base), "user-token");
        let cloud = cloud_from_client(client);

        let folder_id = cloud.ensure_folder("/Readwise").unwrap();
        cloud.replace_in(&folder_id, "Feed", b"%PDF-1".to_vec()).unwrap();
        cloud.replace_in(&folder_id, "Feed", b"%PDF-2".to_vec()).unwrap();

        // Exactly one "Feed" doc remains under the resolved folder id.
        assert_eq!(cloud.doc_ids_in(&folder_id, "Feed").unwrap().len(), 1);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rmapps cloud::tests::replace_in_targets_resolved_folder_id`
Expected: FAIL — `no method named replace_in`.

- [ ] **Step 3: Add the `_in` methods and refactor wrappers** (`apps/rmapps/src/cloud.rs`)

Replace the bodies of `upsert`, `create_if_missing`, and `replace` (lines ~167–209) with wrappers, and add the three `_in` methods immediately after `replace`:

```rust
    /// Create the doc if absent, else replace only its PDF blob (content-only),
    /// preserving on-device handwriting (mechanics §3). `folder` is created if missing.
    pub fn upsert(&self, folder: &str, name: &str, pdf: Vec<u8>) -> Result<()> {
        let folder_id = self.ensure_folder(folder)?;
        self.upsert_in(&folder_id, name, pdf)
    }

    /// Create the doc only if it does not already exist; existing docs are left
    /// completely untouched (no upload), so on-device edits survive.
    pub fn create_if_missing(&self, folder: &str, name: &str, pdf: Vec<u8>) -> Result<()> {
        let folder_id = self.ensure_folder(folder)?;
        self.create_if_missing_in(&folder_id, name, pdf)
    }

    /// Destructive replace: remove EVERY existing doc of this name, then create a
    /// fresh one. For write-only docs (reader PDFs, digests) with no ink to keep.
    pub fn replace(&self, folder: &str, name: &str, pdf: Vec<u8>) -> Result<()> {
        let folder_id = self.ensure_folder(folder)?;
        self.replace_in(&folder_id, name, pdf)
    }

    /// `upsert` against an already-resolved folder id (no path resolution).
    pub fn upsert_in(&self, folder_id: &str, name: &str, pdf: Vec<u8>) -> Result<()> {
        match self.doc_id_in(folder_id, name)? {
            Some(id) => self
                .rt
                .block_on(self.client.put_content_only(&id, pdf))
                .map_err(|e| anyhow!("content-only update {name}: {e}")),
            None => self
                .rt
                .block_on(self.client.put(DocFiles::new_pdf(name, folder_id, pdf)))
                .map_err(|e| anyhow!("create {name}: {e}")),
        }
    }

    /// `create_if_missing` against an already-resolved folder id (no path resolution).
    pub fn create_if_missing_in(&self, folder_id: &str, name: &str, pdf: Vec<u8>) -> Result<()> {
        if self.doc_id_in(folder_id, name)?.is_some() {
            return Ok(());
        }
        self.rt
            .block_on(self.client.put(DocFiles::new_pdf(name, folder_id, pdf)))
            .map_err(|e| anyhow!("create {name}: {e}"))
    }

    /// `replace` against an already-resolved folder id (no path resolution). Sweeps
    /// EVERY same-named doc before creating, so it converges pre-existing duplicates.
    pub fn replace_in(&self, folder_id: &str, name: &str, pdf: Vec<u8>) -> Result<()> {
        for id in self.doc_ids_in(folder_id, name)? {
            // Best-effort remove; individual failures surface on the create below.
            let _ = self.rt.block_on(self.client.rm(&id));
        }
        self.rt
            .block_on(self.client.put(DocFiles::new_pdf(name, folder_id, pdf)))
            .map_err(|e| anyhow!("replace {name}: {e}"))
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p rmapps cloud::`
Expected: PASS (new test + `replace_removes_all_same_named_docs` + `warm_replace_is_account_size_independent`).

- [ ] **Step 5: Commit**

```bash
git add apps/rmapps/src/cloud.rs
git commit -m "refactor(rmapps): id-based deploy methods (*_in) with path wrappers"
```

---

### Task 2: Fake-cloud staleness seam + bug-lock test

**Goal:** The fake can model eventual consistency — a commit's effect stays invisible to the next N root reads — and a test proves that under this lag two `mkdir_p` of the same path mint a duplicate (locking in the defect the fix prevents).

**Files:**
- Modify: `crates/rm-cloud/src/fake/mod.rs` (`State` fields + `lag_next_commit` helper)
- Modify: `crates/rm-cloud/src/fake/handlers.rs` (`root_put` arms lag; `root_get` serves stale index)
- Modify: `crates/rm-cloud/src/porcelain/fs.rs` (`fs_tests`: bug-lock test)

**Acceptance Criteria:**
- [ ] `FakeCloud::lag_next_commit(n)` arms the next commit to lag `n` subsequent root GETs.
- [ ] After an armed commit, the next `n` root GETs report the **current** generation but serve the **pre-commit** root hash; reads beyond `n` are normal.
- [ ] Bug-lock test: under armed lag, two `mkdir_p("/Readwise")` return different ids.

**Verify:** `cargo test -p rm-cloud --features fake fs_tests::lagging_index` → passes.

**Steps:**

- [ ] **Step 1: Write the failing bug-lock test** (append inside `crates/rm-cloud/src/porcelain/fs.rs` `mod fs_tests`)

```rust
    /// Faithful eventual-consistency repro: when the mkdir commit's new folder is not
    /// yet visible to the immediately-following resolve, the sync store is rebuilt
    /// missing it, so the next `mkdir_p` of the same path mints a DUPLICATE. This locks
    /// in the defect that the run-scoped resolver (Task 3) prevents.
    #[tokio::test]
    async fn lagging_index_after_mkdir_duplicates_folder() {
        let fake = FakeCloud::spawn().await;
        let dir = tempfile::tempdir().unwrap();
        let client = Client::from_user_token(Config::single_host(&fake.base), "user-token")
            .with_sync_store(SyncStore::new(dir.path().join("idx.json")));

        // Arm: the mkdir commit's effect is invisible for the next few root reads,
        // poisoning the store so "/Readwise" looks absent on the second resolve.
        fake.lag_next_commit(4);
        let f1 = client.mkdir_p("/Readwise").await.unwrap();
        let f2 = client.mkdir_p("/Readwise").await.unwrap();

        assert_ne!(f1, f2, "stale index made the second resolve mint a duplicate folder");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rm-cloud --features fake fs_tests::lagging_index_after_mkdir_duplicates_folder`
Expected: FAIL — `no method named lag_next_commit` (compile error).

- [ ] **Step 3: Add lag state + helper** (`crates/rm-cloud/src/fake/mod.rs`)

Add three fields to `State` (after `root_gets`):

```rust
    /// Reads to keep stale once the next commit arms the lag (0 = unarmed).
    pub arm_lag: u32,
    /// Remaining root GETs currently serving the pre-commit index (0 = none).
    pub active_lag: u32,
    /// Root hash to serve while a lag window is active (the pre-commit index).
    pub lagged_hash: String,
```

Add the helper in `impl FakeCloud` (next to `inject_rate_limited`):

```rust
    /// Arm the NEXT root PUT so that the following `reads` root GETs report the new
    /// generation but serve the PRE-commit root index — modelling reMarkable's
    /// eventual consistency (commit accepted, read replica lags). Used to reproduce
    /// the duplicate-folder race deterministically.
    pub fn lag_next_commit(&self, reads: u32) {
        self.state.lock().unwrap().arm_lag = reads;
    }
```

- [ ] **Step 4: Arm on commit and serve stale on read** (`crates/rm-cloud/src/fake/handlers.rs`)

In `root_put`, after `s.root_hash = req.hash.clone();` (line ~120) and before `let gen = s.generation;`, capture the pre-commit hash and activate the lag window if armed:

```rust
        if s.arm_lag > 0 {
            s.active_lag = s.arm_lag;
            s.arm_lag = 0;
            // The index visible BEFORE this commit (root_hash before we overwrote it).
            s.lagged_hash = req_prev_hash;
        }
```

To have `req_prev_hash`, capture the old hash before overwriting. Change the two lines around line ~119–120 from:

```rust
        s.generation = req.generation + 1;
        s.root_hash = req.hash.clone();
```

to:

```rust
        let req_prev_hash = s.root_hash.clone();
        s.generation = req.generation + 1;
        s.root_hash = req.hash.clone();
        if s.arm_lag > 0 {
            s.active_lag = s.arm_lag;
            s.arm_lag = 0;
            s.lagged_hash = req_prev_hash;
        }
```

In `root_get`, replace the final response block (after `s.root_gets += 1;`, lines ~87–93) so an active lag serves the stale hash with the current generation:

```rust
        s.root_gets += 1;
        let hash = if s.active_lag > 0 {
            s.active_lag -= 1;
            s.lagged_hash.clone()
        } else {
            s.root_hash.clone()
        };
        Json(RootResp {
            hash,
            generation: s.generation,
            schema_version: 4,
        })
        .into_response()
```

- [ ] **Step 5: Run the bug-lock test to verify it passes**

Run: `cargo test -p rm-cloud --features fake fs_tests::lagging_index_after_mkdir_duplicates_folder`
Expected: PASS (`f1 != f2`). If it unexpectedly passes with equal ids, raise the `lag_next_commit` budget (e.g. 6) — the window must cover the post-mkdir re-resolve plus the second resolve's generation poll. Confirm no other `fake_*` test regressed: `cargo test -p rm-cloud --features fake`.

- [ ] **Step 6: Commit**

```bash
git add crates/rm-cloud/src/fake/mod.rs crates/rm-cloud/src/fake/handlers.rs crates/rm-cloud/src/porcelain/fs.rs
git commit -m "test(rm-cloud): fake eventual-consistency seam + duplicate-folder bug-lock"
```

---

### Task 3: `FolderIds` run-scoped resolver + fix tests

**Goal:** A `FolderIds` memo resolves each distinct path once per run; a test proves that under the same injected lag, resolver-based deploy of two docs to one path yields exactly one folder.

**Files:**
- Modify: `apps/rmapps/src/cloud.rs` (add `FolderIds`; add fix + guard tests; export `FolderIds`)

**Acceptance Criteria:**
- [ ] `FolderIds::new(&cloud)` + `get(path)` resolve-once memo exists and is `pub`.
- [ ] Repeated `get(path)` returns the same id with one underlying `ensure_folder`.
- [ ] Fix test: under armed lag, two docs deployed to `/Readwise` via one `FolderIds` produce exactly one `Readwise` folder.

**Verify:** `cargo test -p rmapps cloud::` → all pass.

**Steps:**

- [ ] **Step 1: Write the failing fix test** (append inside `apps/rmapps/src/cloud.rs` `mod tests`)

```rust
    /// Under the SAME eventual-consistency lag that duplicates folders for the naive
    /// double-resolve path, a run-scoped `FolderIds` resolves "/Readwise" once and
    /// reuses the id, so exactly one folder is ever created.
    #[test]
    fn resolver_prevents_duplicate_folder_under_lag() {
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        let fake = rt.block_on(FakeCloud::spawn());
        let dir = tempfile::tempdir().unwrap();
        let client = Client::from_user_token(CloudConfig::single_host(&fake.base), "user-token")
            .with_sync_store(rm_cloud::SyncStore::new(dir.path().join("idx.json")));
        let cloud = cloud_from_client(client);

        fake.lag_next_commit(4);
        let mut folders = FolderIds::new(&cloud);
        let id_a = folders.get("/Readwise").unwrap();
        cloud.replace_in(&id_a, "Library", b"lib".to_vec()).unwrap();
        let id_b = folders.get("/Readwise").unwrap();
        cloud.replace_in(&id_b, "Feed", b"feed".to_vec()).unwrap();

        // Resolver reused the same id (one ensure_folder), so no second mkdir happened.
        assert_eq!(id_a, id_b);

        // And the cloud holds exactly one "Readwise" folder. Read with a COLD client so
        // the lag window (long spent) can't mask a duplicate via a poisoned store.
        let cold = Client::from_user_token(CloudConfig::single_host(&fake.base), "user-token");
        let tree = rt.block_on(cold.resolved_snapshot()).unwrap();
        let readwise = tree
            .docs
            .values()
            .filter(|d| d.is_folder && d.name == "Readwise")
            .count();
        assert_eq!(readwise, 1, "exactly one Readwise folder");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rmapps cloud::tests::resolver_prevents_duplicate_folder_under_lag`
Expected: FAIL — `cannot find type FolderIds`.

- [ ] **Step 3: Add the `FolderIds` resolver** (`apps/rmapps/src/cloud.rs`, after the `impl Cloud` block, near the top-level items)

Add `use std::collections::HashMap;` to the existing `use std::path::{Path, PathBuf};` import group (as a separate `use`), then:

```rust
/// Run-scoped memo of folder path → resolved id. The first `get` for a path performs
/// the one `ensure_folder` (hence the one possible `mkdir`); later `get`s for the same
/// path return the cached id with no cloud call. Construct one per run/task — NOT per
/// `Cloud`, which the `watch` daemon keeps alive across tasks (a folder can be trashed
/// and recreated between tasks, so a `Cloud`-lifetime cache would deploy into a deleted
/// folder).
pub struct FolderIds<'a> {
    cloud: &'a Cloud,
    ids: HashMap<String, String>,
}

impl<'a> FolderIds<'a> {
    /// A fresh, empty resolver bound to `cloud`.
    pub fn new(cloud: &'a Cloud) -> Self {
        Self {
            cloud,
            ids: HashMap::new(),
        }
    }

    /// Resolve `path` to a folder id, creating it on first miss; cached thereafter.
    pub fn get(&mut self, path: &str) -> Result<String> {
        if let Some(id) = self.ids.get(path) {
            return Ok(id.clone());
        }
        let id = self.cloud.ensure_folder(path)?;
        self.ids.insert(path.to_string(), id.clone());
        Ok(id)
    }
}
```

- [ ] **Step 4: Add a bujo-shaped guard test** (append inside `mod tests`)

```rust
    /// bujo deploys many PDFs to ONE target folder; a single resolver must yield one
    /// stable id for all of them (so only one folder is created).
    #[test]
    fn resolver_single_target_one_folder_for_many_docs() {
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        let fake = rt.block_on(FakeCloud::spawn());
        let client = Client::from_user_token(CloudConfig::single_host(&fake.base), "user-token");
        let cloud = cloud_from_client(client);

        let mut folders = FolderIds::new(&cloud);
        let mut ids = std::collections::HashSet::new();
        for i in 0..14 {
            let id = folders.get("/2026").unwrap();
            cloud.upsert_in(&id, &format!("2026.{i:02} Doc"), b"%PDF".to_vec()).unwrap();
            ids.insert(id);
        }
        assert_eq!(ids.len(), 1, "all 14 docs resolved the same /2026 folder id");
    }
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p rmapps cloud::`
Expected: PASS (both new tests + Task 1 tests).

- [ ] **Step 6: Commit**

```bash
git add apps/rmapps/src/cloud.rs
git commit -m "feat(rmapps): run-scoped FolderIds resolver (resolve each path once)"
```

---

### Task 4: Wire reader and bujo to the resolver

**Goal:** reader and bujo deploy through `FolderIds` + `*_in`, so each distinct destination path is resolved once per run.

**Files:**
- Modify: `apps/rmapps/src/reader.rs` (upload loop, lines ~74–79)
- Modify: `apps/rmapps/src/bujo.rs` (deploy loops at lines ~146–153 and ~159–166)

**Acceptance Criteria:**
- [ ] reader's upload loop resolves each `folder` via one `FolderIds` and calls `replace_in`.
- [ ] bujo's only-month and whole-year loops resolve `target` via one `FolderIds` and call `upsert_in`/`create_if_missing_in`.
- [ ] `cargo test --workspace` passes; `cargo build -p rmapps` clean.

**Verify:** `cargo test --workspace` → green.

**Steps:**

- [ ] **Step 1: Wire reader's upload loop** (`apps/rmapps/src/reader.rs`)

Replace the upload span block (lines ~74–79):

```rust
        {
            let _s = tracing::info_span!("reader.upload", docs = targets.len()).entered();
            for (pdf, folder) in &targets {
                cl.replace(folder, &cloud::doc_name(pdf)?, std::fs::read(pdf)?)?;
            }
        }
```

with a version that resolves each distinct folder once:

```rust
        {
            let _s = tracing::info_span!("reader.upload", docs = targets.len()).entered();
            let mut folders = cloud::FolderIds::new(&cl);
            for (pdf, folder) in &targets {
                let folder_id = folders.get(folder)?;
                cl.replace_in(&folder_id, &cloud::doc_name(pdf)?, std::fs::read(pdf)?)?;
            }
        }
```

- [ ] **Step 2: Wire bujo's only-month loop** (`apps/rmapps/src/bujo.rs`, lines ~146–154)

Replace:

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

with:

```rust
        {
            let _s = tracing::info_span!("bujo.upload", mode = "only_month").entered();
            let mut folders = cloud::FolderIds::new(&cl);
            let target_id = folders.get(&target)?;
            for pdf in &month_pdfs {
                cl.upsert_in(&target_id, &cloud::doc_name(pdf)?, std::fs::read(pdf)?)?;
            }
            for pdf in &extras {
                cl.create_if_missing_in(&target_id, &cloud::doc_name(pdf)?, std::fs::read(pdf)?)?;
            }
        }
```

- [ ] **Step 3: Wire bujo's whole-year loop** (`apps/rmapps/src/bujo.rs`, around lines ~159–166)

Read the block that handles the non-only-month (whole-year) upload — it loops over `paths`/monthly PDFs calling `cl.upsert(&target, …)` inside a `tracing::info_span!("bujo.upload", …)`. Apply the same transform: construct `let mut folders = cloud::FolderIds::new(&cl); let target_id = folders.get(&target)?;` once at the top of the block, then replace each `cl.upsert(&target, name, bytes)` with `cl.upsert_in(&target_id, name, bytes)` and any `cl.create_if_missing(&target, …)` with `cl.create_if_missing_in(&target_id, …)`. Do not change the single-month early-return path at line ~103 (one doc, one resolve — already safe), though converting it for consistency is acceptable.

- [ ] **Step 4: Build and test the whole workspace**

Run: `cargo build -p rmapps`
Expected: clean (no unused-import / dead-code warnings for the old methods — they remain used by digest/push).

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add apps/rmapps/src/reader.rs apps/rmapps/src/bujo.rs
git commit -m "fix(rmapps): resolve reader/bujo destination folders once per run"
```

---

## Self-Review

- **Spec coverage:** id-based methods (Task 1) ✓; `FolderIds` resolver (Task 3) ✓; reader + bujo wiring (Task 4) ✓; fake staleness seam + bug-lock + fix + bujo-guard tests (Tasks 2–3) ✓; non-goals (no cleanup subcommand, no authoritative mkdir resolve) respected — neither appears in any task ✓.
- **Placeholder scan:** every code step shows real code; the only prose-described edit (Task 4 Step 3) names the exact transform and methods because the surrounding block must be read in place — no `TODO`/`TBD`.
- **Type consistency:** `FolderIds::new` / `get`, `upsert_in` / `replace_in` / `create_if_missing_in`, `lag_next_commit`, `arm_lag` / `active_lag` / `lagged_hash` used identically across tasks. `replace_in`/`upsert_in` signatures match their call sites in reader/bujo.
- **Verification scan:** spec requires no user verification → no `requiresUserVerification` task needed.

## Notes on execution

Native task tracking is intentionally kept in the co-located `.tasks.json` rather than `TaskCreate`, because this repo's commit hook blocks `git commit` while native tasks are open (see project memory: "Commit guard vs native tasks"). Each task ends in its own commit per the frequent-commit rule.
