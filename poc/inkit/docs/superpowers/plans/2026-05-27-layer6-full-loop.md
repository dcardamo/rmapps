# Layer 6 — Full agent-driven loop (apps/reader/tests/loop_emitted.rs): implementation plan

> **For agentic workers:** Use superpowers-extended-cc:subagent-driven-development. **Do NOT call `TaskCreate` / `TaskUpdate`** — the plan-file checkboxes are the only tracker. One combined commit at end of layer; FF-merge to `main`; remove worktree.
>
> **Design checkpoint:** This plan resolves real architectural choices about how the reader is "agent-driven" given current inkctl limitations. Before executing Task 2 onward, the reviewer should confirm the **In-process vs subprocess** decision below.

**Goal:** Ship three end-to-end Layer-6 sequences as committed `#[test]`s under `apps/reader/tests/loop_emitted.rs` that drive the reader through the inkapp-harness `Session::step_app` loop using `Session`-level operations (publish, link-follow, ink-tap, step) — matching the spec's three inventory items: happy-path navigation; stale-manifest rejection; offline-connector path. The "agent-driving lens" is exercised by trace-recording the same sequence through `inkctl` for the happy path, then asserting the emitted Rust trace matches what the loop test produces.

**Architecture (the load-bearing decision):**

The spec says: *"Each [sequence] forces `session step` + a reader entry in `inkctl`'s app registry to land — the first known `inkctl` bug we will hit and fix."* Today, `inkctl session step` returns `not_implemented`, and the reader is not in `inkctl::apps::build`. There are two paths to satisfy the spec:

- **(A) In-process loop tests + CLI registry for publish only.** The three `loop_emitted.rs` tests construct a real `App<reader::App, Msg, Connectors>` in-process and call `Session::step_app` directly via the harness Rust API. `inkctl` gets a reader registry entry (`"reader"` → `tests_common::reader_app_fake()`) so an agent can `inkctl document publish <device> reader`; navigation + step is via the Rust API in the emitted tests. The CLI `session step` stub stays `not_implemented`, but the emitted tests cover the loop honestly.
- **(B) Wire CLI `session step` end-to-end.** Each CLI invocation rebuilds the `App<...>` from the registry, loads a serialized `DocSet` from disk, runs `step_app`, persists the new `DocSet`. Requires: `DocSet` serialization, a registry entry that knows how to construct `App<...>` from session state, and a connector-overlay persistence story (`Readwise::persisted(state_dir/connectors/readwise.json)` for the reader).

**This plan picks (A).** Reason: (1) the `loop_emitted.rs` tests are Rust code — they are honest about driving the loop via `Session::step_app` regardless of how an agent records them; (2) `DocSet` serialization is a non-trivial framework change that doesn't unlock new test coverage we don't already have; (3) the CLI's job is to give an agent **a recording surface** for the things it does, not to be the runtime — `Session::step_app` is the runtime. The trace-record + emit-rust path remains: an agent runs `inkctl record start; inkctl document publish; inkctl link-follow ...; inkctl session step ...; inkctl record stop; inkctl record emit-test`, and the emitted Rust calls `session.step_app(...)` directly. To make that work, **we change `inkctl session step` from `not_implemented` to a thin wrapper that does the rebuild-and-step described in (B), but only for apps whose registry entry supplies a `step_handle` builder** (just the reader for now). This lands the spec's "session step + reader registry" without growing `DocSet`'s serde footprint.

So the realized architecture is:
- `inkctl::apps::build_step` (new) returns an optional builder closure `Box<dyn FnOnce(&Session) -> Box<dyn StepDriver>>` per app name. Reader supplies one; smoke / uri-link / multi do not.
- `StepDriver` is a small object trait inside `inkctl` (private to that crate) holding the App + a freshly-rebuilt DocSet. Its `step(...)` calls `Session::step_app` and serializes the new manifest back to disk per doc.
- The CLI `session step` looks up `StepDriver` by `doc.json::app_name`, calls `step`, prints `StepResult` as JSON. Connector overlay persistence: reader uses `Readwise::persisted(state_dir/connectors/readwise.json)` so Move/Delete writes survive across CLI invocations.
- The three `loop_emitted.rs` tests, however, **don't go through the CLI** — they import the harness, build the `App<...>` from `reader_app_fake()`, and call `Session::step_app` directly. This keeps the tests fast (no subprocess), debuggable (panics surface real stack traces), and immune to the DocSet-persistence subtlety in the CLI.

