# Reader App — Spec

Status: draft, awaiting plan
Author: brainstorming session 2026-05-26
Branch: `main` (no worktree yet — plan-writing step picks one)

## Problem

`~/git/rmreader` is the original Readwise Reader → reMarkable PDF generator,
built on `fulgur` (Blitz + krilla, HTML/CSS) before `inkapp` existed. Its
on-device UX — two PDFs (`Library.pdf` and `Feed.pdf`), each containing an
index page followed by every article concatenated, with a four-cell action band
(`Inbox` / `Archive` / `Later` / `Delete`) drawn at the top of every page —
is the target experience we want again, delivered via inkapp.

`apps/reading-queue` already gets us 70% of the way: live Readwise connector,
HTML→Typst `Article` (per-token highlight regions, images, theme),
publish/sync/serve, pairing, doctor. What's missing is the *shape*: today every
article is its own keyed `Document` (one PDF per article in the device folder)
and archive is a footer `Checkbox`. The Reader app needs per-collection
Documents containing many articles, with a per-page action band that knows
which article each page belongs to.

**Guiding constraint (operator directive 2026-05-26):** every gap exposed by
this app must be closed by improving inkapp, not papered over in the app code.
The framework gains reusable primitives; the app collapses to composition.

## Goals

- A new `apps/reader` that, per cycle, produces exactly two `Document`s —
  `Library.pdf` and `Feed.pdf` — driven by the existing Readwise connector's
  `library()` / `feed()` helpers and the existing config schema.
- Each Document contains: a cover/index page (existing `Index` component) listing
  every article in the collection, then every article (heading + body), with the
  four-cell action band on every page header.
- Strike a label on the action band on any page → that article moves
  (`Move` / `Delete`) on the next sync cycle.
- `reading-queue` stays in tree as the minimal worked example for tutorials and
  docs; the Reader is a sibling, not a replacement.
- Five framework additions land alongside (per directive): page-header
  primitive, `Section<M>`, `Heading`, `ActionBand<M>`, and ink-reset push.

## Non-goals (v1)

- Intra-PDF navigation (Typst `#link` to internal labels). Users scroll. Add later.
- A separate `Shortlist.pdf` top-level Document. Shortlist remains a Library
  location, listed in `Library.pdf` alongside `new` and `later`.
- Multi-Readwise-account / multi-instance Reader fanout. One Reader instance
  binds to one connector instance.
- Non-reMarkable devices (Supernote etc.); `DeviceTransport` already isolates
  this so future devices need no app change.
- Rendered-PDF caching (re-render the whole Library on every cycle is fine for
  v1; per-article PDF caching is a follow-on optimization once observed needed).

## On-device shape

```
/Reader/             <-- the configured [app.reader.<instance>].device_folder
  Library.pdf        <-- Document::keyed("library", ...). Index + every Library article.
  Feed.pdf           <-- Document::keyed("feed",    ...). Skipped if feed_enabled = false
                          or the connector reports zero feed articles.
```

`DocKey` is `"library"` / `"feed"`. The device's `visibleName` is the same string;
case-titling to "Library" / "Feed" is either solved by keying as `"Library"`
directly or by a small optional `Document::visible_name` field on the transport
push path — implementation chooses whichever is cleanest.

## Architecture

### What's framework vs what's app

Honest split (every reusable primitive goes into inkapp; the app is composition):

| Piece                                                            | Reusability                                          | Lives                            |
|------------------------------------------------------------------|------------------------------------------------------|----------------------------------|
| Per-page header keyed to dynamic "current section" state         | Every multi-section reading-app                      | inkapp framework (new)           |
| `Section<M>` (opens per-section state, wraps a body)             | Any "N items in one document" layout                 | inkapp-core/components (new)     |
| `Heading` (title / byline / meta / optional subtitle)            | Generic long-form content opener                     | inkapp-core/components (new)     |
| `ActionBand<M>` (N labeled page-header cells, each `Fn(section_id) -> M`) | Reader, journal, agenda — any structured per-page actions | inkapp-core/components (new) |
| `Index` (multi-row listing)                                      | Already framework                                    | inkapp-core/components (existing)|
| Ink-reset push (`push_replace_ink`)                              | Any app whose layout shifts between syncs            | inkapp-core::sync + rm-device    |
| Readwise labels (`Inbox`/`Archive`/`Later`/`Delete`) and Msg map | Readwise-specific                                    | apps/reader                      |
| `ArticleSection` (header + Article body, in a `Section`)         | App-side composition                                 | apps/reader                      |

`ActionBand<M>` uses the appdx-documented stored-closure escape hatch (the
message depends on both *which cell was struck* and *which section the page
belonged to* — both content-derived) because both axes are content-derived.

