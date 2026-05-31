# inkapp — Spec #9: Document- & component-level state field

**Date:** 2026-05-24
**Status:** Approved (design); plan pending

## Context

The build-order spine is complete: **S** secrets → **E** encryption → **C**
connector plugin trait → **M** mode axis → **T** Typst authoring *(all done, Specs
#5–#8)*. What remains in `docs/appdx.md` is either explicitly future
([FUTURE.md](../../FUTURE.md): event sourcing/CRDT, multi-user/cloud) or logged
tidies — **except one near-term `(open)`**: the state-field payload in the State
section. This spec closes it.

Today the appdx's **State** section makes a promise the code does not keep:

- It describes **document state** — *"small, per-document, encoded into the PDF
  (encrypted). Lets the framework decode ink against the right base version"* — and
  an app **state field** *"carried in the document (document- and component-level)
  … your code works in plaintext, the framework encrypts on write and decrypts on
  read."* It flags this **`(Seam ready; no state-field payload is carried yet —
  (open))`**.
- **The code carries no such payload.** `Manifest { version: u64, regions:
  Vec<Region> }` is the only thing sealed into the PDF (`embed.rs`, Info-dict key
  `InkappManifest`, XChaCha20-Poly1305). App/component state lives **server-side
  only**, in the loop's in-memory `DocEntry { manifest, page_h, hash, version, ink
  }` (`runtime.rs`). Nothing app-defined travels *in the document*.
- Consequently `decode(&self, ink, &manifest)` receives regions + version but **no
  app state**, so a component cannot interpret ink against the state it was
  *rendered* with — only against whatever `view` rebuilds from current `Model`.

This is the keystone the largest future work sits on: event sourcing's *"each
`Msg` tagged with the base version it was authored against"* and *"a stale-looking
action is decoded against the document version it was written on, not the latest
state"* both require the document to carry its render-time state. Building it now is
the highest-leverage non-future move, and it rides the **already-proven seal** (E,
Spec #6) — additive, not new infrastructure.

### What this spec makes true

- **The sealed manifest carries an app-defined state payload**, at **both**
  document level (one opaque app-owned blob) and **component level** (a map keyed by
  a stable, props-derived component key). Closes the appdx `(open)` fully.
- **`decode` interprets ink against the carried base state**, not latest server
  state — proven by a component whose decode provably uses the carried value over
  its own current prop.
- **Everything embedded stays encrypted** — the state rides the existing seal; no
  cleartext state reaches the PDF.

### Decisions taken during design

- **Granularity: both** document- and component-level (user choice), fully closing
  the `(open)` rather than a half-measure.
- **Carrier: state lives inside `Manifest`** (Approach 1), on the existing seal — so
  `decode` (which already receives `&manifest`) needs **no signature change**.
  Rejected: a separate sealed blob + `DecodeCx` param (cleaner separation, but a
  signature change across every component and test, for no lifecycle benefit — state
  and manifest are sealed and read together every cycle).
- **Component identity: explicit, props-derived keys** (mirrors how region names are
  already author/props-derived). Rejected: positional walk-order ids — fragile to
  conditional/reordered flows and to `view` drift between the render cycle and the
  next cycle's pre-fold decode (connectors refresh in between).
- **State hooks are default methods on `Component`** (not a separate trait):
  `doc.flow` is `Vec<Box<dyn Component<Msg = M>>>`, so only methods on `Component`
  itself are callable through the trait object. Default no-ops keep existing
  components untouched.

## Architecture

### 1. Data model (`manifest.rs`)

A new `DocState` payload added to `Manifest`, sealed alongside `version`/`regions`:

```rust
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct DocState {
    /// Document-level, app-owned. Set by the app in `view`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc: Option<serde_json::Value>,
    /// Component-level, keyed by each component's `state_key()`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub components: BTreeMap<String, serde_json::Value>,
}

pub struct Manifest {
    pub version: u64,
    pub regions: Vec<Region>,
    #[serde(default)]
    pub state: DocState,   // NEW
}
```

- `serde_json::Value` keeps the framework agnostic to the app's types — it only
  encrypts and carries; the app/component owns serialization.
- `BTreeMap` gives deterministic serialization (stable sealed bytes, stable goldens).
- `#[serde(default)]` keeps deserialization of older/absent payloads robust.
- `recover_regions` initializes `state: DocState::default()`.

### 2. State hooks on `Component` (`component.rs`)

Two default methods; `Some(state_key)` opts a component in. Stateless components
(`Notice`, `Checkbox`, `CalendarView`, `HighlightableText`) inherit the no-ops and
are untouched.

```rust
/// Stable, props-derived key under which this component's state is carried.
/// `None` (default) = stateless.
fn state_key(&self) -> Option<String> { None }

/// The state to seal at render time — the base the document is rendered with.
fn render_state(&self) -> Option<serde_json::Value> { None }
```

The key must be derived from **stable identity props** (e.g.
`format!("stepper:{}", self.name)`), not volatile content, so it is identical when
`view` rebuilds the component at render and again at the next cycle's pre-fold
decode (the React-key discipline).

### 3. Document-level state (`document.rs`)

`Document<M>` gains an app-set state slot and a constructor:

```rust
pub struct Document<M> {
    pub key: DocKey,
    pub flow: Vec<Box<dyn Component<Msg = M>>>,
    pub state: Option<serde_json::Value>,   // NEW
}
// existing `keyed(..)` sets `state: None`; add `keyed_with_state(key, flow, value)`.
```

### 4. Render-side collection (`runtime.rs`)

In `render_document`, after `recover_regions(&compiled)?.with_version(version)`, the
framework walks `doc.flow` and populates the manifest's state, then seals as today:

```rust
let mut manifest = recover_regions(&compiled)?.with_version(version);
manifest.state.doc = doc.state.clone();
for c in &doc.flow {
    if let (Some(k), Some(v)) = (c.state_key(), c.render_state()) {
        manifest.state.components.insert(k, v);
    }
}
let pdf = embed_manifest(&document_to_pdf(&compiled)?, &manifest, key)?;
```

`embed_manifest`/`extract_manifest`/`crypto.rs` are **unchanged** — they serialize
and seal the whole `Manifest`, so the new field rides along. The loop stores this
manifest in `DocEntry` exactly as today.

### 5. Decode-side access — the keystone (no signature change)

The loop already calls `c.decode(&region_ink, &entry.manifest)` with the prior
render's manifest, which now carries state. A stateful component reads its **carried
base**:

```rust
let base = manifest.state.components.get(&self.state_key()?);  // NOT self's prop
```

Document-level state is read from `manifest.state.doc`. Decode interpreting ink
against the *carried* base (even when server `Model` has moved on) is the property
this whole spec exists to establish.

### 6. Proof consumer — `Stepper` (`components/stepper.rs`)

A genuinely stateful component shipped in `inkapp-core`, whose state lives **only in
the document** (no connector behind it):

- `state_key()` → `Some(format!("stepper:{}", self.name))`;
  `render_state()` → its current count.
- render: shows count N (from carried/prop state) and an increment region.
- read/decode: computes its result from the **carried base N** read out of
  `manifest.state.components`, provably ignoring its own current prop.

This is the doc's own model of per-document own-state that "lives nowhere else"
made real, and the consumer that keeps the seam from being dead weight.

### 7. Relationship to existing mechanisms

- **Version vs state.** `version`/`guard_version` already mark the document
  *generation*; the state field carries the *content base*. Together they are the
  appdx's "decode ink against the right base version." `guard_version` is unchanged.
- **Reconcile hash.** `render_document` hashes the Typst source, not the state. A
  state change that *also* changes visible render (the `Stepper`'s count) changes the
  source → hash → re-push, naturally. A state-only change with no visual effect does
  not re-push (correct: nothing visible changed); decode correctness still holds
  because the stored `entry.manifest` carries the right state regardless of hash.
- **Live loop vs PDF travel.** As with `version`+`regions` today, the loop uses the
  in-memory `entry.manifest` copy; a dedicated test proves the *sealed PDF* carries
  the state losslessly. Same split, already established.

## Testing

- **`stepper_state.rs` (keystone):** a `Stepper` built with prop count = 9, decoded
  against a manifest whose carried state says base = 5 with one increment stroke →
  result is base-relative to **5**, not 9. Proves decode uses carried state over
  self-prop.
- **Loop-level step test:** render at count 5 → move `Model` to 9 with no re-render →
  feed ink on the stale doc → assert the decoded message is version-correct against
  base 5. (Realistic path through `App::step`.)
- **Seal round-trip (extend `embed.rs`):** `embed_manifest` → `extract_manifest` on
  a real PDF returns an identical `DocState` (doc blob + component map).
- **No-cleartext (extend `embed.rs`):** the document-level value's bytes and a
  component count do **not** appear anywhere in the PDF bytes (mirrors existing
  manifest-seal tests).
- **Migration:** existing literal `Manifest { version, regions }` constructions
  (`manifest.rs`, `tests/checkbox_state.rs`, and any others) compile with
  `state: DocState::default()` / `..Default::default()`; the full suite stays green.

## Files

- `crates/inkapp-core/src/manifest.rs` — `DocState`, `Manifest.state`,
  `recover_regions` init.
- `crates/inkapp-core/src/component.rs` — `state_key`/`render_state` default methods.
- `crates/inkapp-core/src/document.rs` — `Document.state` + `keyed_with_state`.
- `crates/inkapp-core/src/runtime.rs` — render-side state collection.
- `crates/inkapp-core/src/components/stepper.rs` — new proof component (+ `mod`).
- `crates/inkapp-core/tests/stepper_state.rs` — keystone + loop step test.
- `crates/inkapp-core/tests/embed.rs` — extend with state round-trip + no-cleartext.
- `crates/inkapp-core/src/embed.rs`, `crypto.rs` — **unchanged** (seal the whole
  manifest).
- `docs/appdx.md` — mark the State and Encryption state-field notes **(Built)**;
  update the top banner. *(Definition of done — see the spec's stated goal.)*

## Out of scope (stays open / future)

The event-sourcing log & merge-type declaration, multi-device vector clocks,
demand-driven refresh, and migrating the other three components to authored Typst.
The state field is the **substrate** those build on; none are in this spec.
