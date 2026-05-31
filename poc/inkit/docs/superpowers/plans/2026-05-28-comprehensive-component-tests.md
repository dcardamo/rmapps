# Comprehensive Component Test Suite Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers-extended-cc:subagent-driven-development (recommended) or superpowers-extended-cc:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a three-track comprehensive test suite (automated / synthesizer self-proof / manual real-pen) over the existing inkapp layer-by-layer trust model, with a single `manual-test.toml` source of truth per entry.

**Architecture:** Add a `suite` module to `inkapp-harness` providing `schema`, `build`, `overlay`, `selfproof`, `verify`, `publish`, `reset`, `report`. Add `inkctl suite ...` as a thin clap wrapper. Test corpora (`*.toml`) co-locate with the code they cover under each crate's `tests/suite/` (or `src/components/<name>/tests/suite/`) directory.

**Tech Stack:** Rust 2021, `serde`/`toml` for schema, `typst`/`typst-render`/`lopdf` for doc + overlay, `image` for stroke rasterization, `rm-cloud` (fake feature) for in-process round-trips, `clap` for CLI. Reuses existing `Session::{document_publish, ink_*, step_app, device_sync}` and `inspector::inspect_with_opts` for overlays.

**Reference spec:** `docs/superpowers/specs/2026-05-28-comprehensive-component-tests-design.md`

---

## File structure

New files (all under existing crates):

```
crates/inkapp-harness/src/suite/
  mod.rs            # public re-exports + corpus discovery
  schema.rs         # manual-test.toml types
  build.rs          # toml → published doc (plain instruction docs)
  overlay.rs        # stroke overlay for PDF panels (Track B)
  selfproof.rs      # 3-panel page composition + index page (Track B)
  verify.rs         # decode + expect-eval + checklist parse + rollup
  publish.rs        # idempotent push of doc bytes via DeviceTransport
  reset.rs          # republish clean copies
  report.rs         # JSON + human report + optional _reports PDF
  corpus.rs         # discover() walking tests/suite/ trees

crates/inkctl/src/cmd/suite.rs   # clap subcommand

# Corpora — one .toml per entry, co-located with covered code:
crates/inkapp-core/tests/suite/attribution_boundary.toml   # L3
crates/inkapp-core/tests/suite/multi_page_region_ids.toml  # L3
crates/inkapp-core/src/components/<name>/tests/suite/*.toml  # L4
crates/inkapp-core/tests/suite/mode_axis.toml              # L5
crates/inkapp-core/tests/suite/manifest_version_guard.toml # L5
crates/inkapp-core/tests/suite/pagination_region_stability.toml
crates/inkapp-core/tests/suite/connector_refresh_flush.toml
crates/inkapp-core/tests/suite/manifest_no_secrets.toml
crates/rm-device/tests/suite/coord_transform.toml          # L2
crates/rm-device/tests/suite/rm_parse_fixtures.toml        # L2
apps/reader/tests/suite/reader_loop.toml                   # L6
apps/agenda/tests/suite/agenda_loop.toml                   # L6
```

Modified files:
- `crates/inkapp-harness/src/lib.rs` — add `pub mod suite;`
- `crates/inkapp-harness/Cargo.toml` — add `toml` dep
- `crates/inkctl/src/cmd/mod.rs` — add `pub mod suite;`
- `crates/inkctl/src/main.rs` — wire `Top::Suite`
- `crates/inkctl/Cargo.toml` — no new deps (transitive)
- `docs/appdx.md` — at the end, mark covered behaviors per layer

---

## Task 0: Add `suite::schema` and corpus discovery

**Goal:** Compile a typed `Entry` from a `manual-test.toml` and discover entries on disk.

**Files:**
- Create: `crates/inkapp-harness/src/suite/mod.rs`
- Create: `crates/inkapp-harness/src/suite/schema.rs`
- Create: `crates/inkapp-harness/src/suite/corpus.rs`
- Modify: `crates/inkapp-harness/src/lib.rs` (add `pub mod suite;`)
- Modify: `crates/inkapp-harness/Cargo.toml` (add `toml = "0.8"`)
- Create: `crates/inkapp-harness/tests/suite_schema.rs`
- Create: `crates/inkapp-harness/tests/fixtures/suite/minimal.toml`

**Acceptance criteria:**
- [ ] `Entry::from_toml_str(&str)` returns a fully-typed entry, rejecting unknown keys.
- [ ] Defaults: `tracks = ["A","B","C"]` when omitted; `strict = false` per-case.
- [ ] `corpus::discover(root: &Path)` returns every `*.toml` reachable under `root/tests/suite/` and `root/src/**/tests/suite/`.
- [ ] Round-trip: a hand-written minimal toml parses without error; an unknown top-level key produces `Error::Parse`.

**Verify:** `nix develop -c cargo test -p inkapp-harness --test suite_schema` → 4 passing tests.

**Steps:**

- [ ] **Step 1: Write the failing tests** in `crates/inkapp-harness/tests/suite_schema.rs`:

