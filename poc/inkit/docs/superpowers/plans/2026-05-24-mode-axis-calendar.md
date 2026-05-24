# The Mode Axis + Calendar Connectors ("M") Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers-extended-cc:subagent-driven-development (recommended) or superpowers-extended-cc:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the appdx "Components" mode axis real — a `Mode { ReadOnly, Editable }` field convention with a reusable `CalendarView` that flips between Display-like and Control-like behavior, driven by *real* connector capability (a read-only ICS feed and a writable local calendar), demonstrated in a new `agenda` app.

**Architecture:** `Mode` and `EventRow` are shared types in `inkapp-core`. `CalendarView<M>` carries `events`, a `mode`, and a non-capturing `on_cancel: fn(&str)->M` factory; its `render` and `decode` both branch on the same `mode` value. Two new connector crates produce `EventRow`s: `inkapp-ics` (read-only feed, `flush` no-op) and `inkapp-localcal` (writable, reusing Spec #6's optimistic-overlay + deferred-flush pattern). A new `apps/agenda` wires both and lets `view` choose each `CalendarView`'s mode from the backing connector's capability. The `inkapp-core` `widgets/` module is renamed to `components/` first (the doc's vocabulary; the live view flow is `dyn Component`).

**Tech Stack:** Rust (workspace), Typst 0.14 (render), tokio/async-trait (connector loop), the `ical` crate (ICS parsing), serde/serde_json (localcal persistence), the inkapp harness (e2e).

---

### Task 0: Rename `widgets/` module → `components/`

**Goal:** Align the module name with the doc's vocabulary (and the live `dyn Component` flow) before `CalendarView` lands there. Behavior-preserving; the `Widget` *trait* and `widget.rs` are untouched.

**Files:**
- Rename (git mv): `crates/inkapp-core/src/widgets/` → `crates/inkapp-core/src/components/` (its files: `checkbox.rs`, `highlight_text.rs`, `notice.rs`, `mod.rs`)
- Modify: `crates/inkapp-core/src/lib.rs` (module decl + doc comment)
- Modify: `crates/inkapp/src/lib.rs` (facade re-export)
- Modify (mechanical, scoped replace): every `.rs` under `crates/*/src`, `crates/*/tests`, `apps/*/src`, `apps/*/tests` that references the `widgets` module path
- Leave untouched: `crates/inkapp-core/src/widget.rs`, the `Widget` trait, all `docs/superpowers/specs/*` and `docs/superpowers/plans/*` (historical)

**Acceptance Criteria:**
- [ ] `crates/inkapp-core/src/components/` exists; `src/widgets/` is gone
- [ ] No `.rs` file under `crates/` or `apps/` references `inkapp_core::widgets` / `inkapp::widgets` / `mod widgets`
- [ ] The `Widget` trait and `widget.rs` still exist and compile unchanged
- [ ] Full workspace build + test green

**Verify:** `cargo build --workspace && cargo test --workspace` → all pass; `grep -rn '\bwidgets\b' crates apps --include='*.rs'` → no matches

**Steps:**

- [ ] **Step 1: Move the module directory**

```bash
git mv crates/inkapp-core/src/widgets crates/inkapp-core/src/components
```

- [ ] **Step 2: Update the module declaration and doc comment in `crates/inkapp-core/src/lib.rs`**

Change line 1's doc comment `render, manifest, widgets, readback` → `render, manifest, components, readback`, and the module declaration:

```rust
pub mod components;
```

(replacing `pub mod widgets;` — keep `pub mod widget;` as-is, it is the `Widget` trait).

- [ ] **Step 3: Update the facade re-export in `crates/inkapp/src/lib.rs`**

```rust
pub use inkapp_core::{flow, widget, components};
```

(replacing `pub use inkapp_core::{flow, widget, widgets};`).

- [ ] **Step 4: Scoped, whole-word replace of the module path across code**

The plural token `widgets` is only ever the module name; `\b` word boundaries protect the singular `widget`/`widget.rs`/`RenderCx`. Run:

```bash
grep -rl '\bwidgets\b' crates apps --include='*.rs' \
  | xargs sed -i 's/\bwidgets\b/components/g'
```

This rewrites `use inkapp_core::widgets::checkbox::Checkbox;` → `...::components::checkbox::Checkbox;` and the harmless doc-comment mentions in `widget.rs`/`ink.rs` (now reading "components", which is correct).

- [ ] **Step 5: Tidy the core crate description (optional, cosmetic)**

In `crates/inkapp-core/Cargo.toml`, change the `description` mention of "widgets" to "components" so the manifest matches.

- [ ] **Step 6: Build, test, and confirm no stragglers**

```bash
cargo build --workspace
cargo test --workspace
grep -rn '\bwidgets\b' crates apps --include='*.rs'   # expect: no output
```

Expected: build + tests pass; grep prints nothing.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "inkapp-core: rename widgets/ module -> components/ (doc vocabulary; Widget trait untouched)"
```

---

### Task 1: `Mode` enum + `EventRow` type in `inkapp-core`

**Goal:** Add the two shared types the component and both connectors depend on: the `Mode` axis and the `EventRow` calendar shape.

**Files:**
- Create: `crates/inkapp-core/src/mode.rs`
- Create: `crates/inkapp-core/src/calendar.rs`
- Modify: `crates/inkapp-core/src/lib.rs` (declare + re-export both)
- Test: `crates/inkapp-core/tests/calendar_types.rs`

**Acceptance Criteria:**
- [ ] `inkapp_core::Mode` (`ReadOnly | Editable`) is `Copy + PartialEq`
- [ ] `inkapp_core::EventRow` has `uid, summary, start, end, cancelled` and is `Clone + PartialEq`
- [ ] Both are re-exported from the crate root

**Verify:** `cargo test -p inkapp-core --test calendar_types` → 2 passed

**Steps:**

- [ ] **Step 1: Write the failing test**

`crates/inkapp-core/tests/calendar_types.rs`:

```rust
use inkapp_core::calendar::EventRow;
use inkapp_core::mode::Mode;

#[test]
fn mode_is_copy_and_comparable() {
    let m = Mode::Editable;
    let n = m; // requires Copy
    assert_eq!(m, n);
    assert_ne!(Mode::ReadOnly, Mode::Editable);
}

#[test]
fn event_row_constructs_and_compares() {
    let a = EventRow {
        uid: "e1".into(),
        summary: "Standup".into(),
        start: "20260525T090000Z".into(),
        end: "20260525T091500Z".into(),
        cancelled: false,
    };
    let b = a.clone();
    assert_eq!(a, b);
    assert!(!a.cancelled);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p inkapp-core --test calendar_types`
Expected: FAIL to compile — `unresolved import inkapp_core::mode` / `calendar`.

- [ ] **Step 3: Create `crates/inkapp-core/src/mode.rs`**

```rust
//! The interaction-mode axis. A component carries a `Mode` as a *field* (not a
//! trait parameter); its `render` and `decode` both branch on the same value, so
//! a `ReadOnly` render that drew no affordance cannot have a `decode` that reads
//! one. `view`/`update` chooses the mode from the backing connector's capability
//! — the component never touches a connector.

/// Whether a component exposes edit affordances and decodes structured ink.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Renders content, decodes nothing (Display behavior).
    ReadOnly,
    /// Renders affordances, decodes structured ink into messages (Control behavior).
    Editable,
}
```

- [ ] **Step 4: Create `crates/inkapp-core/src/calendar.rs`**

```rust
//! Shared calendar event shape, produced by calendar connectors (the read-only
//! ICS feed, the writable local calendar) and rendered by `CalendarView`. Kept in
//! core so the component and every calendar connector agree on one type.

/// One calendar event, reduced to the fields inkapp renders this slice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventRow {
    /// Stable id from the source. Used to build the app message on cancel; the
    /// region name is the *index*, not the uid, so uid needs no sanitization.
    pub uid: String,
    pub summary: String,
    /// RFC 5545 DTSTART, carried verbatim (no timezone normalization this slice).
    pub start: String,
    /// RFC 5545 DTEND, carried verbatim.
    pub end: String,
    /// Edit/optimistic state. Read-only feeds always produce `false`.
    pub cancelled: bool,
}
```

- [ ] **Step 5: Declare and re-export in `crates/inkapp-core/src/lib.rs`**

Add to the `pub mod` block (alphabetical neighborhood): `pub mod calendar;` and `pub mod mode;`. Add to the re-export block:

```rust
pub use calendar::EventRow;
pub use mode::Mode;
```

- [ ] **Step 6: Run test to verify it passes**

Run: `cargo test -p inkapp-core --test calendar_types`
Expected: PASS (2 passed).

- [ ] **Step 7: Commit**

```bash
git add crates/inkapp-core/src/mode.rs crates/inkapp-core/src/calendar.rs \
        crates/inkapp-core/src/lib.rs crates/inkapp-core/tests/calendar_types.rs
