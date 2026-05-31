# Layer 5 — Reader composition (apps/reader/src/lib.rs update + view): implementation plan

> **For agentic workers:** Use superpowers-extended-cc:subagent-driven-development. **Do NOT call `TaskCreate` / `TaskUpdate`** — the plan-file checkboxes are the only tracker. The session-scoped pre-commit hook blocks per-task commits when native tasks are pending, so we use plain checkboxes instead. One combined commit at end of layer; FF-merge to `main`; remove worktree.

**Goal:** Cover the reader's `update` exhaustively (one test per `Msg` variant) and the `view` function's branches (canonical-state Library; failed-write banner; empty connector state) that the existing 3 tests in `apps/reader/tests/app.rs` leave unverified. The reader's `Model` is the unit struct `App`, so the spec's "Connector wiring: a `RefreshDone` `Msg` produces the expected model delta" item resolves here to a different shape: there is no internal model delta, so the contract is "`view` reflects the connector's current state, and `update` mutates the connector." Both halves get a dedicated assertion.

**Architecture:** Tests extend `apps/reader/tests/app.rs` (the natural home, where the existing view tests already live). No new files, no new framework surface. Helpers from `apps/reader/tests/shared.rs` (`fake_app`) are reused where useful; most new tests construct a `Connectors::fake()` directly because they don't need the full `App` runtime — they just call `update(msg, &mut App, &cx)` synchronously and then introspect `cx.readwise` and/or `view(&App, &cx)`.

**Tech Stack:** Rust, `cargo test`, `apps/reader`, `inkapp-readwise-reader` (`Readwise::fake`, `ScriptedTransport::always_failing`, `Connector::flush`).

**Spec:** [docs/superpowers/specs/2026-05-27-reader-thorough-test-design.md](../specs/2026-05-27-reader-thorough-test-design.md). Layers 2 + 3 + 4 are shipped; this is Layer 5. Layer 6 follows.

---

## Scope and constraint

**In scope:** `apps/reader/src/lib.rs`'s `update` (3 Msg variants) and `view` (canonical state, banner branch, empty branch, post-mutation reflection). The reader has no `RefreshDone` Msg — see Goal above for how that spec line is interpreted.

**Out of scope:** any reader feature change; rendering pixel diffs; the `loop_emitted.rs` sequences (Layer 6); the `inkctl session step` work and reader-app-registry entry (also Layer 6).

**Spec inventory mapping for Layer 5:**

| Spec inventory item                                                       | Status                                | Task |
|---------------------------------------------------------------------------|---------------------------------------|------|
| `update` exhaustively tested per `Msg` variant                            | 0 tests today                         | Task 1 |
| `view` produces a non-empty doc set for canonical fixture state           | 2 existing tests cover Library + region recovery | (covered) |
| `view` empty branch (no articles in any location)                         | not covered                           | Task 2 |
| `view` failed-write banner branch                                         | not covered                           | Task 2 |
| Connector-wiring contract (`view` reflects `update` mutations)            | not covered                           | Task 3 |

**Implementation context the implementer needs:**
- The fake cassette returned by `Readwise::fake()` contains two articles with `Location::New` (defaults). `library()` returns both; `feed()` returns nothing.
- `Connectors::fake()` wraps the Readwise in an `Arc`, so `cx.readwise.delete(...)` and `cx.readwise.move_to(...)` mutate the same instance the `view` reads back from. No additional plumbing needed.
- To exercise the banner branch, inject `ScriptedTransport::always_failing()` into a custom `Readwise` via `with_transport(Arc::new(...))` and call `Connector::flush(&readwise).await` `MAX_ATTEMPTS` (= 3) times after enqueuing a write. After three failed pushes, the pending write is moved to `overlay.failed`, which is what `failed_writes()` returns.
- `Connector::flush` is on the `Connector` trait, not inherent — bring `inkapp_core::connector::Connector` into scope to call `flush(&*cx.readwise).await`.
- Tests using `flush` need `#[tokio::test]`; pure synchronous tests stay `#[test]`.
- All new tests live in `apps/reader/tests/app.rs` (extend, do not split into a new file).

---

## Task 1 — Exhaustive `update` per `Msg` variant

- [x] **Goal:** Three tests, one per Msg variant. Each calls `update(msg, &mut App, &cx)` and asserts the connector mutation that variant is contracted to perform.