```rust
use inkapp_harness::suite::schema::{Entry, Track};
use std::path::Path;

#[test]
fn parses_minimal_entry() {
    let src = include_str!("fixtures/suite/minimal.toml");
    let entry = Entry::from_toml_str(src).expect("parse");
    assert_eq!(entry.id, "demo-minimal");
    assert_eq!(entry.layer, 4);
    assert_eq!(entry.tracks, vec![Track::A, Track::B, Track::C]);
    assert_eq!(entry.cases.len(), 1);
    assert_eq!(entry.cases[0].key, "only");
    assert!(!entry.cases[0].strict);
}

#[test]
fn rejects_unknown_top_level_key() {
    let src = "id=\"x\"\nlayer=4\nbogus=1\n[[case]]\nkey=\"k\"\nregion=\"r\"";
    let err = Entry::from_toml_str(src).expect_err("must fail");
    assert!(format!("{err}").contains("bogus"), "got: {err}");
}

#[test]
fn discovers_corpora_recursively() {
    let root = Path::new("tests/fixtures/suite-discover");
    let entries = inkapp_harness::suite::corpus::discover(root).expect("discover");
    let ids: Vec<_> = entries.iter().map(|p| p.file_name().unwrap().to_str().unwrap().to_string()).collect();
    assert!(ids.iter().any(|n| n == "a.toml"));
    assert!(ids.iter().any(|n| n == "b.toml"));
}

#[test]
fn defaults_applied() {
    let src = "id=\"d\"\nlayer=4\n[[case]]\nkey=\"k\"\nregion=\"r\"\ninstruction=\"do\"";
    let entry = Entry::from_toml_str(src).unwrap();
    assert_eq!(entry.tracks, vec![Track::A, Track::B, Track::C]);
}
```

- [ ] **Step 2: Create fixture file** `crates/inkapp-harness/tests/fixtures/suite/minimal.toml`:

```toml
id    = "demo-minimal"
title = "demo"
layer = 4

[[case]]
key         = "only"
region      = "body"
instruction = "Tap inside the box."
synth       = { kind = "tap" }
expect      = { msg = "Tapped" }
```

  Also create fixture tree for discovery test: `crates/inkapp-harness/tests/fixtures/suite-discover/tests/suite/a.toml` and `crates/inkapp-harness/tests/fixtures/suite-discover/src/components/foo/tests/suite/b.toml`, each holding a minimal entry.

- [ ] **Step 3: Run tests to verify they fail (module missing).**

Run: `nix develop -c cargo test -p inkapp-harness --test suite_schema`
Expected: compile error / module missing.

- [ ] **Step 4: Implement the schema** in `crates/inkapp-harness/src/suite/schema.rs`:

```rust
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum Track { A, B, C }

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub id: String,
    #[serde(default)]
    pub title: String,
    pub layer: u8,
    #[serde(default)]
    pub component: Option<String>,
    #[serde(default = "default_tracks")]
    pub tracks: Vec<Track>,
    #[serde(default)]
    pub setup: Option<Setup>,
    #[serde(rename = "case", default)]
    pub cases: Vec<Case>,
}

fn default_tracks() -> Vec<Track> { vec![Track::A, Track::B, Track::C] }

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Setup {
    #[serde(default)]
    pub fixture: Option<String>,
    #[serde(default)]
    pub inline: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub key: String,
    pub region: String,
    #[serde(default)]
    pub instruction: Option<String>,
    #[serde(default)]
    pub synth: Option<Synth>,
    #[serde(default)]
    pub expect: Option<Expect>,
    #[serde(default)]
    pub strict: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Synth {
    pub kind: String,                 // "tap" | "swipe" | "highlight" | "fixture" | "draw"
    #[serde(default)]
    pub target: Option<serde_json::Value>,
    #[serde(default)]
    pub fixture: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Expect {
    pub msg: String,
    #[serde(default)]
    pub args: serde_json::Value,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("parse: {0}")] Parse(String),
    #[error("io: {0}")] Io(#[from] std::io::Error),
}

impl Entry {
    pub fn from_toml_str(src: &str) -> Result<Self, Error> {
        toml::from_str(src).map_err(|e| Error::Parse(e.to_string()))
    }
    pub fn from_toml_path(p: &Path) -> Result<Self, Error> {
        Self::from_toml_str(&std::fs::read_to_string(p)?)
    }
}
```

  In `crates/inkapp-harness/src/suite/mod.rs`:

```rust
pub mod schema;
pub mod corpus;

pub use schema::{Entry, Track, Case, Synth, Expect};
```

  In `crates/inkapp-harness/src/suite/corpus.rs`:

```rust
use std::path::{Path, PathBuf};

pub fn discover(root: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for ent in entries.flatten() {
            let p = ent.path();
            if p.is_dir() {
                if p.file_name().map(|n| n != "target").unwrap_or(true) {
                    stack.push(p);
                }
            } else if p.extension().and_then(|s| s.to_str()) == Some("toml") {
                // Only files under .../tests/suite/ count.
                if p.parent().and_then(|q| q.file_name()).and_then(|s| s.to_str()) == Some("suite")
                    && p.parent().and_then(|q| q.parent()).and_then(|q| q.file_name())
                        .and_then(|s| s.to_str()) == Some("tests")
                {
                    out.push(p);
                }
            }
        }
    }
    out.sort();
    Ok(out)
}
```

  Add `pub mod suite;` to `crates/inkapp-harness/src/lib.rs`. Add `toml = "0.8"` and `thiserror = "1"` (if not present) to `[dependencies]` in `crates/inkapp-harness/Cargo.toml`.

- [ ] **Step 5: Run tests** — all pass.

Run: `nix develop -c cargo test -p inkapp-harness --test suite_schema`
Expected: 4 passed.

- [ ] **Step 6: Commit.**

```bash
git add crates/inkapp-harness/src/suite/ crates/inkapp-harness/src/lib.rs \
        crates/inkapp-harness/Cargo.toml crates/inkapp-harness/tests/suite_schema.rs \
        crates/inkapp-harness/tests/fixtures/suite/ crates/inkapp-harness/tests/fixtures/suite-discover/
git commit -m "suite: schema + corpus discovery"
```