git commit -m "inkapp-core: Mode axis enum + EventRow calendar type (shared by component and connectors)"
```

---

### Task 2: `CalendarView` component (mode-branched render + decode)

**Goal:** The heart of M — one reusable component whose `render` and `decode` both branch on `mode`: ReadOnly renders inert rows and decodes nothing; Editable renders a per-event cancel affordance in its own region and decodes a mark into one `on_cancel(uid)` message per event.

**Files:**
- Create: `crates/inkapp-core/src/components/calendar_view.rs`
- Modify: `crates/inkapp-core/src/components/mod.rs` (declare the module)
- Test: `crates/inkapp-core/tests/calendar_view.rs`

**Acceptance Criteria:**
- [ ] `CalendarView::read_only(events)` render emits no `<region>` and no affordance box; event text present
- [ ] `CalendarView::editable(events, f)` render emits one `evt-<i>` region + one affordance box per event
- [ ] Both modes compile through Typst (`render_document` is `Ok`)
- [ ] Identical ink decodes to `[]` in ReadOnly and `[on_cancel(uid)]` per marked event in Editable

**Verify:** `cargo test -p inkapp-core --test calendar_view` → 4 passed

**Steps:**

- [ ] **Step 1: Write the failing test**

`crates/inkapp-core/tests/calendar_view.rs`:

```rust
use inkapp_core::calendar::EventRow;
use inkapp_core::component::Component;
use inkapp_core::components::calendar_view::CalendarView;
use inkapp_core::crypto::Key;
use inkapp_core::document::Document;
use inkapp_core::flow;
use inkapp_core::geometry::{PdfPoint, PdfRect};
use inkapp_core::ink::{RegionInk, Stroke};
use inkapp_core::manifest::{Manifest, Region};
use inkapp_core::runtime::render_document;
use inkapp_core::widget::RenderCx;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Msg {
    Cancel(String),
}

fn ev(uid: &str, summary: &str) -> EventRow {
    EventRow {
        uid: uid.into(),
        summary: summary.into(),
        start: "20260524T090000Z".into(),
        end: "20260524T100000Z".into(),
        cancelled: false,
    }
}

#[test]
fn read_only_emits_no_region_or_affordance() {
    let cv = CalendarView::<Msg>::read_only(vec![ev("e1", "Standup")]);
    let src = cv.render(&mut RenderCx::new(0));
    assert!(!src.contains("<region>"), "read-only renders no region: {src}");
    assert!(!src.contains("rect("), "read-only renders no affordance box: {src}");
    assert!(src.contains("Standup"), "event text present: {src}");
}

#[test]
fn editable_emits_region_and_affordance_per_event() {
    let cv = CalendarView::editable(vec![ev("e1", "Standup"), ev("e2", "Review")], |uid| {
        Msg::Cancel(uid.to_string())
    });
    let src = cv.render(&mut RenderCx::new(0));
    assert!(src.contains("name: \"evt-0\""), "first event region: {src}");
    assert!(src.contains("name: \"evt-1\""), "second event region: {src}");
    assert_eq!(src.matches("rect(").count(), 2, "one affordance box per event");
}

#[test]
fn both_modes_compile_through_typst() {
    let key = Key::from_bytes([0u8; 32]);
    let ro: Document<Msg> = Document::keyed("ro", flow![CalendarView::<Msg>::read_only(vec![ev("e1", "Standup")])]);
    let ed: Document<Msg> = Document::keyed("ed", flow![CalendarView::editable(vec![ev("e1", "Standup")], |uid| Msg::Cancel(uid.to_string()))]);
    assert!(render_document(&ro, 1, &key).is_ok(), "read-only compiles");
    assert!(render_document(&ed, 1, &key).is_ok(), "editable compiles");
}