**Files:**
- Modify: `apps/reader/tests/app.rs` (append).

**Acceptance:**
- New test `update_highlighted_records_highlight` passes; after `Msg::Highlighted { article: a1, text: "note" }`, `cx.readwise.highlights(&a1)` contains `"note"`.
- New test `update_move_archive_optimistically_hides_article` passes; after `Msg::Move { article: a1, to: Location::Archive }`, `cx.readwise.archived()` contains `a1` and `cx.readwise.library()` no longer contains `a1`.
- New test `update_delete_optimistically_hides_article` passes; after `Msg::Delete { article: a1 }`, `cx.readwise.archived()` contains `a1` and `cx.readwise.library()` no longer contains `a1`.
- Existing 3 tests still pass.

**Verify:** `nix develop -c cargo test -p reader --test app`

**Steps:**

- [x] **Step 1: Append to `apps/reader/tests/app.rs`.** Imports needed at the top of the file (if not already present): `reader::{update, Msg, App, Connectors}`, `inkapp_readwise_reader::{ArticleId, Location}`.

```rust
#[test]
fn update_highlighted_records_highlight() {
    let cx = Connectors::fake();
    let id = ArticleId::new("a1");
    let mut model = App;
    update(
        Msg::Highlighted {
            article: id.clone(),
            text: "the slow web".into(),
        },
        &mut model,
        &cx,
    );
    let hs = cx.readwise.highlights(&id);
    assert!(
        hs.iter().any(|t| t == "the slow web"),
        "highlight not recorded; got: {hs:?}"
    );
}

#[test]
fn update_move_archive_optimistically_hides_article() {
    let cx = Connectors::fake();
    let id = ArticleId::new("a1");

    // Pre-condition: a1 is in the Library (fake cassette default).
    let before: Vec<String> = cx.readwise.library().into_iter().map(|a| a.id.0).collect();
    assert!(
        before.iter().any(|s| s == "a1"),
        "fake cassette must seed a1 in Library; got: {before:?}"
    );

    let mut model = App;
    update(
        Msg::Move {
            article: id.clone(),
            to: Location::Archive,
        },
        &mut model,
        &cx,
    );

    assert!(
        cx.readwise.archived().contains(&id),
        "a1 must appear in archived overlay after Move{{to: Archive}}"
    );
    let after: Vec<String> = cx.readwise.library().into_iter().map(|a| a.id.0).collect();
    assert!(
        !after.iter().any(|s| s == "a1"),
        "a1 must no longer appear in Library after archive; got: {after:?}"
    );
}

#[test]
fn update_delete_optimistically_hides_article() {
    let cx = Connectors::fake();
    let id = ArticleId::new("a1");
    let mut model = App;
    update(
        Msg::Delete {
            article: id.clone(),
        },
        &mut model,
        &cx,
    );
    assert!(
        cx.readwise.archived().contains(&id),
        "a1 must appear in archived overlay after Delete"
    );
    let after: Vec<String> = cx.readwise.library().into_iter().map(|a| a.id.0).collect();
    assert!(
        !after.iter().any(|s| s == "a1"),
        "a1 must no longer appear in Library after Delete; got: {after:?}"
    );
}
```

- [x] **Step 2: Run.**

```bash
nix develop -c cargo fmt -p reader
nix develop -c cargo test -p reader --test app
```

All reader app tests pass (existing 3 + new 3 = 6).

- [x] **Step 3: Update this plan file** — flip Task 1's checkboxes to `[x]`.

**DO NOT git add / git commit.**

---

## Task 2 — `view` empty branch + banner branch

- [x] **Goal:** Two tests covering branches of `view` not yet exercised: (a) when both `library()` and `feed()` are empty and there are no failed writes, `view` returns an empty `Documents` — proving the collection-doc `None` branch and no spurious banner; (b) when failed_writes is non-empty, `view`'s first doc has key `_banner` and a `Notice` body — proving the banner-prepend branch.

**Files:**
- Modify: `apps/reader/tests/app.rs` (append).