---

## Task 1: `suite::build` — render a Track-C instruction doc

**Goal:** Given an `Entry`, produce a `BuiltDoc` (PDF bytes + Manifest) whose pages render each case's instruction inside its target region, with a per-page checklist band.

**Files:**
- Create: `crates/inkapp-harness/src/suite/build.rs`
- Modify: `crates/inkapp-harness/src/suite/mod.rs`
- Create: `crates/inkapp-harness/tests/suite_build.rs`

**Acceptance criteria:**
- [ ] `build::manual_doc(&Entry)` returns `BuiltDoc { pdf: Vec<u8>, manifest: Manifest, page_for_case: HashMap<String, usize> }`.
- [ ] Every case has a named region matching `case.region` in the manifest.
- [ ] A checklist band exists on the last page with one checkbox region per case keyed `chk-<case.key>` plus one `notes` region.
- [ ] The manifest is the real (encrypted) manifest produced by the standard render pipeline.

**Verify:** `nix develop -c cargo test -p inkapp-harness --test suite_build` → 3 passing tests.

**Steps:**

- [ ] **Step 1: Write failing tests** in `crates/inkapp-harness/tests/suite_build.rs`:

```rust
use inkapp_harness::suite::{build, schema::Entry};

fn entry() -> Entry {
    Entry::from_toml_str(r#"
id = "build-test"
layer = 4
[[case]]
key = "one"
region = "body"
instruction = "Mark here."
expect = { msg = "Marked" }

[[case]]
key = "two"
region = "body2"
instruction = "Mark there."
expect = { msg = "Marked" }
"#).unwrap()
}

#[test]
fn builds_doc_with_named_regions_per_case() {
    let built = build::manual_doc(&entry()).expect("build");
    let regions: Vec<_> = built.manifest.regions().map(|r| r.name.clone()).collect();
    assert!(regions.iter().any(|n| n == "body"));
    assert!(regions.iter().any(|n| n == "body2"));
}

#[test]
fn checklist_band_has_per_case_checkboxes_and_notes() {
    let built = build::manual_doc(&entry()).expect("build");
    let names: Vec<_> = built.manifest.regions().map(|r| r.name.clone()).collect();
    assert!(names.iter().any(|n| n == "chk-one"));
    assert!(names.iter().any(|n| n == "chk-two"));
    assert!(names.iter().any(|n| n == "notes"));
}

#[test]
fn page_for_case_is_populated() {
    let built = build::manual_doc(&entry()).expect("build");
    assert!(built.page_for_case.contains_key("one"));
    assert!(built.page_for_case.contains_key("two"));
}
```

- [ ] **Step 2: Run — fails** (module missing).

- [ ] **Step 3: Implement** `crates/inkapp-harness/src/suite/build.rs`:

  Use `inkapp_core::render::compile_to_document_with_sources` (already used in `observe.rs`) to compile a Typst source built from the entry: each case yields a `#region("body")[Instruction text]` block, plus a final page with `#stack(#checkbox("chk-<key>", "<key>"), …) #region("notes", height: 60pt)[]`. Extract manifest from the rendered frames using the existing pipeline that `Session::document_publish` uses, then serialize to PDF via the same path. Track which page index each `#region(case.region)` lands on by inspecting frame positions.

  Public API:

```rust
use inkapp_core::manifest::Manifest;
use std::collections::HashMap;

pub struct BuiltDoc {
    pub pdf: Vec<u8>,
    pub manifest: Manifest,
    pub page_for_case: HashMap<String, usize>,
}

pub fn manual_doc(entry: &crate::suite::Entry) -> Result<BuiltDoc, crate::suite::schema::Error> { /* … */ }
```

  Re-export from `mod.rs`: `pub mod build;`.

  Implementation notes: a small Typst template assembles the source from `entry.cases` and `entry.setup.inline.unwrap_or("")`; instruction text is wrapped in `#region(case.region)[...]`. The checklist band lives on a final page, each row built from `crate::components::Checkbox` and a freeform `#region("notes")[]`. If the entry sets `setup.fixture`, look it up via `crate::fixtures` (existing) — for v1 only `inline` and the default empty-page fixture are supported; `fixture` field is reserved.

- [ ] **Step 4: Run tests — pass.**

- [ ] **Step 5: Commit.**

```bash
git add crates/inkapp-harness/src/suite/build.rs crates/inkapp-harness/src/suite/mod.rs \
        crates/inkapp-harness/tests/suite_build.rs
git commit -m "suite: build manual instruction docs from Entry"
```

---

## Task 2: `suite::verify` — decode, eval, checklist parse, rollup

**Goal:** Given an `Entry` + a re-fetched doc bundle (PDF + ink), produce a `Rollup` of per-case automated and human verdicts.

**Files:**
- Create: `crates/inkapp-harness/src/suite/verify.rs`
- Modify: `crates/inkapp-harness/src/suite/mod.rs`
- Create: `crates/inkapp-harness/tests/suite_verify.rs`

**Acceptance criteria:**
- [ ] `verify::run(entry, manifest, attributed_ink) -> Rollup` returns per-case `{ automated: Pass|Fail|Skip, human: Pass|Fail|Unmarked, notes_strokes: Vec<Stroke> }`.
- [ ] Stale manifest (version mismatch with attribution input) yields `Rollup::stale` with a clear diagnostic and no per-case verdicts.
- [ ] `expect.args` defaults to non-strict structural match (subset); `strict = true` rejects extras.
- [ ] Checklist marks: `Checkbox::Marked` in `chk-<key>` → `human: Pass`; `Checkbox::ScribbledOut` → `human: Fail`; `Empty` → `human: Unmarked`. Reuses existing `CheckState`.