#[test]
fn read_only_decodes_nothing_editable_decodes_cancel() {
    let manifest = Manifest {
        version: 1,
        regions: vec![Region {
            name: "evt-0".into(),
            page: 0,
            rect: PdfRect { x0: 0.0, y0: 0.0, x1: 14.0, y1: 14.0 },
        }],
    };
    let ink = vec![RegionInk {
        region: "evt-0".into(),
        strokes: vec![Stroke {
            points: vec![PdfPoint { x: 7.0, y: 7.0 }],
            highlighter: false,
        }],
    }];

    let ro = CalendarView::<Msg>::read_only(vec![ev("e1", "Standup")]);
    assert!(ro.decode(&ink, &manifest).is_empty(), "read-only discards ink");

    let ed = CalendarView::editable(vec![ev("e1", "Standup")], |uid| Msg::Cancel(uid.to_string()));
    assert_eq!(ed.decode(&ink, &manifest), vec![Msg::Cancel("e1".to_string())]);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p inkapp-core --test calendar_view`
Expected: FAIL to compile — `calendar_view` module / `CalendarView` not found.

- [ ] **Step 3: Create `crates/inkapp-core/src/components/calendar_view.rs`**

```rust
//! `CalendarView` — the mode axis made into a component. One list of events whose
//! interaction is governed by its `mode` field:
//! - `ReadOnly` renders inert rows and decodes nothing (Display behavior);
//! - `Editable` renders a per-event cancel affordance in its own region and
//!   decodes a mark into one app message per cancelled event (Control behavior).
//!
//! Reusable across apps: it carries the message *factory* `on_cancel` (a
//! non-capturing `fn(&str) -> M`, uid -> Msg) rather than a stored closure,
//! matching `Checkbox`'s value-message pattern. `view`/`update` chooses the mode
//! from the backing connector's capability — the component never sees a connector.

use crate::calendar::EventRow;
use crate::component::Component;
use crate::ink::RegionInk;
use crate::manifest::Manifest;
use crate::mode::Mode;
use crate::widget::RenderCx;

/// A calendar/agenda component. `M` is the app message type.
pub struct CalendarView<M> {
    events: Vec<EventRow>,
    mode: Mode,
    /// Only set in `Editable`; maps a cancelled event's uid to an app message.
    on_cancel: Option<fn(&str) -> M>,
}

impl<M> CalendarView<M> {
    /// A read-only agenda: inert rows, decodes nothing (Display behavior).
    pub fn read_only(events: Vec<EventRow>) -> Self {
        Self { events, mode: Mode::ReadOnly, on_cancel: None }
    }

    /// An editable calendar: each event gets a cancel affordance; a mark decodes
    /// to `on_cancel(uid)` (Control behavior).
    pub fn editable(events: Vec<EventRow>, on_cancel: fn(&str) -> M) -> Self {
        Self { events, mode: Mode::Editable, on_cancel: Some(on_cancel) }
    }

    /// Escape for a Typst string literal (`\` and `"` only — other markup chars
    /// are literal inside a `#"..."` string expression). Mirrors `Notice`.
    fn esc(s: &str) -> String {
        s.replace('\\', "\\\\").replace('"', "\\\"")
    }
}

impl<M> Component for CalendarView<M> {
    type Msg = M;

    fn render(&self, _cx: &mut RenderCx) -> String {
        let mut s = String::new();
        for (i, ev) in self.events.iter().enumerate() {
            let label = Self::esc(&format!("{} — {}", ev.summary, ev.start));
            match self.mode {
                Mode::ReadOnly => {
                    // Inert row; a cancelled event is struck through. No region:
                    // there is nothing to decode, so nothing to attribute ink to.
                    if ev.cancelled {
                        s.push_str(&format!("#strike[#text[#\"{label}\"]]\n\n"));
                    } else {
                        s.push_str(&format!("#text[#\"{label}\"]\n\n"));
                    }
                }
                Mode::Editable => {
                    // A per-event region `evt-<i>` recovered from layout (the
                    // in-flow `here().position()` pattern shared with Checkbox /
                    // HighlightableText), a cancel-box affordance, and the label.
                    s.push_str(&format!(
                        "#box[#context [#metadata((name: \"evt-{i}\", \
                           page: here().position().page - 1, x: here().position().x / 1pt, \
                           y: here().position().y / 1pt, w: 14, h: 14)) <region>]\
                         #rect(width: 14pt, height: 14pt, stroke: 0.5pt)] #text[#\"{label}\"]\n\n"
                    ));
                }
            }
        }
        s
    }

    fn decode(&self, ink: &[RegionInk], manifest: &Manifest) -> Vec<M> {
        // ReadOnly discards all ink: branching the same `mode` that render used is
        // what guarantees a no-affordance render can't decode an affordance.
        let (Mode::Editable, Some(on_cancel)) = (self.mode, self.on_cancel) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for (i, ev) in self.events.iter().enumerate() {
            let name = format!("evt-{i}");
            let Some(region) = manifest.regions.iter().find(|r| r.name == name) else {
                continue;
            };
            let marked = ink
                .iter()
                .filter(|ri| ri.region == name)
                .flat_map(|ri| &ri.strokes)
                .any(|stroke| stroke.points.iter().any(|p| region.rect.contains(p.x, p.y)));
            if marked {
                out.push(on_cancel(&ev.uid));
            }
        }
        out
    }
}
```

- [ ] **Step 4: Declare the module in `crates/inkapp-core/src/components/mod.rs`**

Add (keep alphabetical with the existing `checkbox`, `highlight_text`, `notice`):

```rust
pub mod calendar_view;
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p inkapp-core --test calendar_view`
Expected: PASS (4 passed).

- [ ] **Step 6: Commit**

```bash
git add crates/inkapp-core/src/components/calendar_view.rs \
        crates/inkapp-core/src/components/mod.rs \
        crates/inkapp-core/tests/calendar_view.rs
git commit -m "inkapp-core: CalendarView component spanning ReadOnly (Display) and Editable (Control) by mode"
```

---

### Task 3: `inkapp-ics` — read-only ICS feed connector

**Goal:** The doc's "read-only feed" archetype: parse a committed `.ics` into `EventRow`s, cache them behind an `RwLock`, single-flight `refresh`, no-op `flush`.

**Files:**
- Create: `crates/inkapp-ics/Cargo.toml`
- Create: `crates/inkapp-ics/src/lib.rs`
- Create: `crates/inkapp-ics/fixtures/feed.ics`
- Create: `crates/inkapp-ics/tests/ics.rs`
- Modify: `Cargo.toml` (workspace members)

**Acceptance Criteria:**
- [ ] `IcsConnector::from_ics(text)` parses VEVENTs into `EventRow`s (uid/summary/start/end)
- [ ] `events()` reads the warm cache; `refresh()` re-parses into it
- [ ] `flush()` is a no-op and does not panic
- [ ] Concurrent `refresh()`es both succeed and leave the cache correct

**Verify:** `cargo test -p inkapp-ics` → all pass

> **Note on the `ical` crate:** API used is `ical::IcalParser::new(reader)` yielding `Result<IcalCalendar, _>`, each `IcalCalendar { events: Vec<IcalEvent> }`, each `IcalEvent { properties: Vec<ical::property::Property> }`, `Property { name: String, value: Option<String>, .. }`. If `cargo build` reveals a version skew in these names, run `cargo doc -p ical --open` and adjust the field access in `parse_ics` only.

**Steps:**

- [ ] **Step 1: Create the crate manifest `crates/inkapp-ics/Cargo.toml`**

```toml
[package]
name = "inkapp-ics"
version = "0.1.0"
edition = "2021"
license = "MIT"
description = "Read-only ICS calendar feed connector for inkapp (the doc's read-only feed archetype)"

[dependencies]
ical = "0.11"
async-trait = "0.1"
inkapp-core = { path = "../inkapp-core" }

[dev-dependencies]
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

- [ ] **Step 2: Register the crate in the workspace `Cargo.toml`**

Add `"crates/inkapp-ics",` to the `members` array.

- [ ] **Step 3: Create the fixture `crates/inkapp-ics/fixtures/feed.ics`**

```text
BEGIN:VCALENDAR
VERSION:2.0
PRODID:-//inkapp//agenda fixture//EN
BEGIN:VEVENT
UID:standup@inkapp
SUMMARY:Standup
DTSTART:20260525T090000Z
DTEND:20260525T091500Z
END:VEVENT
BEGIN:VEVENT
UID:review@inkapp
SUMMARY:Design review
DTSTART:20260525T140000Z
DTEND:20260525T150000Z
END:VEVENT
END:VCALENDAR
```

- [ ] **Step 4: Write the failing test `crates/inkapp-ics/tests/ics.rs`**

```rust
use inkapp_core::connector::Connector;
use inkapp_ics::IcsConnector;
use std::sync::Arc;

const SAMPLE: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:e1\r\nSUMMARY:Standup\r\nDTSTART:20260525T090000Z\r\nDTEND:20260525T091500Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

#[tokio::test]
async fn parses_and_caches_events() {
    let c = IcsConnector::from_ics(SAMPLE);
    let evs = c.events();
    assert_eq!(evs.len(), 1);
    assert_eq!(evs[0].uid, "e1");
    assert_eq!(evs[0].summary, "Standup");
    assert_eq!(evs[0].start, "20260525T090000Z");
    assert!(!evs[0].cancelled);
}

#[tokio::test]
async fn refresh_repopulates_cache() {
    // Two concurrent refreshes (single-flight collapse is core's SingleFlight
    // test; here we assert both succeed and the cache is correct after).
    let c = Arc::new(IcsConnector::from_ics(SAMPLE));
    let (a, b) = tokio::join!(c.refresh(), c.refresh());
    a.unwrap();
    b.unwrap();
    assert_eq!(c.events().len(), 1);
}

#[tokio::test]
async fn flush_is_noop() {
    let c = IcsConnector::from_ics(SAMPLE);
    c.flush().await; // read-only feed: nothing to push, no panic
    assert_eq!(c.events().len(), 1);
}

#[test]
fn fixture_feed_parses() {
    let c = IcsConnector::from_fixture();
    assert!(c.events().len() >= 2, "fixture feed has at least two events");
}
```

- [ ] **Step 5: Run test to verify it fails**

Run: `cargo test -p inkapp-ics`
Expected: FAIL to compile — crate `inkapp_ics` has no `IcsConnector`.

- [ ] **Step 6: Create `crates/inkapp-ics/src/lib.rs`**

```rust
//! Read-only ICS calendar feed, as an inkapp `Connector` plugin (the doc's
//! "read-only feed" archetype: pull, cache, done). `refresh` parses the .ics
//! source into the `RwLock` cache (single-flighted via core's `SingleFlight`);
//! `flush` is a no-op; the app-facing sync `events()` reads the warm cache.

use std::io::BufReader;
use std::sync::{Arc, RwLock};

use ical::IcalParser;
use inkapp_core::calendar::EventRow;
use inkapp_core::connector::{Connector, ConnectorError};
use inkapp_core::single_flight::SingleFlight;

/// A read-only calendar feed. Reads come from the warm cache; `refresh` re-parses
/// `source`. A live build would fetch `source` over HTTP inside `refresh`
/// (outside the lock); here it's a committed string.
pub struct IcsConnector {
    source: String,
    cache: Arc<RwLock<Vec<EventRow>>>,
    refresh_flight: SingleFlight<Result<(), ConnectorError>>,
}

impl IcsConnector {
    /// Build from raw .ics text; the cache is pre-populated so `events()` works
    /// before the first explicit `refresh`.
    pub fn from_ics(source: impl Into<String>) -> Self {
        let source = source.into();
        let events = parse_ics(&source);
        Self {
            source,
            cache: Arc::new(RwLock::new(events)),
            refresh_flight: SingleFlight::new(),
        }
    }

    /// The committed sample feed.
    pub fn from_fixture() -> Self {
        Self::from_ics(include_str!("../fixtures/feed.ics"))
    }

    /// The cached events (warm read under the read lock).
    pub fn events(&self) -> Vec<EventRow> {
        self.cache.read().unwrap().clone()
    }
}

/// Parse VEVENTs into `EventRow`s, taking the fields we render. Events without a
/// UID are skipped (no stable identity); other fields default to empty.
fn parse_ics(source: &str) -> Vec<EventRow> {
    let mut out = Vec::new();
    let parser = IcalParser::new(BufReader::new(source.as_bytes()));
    // `flatten` drops any calendar that failed to parse (Result -> IntoIterator).
    for cal in parser.flatten() {
        for event in cal.events {
            let mut uid = None;
            let (mut summary, mut start, mut end) =
                (String::new(), String::new(), String::new());
            for prop in event.properties {
                let val = prop.value.unwrap_or_default();
                match prop.name.as_str() {
                    "UID" => uid = Some(val),
                    "SUMMARY" => summary = val,
                    "DTSTART" => start = val,
                    "DTEND" => end = val,
                    _ => {}
                }
            }
            if let Some(uid) = uid {
                out.push(EventRow { uid, summary, start, end, cancelled: false });
            }
        }
    }
    out
}

#[async_trait::async_trait]
impl Connector for IcsConnector {
    fn name(&self) -> &str {
        "ics"
    }

    async fn refresh(&self) -> Result<(), ConnectorError> {
        let source = self.source.clone();
        let cache = Arc::clone(&self.cache);
        self.refresh_flight
            .run(move || async move {
                let events = parse_ics(&source);
                // Brief write lock, no await held across it (the doc's rule).
                *cache.write().unwrap() = events;
                Ok(())
            })
            .await
    }

    async fn flush(&self) {} // read-only feed: nothing to push
}
```

- [ ] **Step 7: Run test to verify it passes**

Run: `cargo test -p inkapp-ics`
Expected: PASS. If `parses_and_caches_events` fails on field access, see the `ical` note above and adjust `parse_ics`.

- [ ] **Step 8: Commit (includes the lockfile)**

```bash
git add crates/inkapp-ics/ Cargo.toml Cargo.lock
git commit -m "inkapp-ics: read-only ICS feed connector (ical crate, single-flighted refresh, no-op flush)"
```

---

### Task 4: `inkapp-localcal` — writable local calendar connector

**Goal:** A CalDAV-shaped *local* connector so `CalendarView`'s Editable branch runs against real write capability. Reuses Spec #6's pattern: `cancel(uid)` is optimistic (visible this render) + enqueued; `flush` persists the queued cancels to a local store. No retry (local writes don't fail transiently).

**Files:**
- Create: `crates/inkapp-localcal/Cargo.toml`
- Create: `crates/inkapp-localcal/src/lib.rs`
- Create: `crates/inkapp-localcal/tests/localcal.rs`
- Modify: `Cargo.toml` (workspace members)

**Acceptance Criteria:**
- [ ] `cancel(uid)` makes `events()` show that event `cancelled: true` *before* `flush`
- [ ] `flush()` persists; a fresh `persisted(path)` instance still shows the cancel
- [ ] `refresh()` preserves an un-flushed pending cancel (folds the overlay back in)

**Verify:** `cargo test -p inkapp-localcal` → 3 passed

**Steps:**

- [ ] **Step 1: Create the crate manifest `crates/inkapp-localcal/Cargo.toml`**

```toml
[package]
name = "inkapp-localcal"
version = "0.1.0"
edition = "2021"
license = "MIT"
description = "Writable local (CalDAV-shaped) calendar connector for inkapp — stands in for real CalDAV"

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
async-trait = "0.1"
inkapp-core = { path = "../inkapp-core" }

[dev-dependencies]
tempfile = "3"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

- [ ] **Step 2: Register the crate in the workspace `Cargo.toml`**

Add `"crates/inkapp-localcal",` to the `members` array.

- [ ] **Step 3: Write the failing test `crates/inkapp-localcal/tests/localcal.rs`**

```rust
use inkapp_core::connector::Connector;
use inkapp_localcal::LocalCal;
use std::sync::Arc;
use tempfile::NamedTempFile;

#[tokio::test]
async fn cancel_is_optimistically_visible_same_render() {
    let c = LocalCal::fake();
    let before = c.events();
    assert!(before.iter().all(|e| !e.cancelled), "starts uncancelled");
    let uid = before[0].uid.clone();
    c.cancel(&uid);
    let after = c.events();
    assert!(
        after.iter().find(|e| e.uid == uid).unwrap().cancelled,
        "cancel visible before flush"
    );
}

#[tokio::test]
async fn flush_persists_and_survives_reload() {
    let tmp = NamedTempFile::new().unwrap();
    let path = tmp.path().to_path_buf();
    let uid;
    {
        let c = LocalCal::persisted(&path);
        uid = c.events()[0].uid.clone();
        c.cancel(&uid);
        c.flush().await;
    }
    let c2 = LocalCal::persisted(&path);
    assert!(
        c2.events().iter().find(|e| e.uid == uid).unwrap().cancelled,
        "cancel persisted across reload"
    );
}

#[tokio::test]
async fn refresh_preserves_pending_overlay() {
    let c = Arc::new(LocalCal::fake());
    let uid = c.events()[0].uid.clone();
    c.cancel(&uid);
    c.refresh().await.unwrap();
    assert!(
        c.events().iter().find(|e| e.uid == uid).unwrap().cancelled,
        "pending (un-flushed) cancel survives refresh"
    );
}
```

- [ ] **Step 4: Run test to verify it fails**

Run: `cargo test -p inkapp-localcal`
Expected: FAIL to compile — no `LocalCal`.

- [ ] **Step 5: Create `crates/inkapp-localcal/src/lib.rs`**

```rust
//! A writable, CalDAV-shaped *local* calendar connector — the stand-in that lets
//! `CalendarView`'s Editable branch run end to end without real CalDAV. Reads come
//! from an `RwLock` cache; `cancel(uid)` applies an optimistic overlay (visible
//! this same render) AND enqueues a durable cancel; `flush` persists the queued
//! cancels to the local store. Local writes don't fail over a network, so there is
//! no retry / permanent-failure machinery (unlike the Readwise connector).

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use serde::{Deserialize, Serialize};

use inkapp_core::calendar::EventRow;
use inkapp_core::connector::{Connector, ConnectorError};
use inkapp_core::single_flight::SingleFlight;

/// The durable local store: uids whose cancel has been flushed.
#[derive(Default, Serialize, Deserialize)]
struct Store {
    cancelled: HashSet<String>,
}

/// Optimistic, not-yet-flushed cancels recorded this session.
#[derive(Default)]
struct Overlay {
    pending: HashSet<String>,
}

pub struct LocalCal {
    /// Base events (a committed fixture; a live build would load from CalDAV).
    base: Vec<EventRow>,
    cache: Arc<RwLock<Vec<EventRow>>>,
    overlay: Mutex<Overlay>,
    store: Mutex<Store>,
    persist_path: Option<PathBuf>,
    refresh_flight: SingleFlight<Result<(), ConnectorError>>,
}

impl LocalCal {
    fn build(base: Vec<EventRow>, store: Store, persist_path: Option<PathBuf>) -> Self {
        let cache = apply(&base, &store.cancelled, &HashSet::new());
        Self {
            base,
            cache: Arc::new(RwLock::new(cache)),
            overlay: Mutex::new(Overlay::default()),
            store: Mutex::new(store),
            persist_path,
            refresh_flight: SingleFlight::new(),
        }
    }

    /// A tiny inline calendar for tests / the app.
    pub fn fake() -> Self {
        Self::build(sample_events(), Store::default(), None)
    }

    /// Load persisted cancels from `path` (if present); save on flush.
    pub fn persisted(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let store = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self::build(sample_events(), store, Some(path))
    }

    /// The current events (warm read under the read lock).
    pub fn events(&self) -> Vec<EventRow> {
        self.cache.read().unwrap().clone()
    }

    /// Record a cancel: optimistic (cache reflects it now) and enqueued for flush.
    pub fn cancel(&self, uid: &str) {
        self.overlay.lock().unwrap().pending.insert(uid.to_string());
        self.recompute();
    }

    /// Rebuild the cache from base + persisted store + pending overlay.
    fn recompute(&self) {
        let persisted = self.store.lock().unwrap().cancelled.clone();
        let pending = self.overlay.lock().unwrap().pending.clone();
        *self.cache.write().unwrap() = apply(&self.base, &persisted, &pending);
    }

    fn save(&self) {
        if let Some(path) = &self.persist_path {
            let store = self.store.lock().unwrap();
            if let Ok(json) = serde_json::to_string_pretty(&*store) {
                let _ = std::fs::write(path, json);
            }
        }
    }
}

/// Project base events given persisted + pending cancels.
fn apply(base: &[EventRow], persisted: &HashSet<String>, pending: &HashSet<String>) -> Vec<EventRow> {
    base.iter()
        .map(|e| {
            let mut e = e.clone();
            if persisted.contains(&e.uid) || pending.contains(&e.uid) {
                e.cancelled = true;
            }
            e
        })
        .collect()
}

fn sample_events() -> Vec<EventRow> {
    vec![
        EventRow {
            uid: "mine-1".into(),
            summary: "Write spec".into(),
            start: "20260525T110000Z".into(),
            end: "20260525T120000Z".into(),
            cancelled: false,
        },
        EventRow {
            uid: "mine-2".into(),
            summary: "Gym".into(),
            start: "20260525T180000Z".into(),
            end: "20260525T190000Z".into(),
            cancelled: false,
        },
    ]
}

#[async_trait::async_trait]
impl Connector for LocalCal {
    fn name(&self) -> &str {
        "localcal"
    }

    async fn refresh(&self) -> Result<(), ConnectorError> {
        // "Fetch" = base + persisted store, with the pending overlay folded back
        // in so an un-flushed cancel survives. Single-flighted; no lock across await.
        let base = self.base.clone();
        let cache = Arc::clone(&self.cache);
        let persisted = self.store.lock().unwrap().cancelled.clone();
        let pending = self.overlay.lock().unwrap().pending.clone();
        self.refresh_flight
            .run(move || async move {
                *cache.write().unwrap() = apply(&base, &persisted, &pending);
                Ok(())
            })
            .await
    }

    async fn flush(&self) {
        // Move pending cancels into the persisted store and save. No retry: local
        // writes can't fail transiently, so there's no permanent-failure list.
        let pending = {
            let mut ov = self.overlay.lock().unwrap();
            std::mem::take(&mut ov.pending)
        };
        self.store.lock().unwrap().cancelled.extend(pending);
        self.save();
        self.recompute();
    }
}
```

- [ ] **Step 6: Run test to verify it passes**

Run: `cargo test -p inkapp-localcal`
Expected: PASS (3 passed).

- [ ] **Step 7: Commit**

```bash
git add crates/inkapp-localcal/ Cargo.toml Cargo.lock
git commit -m "inkapp-localcal: writable local calendar connector (optimistic cancel + deferred flush; CalDAV stand-in)"
```

---

### Task 5: `agenda` app — two connectors, mode chosen by capability

**Goal:** A new app where one document holds two `CalendarView`s and `view` chooses each mode from the backing connector's capability: ReadOnly for the ICS feed, Editable for the writable local calendar. This is the appdx's "policy, not just capability," shown literally.

**Files:**
- Create: `apps/agenda/Cargo.toml`
- Create: `apps/agenda/src/lib.rs`
- Create: `apps/agenda/src/main.rs`
- Create: `apps/agenda/src/serve.rs`
- Create: `apps/agenda/tests/app.rs`
- Modify: `Cargo.toml` (workspace members)

**Acceptance Criteria:**
- [ ] `view` returns one document whose source contains the editable calendar's `evt-0` region
- [ ] `update(EventCancelled{uid})` routes to `cx.cal.cancel(uid)` (event shows cancelled)
- [ ] `agenda` binary builds; `main` renders the initial set

**Verify:** `cargo test -p agenda` → 2 passed; `cargo build -p agenda` → ok

**Steps:**

- [ ] **Step 1: Create `apps/agenda/Cargo.toml`**

```toml
[package]
name = "agenda"
version = "0.1.0"
edition = "2021"
license = "MIT"
description = "inkapp agenda app: the mode axis made real (read-only ICS feed + editable local calendar)"

[dependencies]
inkapp = { path = "../../crates/inkapp" }
inkapp-core = { path = "../../crates/inkapp-core" }
inkapp-ics = { path = "../../crates/inkapp-ics" }
inkapp-localcal = { path = "../../crates/inkapp-localcal" }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
zip = "2"
```

- [ ] **Step 2: Register the app in the workspace `Cargo.toml`**

Add `"apps/agenda",` to the `members` array.

- [ ] **Step 3: Write the failing test `apps/agenda/tests/app.rs`**

```rust
use agenda::{update, view, App, Connectors, Msg};
use inkapp::document_source;

#[test]
fn view_renders_one_document_with_an_editable_region() {
    let cx = Connectors::fake();
    let docs = view(&App, &cx);
    assert_eq!(docs.0.len(), 1, "one agenda document");
    let src = document_source(&docs.0[0]);
    // The editable (local) calendar mints per-event regions; the read-only feed
    // mints none — so the only regions present come from the editable calendar.
    assert!(src.contains("name: \"evt-0\""), "editable calendar has regions: {src}");
}

#[test]
fn cancel_routes_to_local_calendar() {
    let cx = Connectors::fake();
    let uid = cx.cal.events()[0].uid.clone();
    let mut m = App;
    update(Msg::EventCancelled { uid: uid.clone() }, &mut m, &cx);
    assert!(
        cx.cal.events().iter().find(|e| e.uid == uid).unwrap().cancelled,
        "cancel reached the writable calendar"
    );
}
```

- [ ] **Step 4: Run test to verify it fails**

Run: `cargo test -p agenda`
Expected: FAIL — crate `agenda` does not exist yet.

- [ ] **Step 5: Create `apps/agenda/src/lib.rs`**

```rust
//! The agenda app — the mode axis made real. Two calendar connectors of differing
//! capability back one document: a read-only ICS feed and a writable local
//! calendar. `view` renders a `CalendarView` for each, choosing the mode from the
//! connector's capability (ReadOnly for the feed, Editable for the local cal) —
//! the appdx's "policy, not just capability." The components never see a connector.

pub mod serve;

use std::sync::Arc;

use inkapp::{flow, Document, Documents};
use inkapp_core::components::calendar_view::CalendarView;
use inkapp_core::connector::{Connector, ConnectorSet};
use inkapp_ics::IcsConnector;
use inkapp_localcal::LocalCal;

/// No own state: the events live in the connectors.
pub struct App;

/// The one thing a user can do here: cancel an event on their own calendar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Msg {
    EventCancelled { uid: String },
}

/// Two connectors of differing capability, each shared as `Arc`.
pub struct Connectors {
    pub feed: Arc<IcsConnector>,
    pub cal: Arc<LocalCal>,
}

impl Connectors {
    pub fn fake() -> Self {
        Self {
            feed: Arc::new(IcsConnector::from_fixture()),
            cal: Arc::new(LocalCal::fake()),
        }
    }

    pub fn persisted(path: impl Into<std::path::PathBuf>) -> Self {
        Self {
            feed: Arc::new(IcsConnector::from_fixture()),
            cal: Arc::new(LocalCal::persisted(path)),
        }
    }
}

impl ConnectorSet for Connectors {
    fn connectors(&self) -> Vec<Arc<dyn Connector>> {
        vec![self.feed.clone(), self.cal.clone()]
    }
}

/// The only place app logic lives: route a cancel to the writable calendar.
pub fn update(msg: Msg, _m: &mut App, cx: &Connectors) {
    match msg {
        Msg::EventCancelled { uid } => cx.cal.cancel(&uid),
    }
}

/// One document: the read-only feed agenda (mode chosen from its read-only
/// capability) above the editable local calendar (mode chosen from its writable
/// capability). The component never sees a connector; `view` decides the mode.
pub fn view(_m: &App, cx: &Connectors) -> Documents<Msg> {
    Documents(vec![Document::keyed(
        "agenda",
        flow![
            CalendarView::<Msg>::read_only(cx.feed.events()),
            CalendarView::editable(cx.cal.events(), |uid| Msg::EventCancelled {
                uid: uid.to_string()
            }),
        ],
    )])
}
```

- [ ] **Step 6: Create `apps/agenda/src/main.rs`**

```rust
//! Assemble and run the agenda app. The framework owns the loop body; on-device
//! transport (rmapi) lives in the manual device bar (`serve`). For now `main`
//! renders the initial set and reports.

use agenda::{update, view, App, Connectors};
use inkapp::{app, DocSet, SecretStore};

#[tokio::main]
async fn main() {
    let key = SecretStore::open_default()
        .and_then(|mut s| s.user_key())
        .expect("open secrets store / load user key");
    let mut application = app(App)
        .connector(Connectors::persisted(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/.localcal.json"
        )))
        .update(update)
        .view(view)
        .key(key)
        .build();
    let mut set = DocSet::default();
    let rendered = application.render(&mut set).await.expect("render");
    println!("agenda: rendered {} document(s)", rendered.len());
}
```

- [ ] **Step 7: Create `apps/agenda/src/serve.rs` by copying reading-queue's, with two edits**

```bash
cp apps/reading-queue/src/serve.rs apps/agenda/src/serve.rs
```

Then make exactly these two changes in `apps/agenda/src/serve.rs`:
1. The folder constant: `const FOLDER: &str = "/ReadingQueue";` → `const FOLDER: &str = "/Agenda";`
2. The pull temp dir: `.join("reading-queue-pull")` → `.join("agenda-pull")`

The module already refers to `crate::{Connectors, Msg}` and `Framework<crate::App, Msg, Connectors>`, which resolve to agenda's own types — no other edits needed.

- [ ] **Step 8: Run tests + build to verify they pass**

Run: `cargo test -p agenda && cargo build -p agenda`
Expected: 2 passed; binary builds. If `flow!` fails to infer `M` on the read-only view, the explicit `CalendarView::<Msg>::read_only(..)` annotation already present pins it.

- [ ] **Step 9: Commit**

```bash
git add apps/agenda/ Cargo.toml Cargo.lock
git commit -m "agenda: new app wiring read-only ICS + editable local calendar; view picks mode by capability"
```

---

### Task 6: agenda harness e2e (real-ink, async loop)

**Goal:** Prove the axis end to end through the device path: a real-ink mark on the editable calendar's event region decodes to `EventCancelled` and cancels the event on the writable connector; the read-only feed contributes no regions, so its content can't be edited.

**Files:**
- Create: `crates/inkapp-harness/tests/agenda_loop.rs`
- Modify: `crates/inkapp-harness/Cargo.toml` (dev-deps)

**Acceptance Criteria:**
- [ ] Rendered "agenda" manifest contains `evt-0`/`evt-1` (editable cal) and no feed regions
- [ ] A checkmark on `evt-0` decodes to `EventCancelled { uid: "mine-1" }`
- [ ] After the step, the local calendar shows `mine-1` cancelled

**Verify:** `cargo test -p inkapp-harness --test agenda_loop` → 1 passed

**Steps:**

- [ ] **Step 1: Add dev-dependencies to `crates/inkapp-harness/Cargo.toml`**

Under `[dev-dependencies]`, add:

```toml
agenda = { path = "../../apps/agenda" }
inkapp-ics = { path = "../inkapp-ics" }
inkapp-localcal = { path = "../inkapp-localcal" }
```

- [ ] **Step 2: Write the e2e test `crates/inkapp-harness/tests/agenda_loop.rs`**

```rust
use std::collections::HashMap;