### The app, in full

The `view` is the only place app logic lives, and it is composition end to end:

```rust
enum Msg {
    Highlighted { article: ArticleId, text: String },
    Move        { article: ArticleId, to: Location },   // Inbox | Later | Archive
    Delete      { article: ArticleId },
}

fn update(msg: Msg, _m: &mut Model, cx: &Connectors) {
    match msg {
        Msg::Highlighted { article, text } => cx.readwise.add_highlight(&article, &text),
        Msg::Move        { article, to }   => cx.readwise.move_to(&article, to),
        Msg::Delete      { article }       => cx.readwise.delete(&article),
    }
}

fn view(_m: &Model, cx: &Connectors) -> Documents<Msg> {
    let band = ActionBand::new([
        ("Inbox",   |id: &str| Msg::Move   { article: ArticleId::new(id), to: Location::New } ),
        ("Archive", |id: &str| Msg::Move   { article: ArticleId::new(id), to: Location::Archive } ),
        ("Later",   |id: &str| Msg::Move   { article: ArticleId::new(id), to: Location::Later } ),
        ("Delete",  |id: &str| Msg::Delete { article: ArticleId::new(id) } ),
    ]);

    let mut docs: Vec<Document<Msg>> = Vec::new();
    if let Some(d) = collection_doc("library", "Library", &band, cx.readwise.library()) { docs.push(d); }
    if let Some(d) = collection_doc("feed",    "Feed",    &band, cx.readwise.feed())    { docs.push(d); }
    Documents(docs)
}

fn collection_doc(
    key: &str,
    _title: &str,
    band: &ActionBand<Msg>,
    articles: Vec<Article>,
) -> Option<Document<Msg>> {
    if articles.is_empty() { return None; }
    let entries: Vec<IndexEntry> = articles.iter().map(IndexEntry::from).collect();
    let mut items: Vec<_> = vec![Index::new(entries).into_boxed()];
    for a in &articles {
        items.push(Section::new(&a.id.0, flow![
            Heading::for_article(a),
            article_body(a),
        ]).into_boxed());
    }
    Some(Document::keyed(key, items).page_header(band.clone()))
}

// Article wired with the on-highlight closure into the app's Msg.
fn article_body(a: &Article) -> inkapp_content::Article<Msg> {
    let id = a.id.clone();
    inkapp_content::Article::new(
        a.html_content.as_deref().unwrap_or(""),
        &a.highlights,
        move |s| Msg::Highlighted { article: id.clone(), text: s.to_string() },
    )
}
```

(Method names like `Index::new`, `Document::keyed`, `flow!`, and
`inkapp_content::Article::new` are real today. `Index::with_title`,
`Section::new`, `Heading::for_article`, `Document::page_header`,
`ActionBand::new`, and `into_boxed`/equivalent are the v1 surface this spec
introduces; the writing-plans step locks the final signatures. Sync-failure
banner via `Notice` is included identically to `reading-queue`'s pattern;
omitted above for brevity.)

## Framework additions

Five sharp primitives, each independently testable, none reader-specific.

**1. Per-page header on `Document`.** `Document::page_header(component)` registers
a component the renderer wires into `#set page(header: ...)` for that Document.
The header is a normal `Component<M>` — `render` emits Typst, `decode` reads its
per-page regions and emits Msgs. Region recovery already walks every page frame,
so each physical page produces its own header-region instances.

**2. `Section<M>` component.** Render emits
`#state("inkapp.section").update("<id>")` + a `weak: true` page break, then the
body. Decode delegates to children. The per-section state is what the page
header reads via `#context` to know which section's actions to render.

**3. `Heading` component.** Display-mode: title (heading style), byline (single
line — author OR site_name fallback), reading-time, optional subtitle/summary
line. Theme-aware: uses the existing `Theme` typography and palette. Pure render;
decode returns empty.

**4. `ActionBand<M>` component.** Constructed with N `(label, Fn(section_id) -> M)`
pairs. Render emits a Typst row of cells inside the page header (one
`#region("action-{label}-{section}", ...)` per cell, where `{section}` resolves
from the section state). Decode iterates regions per page, classifies pen
strikes by which cell they overlap (re-using `GestureAction`'s strike heuristic
— non-highlighter stroke whose bbox spans most of the cell width), and calls
the matching closure with the parsed section id.

**5. Ink-reset push.** `DeviceTransport` gains `push_replace_ink(key, pdf)` (or a
mode flag on `push`). `publish` keeps using content-only-swap (so a first deploy
preserves any pending pre-fold ink on the device). `sync_once`'s post-fold push
uses `push_replace_ink`, because absorbed ink → next render's `#highlight` →
the per-page raster is now stale relative to the new layout. `CloudTransport`
implements it by calling `rm_cloud`'s full `put` (whole `DocFiles` bundle)
rather than `put_content_only`.