**Verify:** `nix develop -c cargo test -p inkapp-harness --test suite_verify` → 4 passing tests.

**Steps:**

- [ ] **Step 1: Write failing tests** covering: clean pass, expected/actual msg mismatch, scribble-out → human fail, stale manifest rejection.

```rust
use inkapp_core::ink::{RegionInk, Stroke};
use inkapp_harness::suite::{schema::Entry, verify::{self, Verdict}};

fn entry() -> Entry {
    Entry::from_toml_str(r#"
id="v"
layer=4
[[case]]
key="k"
region="body"
expect = { msg = "Tapped", args = { region = "body" } }
"#).unwrap()
}

#[test] fn automated_pass_when_decoded_msg_matches() { /* synth a tap stroke in "body" region, call verify::run, assert Pass */ }
#[test] fn automated_fail_when_msg_differs() { /* … */ }
#[test] fn human_fail_when_checkbox_scribbled() { /* … */ }
#[test] fn stale_manifest_yields_stale_diagnostic() { /* … */ }
```

  The tests build a minimal manifest + ink directly (no doc round-trip required — the verify path is pure-data).

- [ ] **Step 2: Run — fails.**

- [ ] **Step 3: Implement** `crates/inkapp-harness/src/suite/verify.rs`:

```rust
use crate::suite::Entry;
use inkapp_core::ink::Stroke;
use inkapp_core::manifest::Manifest;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict { Pass, Fail(String), Skip, Unmarked }

#[derive(Debug)]
pub struct CaseResult {
    pub key: String,
    pub automated: Verdict,
    pub human: Verdict,
    pub notes_strokes: Vec<Stroke>,
}

#[derive(Debug)]
pub struct Rollup {
    pub entry_id: String,
    pub cases: Vec<CaseResult>,
    pub stale: Option<String>,
}

pub fn run(entry: &Entry, manifest: &Manifest, ink: &[Stroke], manifest_version_seen: u32) -> Rollup {
    if manifest_version_seen != manifest.version() {
        return Rollup { entry_id: entry.id.clone(), cases: vec![], stale:
            Some(format!("manifest v{} but ink attributed against v{}", manifest.version(), manifest_version_seen)) };
    }
    let mut cases = Vec::new();
    for case in &entry.cases {
        let automated = eval_automated(case, manifest, ink);
        let human = eval_checklist(case, manifest, ink);
        let notes_strokes = collect_region_ink(manifest, ink, "notes");
        cases.push(CaseResult { key: case.key.clone(), automated, human, notes_strokes });
    }
    Rollup { entry_id: entry.id.clone(), cases, stale: None }
}

fn eval_automated(case: &crate::suite::Case, manifest: &Manifest, ink: &[Stroke]) -> Verdict { /* … */ Verdict::Skip }
fn eval_checklist(case: &crate::suite::Case, manifest: &Manifest, ink: &[Stroke]) -> Verdict { /* … */ Verdict::Unmarked }
fn collect_region_ink(manifest: &Manifest, ink: &[Stroke], region: &str) -> Vec<Stroke> { /* … */ vec![] }
```

  `eval_automated`: attribute `ink` to regions via `inkapp_core::readback::attribute`; for the case's region run the synthesized-msg decoder (use the matching component's `decode` impl based on `case.region` content + the case's expected msg type — for v1 we drive this with a small `App` model the entry implies; entry-level `setup` is enough for now to know which decoder to call).

  `eval_checklist`: locate region `chk-<case.key>`; reuse `Checkbox::read(ink, manifest)` (existing) to get `CheckState`; map to `Verdict`.

  `expect` matching: deep-merge JSON subset check (msg name equality; `args` is a JSON object where every key in `expect.args` must equal the corresponding key in the decoded msg; if `strict` then no extra keys allowed in the decoded msg).

- [ ] **Step 4: Run tests — pass.**

- [ ] **Step 5: Commit.**

```bash
git add crates/inkapp-harness/src/suite/verify.rs crates/inkapp-harness/src/suite/mod.rs \
        crates/inkapp-harness/tests/suite_verify.rs
git commit -m "suite: verify (decode + expect-eval + checklist + stale guard)"
```

---

## Task 3: `suite::publish` and `suite::reset` (in-process Session path)

**Goal:** Publish a `BuiltDoc` to a Session's fake cloud under a chosen path, and reset (republish a clean copy) idempotently.

**Files:**
- Create: `crates/inkapp-harness/src/suite/publish.rs`
- Create: `crates/inkapp-harness/src/suite/reset.rs`
- Modify: `crates/inkapp-harness/src/suite/mod.rs`
- Create: `crates/inkapp-harness/tests/suite_publish.rs`

**Acceptance criteria:**
- [ ] `publish::push(&mut Session, &BuiltDoc, remote_path: &str) -> std::io::Result<DocId>` uploads PDF bytes via `Session::document_publish` and stores the `manual-test.toml` entry id in the doc record.
- [ ] Republish with identical content → same `DocId`, no new cloud generation (idempotent).
- [ ] `reset::clean(&mut Session, &Entry) -> Result<()>` republishes a freshly built doc, wiping ink. Refuses if `pending_ink` is present unless `force: true`.

**Verify:** `nix develop -c cargo test -p inkapp-harness --test suite_publish` → 3 passing tests using the in-process fake cloud.

**Steps:**