**Acceptance:**
- New test `view_is_empty_when_no_articles_remain` passes; after deleting both fake cassette articles, `view(&App, &cx).0` is an empty `Vec`.
- New test `view_prepends_banner_on_failed_writes` passes; with a `Readwise` whose write transport is `ScriptedTransport::always_failing`, after enqueuing `Move{to: Archive}` and calling `flush` `MAX_ATTEMPTS` times, `view`'s first document has key `_banner`.
- Existing tests still pass.

**Verify:** `nix develop -c cargo test -p reader --test app`

**Steps:**

- [x] **Step 1: Append to `apps/reader/tests/app.rs`.** Additional imports: `inkapp_core::connector::Connector`, `inkapp_readwise_reader::{Readwise, ScriptedTransport, MAX_ATTEMPTS}`, `std::sync::Arc`, `reader::view`.

```rust
#[test]
fn view_is_empty_when_no_articles_remain() {
    let cx = Connectors::fake();
    let mut model = App;
    // Wipe the fake cassette by deleting both seeded articles.
    update(
        Msg::Delete {
            article: ArticleId::new("a1"),
        },
        &mut model,
        &cx,
    );
    update(
        Msg::Delete {
            article: ArticleId::new("a2"),
        },
        &mut model,
        &cx,
    );

    let docs = view(&App, &cx);
    assert!(
        docs.0.is_empty(),
        "view must be empty when no articles remain and no failed writes; got keys: {:?}",
        docs.0.iter().map(|d| d.key.0.as_str()).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn view_prepends_banner_on_failed_writes() {
    // Build a custom Readwise whose write transport always fails, so the
    // pending write is moved to overlay.failed after MAX_ATTEMPTS flushes.
    let transport = Arc::new(ScriptedTransport::always_failing());
    let readwise = Arc::new(Readwise::fake().with_transport(transport));

    let cx = reader::Connectors {
        readwise: readwise.clone(),
    };

    let mut model = App;
    update(
        Msg::Move {
            article: ArticleId::new("a1"),
            to: Location::Archive,
        },
        &mut model,
        &cx,
    );

    // Exhaust the retry budget. flush() is on the Connector trait.
    for _ in 0..MAX_ATTEMPTS {
        Connector::flush(&*readwise).await;
    }

    assert!(
        !cx.readwise.failed_writes().is_empty(),
        "always-failing transport must produce at least one failed write after {MAX_ATTEMPTS} flushes"
    );

    let docs = view(&App, &cx);
    let first_key = docs
        .0
        .first()
        .map(|d| d.key.0.as_str())
        .unwrap_or_default();
    assert_eq!(
        first_key, "_banner",
        "view must prepend the _banner Document when failed_writes is non-empty; got keys: {:?}",
        docs.0.iter().map(|d| d.key.0.as_str()).collect::<Vec<_>>()
    );
}
```

- [x] **Step 2: Run.**

```bash
nix develop -c cargo fmt -p reader
nix develop -c cargo test -p reader --test app
```

All reader app tests pass (3 existing + 3 from Task 1 + 2 new = 8).

- [x] **Step 3: Update this plan file** — flip Task 2's checkboxes to `[x]`.

**DO NOT git add / git commit.**

---

## Task 3 — `view` reflects `update` mutations (connector-wiring contract)

- [x] **Goal:** Single integration test pinning the spec's connector-wiring contract for the reader's unit Model: after `update(Msg::Move{to: Archive})`, the Library document in `view` no longer carries the moved article in its Section flow. This is the post-mutation analog of "RefreshDone produces the expected model delta" — the reader has no internal model state, so the delta is observable in the next `view`.

**Files:**
- Modify: `apps/reader/tests/app.rs` (append).

**Acceptance:**
- New test `view_after_archive_drops_article_from_library` passes; before update, the Library doc contains a Section keyed `art-a1` (or whatever the section id shape is — discovered in Step 1); after archiving a1, the Library doc either omits that section or the Library doc is omitted entirely if a1 was the only remaining article.
- Existing tests still pass.

**Verify:** `nix develop -c cargo test -p reader --test app`

**Steps:**