This means Tasks 1, 2, 3 below land the **emitted tests** through the Rust API; Task 4 lands the **CLI `session step` wiring** so an agent can record a session end-to-end; Task 5 lands the reader in inkctl's app registry; Task 6 closes appdx + verify + commit.

**Tech Stack:** Rust, `cargo test`, `inkapp-harness` (`Session::step_app`, `link_follow`, `ink_tap`, `record_*`), `inkapp-readwise-reader` (`Readwise::fake`, `Readwise::persisted`), `inkapp-core` (`App` builder, `DocSet`).

**Spec:** [docs/superpowers/specs/2026-05-27-reader-thorough-test-design.md](../specs/2026-05-27-reader-thorough-test-design.md). Layers 2-5 are shipped; this is Layer 6.

---

## Scope and constraint

**In scope:**
- Three `#[test]`s under `apps/reader/tests/loop_emitted.rs` covering the spec's Layer-6 inventory.
- Wiring `inkctl session step` from `not_implemented` to a working command for the reader.
- Reader entry in `inkctl::apps::build` so `inkctl document publish <did> reader` works.
- `apps/reader/tests/shared.rs` helper `reader_app_fake() -> App<reader::App, Msg, Connectors>` for in-process loop tests.

**Out of scope:**
- DocSet serialization (CLI step uses fresh-rebuild + on-disk manifest reload; full serialization of in-memory entries deferred).
- Visual / PNG-diff assertions.
- Reader feature work (any reader bug uncovered here is filed, not fixed).
- The `inkctl record emit-test` for loop sequences — emission already exists (`inkapp_harness::emit::to_rust`); we don't enhance it.
- A second sequence per inventory item — one is enough.

**Spec inventory mapping for Layer 6:**

| Spec inventory item                                                                | Task |
|------------------------------------------------------------------------------------|------|
| Publish → tap Index entry → step → Article page → NavBand Next → step → ... → Home | Task 2 |
| Stale-manifest path: v1 publish, ink, v2 publish without applying v1's ink, replay v1's ink → version-guard | Task 3 |
| Offline-connector path: cassette in offline mode → Index renders cached state, no crash | Task 4 |
| inkctl `session step` wired up; reader in app registry                              | Tasks 5 + 6 |

**Implementation context the implementer needs:**
- `Session::step_app(&mut self, device, &mut app, &mut set, opts)` already exists and orchestrates the loop fully — see `crates/inkapp-harness/src/session.rs:598`. Use it as-is.
- `Session::ink_tap(device, doc_id, page, region)` writes pending ink under `state_dir/devices/<dev>/pending/<doc_id>/<page>.json`; `step_app` reads from there.
- `Session::link_follow(device, doc_id, page, region)` reads `pdf.pdf` link annotations, finds the one inside `region`, and updates the device cursor's `current_page`. Use this for Index row taps and NavBand Prev/Home/Next.
- The reader's `Index` and `NavBand` emit PDF link annotations. So Index-row "tap" and NavBand "tap" both use `link_follow`, not `ink_tap`.
- The reader's `ActionBand` emits decode-able regions; tapping an action cell uses `ink_tap` with a wide pen stroke, then `step_app` decodes the resulting `Msg::Move`/`Msg::Delete`.
- `readback::guard_version` returns an error from `App::step` when ink references a stale manifest version. Layer 3 already tests this at the unit level; Layer 6 tests it through the full loop.
- For the offline path: `Readwise::with_cache_hydrated` pre-loads the cache from a directory; an offline run uses a `NoopFetch` (or whichever the codebase already has) so `refresh()` doesn't try to hit the network. Check `inkapp_readwise_reader::FetchTransport` for an existing noop variant; if none exists, the test can pre-populate the cache and use the default fake transport (which already does no network work).
- All emitted tests live in `apps/reader/tests/loop_emitted.rs` — one new file; do not split.

---

## Task 1 — `reader_app_fake()` helper

- [ ] **Goal:** Provide one shared in-process App constructor for the loop tests so each test starts from the same canonical state.

**Files:**
- Modify: `apps/reader/tests/shared.rs` (add `reader_app_fake`).