- [ ] **Step 1: Write failing tests** that spin up `Session::new_fake`, call `suite::build::manual_doc`, push via `publish::push`, then verify the doc is reachable via `Session::pending_ink` and the cloud snapshot.

- [ ] **Step 2: Run — fails.**

- [ ] **Step 3: Implement** by composing existing `Session::document_publish` (it already returns a `DocId` and writes to the fake cloud). For idempotency, compute `sha256(pdf)`; record under `state_dir/suite/<entry-id>.json`; skip republish when hash matches the last push. `reset::clean` checks `Session::pending_ink(doc_id, page).is_empty()` for each page, errors otherwise unless `force`.

- [ ] **Step 4: Run — pass.**

- [ ] **Step 5: Commit.**

```bash
git add crates/inkapp-harness/src/suite/publish.rs crates/inkapp-harness/src/suite/reset.rs \
        crates/inkapp-harness/src/suite/mod.rs crates/inkapp-harness/tests/suite_publish.rs
git commit -m "suite: publish + reset (idempotent, ink-aware)"
```

---

## Task 4: `inkctl suite` CLI surface (publish, verify, reset, status)

**Goal:** Thin clap subcommand wired into `inkctl` so the agent can drive the suite end-to-end.

**Files:**
- Create: `crates/inkctl/src/cmd/suite.rs`
- Modify: `crates/inkctl/src/cmd/mod.rs` (add `pub mod suite;`)
- Modify: `crates/inkctl/src/main.rs` (add `Top::Suite`)
- Create: `crates/inkctl/tests/smoke_suite.rs`

**Acceptance criteria:**
- [ ] `inkctl suite publish --track manual --id <entry-id>` builds and pushes a single entry; emits `{ ok: true, data: { doc_id, remote_path } }`.
- [ ] `inkctl suite verify --all` returns a JSON rollup over every doc currently under `/inkapp-tests/manual/`.
- [ ] `inkctl suite reset <entry-id>` republishes cleanly; refuses with `error.kind = "has_ink"` if ink present and `--force` not set.
- [ ] `inkctl suite status` lists entries on disk and their last-publish / last-verify times.

**Verify:** `nix develop -c cargo test -p inkctl --test smoke_suite` → 4 passing tests using an isolated `INKCTL_HOME` tempdir.

**Steps:**

- [ ] **Step 1: Write failing smoke tests** that invoke the CLI binary against a tempdir-backed session and parse the JSON output. Pattern matches `crates/inkctl/tests/smoke_session.rs`.

- [ ] **Step 2: Run — fails.**

- [ ] **Step 3: Implement** `crates/inkctl/src/cmd/suite.rs`:

```rust
use clap::Subcommand;
use crate::{output, util};

#[derive(clap::Args)]
pub struct Args {
    #[arg(long, global = true)] session: Option<String>,
    #[command(subcommand)] cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    Publish { #[arg(long)] track: String, #[arg(long, conflicts_with_all=["layer","all"])] id: Option<String>,
              #[arg(long, conflicts_with="all")] layer: Option<u8>, #[arg(long)] all: bool },
    Verify  { #[arg(long)] all: bool, #[arg(long)] track: Option<String> },
    Reset   { id: String, #[arg(long)] force: bool },
    Status  { },
}

pub async fn run(args: Args) -> ! { /* …compose inkapp_harness::suite::{build, publish, verify, reset}, emit output via crate::output… */ }
```

  Wire `Top::Suite(a) => cmd::suite::run(a).await` in `main.rs`.

- [ ] **Step 4: Run — pass.**

- [ ] **Step 5: Commit.**

```bash
git add crates/inkctl/src/cmd/suite.rs crates/inkctl/src/cmd/mod.rs crates/inkctl/src/main.rs \
        crates/inkctl/tests/smoke_suite.rs
git commit -m "inkctl: suite subcommand (publish/verify/reset/status)"
```

---

## Task 5: First worked example — one L3 attribution entry, Track C end-to-end

**Goal:** Prove the Track C path with one real entry before broadening. Worked example also validates the round-trip in-process (build → publish → synth ink → device sync → verify).

**Files:**
- Create: `crates/inkapp-core/tests/suite/attribution_boundary.toml`
- Create: `crates/inkapp-core/tests/suite/mod.rs` placeholder? — *no*: keep `*.toml` only; runner is in `inkapp-harness`.
- Create: `crates/inkapp-core/tests/attribution_boundary_e2e.rs`

**Acceptance criteria:**
- [ ] Hand-written test loads the entry, builds the doc, publishes to the fake cloud, synthesizes ink via `Session::ink_draw` per the entry's `synth`, syncs, re-fetches, runs `verify::run`, and asserts `automated == Pass` for every case.
- [ ] Entry covers: stroke inside region, stroke fully outside region, stroke straddling boundary.

**Verify:** `nix develop -c cargo test -p inkapp-core --test attribution_boundary_e2e` → 1 passing test (3 cases inside).

**Steps:**

- [ ] **Step 1: Author** `crates/inkapp-core/tests/suite/attribution_boundary.toml`:

```toml
id    = "l3-attribution-boundary"
title = "stroke containment vs region bounds"
layer = 3
tracks = ["A", "C"]                  # Track B comes in Task 7

[setup]
inline = """
#region("body", height: 200pt)[
  This box is the body region.
]
#region("outside", height: 100pt)[
  Strokes here should attribute to "outside".
]
"""

[[case]]
key = "fully-inside"
region = "body"
instruction = "Draw a short stroke entirely inside the body box."
synth = { kind = "draw", target = { path = "10,10 90,90", inside = "body" } }
expect = { msg = "InkAttributed", args = { region = "body" } }

[[case]]
key = "fully-outside"
region = "body"
instruction = "Draw a stroke entirely outside the body box (above it)."
synth = { kind = "draw", target = { path = "10,-50 90,-30", inside = "outside" } }
expect = { msg = "InkAttributed", args = { region = "outside" } }

[[case]]
key = "straddling"
region = "body"
instruction = "Draw a stroke that starts inside the body and ends below it."
synth = { kind = "draw", target = { path = "10,180 90,250" } }
expect = { msg = "InkAttributed", args = { region_majority = "body" } }
```

  The test app for this layer is a minimal "attribution echo" app that emits `InkAttributed { region, region_majority }` from each stroke's containment.

- [ ] **Step 2: Write** `crates/inkapp-core/tests/attribution_boundary_e2e.rs`:

```rust
#[tokio::test]
async fn attribution_boundary_e2e() {
    let tmp = tempfile::tempdir().unwrap();
    let mut session = inkapp_harness::session::Session::new_fake(tmp.path()).await.unwrap();
    let entry = inkapp_harness::suite::Entry::from_toml_path(
        "tests/suite/attribution_boundary.toml".as_ref()).unwrap();
    let built = inkapp_harness::suite::build::manual_doc(&entry).unwrap();
    let doc_id = inkapp_harness::suite::publish::push(&mut session, &built, "/inkapp-tests/manual/l3-attribution-boundary.pdf").await.unwrap();
    // synth ink per cases:
    for case in &entry.cases {
        // Drive Session::ink_draw with case.synth.target.path coords (in PDF pt within the case region).
    }
    session.device_sync(&device_id).await.unwrap();
    let snap = session.observe_doc(&doc_id).await.unwrap(); // pseudo-API; use observe::*
    let rollup = inkapp_harness::suite::verify::run(&entry, &built.manifest, snap.strokes(), snap.manifest_version());
    for case in rollup.cases {
        assert_eq!(case.automated, inkapp_harness::suite::verify::Verdict::Pass, "{}", case.key);
    }
}
```

- [ ] **Step 3: Run — fails / passes; iterate on `eval_automated` until green.**

- [ ] **Step 4: Commit.**

```bash
git add crates/inkapp-core/tests/suite/attribution_boundary.toml \
        crates/inkapp-core/tests/attribution_boundary_e2e.rs
git commit -m "suite: L3 attribution boundary, end-to-end Track A/C path"
```

---

## Task 6: `suite::overlay` — PDF-aware stroke overlay for Track B

**Goal:** Given a base page raster + a set of `Stroke`s + a region rect, produce an RGBA image with strokes drawn in a contrasting color, suitable for embedding into the Track B self-proof PDF.

**Files:**
- Create: `crates/inkapp-harness/src/suite/overlay.rs`
- Create: `crates/inkapp-harness/tests/suite_overlay.rs`
- Modify: `crates/inkapp-harness/src/suite/mod.rs`

**Acceptance criteria:**
- [ ] `overlay::draw_strokes(base: &mut RgbaImage, strokes: &[Stroke], rect: PdfRect, color: Rgba<u8>)` draws each stroke in the supplied color clipped to `rect`.
- [ ] `overlay::region_outlines(base: &mut RgbaImage, manifest: &Manifest, page: usize, color: Rgba<u8>)` draws bbox outlines for every region on that page.
- [ ] Output is deterministic: identical inputs → identical bytes (golden image test).

**Verify:** `nix develop -c cargo test -p inkapp-harness --test suite_overlay` → 2 passing tests including one golden comparison.

**Steps:**

- [ ] **Step 1: Write failing tests** including a golden under `crates/inkapp-harness/tests/golden/suite_overlay_basic.png`.

- [ ] **Step 2: Run — fails.**

- [ ] **Step 3: Implement** by lifting the existing line-rasterizer from `inkapp_harness::inspector` (`inspect_with_opts` already draws synth strokes — extract its private drawing helpers into `suite::overlay` as `pub(crate)`).

- [ ] **Step 4: Run — pass.**

- [ ] **Step 5: Commit.**

```bash
git add crates/inkapp-harness/src/suite/overlay.rs crates/inkapp-harness/src/suite/mod.rs \
        crates/inkapp-harness/tests/suite_overlay.rs crates/inkapp-harness/tests/golden/suite_overlay_basic.png
git commit -m "suite: stroke + region-bbox overlay (lifted from inspector)"
```

---

## Task 7: `suite::selfproof` — 3-panel Track B PDF + first self-proof doc

**Goal:** Produce a publishable PDF where each case occupies one page laid out as (original / synth-overlay / decoded), plus a summary index page. Land the L3 attribution self-proof doc as the first real artifact — the trust-earning moment.

**Files:**
- Create: `crates/inkapp-harness/src/suite/selfproof.rs`
- Modify: `crates/inkapp-harness/src/suite/mod.rs`
- Modify: `crates/inkapp-core/tests/suite/attribution_boundary.toml` (add `"B"` to `tracks`)
- Create: `crates/inkapp-harness/tests/suite_selfproof.rs`

**Acceptance criteria:**
- [ ] `selfproof::build(&Entry, &Synthesized) -> BuiltDoc` produces a PDF with N+1 pages where N = case count.
- [ ] Each case page has three labeled panels (Original / Synthesized ink overlay / Decoded) and a checklist row (`chk-<key>` with "looks right" / "looks wrong" checkboxes + `notes`).
- [ ] Final page is a summary index keyed by case id, showing automated PASS/FAIL.
- [ ] Self-proof doc for L3 attribution renders, publishes via `suite::publish::push` to `/inkapp-tests/self-proof/L3.pdf`, and round-trips through verify.