- [x] **Step 1: Discover the observable shape.** The reader builds Library/Feed via `collection_doc`, which pushes a `Section::new(&a.id.0, …)` per article. The Section's render emits `#section("a1", …)` and an `<art-a1>` label. The Document also contains an `Index` whose entries list the articles. The most stable observation point in a Layer-5 (non-render) test is: compile the Library doc to typst source via `document_source_in` (already done in the existing `library_compiles_and_recovers_action_plus_token_regions` test), search the source string for the article id token. Easier alternative: render the doc and inspect the recovered manifest's region names for `tok-a1-…` prefixes (per the per-article prefix landed in commit 1ab6729). If neither is convenient, fall back to checking that the post-update `library()` connector method excludes the article AND that view's Library doc count of sections drops by one.

  Pick the simplest observation that isn't brittle. The recommended approach: compile the doc via `document_source_in` (the same path the existing test uses) and grep the source string for `"a1"`. Before: contains "a1". After: does not contain "a1" (the Library section for a1 is gone; if a1 was the only remaining article and the index also drops it, "a1" disappears entirely).

- [x] **Step 2: Append to `apps/reader/tests/app.rs`.**

```rust
#[test]
fn view_after_archive_drops_article_from_library() {
    use inkapp_core::geometry::PageGeom;
    use inkapp_core::runtime::document_source_in;
    use inkapp_core::theme::Theme;

    let cx = Connectors::fake();

    // Sanity: before any mutation, the Library doc references "a1".
    let docs_before = view(&App, &cx);
    let library_before = docs_before
        .0
        .iter()
        .find(|d| d.key.0 == "Library")
        .expect("Library document present before mutation");
    let src_before = document_source_in(library_before, PageGeom::default(), &Theme::reader());
    assert!(
        src_before.contains("\"a1\""),
        "Library typst source must mention a1 before archive; first 200 chars: {}",
        &src_before[..src_before.len().min(200)]
    );

    let mut model = App;
    update(
        Msg::Move {
            article: ArticleId::new("a1"),
            to: Location::Archive,
        },
        &mut model,
        &cx,
    );

    // After: a1 has been removed from the optimistic library view, so the
    // Library doc (still present because a2 remains) must no longer carry
    // any reference to "a1".
    let docs_after = view(&App, &cx);
    let library_after = docs_after
        .0
        .iter()
        .find(|d| d.key.0 == "Library")
        .expect("Library document still present (a2 remains)");
    let src_after = document_source_in(library_after, PageGeom::default(), &Theme::reader());
    assert!(
        !src_after.contains("\"a1\""),
        "Library typst source must NOT mention a1 after archive; offending excerpt: {}",
        src_after
            .find("\"a1\"")
            .map(|i| &src_after[i.saturating_sub(40)..src_after.len().min(i + 40)])
            .unwrap_or("")
    );
    // And the index must still mention a2.
    assert!(
        src_after.contains("\"a2\""),
        "Library typst source must still mention a2 (it was not archived)"
    );
}
```

- [x] **Step 3: Run.**

```bash
nix develop -c cargo fmt -p reader
nix develop -c cargo test -p reader --test app
```

All reader app tests pass (3 existing + 3 from Task 1 + 2 from Task 2 + 1 new = 9).

If the assertion `!src_after.contains("\"a1\"")` fails, that is a real bug in the reader's `view` (the connector overlay is not propagating). Investigate before patching — likely `cx.readwise.library()` is reading from a stale snapshot. Fix at source.

- [x] **Step 4: Update this plan file** — flip Task 3's checkboxes to `[x]`.

**DO NOT git add / git commit.**

---

## Task 4 — appdx update + workspace verify + one combined commit + merge

- [x] **Goal:** Add a Layer-5 subsection to `docs/appdx.md`'s "Test coverage by layer", run workspace verify, commit, FF-merge, clean up.

**Files:**
- Modify: `docs/appdx.md`
- Modify: this plan file (flip Task 4 checkboxes)

**Verify (all three before commit):**
- `nix develop -c cargo fmt --check`
- `nix develop -c cargo test --workspace`
- `nix develop -c cargo clippy --all-targets -- -D warnings`

**Steps:**

- [x] **Step 1: Add a Layer-5 subsection to `docs/appdx.md`.** Insert after the Layer-4 bullet block (before the closing `---` of the "Test coverage by layer" section):