**Acceptance:**
- New helper `pub fn reader_app_fake() -> inkapp_core::runtime::App<reader::App, reader::Msg, reader::Connectors>` exists.
- It returns an App built via `inkapp::app(reader::App)...build()` with `Connectors::fake()` and `Theme::reader()`, mirroring the existing `fake_app` helper.
- Existing `fake_app` stays as-is (it's used by the existing `view_yields_library_document` test).

**Verify:** `nix develop -c cargo test -p reader --test app` (existing tests still compile; `shared.rs` is `#[allow(dead_code)]` so unused-helper warnings are suppressed).

**Steps:**

- [ ] **Step 1: Append to `apps/reader/tests/shared.rs`.**

```rust
/// Construct a reader App backed by the fake Readwise cassette. Differs from
/// `fake_app` in that callers can pass it directly to `Session::step_app`.
pub fn reader_app_fake() -> App<Model, Msg, Connectors> {
    app(Model)
        .connector(Connectors::fake())
        .update(update)
        .view(view)
        .key(Key::from_bytes([0u8; 32]))
        .theme(Theme::reader())
        .build()
}
```

(`fake_app` already has this body — `reader_app_fake` is just a more descriptively-named alias. Keep both; the loop tests use `reader_app_fake` for readability.)

- [ ] **Step 2: Update this plan file** — flip Task 1's checkboxes to `[x]`.

**DO NOT git add / git commit.**

---

## Task 2 — Sequence 1: happy-path agent loop

- [ ] **Goal:** Emit `apps/reader/tests/loop_emitted.rs::happy_path_index_to_article_and_back` that walks publish → Library page 0 (Index) → link_follow to article row a1 → page describe asserts action regions → ink_tap on action-Archive-art-a1 → step_app decodes `Msg::Move{to: Archive}` → re-rendered Library no longer contains a1.

**Files:**
- Create: `apps/reader/tests/loop_emitted.rs`.

**Acceptance:**
- Test `happy_path_index_to_article_and_back` passes.
- After step, `app.connectors().readwise.archived()` contains `a1`.
- After step, the new Library document's pages don't reference `a1` (use `document_source_in` to grep, as in Layer 5).
- NavBand Next + Home navigation is exercised via `Session::link_follow`; cursor's `current_page` updates accordingly. (The test asserts the cursor.json `current_page` value; no Msg is expected because NavBand decode is a no-op — proven in Layer 4.)

**Verify:** `nix develop -c cargo test -p reader --test loop_emitted happy_path`

**Steps:**

- [ ] **Step 1: Create `apps/reader/tests/loop_emitted.rs`.** Use a `tempfile::tempdir` as `state_dir`, instantiate a `Session` via `Session::new_fake(state_dir)`, create a device via `session.device_new(None)`, instantiate the App via `reader_app_fake`, do an initial `app.render(&mut set)` and call `session.document_publish` for each rendered doc. Then drive the loop:

```rust
mod shared;

use inkapp_core::runtime::DocSet;
use inkapp_harness::session::{Session, StepOpts};
use inkapp_readwise_reader::ArticleId;

#[tokio::test]
async fn happy_path_index_to_article_and_back() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut session = Session::new_fake(tmp.path()).await.expect("session");
    let device = session.device_new(None).expect("device");

    let mut app = shared::reader_app_fake();
    let mut set = DocSet::default();

    // Initial render + publish each Document.
    let rendered = app.render(&mut set).await.expect("initial render");
    let mut doc_ids: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for r in &rendered {
        let did = session
            .document_publish(
                &device,
                &r.key.0,
                r.pdf_bytes.clone(),
                r.manifest.clone(),
            )
            .await
            .expect("publish");
        doc_ids.insert(r.key.0.clone(), did);
    }
    let library_doc_id = doc_ids
        .get("Library")
        .expect("Library doc published")
        .clone();

    // Find the Index row region for a1 by listing manifest regions for page 0.
    let (_, manifest_lib) =
        inkapp_harness::observe::load_doc(session.state_dir(), &library_doc_id).unwrap();
    let row_region = manifest_lib
        .regions
        .iter()
        .find(|r| r.page == 0 && r.name.starts_with("row-"))
        .or_else(|| {
            // Index region naming may differ; fall back to "idx-" or whatever
            // the Layer-4 inventory used. Check `Index::render` for the actual
            // shape if neither prefix is present.
            manifest_lib
                .regions
                .iter()
                .find(|r| r.page == 0 && r.name.contains("a1"))
        })
        .expect("index row region for a1 on page 0");

    // link_follow to navigate from Index to Article.
    let follow = session
        .link_follow(&device, &library_doc_id, 0, &row_region.name)
        .expect("link_follow Index row");
    let article_page = follow.target_page.expect("Index row links to article page");

    // Find the action-Archive-{art-id} region on article_page.
    let action_region = manifest_lib
        .regions
        .iter()
        .find(|r| r.page == article_page && r.name.starts_with("action-Archive-"))
        .expect("action-Archive on article page")
        .clone();

    // Tap (wide pen strike across the cell) — uses ink_tap with the region name.
    session
        .ink_tap(&device, &library_doc_id, article_page, &action_region.name)
        .expect("ink_tap action-Archive");

    // Step.
    let step = session
        .step_app(&device, &mut app, &mut set, StepOpts::default())
        .await
        .expect("step_app");

    // Assert: a Msg::Move was decoded for a1.
    assert!(
        step.msgs.iter().any(|m| m.to_string().contains("Archive")
            && m.to_string().contains("a1")),
        "expected a Move{{to:Archive}} for a1 in decoded msgs; got: {:?}",
        step.msgs
    );

    // Connector overlay reflects the mutation.
    assert!(
        app.connectors().readwise.archived().contains(&ArticleId::new("a1")),
        "a1 must be in archived overlay after step"
    );

    // Post-step Library re-render no longer mentions a1.
    let after = app.render(&mut set).await.expect("post-step render");
    let library_after = after
        .iter()
        .find(|r| r.key.0 == "Library")
        .expect("Library re-rendered");
    // Verify by recovering regions and asserting no regions reference a1.
    let names_after: Vec<&str> = library_after
        .manifest
        .regions
        .iter()
        .map(|r| r.name.as_str())
        .collect();
    assert!(
        !names_after.iter().any(|n| n.contains("a1")),
        "post-step Library regions must not reference a1: {names_after:?}"
    );
}
```

**Caveats baked into the sketch the implementer must verify:**
- Whether `App::connectors()` is a public accessor — if not, the test must hold its own `Arc<Readwise>` from the connector builder (Step 1 should pre-construct `Connectors::fake()`, clone the `Arc`, and build the App over that clone so the test can introspect via the kept clone).
- Index row region naming — Layer 4 audited Index inline tests and didn't standardize a prefix; the test must inspect `Index::render` or the manifest to discover the actual shape.
- `inkapp_harness::observe::load_doc` exists and is used by `lens_parity_layer4.rs` — same signature.

- [ ] **Step 2: Run.**

```bash
nix develop -c cargo fmt -p reader
nix develop -c cargo test -p reader --test loop_emitted happy_path
```

- [ ] **Step 3: Update this plan** — flip Task 2's checkboxes to `[x]`.

**DO NOT git add / git commit.**

---

## Task 3 — Sequence 2: stale-manifest rejection

- [ ] **Goal:** Add `stale_manifest_ink_is_rejected` to `loop_emitted.rs`. The flow: publish v1 → ink_tap on v1 → publish v2 (re-render without applying v1's ink, so the manifest version bumps) → call step_app with the v1 ink still pending → expect `step_app` to surface a version-guard error (or a no-op with no Msgs decoded, depending on what `App::step` returns when `guard_version` fails).

**Files:**
- Modify: `apps/reader/tests/loop_emitted.rs` (append).

**Acceptance:**
- Test passes; the decode pass is skipped for v1 ink against a v2 manifest. The exact assertion shape is "no Msg::Move decoded for a1 even though the ink was on the Archive cell" — because the version-guard rejected the ink before decode.
- Existing tests still pass.

**Verify:** `nix develop -c cargo test -p reader --test loop_emitted stale_manifest`

**Steps:**

- [ ] **Step 1: Append.** Mirror the structure of Task 2, but between the `ink_tap` and the `step_app` call, force a re-render that bumps the manifest version: call `app.render(&mut set)` (this re-renders and writes new entries with a higher version) then call `session.document_publish` for the new render (overwriting the stored manifest on disk). Then call `step_app`.

```rust
#[tokio::test]
async fn stale_manifest_ink_is_rejected() {
    // (... same setup as happy_path through ink_tap ...)
    //
    // Force a manifest version bump by re-rendering and re-publishing.
    let v2 = app.render(&mut set).await.expect("re-render v2");
    for r in &v2 {
        let did = doc_ids.get(&r.key.0).expect("doc previously published");
        session
            .document_publish(&device, did, r.pdf_bytes.clone(), r.manifest.clone())
            .await
            .expect("re-publish v2 manifest");
    }

    // Now step. The pending v1 ink references a stale manifest version.
    let step_result = session
        .step_app(&device, &mut app, &mut set, StepOpts::default())
        .await;

    // Two acceptable shapes, per the spec line "version-guard rejection
    // surfaces as the documented Msg/no-op":
    //   (a) step_app returns Err with a version-guard error class, OR
    //   (b) step_app returns Ok but with msgs.is_empty() — decode skipped.
    // The exact shape is decided by reading App::step's current contract;
    // pin whichever holds. If a third shape emerges (e.g. a typed Msg
    // signaling rejection), update App::step contract or the test.
    match step_result {
        Err(_) => {} // (a) acceptable
        Ok(step) => {
            assert!(
                step.msgs.is_empty(),
                "stale ink must not decode to a Msg; got: {:?}",
                step.msgs
            );
        }
    }

    // Either way, the connector overlay is unchanged.
    assert!(
        !app.connectors().readwise.archived().contains(&ArticleId::new("a1")),
        "stale ink must NOT mutate the connector overlay"
    );
}
```

- [ ] **Step 2: Run.**

```bash
nix develop -c cargo test -p reader --test loop_emitted stale_manifest
```

- [ ] **Step 3: Update this plan** — flip Task 3's checkboxes to `[x]`.

**DO NOT git add / git commit.**

---

## Task 4 — Sequence 3: offline-connector path

- [ ] **Goal:** Add `offline_connector_renders_cached_state` to `loop_emitted.rs`. The reader's connector is offline (no network fetch); `app.render` must still produce the Library Document using cached state and not panic.

**Files:**
- Modify: `apps/reader/tests/loop_emitted.rs` (append).

**Acceptance:**
- Test passes; `app.render(&mut set)` succeeds without panic, and the rendered set contains the Library Document.
- Existing tests still pass.

**Verify:** `nix develop -c cargo test -p reader --test loop_emitted offline_connector`

**Steps:**

- [ ] **Step 1: Discover the offline-fetch shape.** Look at `inkapp_readwise_reader::FetchTransport` and `Readwise::with_fetch`. If a `NoopFetch` or equivalent exists, use it. Otherwise the simplest offline-shaped fake is `Readwise::fake().with_fetch(Arc::new(YourOwnNoopFetch))` defined inline in the test (a trait impl that returns an empty `Page`).

- [ ] **Step 2: Append.**

```rust
#[tokio::test]
async fn offline_connector_renders_cached_state() {
    // Build a Readwise whose fetch never returns articles (offline) and whose
    // cache is already populated with the fake cassette articles.
    //
    // (Implementation detail: see Step 1 — either NoopFetch already exists or
    //  is defined inline here.)

    // For the cache: the simplest path is to use `Readwise::fake()` (in-memory
    // articles) and just override the fetch to noop. The fake already has
    // articles in its cache_articles RwLock — refresh() never running just
    // means the cache stays at its construction-time state, which is what we
    // want for "offline + warm cache".

    let readwise = std::sync::Arc::new(
        inkapp_readwise_reader::Readwise::fake()
            .with_fetch(std::sync::Arc::new(/* NoopFetch */)),
    );
    let cx = reader::Connectors { readwise: readwise.clone() };
    let mut app = inkapp::app(reader::App)
        .connector(cx)
        .update(reader::update)
        .view(reader::view)
        .key(inkapp_core::crypto::Key::from_bytes([0u8; 32]))
        .theme(inkapp_core::theme::Theme::reader())
        .build();

    let mut set = inkapp_core::runtime::DocSet::default();
    let rendered = app.render(&mut set).await.expect("offline render succeeds");
    let keys: Vec<&str> = rendered.iter().map(|r| r.key.0.as_str()).collect();
    assert!(
        keys.contains(&"Library"),
        "offline render must still produce Library from warm cache; got: {keys:?}"
    );
}
```

- [ ] **Step 3: Run.**

```bash
nix develop -c cargo test -p reader --test loop_emitted offline_connector
```

- [ ] **Step 4: Update this plan** — flip Task 4's checkboxes to `[x]`.

**DO NOT git add / git commit.**

---

## Task 5 — Reader in `inkctl::apps::build`

- [ ] **Goal:** `inkctl document publish <device> reader` succeeds and writes a PublishedApp built from `Readwise::fake()`.

**Files:**
- Modify: `crates/inkapp-harness/src/tests_common.rs` (add a `reader_publish_fake() -> PublishedApp` builder that wraps `inkapp::app(reader::App)...build()` and then calls `app.render(&mut DocSet::default())` to extract one PDF + manifest. Choose which Document to publish: `"Library"` since the fake cassette only populates Library).
- Modify: `crates/inkctl/src/apps.rs` (register `"reader"` → `tests_common::reader_publish_fake()`).

**Acceptance:**
- `inkctl document publish <did> reader` returns `ok: true` with a `doc_id`.
- `inkctl page describe <doc_id> 0` returns Index regions consistent with the reader's view.

**Verify:** Add `crates/inkctl/tests/smoke_reader_publish.rs` with a single test that publishes and runs `page describe`.

**Steps:**

- [ ] **Step 1: Add the builder to `tests_common.rs`.** This crosses a dependency boundary — `inkapp-harness` would depend on `reader`. Confirm direction is acceptable; if not, define the builder in `crates/inkctl/src/apps.rs` itself (inkctl already depends on reader transitively via app builds). Probably the right home is **`crates/inkctl/src/apps.rs`** so the dependency from harness to reader is avoided.

```rust
// In crates/inkctl/src/apps.rs:
fn reader_publish_fake() -> PublishedApp {
    use inkapp_core::runtime::DocSet;
    use inkapp_core::theme::Theme;
    use inkapp_core::crypto::Key;

    let cx = reader::Connectors::fake();
    let mut app = inkapp::app(reader::App)
        .connector(cx)
        .update(reader::update)
        .view(reader::view)
        .key(Key::from_bytes([0u8; 32]))
        .theme(Theme::reader())
        .build();

    let mut set = DocSet::default();
    // App::render is async; we're sync here. Bridge via futures::executor or
    // require the registry to return a future. The existing registry signature
    // is sync — use a tiny block_on:
    let rendered = futures::executor::block_on(app.render(&mut set))
        .expect("reader fake renders");
    // The reader produces a Library Document; pick that one.
    let library = rendered
        .into_iter()
        .find(|r| r.key.0 == "Library")
        .expect("Library document present from fake cassette");
    PublishedApp {
        app_name: "reader".to_string(),
        pdf_bytes: library.pdf_bytes,
        manifest: library.manifest,
        source_typ: None,
    }
}
```

If `futures::executor::block_on` isn't already a dep of inkctl, prefer changing the registry signature to async (`pub async fn build(...)`); the call sites in `cmd/document.rs` are already async.

- [ ] **Step 2: Register `"reader"` in the match.**

- [ ] **Step 3: Add `crates/inkctl/tests/smoke_reader_publish.rs`** — publish + `page describe 0` round-trip.

- [ ] **Step 4: Run.**

```bash
nix develop -c cargo fmt -p inkctl
nix develop -c cargo test -p inkctl smoke_reader_publish
```

- [ ] **Step 5: Update this plan** — flip Task 5's checkboxes to `[x]`.

**DO NOT git add / git commit.**

---

## Task 6 — `inkctl session step` for the reader

- [ ] **Goal:** Replace the `not_implemented` stub in `cmd/session.rs::step` with a working command for the reader. On invocation: locate the device's pending ink, look up each doc's `app_name`, rebuild an App for any `app_name` we know about (just `"reader"` today), call `Session::step_app`, return the JSON `StepResult`. For apps without a step builder, return a clear `unsupported_app` error citing how to register one.

**Files:**
- Modify: `crates/inkctl/src/cmd/session.rs` (implement `step`).
- Modify: `crates/inkctl/src/apps.rs` (add `pub fn supports_step(name) -> bool` and a sibling `pub async fn step_one_doc(name, session, device, doc_id) -> Result<StepResult, ...>` that does the per-app App construction + step).
- Modify: `crates/inkctl/tests/smoke_session.rs` (update `session_step_is_not_implemented` to a positive test: `session_step_is_noop_when_no_pending_ink`).

**Acceptance:**
- `inkctl session step --device <did>` on a session with reader published and no pending ink returns `ok: true` with `data.cycle: 1, data.msgs: []`.
- `inkctl session step --device <did>` after `inkctl ink tap ... action-Archive-art-a1` returns a `StepResult` whose `msgs` contains a `Move`-shaped entry and whose `pages_changed` lists `"Library"`.
- The pre-existing `smoke_session.rs::session_step_is_not_implemented` test is renamed and rewritten to assert the new positive contract.

**Verify:** `nix develop -c cargo test -p inkctl session_step`

**Steps:**

- [ ] **Step 1: Implement `apps::step_one_doc` in `crates/inkctl/src/apps.rs`.** Walk the session's docs dir, group by app_name from `doc.json`, build the App once per app_name, drive the loop. For the reader, the connector overlay must persist across CLI invocations — use `Readwise::persisted(session.state_dir().join("connectors").join("readwise.json"))` so each CLI invocation reads the same overlay. The DocSet is rebuilt fresh per invocation: call `app.render(&mut set).await` (no-op if state is unchanged) to populate entries before `step_app`. This is correct because the persisted manifest version stays in step with the on-disk one as long as the App's render is deterministic for the same connector state.

- [ ] **Step 2: Wire `cmd/session.rs::step`.**

```rust
async fn step(device: String, session: Option<String>) -> ! {
    let sid = match util::resolve_session_id(session) {
        Ok(s) => s,
        Err(e) => output::print_err("missing_session", e),
    };
    let dir = util::session_dir(&sid);
    let mut s = match Session::open(&dir).await {
        Ok(s) => s,
        Err(e) => output::print_err("io_error", e),
    };
    let dev = DeviceId::new(device);
    let result = crate::apps::step_all_docs(&mut s, &dev).await;
    let _ = s.flush();
    match result {
        Ok(json) => output::print_ok(json),
        Err((kind, msg)) => output::print_err(kind, msg),
    }
}
```

`step_all_docs` walks the device's published docs, groups by app_name, runs one `step_app` per app, and aggregates the StepResults into a single JSON output. (For now, only `"reader"` is supported; others surface `unsupported_app`.)

- [ ] **Step 3: Update `smoke_session.rs`.**

```rust
#[test]
fn session_step_returns_ok_with_no_pending_ink() {
    // Setup: session new + device new + reader publish + step. Expect ok=true,
    // msgs=[], pages_changed unchanged.
    // (Full test body follows the smoke_session.rs pattern.)
}
```

- [ ] **Step 4: Run.**

```bash
nix develop -c cargo fmt -p inkctl
nix develop -c cargo test -p inkctl session_step
```

- [ ] **Step 5: Update this plan** — flip Task 6's checkboxes to `[x]`.

**DO NOT git add / git commit.**

---

## Task 7 — appdx update + workspace verify + one combined commit + merge

- [ ] **Goal:** Add a Layer-6 subsection to `docs/appdx.md`'s "Test coverage by layer", run workspace-wide verify, commit, FF-merge, clean up.

**Files:**
- Modify: `docs/appdx.md`.
- Modify: this plan file.

**Verify (all three before commit):**
- `nix develop -c cargo fmt --check`
- `nix develop -c cargo test --workspace`
- `nix develop -c cargo clippy --all-targets -- -D warnings`

**Steps:**

- [ ] **Step 1: Append Layer-6 bullet to `docs/appdx.md`'s "Test coverage by layer" subsection.**

```markdown
- **Layer 6 — full agent-driven loop (`apps/reader/tests/loop_emitted.rs`).** Covered 2026-05-27.
  - `happy_path_index_to_article_and_back`: publish reader → link-follow
    Index row a1 → `ink_tap` on `action-Archive-art-a1` → `step_app`
    decodes `Msg::Move{to: Archive}` → re-rendered Library no longer
    references a1. Exercises the full publish → navigate → tap →
    decode → mutate → re-render pipeline through `Session::step_app`.
  - `stale_manifest_ink_is_rejected`: publish v1, enqueue ink, force a
    v2 re-render that bumps the manifest version, then `step_app` —
    the pending v1 ink is rejected by `guard_version` (no Msg decoded;
    connector overlay unchanged). Validates the framework's version-
    barrier at the full-loop level.
  - `offline_connector_renders_cached_state`: a `Readwise` with a
    NoopFetch (or equivalent) and a warm cache renders the Library
    Document without panic. Validates the offline-friendly path the
    spec calls out.
  - `inkctl session step` no longer `not_implemented`: wired to look
    up the published app by `doc.json::app_name`, rebuild the App, and
    call `Session::step_app`. Reader entry registered in
    `inkctl::apps::build`. Connector overlay persisted across CLI
    invocations via `Readwise::persisted`. Covered by
    `crates/inkctl/tests/smoke_reader_publish.rs` and the rewritten
    `crates/inkctl/tests/smoke_session.rs`.
```

- [ ] **Step 2: Flip Task 7 checkboxes** to `[x]`.

- [ ] **Step 3: Workspace verify.**

- [ ] **Step 4: Check staged set.** Expected files:

- `M apps/reader/tests/shared.rs`                                          (Task 1)
- `A apps/reader/tests/loop_emitted.rs`                                    (Tasks 2-4)
- `M crates/inkctl/src/apps.rs`                                            (Tasks 5-6)
- `M crates/inkctl/src/cmd/session.rs`                                     (Task 6)
- `A crates/inkctl/tests/smoke_reader_publish.rs`                          (Task 5)
- `M crates/inkctl/tests/smoke_session.rs`                                 (Task 6)
- `M docs/appdx.md`                                                        (Task 7)
- `A docs/superpowers/plans/2026-05-27-layer6-full-loop.md`                (this plan)

Possibly:
- `M crates/inkctl/Cargo.toml` if Task 5/6 needed new deps (`futures`).
- `M crates/inkapp-readwise-reader/src/lib.rs` if Step 1 of Task 4 had to add a `NoopFetch`.

- [ ] **Step 5: Commit.**

```bash
git commit -m "$(cat <<'EOF'
tests(layer-6): full agent-driven loop + inkctl session step

Closes Layer 6 of docs/superpowers/specs/2026-05-27-reader-thorough-test-design.md
(plan: docs/superpowers/plans/2026-05-27-layer6-full-loop.md).

- apps/reader/tests/loop_emitted.rs:
  - happy_path_index_to_article_and_back: publish → link_follow row a1
    → ink_tap action-Archive → step_app → Msg::Move decoded → archived
    → Library re-render no longer references a1.
  - stale_manifest_ink_is_rejected: v1 publish + ink, v2 re-render
    bumps manifest version, step_app rejects v1 ink (guard_version) —
    no Msg, overlay unchanged.
  - offline_connector_renders_cached_state: Readwise with NoopFetch
    renders Library from warm cache without panic.
- inkctl session step wired up: rebuilds the App by doc.json::app_name,
  calls Session::step_app; connector overlay persists via
  Readwise::persisted across CLI invocations. Reader registered in
  inkctl::apps::build. smoke_session::session_step_is_not_implemented
  rewritten to assert the new positive contract.
- docs/appdx.md: Layer-6 subsection added.
EOF
)"
```

- [ ] **Step 6: FF-merge to main + clean up.**

```bash
cd /home/dan/git/inkapp
git checkout main
git merge --ff-only layer-6
git worktree remove .worktrees/layer-6
git branch -d layer-6
```

## Self-review checklist

- No `TBD` / `TODO` / `todo!()` markers committed.
- `loop_emitted.rs` has 3 tests, all passing.
- `inkctl session step` returns `ok: true` for the reader; the legacy `not_implemented` assertion is gone.
- `cargo test --workspace` clean.
- `cargo clippy --all-targets -- -D warnings` clean.
- `cargo fmt --check` clean.
- FF-merge to main succeeded; worktree and branch removed.

## Open design questions worth confirming before Task 2

1. **Index row region naming.** Layer 4 audited `Index` without standardizing a region prefix for rows. If Index entries today render purely as PDF link annotations without a corresponding `#region` block, then `manifest.regions` will NOT contain a per-row region — and the test's `link_follow` call needs the rect of the row instead. In that case the test must locate the row some other way: either by inspecting the Typst source for the entry's vertical position, or by extending `Index` to emit a per-row region (out of scope here). Verify before writing the test.
2. **Connector overlay persistence direction.** Task 6 assumes `Readwise::persisted` with a JSON path under the session dir is acceptable. If Dan prefers ephemeral-only overlay (each CLI invocation starts fresh), drop `persisted` and accept that consecutive CLI `step` invocations won't see each other's overlay mutations. This matters only for live agent-driven sessions, not the `loop_emitted.rs` tests.
3. **`App::connectors()` accessor.** Task 2's assertions assume the App exposes a public `connectors()` getter (or similar). If not, restructure tests to hold an external `Arc<Readwise>` they pass into `Connectors {…}` then keep a clone for introspection.

These are not blockers — they get resolved in flight during Task 2/Task 6. They are listed here so the reviewer can flag any preferred direction up-front.