**Verify:**
- `nix develop -c cargo test -p inkapp-harness --test suite_selfproof` → 2 passing tests (page count, regions present).
- `nix develop -c cargo test -p inkapp-core --test attribution_boundary_e2e` → still green; new self-proof publish step covered.

**Steps:**

- [ ] **Step 1: Write failing tests** asserting page count, presence of `chk-<key>` regions, and presence of an `index` region on the last page.

- [ ] **Step 2: Run — fails.**

- [ ] **Step 3: Implement** `selfproof::build` by:
  - Rendering the underlying doc once via `build::manual_doc` to get region rects.
  - For each case: rasterize the source page (`inspector::render_page`), crop to the case region, then produce three sub-images (Panel 1 plain; Panel 2 overlay via `suite::overlay`; Panel 3 a generated text PNG with msg/expected/PASS-FAIL). Compose into a single PDF page via `lopdf`.
  - Final summary page: a Typst-rendered table.

- [ ] **Step 4: Run — pass.**

- [ ] **Step 5: Commit.**

```bash
git add crates/inkapp-harness/src/suite/selfproof.rs crates/inkapp-harness/src/suite/mod.rs \
        crates/inkapp-core/tests/suite/attribution_boundary.toml \
        crates/inkapp-harness/tests/suite_selfproof.rs
git commit -m "suite: Track B selfproof renderer + L3 attribution self-proof doc"
```

---

## Task 8: `suite::report` — JSON + human report, optional on-device PDF

**Goal:** Turn a `Rollup` (or many) into machine + human output and, optionally, a `_reports/<ts>.pdf` published back to the device.

**Files:**
- Create: `crates/inkapp-harness/src/suite/report.rs`
- Modify: `crates/inkapp-harness/src/suite/mod.rs`
- Create: `crates/inkapp-harness/tests/suite_report.rs`

**Acceptance criteria:**
- [ ] `report::to_json(rollups) -> serde_json::Value` includes per-entry, per-case `{automated, human, stale}` plus an aggregate summary.
- [ ] `report::to_human(rollups) -> String` is a plaintext table that fits 110 cols, padded so pipes align (per CLAUDE.md).
- [ ] `report::to_device_pdf(rollups) -> Vec<u8>` produces a small PDF readable on the device.

**Verify:** `nix develop -c cargo test -p inkapp-harness --test suite_report` → 3 passing tests including a golden human-report snapshot.

**Steps:**
- [ ] **Step 1:** failing tests with golden under `tests/golden/suite_report.txt`.
- [ ] **Step 2:** run — fails.
- [ ] **Step 3:** implement; human renderer pads columns; PDF renderer uses a tiny Typst template.
- [ ] **Step 4:** run — pass.
- [ ] **Step 5:** commit `"suite: report (json + human + device pdf)"`.

---

## Task 9: L2 corpus — coord transform + .rm fixture

**Goal:** Bring L2 into the suite pattern.

**Files:**
- Create: `crates/rm-device/tests/suite/coord_transform.toml`
- Create: `crates/rm-device/tests/suite/rm_parse_fixtures.toml`
- Create: `crates/rm-device/tests/suite_l2_e2e.rs`

**Acceptance criteria:**
- [ ] Two `.toml` entries covering: PDF pt ↔ device px round-trip at default + non-default page geometry; one `.rm` v6 fixture decode landing in the expected region.
- [ ] One hand-written `#[test]` exercising both entries via `suite::run_entry` (in-process).

**Verify:** `nix develop -c cargo test -p rm-device --test suite_l2_e2e` → 2 passing tests.

**Steps:** standard TDD pattern — write entry, write test, iterate. Commit `"suite: L2 coord-transform + rm-parse entries"`.

---

## Task 10: L4 component corpus — instruction-only entries for every interactive component

**Goal:** One `.toml` per component covering each interaction it claims. No code changes per component; only data + a shared runner test.

**Files:**
- Create:
  - `crates/inkapp-core/src/components/heading/tests/suite/heading.toml`
  - `crates/inkapp-core/src/components/section/tests/suite/section.toml`
  - `crates/inkapp-core/src/components/action_band/tests/suite/action_band.toml`
  - `crates/inkapp-core/src/components/nav_band/tests/suite/nav_band.toml`
  - `crates/inkapp-core/src/components/index/tests/suite/index.toml`
  - `crates/inkapp-core/src/components/gesture/tests/suite/gesture.toml`
  - `crates/inkapp-core/src/components/highlight_text/tests/suite/single_word.toml`
  - `crates/inkapp-core/src/components/highlight_text/tests/suite/multi_word.toml`
  - `crates/inkapp-core/src/components/highlight_text/tests/suite/line_spanning.toml`
  - `crates/inkapp-core/src/components/highlight_text/tests/suite/paragraph_spanning.toml`
  - `crates/inkapp-core/src/components/checkbox/tests/suite/checkbox.toml`
- Create: `crates/inkapp-core/tests/suite_l4_runner.rs`

**Acceptance criteria:**
- [ ] Every interactive component above has at least one entry, listing every interaction the component claims.
- [ ] The runner walks `suite::corpus::discover("crates/inkapp-core")`, filters `layer == 4`, runs each through Track A in-process, asserts every case `automated == Pass`.

**Verify:** `nix develop -c cargo test -p inkapp-core --test suite_l4_runner` → N passing assertions (one per case across all L4 entries).