mod common;

use agenda::{update, view, App, Connectors, Msg};
use inkapp_core::document::DocKey;
use inkapp_core::geometry::PdfRect;
use inkapp_core::ink::Stroke;
use inkapp_core::manifest::Manifest;
use inkapp_core::runtime::{app, DocSet};
use inkapp_harness::fixtures::GestureFixture;
use inkapp_remarkable::Remarkable;

fn fixture(name: &str) -> GestureFixture {
    let path = format!(
        "{}/tests/fixtures/gestures/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    GestureFixture::from_json(&bytes).unwrap()
}

fn region_rect(m: &Manifest, name: &str) -> PdfRect {
    m.regions
        .iter()
        .find(|r| r.name == name)
        .unwrap_or_else(|| panic!("region {name:?} not found in manifest"))
        .rect
}

fn device_ink(device: &Remarkable, fix: &GestureFixture, rect: PdfRect, page_h: f64) -> Vec<Stroke> {
    let pdf = fix.transplant_default(rect);
    let bytes = device.write_ink(&pdf, page_h).unwrap();
    device.read_ink(&bytes, page_h).unwrap()
}

#[tokio::test]
async fn agenda_cancel_marks_editable_event_only() {
    let device = Remarkable::new();
    let mut application = app(App)
        .connector(Connectors::fake())
        .update(update)
        .view(view)
        .key(common::test_key())
        .build();
    let mut set = DocSet::default();

    // Cycle 0: render the agenda document.
    let rendered = application.render(&mut set).await.unwrap();
    assert_eq!(rendered.len(), 1, "one agenda document");

    let key = DocKey::new("agenda");
    let manifest = set.manifest(&key).unwrap().clone();
    let page_h = set.page_h(&key).unwrap();

    // The editable (local) calendar mints evt-0/evt-1; the read-only feed mints
    // none — so every region in the manifest belongs to the editable calendar.
    assert!(manifest.regions.iter().any(|r| r.name == "evt-0"));
    assert!(manifest.regions.iter().any(|r| r.name == "evt-1"));
    assert!(
        manifest.regions.iter().all(|r| r.name.starts_with("evt-")),
        "read-only feed contributes no editable regions: {:?}",
        manifest.regions.iter().map(|r| &r.name).collect::<Vec<_>>()
    );

    // Mark the first editable event (evt-0 -> localcal uid "mine-1").
    let check = fixture("checkmark");
    let mut ink: HashMap<String, Vec<Stroke>> = HashMap::new();
    ink.insert(
        key.0.clone(),
        device_ink(&device, &check, region_rect(&manifest, "evt-0"), page_h),
    );

    // Cycle 1: step.
    let cycle = application.step(&mut set, &ink).await.unwrap();

    assert!(
        cycle.decoded.contains(&Msg::EventCancelled { uid: "mine-1".to_string() }),
        "decoded a cancel for the editable event: {:?}",
        cycle.decoded
    );

    // The writable calendar recorded the cancel.
    assert!(
        application
            .connectors
            .cal
            .events()
            .iter()
            .find(|e| e.uid == "mine-1")
            .unwrap()
            .cancelled,
        "mine-1 is cancelled on the local calendar"
    );
}
```

- [ ] **Step 3: Run the test to verify it passes**

Run: `cargo test -p inkapp-harness --test agenda_loop`
Expected: PASS (1 passed). If `evt-0` is not first in the manifest's region order, assert membership rather than position (the test already looks regions up by name, so order doesn't matter).

- [ ] **Step 4: Commit**

```bash
git add crates/inkapp-harness/tests/agenda_loop.rs crates/inkapp-harness/Cargo.toml Cargo.lock
git commit -m "inkapp-harness: agenda e2e — real-ink cancel marks the editable calendar; read-only feed inert"
```

---

### Task 7: Reconcile `docs/appdx.md` ("make the doc true")

**Goal:** Flip the doc from "mode axis aspirational" to "M built," and align its prose with the shipped code (real `Mode`/`CalendarView`, the ICS feed + local calendar, the `components` terminology).

**Files:**
- Modify: `docs/appdx.md`

**Acceptance Criteria:**
- [ ] Status banner + build-order line say S, E, C, M done; only T ahead
- [ ] "Three interaction modes" / "Components never talk to connectors" describe the real `Mode` axis and `CalendarView`
- [ ] Connectors section notes the read-only ICS feed is built and the local writable calendar stands in for CalDAV
- [ ] Ergonomics check no longer says "`mode` wasn't needed here"
- [ ] Parking lot records the `Widget`-trait two-layer consolidation follow-up

**Verify:** `grep -n "aspirational below\|wasn't needed here" docs/appdx.md` → no matches; manual read of the edited sections.

**Steps:**

- [ ] **Step 1: Flip the status banner (top of file)**

Replace the existing status block (lines ~3-13) so it reads (adjust surrounding wording to fit):

```markdown
> **Status: partially built.** The bottom half (render, manifest, ink attribution,
> the MVU loop, the reading-queue worked example), the **secrets store +
> embedded-manifest encryption**, the **connector plugin trait + async loop**, and
> now the **mode axis** (a `Mode { ReadOnly, Editable }` field carried by
> components, with a `CalendarView` spanning Display↔Control and the read-only ICS
> feed + writable local-calendar connectors behind it) are implemented and tested.
> Still ahead: Typst component *authoring* — the one remaining aspirational piece.
> Open questions are marked **(open)** inline.
>
> **Build order** (making this doc true): **S** secrets → **E** encryption →
> **C** connector plugin trait → **M** mode axis *(all four done)* → **T** Typst
> authoring. Event sourcing/CRDT and multi-user/cloud stay future (see
> [FUTURE.md](FUTURE.md)).
```

- [ ] **Step 2: Rewrite the "Three interaction modes" section**

Keep the Display/Capture/Control table, then replace the aspirational follow-up prose with the shipped reality:

```markdown
These three behaviors are now a real **axis**: a component carries a
`Mode { ReadOnly, Editable }` field, and its `render` and `decode` both branch on
that *one* value — so a ReadOnly render that drew no affordance cannot have a
decode that reads one. The framework ships `CalendarView`, which spans the range:
in `ReadOnly` it renders inert rows and decodes nothing (Display behavior); in
`Editable` it renders a per-event cancel affordance in its own region and decodes a
mark into one app message per event (Control behavior). `Notice` (Display) and
`Checkbox` (Control) remain fixed-affordance components that carry no mode.
```

- [ ] **Step 3: Rewrite "Components never talk to connectors" to reference the real example**

Replace the `CalendarView { events, mode: ReadOnly | Editable }` aspirational snippet's surrounding prose so it points at the agenda app: the component carries `mode` as a field; `view` sets it from the backing connector's capability (`ReadOnly` over the ICS feed, `Editable` over the writable local calendar); the component never sees a connector. Keep the two numbered reasons (policy-not-capability; testability) — they now describe shipped behavior. Update the inline type to match the code's constructors:

```markdown
    CalendarView::read_only(events)              // Display behavior
    CalendarView::editable(events, on_cancel)    // Control behavior; on_cancel: uid -> Msg
```

- [ ] **Step 4: Update the Connectors section**

Add, near the "Two kinds of connector" prose, that both archetypes now exist:

```markdown
Both archetypes are now built: the **read-only feed** is `inkapp-ics` (parses an
ICS calendar, caches `EventRow`s, no write queue, no-op `flush`), and a
**writable** local calendar `inkapp-localcal` stands in for CalDAV (optimistic
`cancel` + deferred `flush` to a local store; real CalDAV transport stays future).
```

- [ ] **Step 5: Update the ergonomics check**

Replace the bullet that begins "**`mode` wasn't needed here.**" with:

```markdown
- **`mode` earns its keep in the agenda app.** Reading-queue's components have
  fixed affordances, so they carry no mode. The agenda app shows the axis working:
  one `CalendarView` type, two instances, modes chosen by `view` from each
  connector's capability (read-only ICS feed vs writable local calendar).
```

- [ ] **Step 6: Add the parking-lot follow-up**

Append to the "Open questions parking lot":

```markdown
- **`Widget`/`Component` two-layer consolidation.** `Widget` (`render` + typed
  `read`) is a lower-level primitive distinct from `Component` (`render` +
  `decode` → `Msg`); the module is now named `components`, but whether the typed-
  `read` layer should fold into `Component` is an open tidy.
```

- [ ] **Step 7: Verify and commit**

```bash
grep -n "aspirational below\|wasn't needed here" docs/appdx.md   # expect: no output
git add docs/appdx.md
git commit -m "appdx: M (mode axis) built — real Mode/CalendarView, ICS feed + local calendar; components terminology; Widget-trait follow-up logged"
```

---

## Self-Review

**Spec coverage:**
- Mode axis (shared enum, field convention) → Task 1, used in Task 2. ✓
- `CalendarView` spanning ReadOnly/Editable, render+decode branch on mode → Task 2. ✓
- `EventRow` in core → Task 1. ✓
- Read-only ICS feed via `ical` crate → Task 3. ✓
- Writable local calendar (deferred write + optimistic overlay) → Task 4. ✓
- New `agenda` app, `view` chooses mode by capability → Task 5. ✓
- `widgets/` → `components/` rename, first → Task 0. ✓
- Harness e2e (async loop, two connectors, real ink) → Task 6. ✓
- All six spec test scenarios → Tasks 1-6 tests (render-flow, decode-flow, ICS parse/refresh, localcal write, agenda banner/cancel, async e2e, rename green). ✓
- appdx reconciliation → Task 7. ✓
- Widget-trait left alone, logged as follow-up → Task 0 (untouched) + Task 7 (parking lot). ✓

**Placeholder scan:** No TBD/TODO/"add error handling"/"similar to Task N" — every code step shows complete code; the one copy (serve.rs) lists its exact two edits. ✓

**Type consistency:** `Mode { ReadOnly, Editable }`, `EventRow { uid, summary, start, end, cancelled }`, `CalendarView::{read_only, editable}`, `on_cancel: fn(&str)->M`, region names `evt-<i>`, `Msg::EventCancelled { uid }`, connectors `IcsConnector`/`LocalCal` with `events()`/`cancel()`/`refresh()`/`flush()` — used consistently across Tasks 1-7. The agenda `Connectors { feed, cal }` field names match the e2e's `application.connectors.cal`. ✓

**Risk notes:** The `ical` crate field names (`IcalEvent.properties`, `Property.{name,value}`) are the one external unknown — Task 3 calls this out with a `cargo doc` fallback localized to `parse_ics`. The `widgets`→`components` rename is broad but mechanical and guarded by `\b` word boundaries + a full-workspace test gate.