**Optional bonus (6).** `DocKey` → `visibleName` is currently identical; the
reader wants `library` → "Library". Cheapest: key as `"Library"` directly.
Alternative: add `Document::visible_name(...)` and have the transport prefer it.
Pick during implementation; not on the critical path.

## Data flow

One sync cycle, end to end:

1. **Refresh** — `refresh_all` warms the Readwise cache (paginated list, deduped,
   persisted to the foyer cache). Existing behaviour, no change.
2. **View** — `view(cx)` reads `library()` / `feed()` and emits one or two
   Documents per the composition above.
3. **Resolve images** — framework collects `image_urls()` across each Document's
   flow (existing hook on `Article`), runs the image pipeline (PNG normalize,
   placeholder on failure), registers bytes in the per-doc `InkWorld`.
4. **Render** — Typst compiles each Document with the ActionBand page header
   active and per-section state hooked. Per-page region recovery extracts both
   article token regions AND header action regions per page frame.
5. **Push** — `publish` content-only-swaps; `sync_once`'s post-fold push uses
   `push_replace_ink`.
6. **Flush** — `flush_all` drains pending Readwise writes (move / delete /
   create-highlight) through the live HTTP write transport.

## Error handling

Three failure surfaces, each handled by an existing inkapp pattern:

- **Connector refresh fails / rate-limited.** Refresh errors swallow into the
  warm cache; `view` reads stale data; `doctor` surfaces the failure on demand.
  No new code.
- **Image fetch fails.** Placeholder PNG registered under the asset key
  (existing behaviour). Article still renders.
- **Readwise write fails after 3 retries (`MAX_ATTEMPTS`).** A `Notice` banner
  Document at the top of the set, identical to reading-queue's pattern:
  `Document::keyed("_banner", flow![Notice::line(format!("couldn't sync {} change(s)", failed.len()))])`.

## Testing strategy

Three layers, each with a clear acceptance bar:

**Unit (per framework primitive).**
- `Heading` text formatting: byline fallback `author` → `site_name`,
  `reading_time` string passthrough (the Readwise quirk).
- `Section`: render emits state update + page break; compile snapshot.
- `ActionBand`: decode of synthetic strike ink across cells, mirroring
  `GestureAction`'s tests + the harness `simulate` + the existing
  `strike-through.json` fixture.
- `push_replace_ink`: semantic test against a fake `DeviceTransport` —
  `publish` calls content-only-swap; `sync_once`'s post-fold push calls replace.

**Harness golden (end-to-end render).** A small reader fixture — cassette of
3 Library articles + 1 Feed article — renders both Documents, asserts each PDF
compiles, paginates with the action-band header on every page, recovers per-
article token regions per page, and the ActionBand regions are present and
keyed by article id. One article has an image (exercising the asset
registration + placeholder fallback path).

**On-device manual (the actual verification).** A single `#[ignore]`d
integration test documents the exact steps: `app pair <code>`, `app secret set
connector readwise <token>`, `doctor` green, `publish`, on the device strike
`Archive` on a Library article on any page, `sync`, confirm the article
disappears from `library()` and a `Move(_, Archive)` lands in Readwise. This is
the explicit handoff to the operator's real-device round-trip.

## Open questions

None blocking. Two pre-implementation decisions to make:

- `Document::visible_name` field vs keying as title-cased strings directly — pick
  at implementation time based on whether the visible-name vs reconcile-key
  separation pays for itself.
- Whether `ActionBand` should accept an arbitrary section-state key name (not
  hardcoded `inkapp.section`) for future extension to multi-axis sections;
  default-arg path looks fine, leave it concrete unless a test wants generality.

One known race accepted for v1, documented for follow-on:

- **Mid-cycle ink race with `push_replace_ink`.** Between `sync_once`'s pull
  and the post-fold push (a few seconds), the user can draw new ink on the
  device. `push_replace_ink` wipes that in-flight ink. content-only-swap would
  preserve it, but mis-attributed against the new layout. Neither is right.
  v1 picks replace (the less-confusing failure mode: clean page rather than
  stranded mystery ink). v2 fix path: a second pre-push pull that folds any
  delta before pushing. Defer the protocol change; surface the trade-off in the
  reader's on-device manual checklist.

## What this replaces

The old `~/git/rmreader` (fulgur-based) becomes obsolete on first cycle that
this app deploys successfully to a real device. `apps/reading-queue` stays;
it remains the smallest possible inkapp app for tutorials and docs.