**Steps:** author each `.toml`; iterate runner; commit `"suite: L4 component corpora"`.

---

## Task 11: L4 self-proof PDF rollup

**Goal:** A single `L4-components.pdf` covering every L4 entry's cases, publishable for visual inspection on device.

**Files:**
- Modify: `crates/inkapp-harness/src/suite/selfproof.rs` (add `build_layer_rollup`)
- Create: `crates/inkapp-harness/tests/suite_selfproof_layer.rs`

**Acceptance criteria:**
- [ ] `selfproof::build_layer_rollup(layer: u8, entries: &[Entry]) -> BuiltDoc` concatenates per-entry pages with a section header per entry, single trailing summary.
- [ ] `inkctl suite publish --track self-proof --layer 4` produces an artifact under `/inkapp-tests/self-proof/L4-components.pdf`.

**Verify:** `nix develop -c cargo test -p inkapp-harness --test suite_selfproof_layer` → 2 passing tests. Plus `inkctl suite publish --track self-proof --layer 4` against an isolated `INKCTL_HOME` tempdir, asserted in `crates/inkctl/tests/smoke_suite.rs`.

**Steps:** standard TDD; commit `"suite: L4 self-proof rollup PDF"`.

---

## Task 12: L5 framework corpus

**Goal:** Cover mode-axis, manifest version guard, pagination region stability, connector refresh/flush ordering, encrypted manifest round-trip, secret isolation.

**Files:**
- Create:
  - `crates/inkapp-core/tests/suite/mode_axis.toml`
  - `crates/inkapp-core/tests/suite/manifest_version_guard.toml`
  - `crates/inkapp-core/tests/suite/pagination_region_stability.toml`
  - `crates/inkapp-core/tests/suite/connector_refresh_flush.toml`
  - `crates/inkapp-core/tests/suite/manifest_no_secrets.toml`
- Create: `crates/inkapp-core/tests/suite_l5_runner.rs`

**Acceptance criteria:**
- [ ] All five entries run through Track A green.
- [ ] `manifest_version_guard.toml` deliberately mutates the manifest version between publish and ink synthesis; `verify::run` returns `stale = Some(_)` and the test asserts on the diagnostic text.
- [ ] `manifest_no_secrets.toml` round-trips the manifest through serialization and asserts no key matches `r"(?i)token|secret|password|key"` via a small helper.

**Verify:** `nix develop -c cargo test -p inkapp-core --test suite_l5_runner` → 5 passing tests.

**Steps:** standard; commit `"suite: L5 framework corpora"`.

---

## Task 13: L5 self-proof PDF rollup

**Goal:** Mirror Task 11 for L5.

**Files:**
- Create: `crates/inkapp-harness/tests/suite_selfproof_l5.rs`
- (No code beyond Task 11's `build_layer_rollup` — call with `layer = 5`.)

**Acceptance criteria:**
- [ ] `inkctl suite publish --track self-proof --layer 5` produces `L5-framework.pdf` covering every L5 entry.

**Verify:** smoke test in `crates/inkctl/tests/smoke_suite.rs`. Commit `"suite: L5 self-proof rollup"`.

---

## Task 14: L6 end-to-end samples + Track C reader sample

**Goal:** One full-loop entry per app + one manual sample on the reader.

**Files:**
- Create: `apps/reader/tests/suite/reader_loop.toml` (Tracks A + C)
- Create: `apps/agenda/tests/suite/agenda_loop.toml` (Track A)
- Create: `apps/reading-queue/tests/suite/reading_queue_loop.toml` (Track A)
- Create: `apps/reader/tests/suite_loop_e2e.rs`
- Create: `apps/agenda/tests/suite_loop_e2e.rs`
- Create: `apps/reading-queue/tests/suite_loop_e2e.rs`

**Acceptance criteria:**
- [ ] Each app's loop entry exercises one canonical user sequence (open doc → tap something → see msg → re-render).
- [ ] `reader_loop.toml` includes Track C (one manual sample): publish reader doc, instruct human to mark up a passage, verify after sync-back.
- [ ] Per-app `#[test]` runs Track A in-process and asserts green.

**Verify:** `nix develop -c cargo test -p reader -p agenda -p reading-queue --test suite_loop_e2e` → 3 passing tests. Commit `"suite: L6 app-loop entries + reader Track C sample"`.

---

## Task 15: Update `docs/appdx.md`

**Goal:** Per project convention, reconcile appdx by marking covered layers/components.

**Files:**
- Modify: `docs/appdx.md`

**Acceptance criteria:**
- [ ] Components and behaviors covered by the corpus are marked as covered (per the appdx convention used in prior specs).
- [ ] A short pointer to the spec + plan is added under the relevant section.

**Verify:** `git diff --stat docs/appdx.md` shows only that file changed. Commit `"appdx: comprehensive component test suite landed"`.

---

## Notes for the implementer

- **No `Cargo.lock` in commits.** Per project convention.
- **`make clippy` must stay clean.** Warnings are errors.
- **Idempotency:** Track A is the only track CI runs; `inkctl suite publish` is invoked by humans/agents, never CI.
- **Test isolation:** Every harness test uses `tempfile::tempdir()` for `INKCTL_HOME` / `state_dir`.
- **Tasks 0–4 are strictly sequential.** Tasks 5–8 require 0–4. Tasks 9, 10, 12 can run in parallel once 8 is green. Tasks 11 and 13 depend on 10 and 12 respectively. Task 14 depends on 11+13. Task 15 is last.
