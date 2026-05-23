# Building apps on inkapp (developer experience)

> **Status: exploratory.** This captures how we *want* it to feel to build an app
> on inkapp. Much of it is not built yet, and some of it is still being argued out.
> Open questions are marked **(open)** inline. Expect this doc to churn.

The touchstone example throughout is a reading-queue app in the spirit of
`rmreader`: articles flow in from a service, render as PDFs on the device, the
user highlights and marks them read, and those actions flow back out.

---

## The mental model: MVU for pen devices

A pen device is an inherently **render-everything / interpret-input-in-batches**
surface: there is no live UI, output is a full re-render, and input arrives as one
coarse batch of ink after the user has finished. That is almost the literal
description of the **Model-View-Update** (Elm) architecture, so that's the model
inkapp uses. (The *loop* is still CGI/request-response — see
[inspiration.md](inspiration.md) — MVU is how the app code inside the loop is
shaped.)

| MVU part      | inkapp                                                           | Reading-queue example                       |
|---------------|-----------------------------------------------------------------|---------------------------------------------|
| **Model**     | your app's *own* state — often thin, since connectors hold data  | maybe nothing; the queue lives in Readwise  |
| **View**      | **components** (nested), rendered to a document                  | an article-body component + an archive control |
| **Msg**       | a semantic change *decoded from ink*                             | `Highlighted`, `Archived`                   |
| **Update**    | `fn(Msg, &mut Model, &Connectors)` — mutate state, call connectors | "set archived; tell Readwise"             |
| **Connector** | a plugin that fetches/caches external data and accepts writes    | Readwise, an ICS calendar, CalDAV           |

The framework supplies the loop, the render pipeline, sync, and ink decoding. You
supply the `Model`, the `update` function, the `view` (composed of components), and
any `connectors` you can't reuse. **`update` is the only part that does I/O** — it
calls connectors directly; components stay pure.

(Why no separate "effects" type, unlike Elm? Elm returns effects-as-data because
its UI thread must stay pure and can't block. inkapp runs server-side in batches
between syncs, so `update` can just call connectors — reads hit their local cache,
writes go straight through. One fewer concept; still testable via a fake
`Connectors`.)

```
              ┌──────────────────────── Model ───────────────────────┐
              │              (your app's own state — often thin)       │
   update     │                                                       │  view: render
   mutates    │                                                       ▼  components → document
        ┌─────┴──────┐                                        ┌──────────────┐
        │   update   │── reads/writes ──▶ Connectors          │     view     │
        │ (Msg, &mut │      (cached external data) ──http──▶   │ (components) │
        │  Model, &Cx)│        Readwise · ICS · CalDAV         └──────┬───────┘
        └─────▲──────┘            ▲                                   │ framework:
              │ Msgs               │ view also reads connectors        │ paginate · sync
              │ (decoded ink)      └───────────────────────────────────┤
        ┌─────┴──────┐                                                 ▼  per device
        │ components │◀───────────────  ink  ─────────────────┌──────────────┐
        │   decode   │   (framework parses .rm and attributes │  device(s)   │
        │  ink→Msg   │    ink to each component's regions)     │ reMarkable·… │
        └────────────┘                                        └──────────────┘

   components are pure (render + decode); only update (and view, for reads)
   touch connectors.
```

---

## What you write vs what the framework does

**You write:**
- A **Model** — your app's own state type (often small; connectors hold the data).
- An **update** — `fn(Msg, &mut Model, &Connectors)`: mutate state and call
  connectors for any reads/writes.
- A **view** — a content flow assembled from **components**.
- **Connectors** for each external system (or reuse existing ones).

**The framework does:**
- Renders your view per device, paginates it, embeds the (encrypted) manifest.
- Syncs documents to the device and pulls annotated ones back.
- Parses `.rm` ink, attributes it to component regions, asks components to **decode**
  it into `Msg`s, and folds them through your `update`.
- Hides pages, device sizes, and per-page ink stitching from you entirely.

---

## The loop, from the app's point of view

```
   ┌──────────────────────────────────────────────────────────────┐
   │                                                              │
   │  (1) sync starts — pull back annotated documents   [FRAMEWORK]│
   │       │                                                      │
   │       ▼                                                      │
   │  (2) parse ink → components decode → batch of Msg  [FRAMEWORK]│
   │       │                                                      │
   │       ▼                                                      │
   │  (3) fold each Msg: update(Msg, &mut Model, &cx)  [UPDATE]   │
   │       — mutate state; call connectors to read/write          │
   │       │                                                      │
   │       ▼                                                      │
   │  (4) view renders the document from new state     [VIEW]     │
   │       │                                                      │
   │       ▼                                                      │
   │  (5) render per device + sync out                 [FRAMEWORK]│
   │       │                                                      │
   │       └────────► user reads & writes, hours/days later ──────┘
   └──────────────────────────────────────────────────────────────┘
```