```markdown
- **Layer 5 — reader composition (`apps/reader/src/lib.rs`).** Covered 2026-05-27.
  - `update` exhaustively pinned per Msg variant in
    `apps/reader/tests/app.rs`: `Highlighted` records the highlight via
    the connector's overlay; `Move{to: Archive}` and `Delete` each move
    the article id into the optimistic-archived overlay and remove it
    from the next `library()` snapshot.
  - `view` branches covered: (a) canonical Library state (already pinned
    by the existing `view_yields_library_document` and the end-to-end
    region-recovery tests); (b) empty connector state — after deleting
    both seeded articles, `view` returns an empty `Documents`, with no
    spurious banner; (c) banner branch — with `ScriptedTransport::
    always_failing` injected, a `Move` enqueued, and `flush` called
    `MAX_ATTEMPTS` times, `failed_writes` is non-empty and the first
    document `view` returns has key `_banner`.
  - Connector-wiring contract (the reader has no internal model state,
    so the spec's "RefreshDone produces expected model delta" simplifies
    to "`view` reflects `update` mutations"): after
    `update(Move{to: Archive})` on article `a1`, the Library document's
    compiled Typst source no longer mentions `a1`, while `a2` is still
    referenced — proving the optimistic overlay propagates through to
    the next `view`.
```

- [x] **Step 2: Flip Task 4 checkboxes** in this plan file to `[x]`.

- [x] **Step 3: Workspace verify** — all three:

```bash
nix develop -c cargo fmt --check
nix develop -c cargo test --workspace
nix develop -c cargo clippy --all-targets -- -D warnings
```

Pre-existing drift unrelated to Layer 5: STOP and report. Layer-5-introduced lints: fix.

- [x] **Step 4: Check staged set.** Expected files:

- `M apps/reader/tests/app.rs`                                            (Tasks 1-3)
- `M docs/appdx.md`                                                        (Task 4)
- `A docs/superpowers/plans/2026-05-27-layer5-reader-composition.md`       (this plan, new file)

Anything else is pre-existing drift; do not stage.

- [x] **Step 5: Commit.**

```bash
git add \
  apps/reader/tests/app.rs \
  docs/appdx.md \
  docs/superpowers/plans/2026-05-27-layer5-reader-composition.md

git commit -m "$(cat <<'EOF'
tests(layer-5): reader update + view branches + connector-wiring

Closes Layer 5 of docs/superpowers/specs/2026-05-27-reader-thorough-test-design.md
(plan: docs/superpowers/plans/2026-05-27-layer5-reader-composition.md).

- update exhaustively pinned per Msg variant: Highlighted records the
  highlight; Move{to: Archive} and Delete each move the article id into
  the optimistic-archived overlay and remove it from library().
- view branches covered: empty connector state returns an empty
  Documents with no spurious banner; failed_writes (via
  ScriptedTransport::always_failing + flush × MAX_ATTEMPTS) prepends
  the _banner Document; canonical Library state was already pinned by
  the existing view_yields_library_document and end-to-end
  region-recovery tests.
- Connector-wiring contract (reader's Model is unit, so the spec's
  RefreshDone-produces-model-delta line resolves to view-reflects-
  update-mutations): after update(Move{to: Archive}) on a1, the
  Library document's compiled Typst source no longer mentions a1
  while a2 is still referenced.
- docs/appdx.md: Layer-5 subsection added to "Test coverage by layer".
EOF
)"
```

- [x] **Step 6: FF-merge to main and clean up.**

```bash
cd /home/dan/git/inkapp
git checkout main
git merge --ff-only layer-5
git log -1 --stat

git worktree remove .worktrees/layer-5
git branch -d layer-5
```

If `git merge --ff-only` rejects, main has moved during the work — rebase the layer branch onto main, re-verify, then re-attempt the FF-merge.

## Self-review checklist

- No `TBD` / `TODO` / `todo!()` markers committed.
- `apps/reader/tests/app.rs` count is 9 (existing 3 + 3 + 2 + 1).
- `cargo test --workspace` clean.
- `cargo clippy --all-targets -- -D warnings` clean.
- `cargo fmt --check` clean.
- FF-merge to main succeeded; worktree and branch removed.

## Out of scope

- Layer 6 (full agent-driven loop: emitted sequences, stale-manifest path, offline-connector path; `inkctl session step` work; reader registry entry in `inkctl`'s app registry) — separate plan.
- Any reader feature work — testing pass only.
- Visual / PNG-diff assertions — region presence and connector overlay state are the contracts here.