Ink becomes messages; `update` folds each one — mutating state and calling
connectors for reads and writes inline — then `view` renders. A stale-looking
action is decoded against the document version it was written on, not the latest
state — see [event sourcing](#state-over-time-event-sourcing--merge) for how that's
reconciled.

---

## Documents, pages, and devices

You author a **content flow**, not pages. The framework paginates it, and it
paginates *differently per device* because e-ink devices differ in size, aspect,
and DPI. Pages don't scroll on e-ink — you swipe — so fixed pages are the right
unit, but they're an *output* of layout, not something you place.

Consequences for app authors:

- **You never think in pages.** "Page 4" is not a stable thing across devices.
- **Regions are content-relative.** A component marks *"this paragraph is a
  highlight region"*; the framework resolves it to whatever page(s) it lands on.
- **Device dimensions are a render input.** One content flow → N device renders.
- The framework absorbs the ugly case where a region is **split across a page
  break** on one device but not another (it stitches the ink back together before
  the component decodes it).

This mirrors how Typst itself works: content is a flow, the engine paginates into
frames, and inkapp recovers regions from those frames.

```
          one content flow  +  content-relative regions
                            │
          framework paginates per device (sizes differ)
      ┌─────────────────────┼─────────────────────┐
      ▼                     ▼                     ▼
  reMarkable            Supernote              device N
  (5 pages)             (4 pages)                 …
      │ user inks            │ user inks
      ▼                      ▼
  per-page .rm          per-page .rm
      └─────────────────────┼─────────────────────┘
                            ▼
      framework lifts ink → content-relative regions
      (re-stitches regions split across page breaks)
                            │
                            ▼
      components decode region ink → Msgs
      (page-blind, device-blind)
```

---

## Components

A component is a pure, nestable unit of view with two halves:

- **render** — `props → Typst`. Emits content (and, if it subdivides, sub-regions).
- **decode** — `ink → Msg`. Turns the ink on it into messages.

This mirrors Elm's `view`, which both *draws* and declares how events become
messages. Components co-locate the two so a component is a self-contained piece:
it knows how to draw *and* how to read itself back.

### Regions are automatic by default

A component's region is **its own bounding box** — you don't name anything. `decode`
just receives "the ink on me." So a checkbox's decode is `if ink.any() { … }`, with
no stringly-typed region name to keep in sync between render and decode.

You only name sub-regions when a component **subdivides** itself (an article body
giving each token its own region so a highlight maps to a span). Those names are
minted *programmatically* (`tok-0`, `tok-1`, …) during render, never hand-typed in
two places — so even then there's no fragile contract to typo.

### Components are real Typst components

A component's render half is authored in **Typst's own scripting language** —
functions, `#let`, conditionals, loops, `context` — not by string-building Typst
markup from Rust. Region declaration, per-device conditional layout, and
composition all live in Typst, where they belong.

The boundary to keep in mind: **Typst owns the render half only.**

- Typst scripting is **render-time, sandboxed, no I/O** — it can compute layout,
  not call Readwise or decide to archive something. App logic stays in `update`.
- The **decode half is host-language** (Rust): interpreting parsed ink isn't a
  render-time activity. So every component is bifurcated — Typst render, Rust
  decode — and that seam is explicit by design.
- Typst's own `state`/`counter` exist only during one compile; they do **not**
  survive the round-trip. Persistent state is a separate mechanism (see below).

### Three interaction modes

You can't make a surface non-writable on a pen device — the user can always ink
anywhere. So "read-only" is a choice to *discard* ink, not an inability to capture
it. That gives three modes:

| Mode        | Renders              | Decodes ink                       | Example                    |
|-------------|----------------------|-----------------------------------|----------------------------|
| **Display** | content              | no (ignored)                      | a header, a reference page |
| **Capture** | content, no controls | yes, freeform (highlights, notes) | an article body            |
| **Control** | explicit affordances | yes, structured                   | a checkbox, a task line    |

`rmreader`'s article body is **Capture**: it looks read-only, but the highlights
on it are the whole point.

### Components never talk to connectors

A component's mode and a connector's read/write capability are **separate axes**. A
Capture component (article) can front a read-write connector (highlights sync out);
a Control component can front a read-only connector.

Components stay pure: no I/O, no connector awareness. Everything a component needs
is in its render input, including its affordances —

```
CalendarView { events, mode: ReadOnly | Editable }
```

— and **`update`/`view` decides `mode`**, because that side knows the connector.
Two reasons this beats letting a component reach into a connector:

1. **It's policy, not just capability.** A connector *reports* "I can write"; your
   app *decides* whether to expose it. A CalDAV-backed but intentionally read-only
   agenda is just `view` passing `ReadOnly`.
2. **It stays testable.** Render with `ReadOnly` / `Editable`, no connector, no
   I/O — the property that makes components provable in the harness.

The same `mode` reaches `decode`, so a component that rendered no controls doesn't
try to decode structured ink. Render and decode agree because they share one input.

### Composing components: carry a message, don't store a closure

A reusable component shouldn't know your app's `Msg`. The clean way to wire it is to
have it **carry the message value to emit** (Elm's `Html.map`, as a value, not a
function):

```rust
Checkbox { label: "Archive", checked: a.archived, on_check: Msg::Archived { article: a.id } }
```

No closures, no generics-over-functions. For a component whose message depends on
*what* was decoded (which span got highlighted), the natural home is a **bespoke,
app-specific component** that builds the `Msg` directly in `decode` — content
components like an article body or a journal page are app-specific anyway. Only if
you genuinely need a *reusable content* component do you reach for a stored
`Box<dyn Fn(..) -> Msg>`; it's the rare escape hatch, not the default path.

(`From` is still handy for the dumb **leaf** conversions inside `view` — one
`connector::Event` → one `EventRow`: total, 1:1, context-free, tidy to test.)

---

## Connectors

A connector wraps an external system: it fetches and caches its data, and accepts
writes. `update` calls it directly. Connectors are **plugins**, and **more than one
app can share one** (your calendar might back several apps), with a shared cache.

Two kinds, and the second is the hard one:

- **Read-only feed** — pull, cache, done. (ICS.)
- **Bidirectional** — `update` calls its write methods. (Readwise: mark read,
  archive, create highlight. CalDAV someday.)

**Each connector owns its own cache.** Storage is the connector's choice — sqlite,
plain files, whatever fits the system — hidden behind the connector interface; the
framework imposes no storage engine. This settles most of the cache questions:

- **TTL / invalidation** — connector-internal policy. A read call can pass a hint
  ("force fresh" vs "recent is fine"), but the connector decides.
- **Who triggers refresh** — `update` or `view`, by reading the connector; the
  connector chooses cache-vs-network behind that call.

**Concurrency** — two apps hitting one shared connector at once. The framework
shares each connector as `Arc<dyn Connector>`; methods take `&self` and the
connector uses **interior mutability** for its cache (`Mutex`/`RwLock`/atomics
*inside* the connector). The lock lives inside the connector — consistent with "the
connector owns its cache" — and `update` never sees it. Two rules keep this from
biting:

- **Never hold the lock across network I/O.** Fetch outside the lock, then briefly
  lock only to write the result. `RwLock` lets apps read cache concurrently while
  only a refresh takes the write lock. (`Arc<Mutex<WholeConnector>>` around
  everything is the easy-wrong version: one app's refresh stalls all others.)
- **Single-flight.** Collapse simultaneous refreshes into one network call via an
  in-flight guard — the real value-add beyond mere safety.

(Lock primitive follows the I/O model: std `Mutex`/`RwLock` for blocking
connectors, tokio's for async. Not decided yet.)

---

## State

Three kinds of state, in three places:

- **Document state** — small, per-document, *encoded into the PDF* (encrypted).
  Lets the framework decode ink against the right base version ("which article,
  which version, where the regions are").
- **App state (the MVU `Model`)** — server-side, per user; the authoritative state
  your `update` evolves. For multi-party apps this is an event log (below).
- **Connector state** — the connector caches of external data. Server-side,
  per user, owned per connector.

**Encryption — everything embedded is encrypted.** The rule is simple: *the device
reads none of our embedded metadata; the framework reads all of it and holds the
key; therefore all of it is encrypted.* The reMarkable only renders PDF pages to
pixels and stores ink by page — it never introspects our embedded data. The
framework reads it server-side on readback, where it has the key (from the config
store). The only reader lacking the key is a third party you shared the PDF with —
exactly who you're hiding it from.

So there is **no cleartext tier**:

- The app's **state field** carried in the document (document- and component-level)
  is encrypted. Your code works in plaintext; the framework encrypts on write,
  decrypts on read.
- The **structural manifest** (regions, version marker) is encrypted too — which
  also stops region names (`done`, `habit_streak`, article tokens) from leaking.

---

## State over time: event sourcing & merge

The hardest problem is reconciliation: a user wrote on document version N while the
state moved on to M. MVU already points at the answer, because **a `Msg` *is* an
event and `update` *is* "apply event."** Event sourcing is just MVU with the
message stream persisted — not a second architecture.

- **The `Model` becomes an append-only log of events** (the decoded `Msg`s), each
  tagged with the base version it was authored against; current state is a fold
  over the log.
- **Staleness dissolves.** You don't overwrite — you *append* the user's events and
  re-fold. Most ink is *additive* (highlights, notes), so most events **commute**
  and merge cleanly regardless of base version. Real conflict shrinks to the few
  events that touch the same single value.
- **CRDT as discipline, not distribution.** There's always a central coordinator
  (the render server), so full distributed CRDT isn't needed — each piece of state
  declares a **merge type** so any fold order is deterministic: **OR-Set** for
  highlights/notes, **LWW-Register** (timestamp + actor tiebreak) for a single
  field, **PN-Counter** for a tally, **OR-Map** for shared-but-per-user status.
- **The version marker grows into a vector clock** per `(user × device)` only when
  you add parties; single-user/single-device it's just a counter. Incremental, no
  redesign.
- **Snapshots fall out of what exists.** A connector cache is already a snapshot;
  stale-ink rejection is already the compaction boundary.
- **Idempotency.** Each event gets a stable id derived from stroke identity (from
  `.rm`) so a re-sync doesn't double-append.
- **Conflict UX is already designed.** "An event lost its LWW merge" surfaces as
  *conflict-as-next-render*: "A wrote X, B wrote Y — circle one."

**Adoption, not tax.** A basic single-user app does the simple thing: `update`
mutates state and calls connectors inline (as in the worked example). Event
sourcing is opt-in for when multi-device or collaboration arrives (see
[FUTURE.md](FUTURE.md) — shared documents). The one change it asks for is that
`update` become *pure* — defer writes so they aren't replayed when folding the log —
which the clean `Msg` boundary makes a refactor, not a rewrite.

---

## A worked example: a reading queue

Let's build the touchstone app end to end, kept deliberately small, to see whether
the pieces actually feel good together. *(Illustrative — macro/API names are
sketches, not a committed surface.)*

**Model + messages.** Readwise is the source of truth, so this app keeps almost no
state of its own — its `Model` is nearly empty. The messages are just the things a
user can do:

```rust
struct App;   // the Model — no own state; the queue/highlights live in Readwise

enum Msg {
    Highlighted { article: ArticleId, text: String },
    Archived    { article: ArticleId },
}
```

**Update — the only place app logic lives.** It mutates state (none here) and calls
connectors directly. No `Effect` type, no result-messages:

```rust
fn update(msg: Msg, _m: &mut App, cx: &Connectors) {
    match msg {
        Msg::Highlighted { article, text } => cx.readwise.add_highlight(article, &text),
        Msg::Archived    { article }       => cx.readwise.archive(article),
    }
}
```

**View — one document per article, keyed by a stable id** so the framework preserves
ink across re-renders. It reads the queue straight from the connector cache:

```rust
fn view(_m: &App, cx: &Connectors) -> Documents {
    cx.readwise.queue().iter().map(|a| {
        Document::keyed(a.id, flow![
            ArticleBody { article: a.id, body: &a.body, highlights: &a.highlights },
            Checkbox    { label: "Archive", on_check: Msg::Archived { article: a.id } },
        ])
    }).collect()
}
```

**A reusable component** knows nothing about this app's `Msg` — it carries the
message value to emit. One region (itself), so `decode` never names anything:

```rust
struct Checkbox<M> { label: &'static str, on_check: M }

impl<M: Clone> Component for Checkbox<M> {
    type Msg = M;
    fn render(&self, _: &mut Render) -> Typst {       // render half = Typst (inline here)
        typst!(r#"#checkbox(label: "{label}")"#, label = self.label)
    }
    fn decode(&self, ink: Ink) -> Vec<M> {            // ink = whatever landed on me
        if ink.any() { vec![self.on_check.clone()] } else { vec![] }
    }
}
```

```typst
// components/checkbox.typ — render half only; the framework wraps it in a region
#let checkbox(label: "") = box[
  #box(width: 12pt, height: 12pt, stroke: 0.5pt) #h(4pt) #label
]
```

**A content component** is app-specific, so it builds the `Msg` directly in
`decode` — no closure, no genericity. It subdivides into per-token regions (minted
programmatically during render) so a highlight maps to a span:

```rust
struct ArticleBody<'a> { article: ArticleId, body: &'a Html, highlights: &'a [Highlight] }

impl Component for ArticleBody<'_> {
    type Msg = Msg;
    fn render(&self, _: &mut Render) -> Typst { render_article(self.body, self.highlights) }
    fn decode(&self, ink: Ink) -> Vec<Msg> {
        ink.highlighted_spans()                       // freeform highlighter ink → tokens
           .map(|s| Msg::Highlighted { article: self.article, text: s.text })
           .collect()
    }
}
```

**That's the whole app.** You never wrote the loop, sync, pagination, ink parsing,
encryption, or per-device rendering. The framework calls `decode` on returned ink,
folds the `Msg`s through `update` (handing it the connectors), and re-renders `view`.

### Ergonomics check — what feels good, what's left

Good:

- **`update` is the one place logic lives** — a small `match` that calls connectors
  directly. No effect type, no result-message ping-pong, no runtime indirection.
- **The `Model` is nearly empty.** When connectors hold the data, the app is a thin
  lens over them. Apps with genuine own-state (a game, a journal draft) put it in
  `Model`; this one barely needs one.
- **Reusable components carry a value message** (`on_check: Msg::…`), so a generic
  `Checkbox` works in any app without knowing its `Msg` — no closures, no
  generics-over-functions.
- **No stringly-typed regions.** The checkbox names nothing; `decode` just gets the
  ink on it. Subdivided regions (article tokens) are minted in code, never typed
  twice.
- **Identity rides in props**, and **the collection is just `view -> Documents`** —
  one article → one keyed document; add/remove falls out of the queue changing.

Left (accepted, not rough):

- **Two languages per component** — render in Typst, decode in Rust. This is a
  clean concern split (presentation vs logic), not an accident, and with auto-
  regions there's no shared stringly contract anymore. Small components keep render
  inline (one file); only elaborate presentation graduates to a `.typ` file.
- **The rare *reusable content* component** (shared across apps *and* with
  content-derived messages) still wants a stored `Box<dyn Fn(..) -> Msg>`. It's the
  documented escape hatch, not the default path — app-specific content components
  (the common case) avoid it entirely.
- **Connector-as-source-of-truth is a style, not a rule.** It makes this app tiny,
  but an app is free to hold real state in `Model` instead.

---

## Secrets & config

The framework manages a config/secrets store holding per-**user** secrets in three
scopes:

- **Per-connector credentials** — Readwise token, CalDAV login.
- **Per-device auth** — reMarkable cloud auth, Supernote auth, etc.
- **Per-user key** — the document-state encryption key.

## Scope & threat model

Design for these from the start, even if the first deployment is small:

- **Multi-device per user** — one user may run several devices, *including
  different manufacturers* (a reMarkable and a Supernote). A logical document fans
  out to a per-device render, and ink comes back per device. So a document instance
  is really *(logical doc) × (device)*, and the same user reading the same thing on
  two devices produces two ink streams to reconcile — the same merge problem as the
  loop's, one axis over.
- **Multi-user** — assume it. Everything user-scoped: the secrets store, the
  encryption key, and **connector caches** (my calendar ≠ yours, so a "shared"
  connector is shared across a user's *apps*, never across users).
- **Self-hosted now, cloud later** — fine to start self-hosted, but don't bake in
  single-host assumptions. Cloud means the framework custodies many users' creds
  and keys at once: tenant isolation becomes a hard requirement, and key
  management gets heavier (per-user keys, where they live, KMS, rotation).

**Threat model** is therefore two layered concerns: (1) the share-leak case — "I
handed someone a PDF and a token/region name leaked" (closed by encrypting
everything embedded), and (2) tenant isolation once multi-user/cloud — one user's
data, caches, and keys never reachable by another. **(open)** Concrete key
management and tenant-isolation mechanics are undesigned.

---

## Open questions parking lot

- Key management & tenant isolation once multi-user / cloud.
- The exact shape of the event log / merge-type declaration (which CRDT types are
  built in, how an app declares them, where the log lives).
- Cross-device reconciliation specifics (one user, two devices, two ink streams).
- Lossy input: should `decode` be able to emit "I couldn't tell," so `update` can
  re-ask on the next render?
- The shape of the `Msg` batch handed from decode to update: flat stream vs a tree
  mirroring component nesting.
- Connector write semantics: ordering and retries for the writes `update` issues
  (now the connector's concern, not a runtime effect queue).
