# Per-Day Agenda/Details Pages Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers-extended-cc:subagent-driven-development (recommended) or superpowers-extended-cc:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Restructure the month notebook so each day with events gets its own page block (Agenda then Details, combined flow), and the calendar pill lands directly on that one day.

**Architecture:** Replace the two month-wide paginations (all-agenda pages, then all-details pages) with a single per-day pagination. A new `paginate_day` lays out one day's events — agenda lines first, then detail blocks — across page descriptors (`DayPagePlan`). A new `DayEvents` askama template renders each descriptor to one page. Existing `#agenda-{day}` and `#evt-{idx}` anchor names are preserved so `monthly_view.html` / `daily_page.html` need no changes.

**Tech Stack:** Rust, askama (compile-time HTML templates), chrono, lopdf (test assertions), pdftoppm (visual goldens). Build/test via `nix develop -c cargo …` (see Makefile).

**Spec:** `docs/superpowers/specs/2026-05-22-per-day-agenda-details-pages-design.md`

**Sequencing note:** This is a refactor. Tasks 1–2 add the new logic/template *alongside* the old code so the build stays green at every commit. Task 3 switches `mod.rs` to the new path. Task 4 removes the now-dead old code and regenerates visual goldens. Run all `cargo`/`make` commands through the Nix dev shell.

---

### Task 1: `paginate_day` + `DayPagePlan` (per-day pagination logic)

**Goal:** Add a pure function that paginates one day's events into per-page plans for the combined agenda+details layout, leaving the old `paginate` in place.

**Files:**
- Modify: `src/notebooks/month/agenda.rs` (add `SUBHEAD_PT`, `DayPagePlan`, `paginate_day`; keep `paginate`)
- Test: `tests/month.rs` (add `paginate_day` unit tests)

**Acceptance Criteria:**
- [ ] `DayPagePlan` describes one page: `agenda`/`details` slices, `show_agenda_heading`, `show_details_heading`, `continued`, `first_page`.
- [ ] `paginate_day` places agenda items before detail items, charges `header_pt` per page and each sub-heading once on its section's first page, never orphans a heading, and places an oversized lone item as-is.
- [ ] `paginate` and `HEADER_PT` remain unchanged (old test still passes).
- [ ] Four new tests pass: small day = 1 page; agenda overflow split; orphan-heading guard; oversized lone item.

**Verify:** `nix develop -c cargo test --test month` → all tests pass

**Steps:**

- [ ] **Step 1: Write the failing tests** — append to `tests/month.rs`:

```rust
#[test]
fn paginate_day_small_day_one_page() {
    use rmbujo::notebooks::month::agenda::paginate_day;
    use rmbujo::templates::{AgendaDay, AgendaEvent};
    let mk = |i| AgendaEvent {
        idx: i,
        label: "09:00".into(),
        end_label: None,
        title: "Event".into(),
        location: None,
        description: None,
        attendees: vec![],
        color: "cal1".into(),
        is_all_day: false,
    };
    let day = AgendaDay { day: 5, weekday: "Wed", events: (0..2).map(mk).collect() };
    // header 20 + agenda(subhead 5 + 2*10) + details(subhead 5 + 2*10) = 70 <= 200
    let pages = paginate_day(&day, 200.0, 20.0, 5.0, |_| 10.0, |_| 10.0);
    assert_eq!(pages.len(), 1, "small day fits on one page");
    let p = &pages[0];
    assert!(p.first_page && !p.continued);
    assert!(p.show_agenda_heading && p.show_details_heading);
    assert_eq!(p.agenda.len(), 2);
    assert_eq!(p.details.len(), 2);
}

#[test]
fn paginate_day_agenda_overflows_then_details() {
    use rmbujo::notebooks::month::agenda::paginate_day;
    use rmbujo::templates::{AgendaDay, AgendaEvent};
    let mk = |i| AgendaEvent {
        idx: i,
        label: "09:00".into(),
        end_label: None,
        title: "Event".into(),
        location: None,
        description: None,
        attendees: vec![],
        color: "cal1".into(),
        is_all_day: false,
    };
    let day = AgendaDay { day: 5, weekday: "Wed", events: (0..3).map(mk).collect() };
    // header 20, subhead 0, each item 40, usable 100.
    // p0: a0(60),a1(100) -> flush. p1: a2(60),d0(100) -> flush. p2: d1(60),d2(100).
    let pages = paginate_day(&day, 100.0, 20.0, 0.0, |_| 40.0, |_| 40.0);
    assert_eq!(pages.len(), 3);
    assert!(pages[0].first_page && pages[0].show_agenda_heading);
    assert_eq!(pages[0].agenda.len(), 2);
    assert!(pages[0].details.is_empty());
    assert!(pages[1].continued && pages[1].show_details_heading);
    assert_eq!(pages[1].agenda.len(), 1);
    assert_eq!(pages[1].details.len(), 1);
    assert!(!pages[1].show_agenda_heading);
    assert_eq!(pages[2].details.len(), 2);
    assert!(!pages[2].show_agenda_heading && !pages[2].show_details_heading);
    let total: usize = pages.iter().map(|p| p.agenda.len() + p.details.len()).sum();
    assert_eq!(total, 6, "no events lost (3 agenda + 3 detail)");
}

#[test]
fn paginate_day_details_heading_not_orphaned() {
    use rmbujo::notebooks::month::agenda::paginate_day;
    use rmbujo::templates::{AgendaDay, AgendaEvent};
    let mk = |i| AgendaEvent {
        idx: i,
        label: "09:00".into(),
        end_label: None,
        title: "Event".into(),
        location: None,
        description: None,
        attendees: vec![],
        color: "cal1".into(),
        is_all_day: false,
    };
    let day = AgendaDay { day: 5, weekday: "Wed", events: (0..2).map(mk).collect() };
    // header 20, subhead 20, each item 40, usable 100.
    // p0: header20 + agenda-head20 + a0(40)=80; a1 needs 40 -> 120>100 flush.
    // p1: header20 + a1(40)=60; d0 needs head20+40=60 -> 120>100 flush (no orphan head).
    // p2: header20 + details-head20 + d0(40)=80; d1 needs 40 -> flush. p3: d1.
    let pages = paginate_day(&day, 100.0, 20.0, 20.0, |_| 40.0, |_| 40.0);
    // The details heading lands on the page that holds the first detail item.
    let det_page = pages.iter().find(|p| !p.details.is_empty()).unwrap();
    assert!(det_page.show_details_heading);
    assert_eq!(det_page.details[0].idx, 0, "heading travels with its first item");
    // Exactly one page shows the details heading.
    assert_eq!(pages.iter().filter(|p| p.show_details_heading).count(), 1);
    assert_eq!(pages.iter().filter(|p| p.show_agenda_heading).count(), 1);
}

#[test]
fn paginate_day_oversized_lone_item_placed() {
    use rmbujo::notebooks::month::agenda::paginate_day;
    use rmbujo::templates::{AgendaDay, AgendaEvent};
    let mk = |i| AgendaEvent {
        idx: i,
        label: "09:00".into(),
        end_label: None,
        title: "Event".into(),
        location: None,
        description: None,
        attendees: vec![],
        color: "cal1".into(),
        is_all_day: false,
    };
    let day = AgendaDay { day: 5, weekday: "Wed", events: (0..1).map(mk).collect() };
    // One agenda item far taller than the page: must be placed, not dropped/looped.
    let pages = paginate_day(&day, 100.0, 20.0, 5.0, |_| 500.0, |_| 10.0);
    assert_eq!(pages[0].agenda.len(), 1);
    assert_eq!(pages.iter().map(|p| p.agenda.len()).sum::<usize>(), 1);
    assert_eq!(pages.iter().map(|p| p.details.len()).sum::<usize>(), 1);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `nix develop -c cargo test --test month`
Expected: FAIL — `cannot find function paginate_day` / `cannot find type DayPagePlan`.

- [ ] **Step 3: Add `SUBHEAD_PT`, `DayPagePlan`, and `paginate_day`** in `src/notebooks/month/agenda.rs`.

Add the constant next to `HEADER_PT` (after line 90):

```rust
/// Vertical cost (pt) of an "Agenda"/"Details" sub-heading on a day page.
pub const SUBHEAD_PT: f32 = 16.0;
```

Add the struct and function (place after the existing `paginate` function so `paginate` is untouched):

```rust
/// One rendered page of a single day's events. A page is always laid out as
/// `[agenda lines] [detail blocks]` (agenda always precedes details within a
/// day), so two slices plus the heading/continuation flags fully describe it:
/// an agenda-only page, a details-only continuation page, or a shared page that
/// carries the tail of the agenda and the head of the details.
#[derive(Clone, Debug, Default)]
pub struct DayPagePlan {
    pub agenda: Vec<AgendaEvent>,
    pub details: Vec<AgendaEvent>,
    pub show_agenda_heading: bool,
    pub show_details_heading: bool,
    /// True on every page after the day's first (drives the "· cont." marker).
    pub continued: bool,
    /// True on the day's first page (the `#agenda-{day}` pill target).
    pub first_page: bool,
}

/// Paginate ONE day's events into per-page plans for the combined agenda+details
/// layout. Agenda lines come first (`agenda_pt`), then detail blocks (`detail_pt`),
/// both in event order. Each page costs `header_pt` (the running date header);
/// each section's sub-heading (`subhead_pt`) is charged once, on the first page
/// that section's content appears. A sub-heading and its section's first item are
/// placed together, so a heading never orphans at the foot of a page. An item
/// taller than a fresh page is placed as-is (field capping bounds item height).
pub fn paginate_day(
    day: &AgendaDay,
    usable_pt: f32,
    header_pt: f32,
    subhead_pt: f32,
    agenda_pt: impl Fn(&AgendaEvent) -> f32,
    detail_pt: impl Fn(&AgendaEvent) -> f32,
) -> Vec<DayPagePlan> {
    enum Kind {
        Agenda,
        Detail,
    }
    // Agenda items first, then detail items — each tagged with its height.
    let mut items: Vec<(Kind, &AgendaEvent, f32)> = Vec::new();
    for e in &day.events {
        items.push((Kind::Agenda, e, agenda_pt(e)));
    }
    for e in &day.events {
        items.push((Kind::Detail, e, detail_pt(e)));
    }

    let mut pages: Vec<DayPagePlan> = Vec::new();
    let mut cur = DayPagePlan {
        first_page: true,
        ..Default::default()
    };
    let mut h = header_pt;
    let mut agenda_heading_done = false;
    let mut details_heading_done = false;

    for (kind, e, eh) in items {
        let triggers_heading = match kind {
            Kind::Agenda => !agenda_heading_done,
            Kind::Detail => !details_heading_done,
        };
        let need = eh + if triggers_heading { subhead_pt } else { 0.0 };
        let has_content = !cur.agenda.is_empty() || !cur.details.is_empty();
        if h + need > usable_pt && has_content {
            pages.push(std::mem::take(&mut cur));
            cur = DayPagePlan {
                continued: true,
                ..Default::default()
            };
            h = header_pt;
        }
        if triggers_heading {
            match kind {
                Kind::Agenda => {
                    cur.show_agenda_heading = true;
                    agenda_heading_done = true;
                }
                Kind::Detail => {
                    cur.show_details_heading = true;
                    details_heading_done = true;
                }
            }
            h += subhead_pt;
        }
        match kind {
            Kind::Agenda => cur.agenda.push(e.clone()),
            Kind::Detail => cur.details.push(e.clone()),
        }
        h += eh;
    }
    if !cur.agenda.is_empty() || !cur.details.is_empty() {
        pages.push(cur);
    }
    pages
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `nix develop -c cargo test --test month`
Expected: PASS (new `paginate_day_*` tests and the existing `busy_day_splits_across_pages_with_repeated_header`).

- [ ] **Step 5: Commit**

```bash
git add src/notebooks/month/agenda.rs tests/month.rs
git commit -m "Add per-day paginate_day + DayPagePlan (combined agenda/details flow)"
```

---

### Task 2: `DayEvents` template + `.h-evt` CSS

**Goal:** Add the `DayEvents` page struct, the `day_events.html` template, and the sub-heading CSS, with HTML unit tests — leaving the old `Agenda`/`Details` structs and templates in place.

**Files:**
- Create: `templates/day_events.html`
- Modify: `src/templates.rs` (add `DayEvents` struct)
- Modify: `src/render.rs` (add `.h-evt` to the Fraunces selector and a `.h-evt` sizing rule)
- Test: `tests/templates_html.rs` (add `DayEvents` tests)

**Acceptance Criteria:**
- [ ] `DayEvents` renders the running date header linking to `#day-{day}`, with `id="agenda-{day}"` only on `first_page`.
- [ ] Agenda lines link to `#evt-{idx}`; detail blocks carry `id="evt-{idx}"`; empty Where/Notes/Who omitted.
- [ ] `show_agenda_heading` / `show_details_heading` gate the `Agenda` / `Details` sub-heads; `continued` emits a "cont." marker.
- [ ] Old `Agenda`/`Details` structs and templates still compile and their tests still pass.

**Verify:** `nix develop -c cargo test --test templates_html` → all tests pass

**Steps:**

- [ ] **Step 1: Write the failing tests** — append to `tests/templates_html.rs`:

```rust
fn sample_day_events() -> Vec<AgendaEvent> {
    vec![
        AgendaEvent {
            idx: 0,
            label: "09:00".into(),
            end_label: Some("10:00".into()),
            title: "Standup".into(),
            location: Some("Zoom".into()),
            description: None,
            attendees: vec![],
            color: "accent".into(),
            is_all_day: false,
        },
        AgendaEvent {
            idx: 1,
            label: "12:00".into(),
            end_label: None,
            title: "Lunch".into(),
            location: None,
            description: None,
            attendees: vec!["Sam".into()],
            color: "rust".into(),
            is_all_day: false,
        },
    ]
}

#[test]
fn day_events_first_page_anchor_and_links() {
    let evs = sample_day_events();
    let html = DayEvents {
        month_num: 5,
        day: 5,
        day_pad: "05".into(),
        weekday: "Wed",
        agenda: &evs,
        details: &evs,
        show_agenda_heading: true,
        show_details_heading: true,
        continued: false,
        first_page: true,
    }
    .render()
    .unwrap();
    assert!(html.contains("id=\"agenda-5\""), "pill target on first page");
    assert!(html.contains("href=\"#day-5\""), "header links to daily page");
    assert_eq!(html.matches("href=\"#evt-").count(), 2, "agenda lines link to details");
    assert!(html.contains("id=\"evt-0\"") && html.contains("id=\"evt-1\""));
    assert!(html.contains(">Agenda<") && html.contains(">Details<"));
    assert_eq!(html.matches("class=\"swatch\"").count(), 4, "2 agenda + 2 detail swatches");
    assert!(html.contains("var(--accent)"));
    assert!(html.contains("09:00&#8211;10:00"), "details show start-end range");
    // Empty fields omitted; only the populated meta lines render.
    assert!(html.contains("Where: Zoom"));
    assert!(html.contains("Who: Sam"));
    assert_eq!(html.matches("Notes:").count(), 0);
}

#[test]
fn day_events_continuation_omits_anchor_and_headings() {
    let evs = sample_day_events();
    let html = DayEvents {
        month_num: 5,
        day: 5,
        day_pad: "05".into(),
        weekday: "Wed",
        agenda: &[],
        details: &evs,
        show_agenda_heading: false,
        show_details_heading: false,
        continued: true,
        first_page: false,
    }
    .render()
    .unwrap();
    assert!(!html.contains("id=\"agenda-5\""), "anchor only on first page");
    assert!(html.contains("cont."), "continuation marker");
    assert!(!html.contains(">Agenda<") && !html.contains(">Details<"));
    assert!(html.contains("href=\"#day-5\""), "running header still links to daily page");
}
```

Add `DayEvents` to the import block at the top of `tests/templates_html.rs`:

```rust
use rmbujo::templates::{
    Agenda, AgendaDay, AgendaEvent, Cover, DayEvents, DayRow, Details, FutureLog, MonthlyView,
    Reference, Tasks,
};
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `nix develop -c cargo test --test templates_html`
Expected: FAIL — `cannot find type DayEvents` / template not found.

- [ ] **Step 3: Create `templates/day_events.html`**

```html
<section class="page"{% if first_page %} id="agenda-{{ day }}"{% endif %}>
  <div class="h-month"><a href="#day-{{ day }}">{{ month_num }}.{{ day_pad }} {{ weekday }}</a>{% if continued %} &middot; cont.{% endif %}</div>
  {% if show_agenda_heading %}<div class="h-evt">Agenda</div>{% endif %}
  {% for e in agenda %}
  <div class="agenda-line"><a href="#evt-{{ e.idx }}"><span class="swatch" style="background:var(--{{ e.color }})"></span><b>{{ e.label }}</b>&nbsp;&nbsp;{{ e.title }}{% if let Some(loc) = e.location %} &#8212; {{ loc }}{% endif %}</a></div>
  {% endfor %}
  {% if show_details_heading %}<div class="h-evt">Details</div>{% endif %}
  {% for e in details %}
  <div class="detail-evt" id="evt-{{ e.idx }}">
    <div class="detail-title"><span class="swatch" style="background:var(--{{ e.color }})"></span><b>{{ e.label }}{% if let Some(end) = e.end_label %}&#8211;{{ end }}{% endif %}</b>&nbsp;&nbsp;{{ e.title }}</div>
    {% if let Some(loc) = e.location %}<div class="detail-meta">Where: {{ loc }}</div>{% endif %}
    {% if let Some(desc) = e.description %}<div class="detail-meta">Notes: {{ desc }}</div>{% endif %}
    {% if !e.attendees.is_empty() %}<div class="detail-meta">Who: {{ e.attendees|join(", ") }}</div>{% endif %}
  </div>
  {% endfor %}
</section>
```

- [ ] **Step 4: Add the `DayEvents` struct** in `src/templates.rs` (after the `Details` struct, before the end of file):

```rust
/// One page of a single day's events: a compact agenda list followed by expanded
/// details. `agenda`/`details` hold only the events on THIS page; the heading and
/// continuation flags come from `notebooks::month::agenda::DayPagePlan`.
#[derive(Template)]
#[template(path = "day_events.html")]
pub struct DayEvents<'a> {
    pub month_num: u32,
    pub day: u32,
    pub day_pad: String,
    pub weekday: &'a str,
    pub agenda: &'a [AgendaEvent],
    pub details: &'a [AgendaEvent],
    pub show_agenda_heading: bool,
    pub show_details_heading: bool,
    pub continued: bool,
    pub first_page: bool,
}
```

- [ ] **Step 5: Add the `.h-evt` CSS** in `src/render.rs`.

In the Fraunces font-family selector (currently `.h-month, .h-section, .dayhead-date, .cover .title`), add `.h-evt`:

```rust
.h-month, .h-section, .h-evt, .dayhead-date, .cover .title {{ font-family: \"Fraunces 72pt\", serif; }}\n\
```

Add a sizing rule immediately after the `.h-section` rule line:

```rust
.h-evt {{ color: var(--primary); font-size: 11pt; font-weight: bold; margin: 6pt 0 3pt; }}\n\
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `nix develop -c cargo test --test templates_html`
Expected: PASS (new `day_events_*` tests plus the existing agenda/details tests).

- [ ] **Step 7: Commit**

```bash
git add templates/day_events.html src/templates.rs src/render.rs tests/templates_html.rs
git commit -m "Add DayEvents template + .h-evt sub-heading CSS"
```

---

### Task 3: Switch `mod.rs` to per-day pages

**Goal:** Replace the two month-wide pagination loops in `build_month_pdf` with a per-day loop that renders `DayEvents` pages, and update the month-level page-count tests.

**Files:**
- Modify: `src/notebooks/month/mod.rs:54-118` (imports + the agenda/details emission block)
- Test: `tests/month.rs` (`events_only_add_trailing_pages`, `busy_month_paginates_agenda_and_details`)

**Acceptance Criteria:**
- [ ] A month with events emits one or more `DayEvents` pages per event-day, in date order, after the daily writing pages.
- [ ] A single small one-event day produces exactly **one** combined page (not separate agenda + details pages).
- [ ] A busy month still paginates beyond one page.
- [ ] Static months (no events) keep their exact page count (`month_page_count_static`, `month_pages_per_day_multiplies_daily` unchanged).

**Verify:** `nix develop -c cargo test --test month` → all tests pass

**Steps:**

- [ ] **Step 1: Update the page-count expectations** in `tests/month.rs`.

In `events_only_add_trailing_pages`, change the trailing-page assertion (currently `base + 2`) to a single combined page:

```rust
    assert!(withev > base, "events add trailing pages");
    assert_eq!(
        withev,
        base + 1,
        "a small one-event day is a single combined agenda+details page"
    );
```

In `busy_month_paginates_agenda_and_details`, the structure changes from "agenda pages + details pages" to "per-day combined pages"; relax the comment/threshold to assert the event pages still exceed one page total:

```rust
    let pages = lopdf::Document::load(&out).unwrap().get_pages().len();
    // Jan static = 2 + 31 = 33; 28 busy days each produce 1+ combined pages, so the
    // event pages add well beyond a single trailing page.
    assert!(
        pages > 33 + 1,
        "busy month should paginate per-day event pages beyond one page (got {pages})"
    );
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `nix develop -c cargo test --test month`
Expected: FAIL — `events_only_add_trailing_pages` expects `base + 1` but the old code still emits `base + 2`.

- [ ] **Step 3: Update the import in `src/notebooks/month/mod.rs`** (line 12) to bring in `DayEvents` and drop `Agenda`/`Details`:

```rust
use crate::templates::{DailyPage, DayEvents, DayRow, DotGrid, MonthlyView, Tasks};
```

- [ ] **Step 4: Replace the agenda/details emission block** in `src/notebooks/month/mod.rs`.

Replace the entire block from the `// Agenda + Details are appended ONLY …` comment through the closing brace of `if !days.is_empty() { … }` (currently lines 81–116) with:

```rust
    // Per-day event pages are appended ONLY when the month has events, so a static
    // month keeps its exact page count. Each day with events gets its own page
    // block (Agenda then Details, combined flow): a light day is one page, a busy
    // day spills onto more pages, and a day never shares a page with another day.
    // The `#agenda-{day}` pill from the monthly/daily pages lands on the day's
    // first page. Usable height = page minus the toolbar reserve, bottom margin,
    // and title block; content width accounts for side margins (and the details
    // indent) so wrapping is estimated.
    let days = agenda::agenda_days(&m, events, config.year, month);
    if !days.is_empty() {
        let usable = dev.height_pt() - crate::geometry::TOOLBAR_SAFE_PT - grid.margin_pt - 30.0;
        let content_w = dev.width_pt() - 2.0 * grid.margin_pt - 8.0;
        for day in &days {
            for plan in agenda::paginate_day(
                day,
                usable,
                agenda::HEADER_PT,
                agenda::SUBHEAD_PT,
                |e| agenda::agenda_event_pt(content_w, e),
                |e| agenda::detail_event_pt(content_w, e),
            ) {
                fragments.push(
                    DayEvents {
                        month_num: month,
                        day: day.day,
                        day_pad: format!("{:02}", day.day),
                        weekday: day.weekday,
                        agenda: &plan.agenda,
                        details: &plan.details,
                        show_agenda_heading: plan.show_agenda_heading,
                        show_details_heading: plan.show_details_heading,
                        continued: plan.continued,
                        first_page: plan.first_page,
                    }
                    .render()?,
                );
            }
        }
    }
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `nix develop -c cargo test --test month`
Expected: PASS (all four month tests, including the updated assertions).

- [ ] **Step 6: Confirm the whole crate still builds** (old `Agenda`/`Details`/`paginate` now unused but still defined and used by other tests):

Run: `nix develop -c cargo test`
Expected: PASS. (The `visual` test still renders the old `agenda`/`details` goldens — untouched until Task 4.)

- [ ] **Step 7: Commit**

```bash
git add src/notebooks/month/mod.rs tests/month.rs
git commit -m "Render per-day event pages; pill lands on a single day"
```

---

### Task 4: Remove dead code; regenerate visual goldens

**Goal:** Delete the now-unused `Agenda`/`Details` structs, their templates, the old `paginate`, the stale CSS rules, and the old tests; switch the visual fixture to `DayEvents` and regenerate goldens.

**Files:**
- Delete: `templates/agenda.html`, `templates/details.html`
- Delete: `tests/goldens/agenda.png`, `tests/goldens/details.png`
- Modify: `src/templates.rs` (remove `Agenda`, `Details`)
- Modify: `src/notebooks/month/agenda.rs` (remove `paginate`)
- Modify: `src/render.rs` (remove `.agenda-day, .detail-day` and `.agenda-date` rules)
- Modify: `tests/templates_html.rs` (remove old agenda/details tests + `sample_agenda_days`; trim imports)
- Modify: `tests/month.rs` (remove `busy_day_splits_across_pages_with_repeated_header`)
- Modify: `tests/visual.rs` (replace agenda/details fragments with a `day_events` fragment)
- Create: `tests/goldens/day_events.png` (via `make update-goldens`)

**Acceptance Criteria:**
- [ ] No references to `Agenda`, `Details`, or `paginate` remain anywhere in the crate or tests.
- [ ] `tests/visual.rs` renders a `day_events` fragment; `day_events.png` golden exists; `agenda.png`/`details.png` removed.
- [ ] `nix develop -c cargo test` passes; `nix develop -c cargo clippy --all-targets -- -D warnings` passes.

**Verify:** `nix develop -c cargo test && nix develop -c cargo clippy --all-targets -- -D warnings` → all pass

**Steps:**

- [ ] **Step 1: Remove old tests.**

In `tests/templates_html.rs`: delete the `sample_agenda_days` function, the `agenda_links_and_swatches` test, and the `details_omit_empty_fields` test. Update the import to drop `Agenda`, `Details`, and `AgendaDay` (kept: `AgendaEvent`, `DayEvents`, and the rest):

```rust
use rmbujo::templates::{
    AgendaEvent, Cover, DayEvents, DayRow, FutureLog, MonthlyView, Reference, Tasks,
};
```

In `tests/month.rs`: delete the entire `busy_day_splits_across_pages_with_repeated_header` test (its coverage is now in the `paginate_day_*` tests).

- [ ] **Step 2: Replace the visual fixture** in `tests/visual.rs`.

Update the import (lines 9–12) to drop `Agenda`, `AgendaDay`, `Details` and add `DayEvents`:

```rust
use rmbujo::templates::{
    AgendaEvent, Cover, DailyPage, DayEvents, DayRow, DotGrid, FutureLog, MonthlyView, Reference,
    Tasks,
};
```

Replace the agenda/details fixture block (currently lines 82–124, the `let agenda_days = …` through `let details = … .unwrap();`) with a single combined-page fixture:

```rust
    // day_events — one combined page: Agenda list + Details for a single day.
    let day_evts = vec![
        AgendaEvent {
            idx: 0,
            label: "All Day".into(),
            end_label: None,
            title: "Victoria Day".into(),
            location: None,
            description: None,
            attendees: vec![],
            color: "accent".into(),
            is_all_day: true,
        },
        AgendaEvent {
            idx: 1,
            label: "14:00".into(),
            end_label: Some("15:00".into()),
            title: "Dentist".into(),
            location: Some("Downtown".into()),
            description: Some("Bring card".into()),
            attendees: vec!["Dr. Lee".into()],
            color: "rust".into(),
            is_all_day: false,
        },
    ];
    let day_events = DayEvents {
        month_num: 5,
        day: 19,
        day_pad: "19".into(),
        weekday: "Tue",
        agenda: &day_evts,
        details: &day_evts,
        show_agenda_heading: true,
        show_details_heading: true,
        continued: false,
        first_page: true,
    }
    .render()
    .unwrap();
```

Replace the two trailing fixture entries in the returned vec (currently `("agenda", agenda),` and `("details", details),`) with a single entry:

```rust
        ("day_events", day_events),
```

- [ ] **Step 3: Remove the `Agenda` and `Details` structs** from `src/templates.rs` (the two `#[derive(Template)]` blocks for `agenda.html` and `details.html`). Keep `AgendaEvent` and `AgendaDay` (still used by `agenda.rs` / `paginate_day`).

- [ ] **Step 4: Remove the old `paginate` function** from `src/notebooks/month/agenda.rs` (the `pub fn paginate(…) -> Vec<Vec<AgendaDay>>` and its doc comment). Keep `agenda_days`, `agenda_event_pt`, `detail_event_pt`, `lines_for`, `cap`, `HEADER_PT`, `SUBHEAD_PT`, `DayPagePlan`, `paginate_day`.

- [ ] **Step 5: Remove the stale CSS** in `src/render.rs`.

Remove `.agenda-date` from the Hanken selector:

```rust
.day, .day .wd, .cbadge, .pill, .detail-meta {{ font-family: \"Hanken Grotesk\", sans-serif; }}\n\
```

Delete these two rule lines entirely:

```rust
.agenda-day, .detail-day {{ break-inside: avoid; margin-bottom: 8pt; }}\n\
.agenda-date {{ font-weight: bold; color: var(--primary); font-size: 11pt; text-decoration: none; border-bottom: 0.75pt solid var(--rule); padding-bottom: 1.5pt; }}\n\
```

- [ ] **Step 6: Delete the old templates and goldens**

```bash
git rm templates/agenda.html templates/details.html tests/goldens/agenda.png tests/goldens/details.png
```

- [ ] **Step 7: Regenerate the visual golden**

Run: `make update-goldens`
Expected: writes `tests/goldens/day_events.png` (the new fragment). Inspect it: it should show "5.19 Tue", an **Agenda** list (Victoria Day, 14:00 Dentist — Downtown), then **Details** with Where/Notes/Who.

- [ ] **Step 8: Verify the full suite and lints**

Run: `nix develop -c cargo test`
Expected: PASS, including `visual::visual_regression` against the new `day_events.png`.

Run: `nix develop -c cargo clippy --all-targets -- -D warnings`
Expected: no warnings (confirms no dead `Agenda`/`Details`/`paginate` references remain).

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "Remove month-wide agenda/details; day_events golden + cleanup"
```

---

## Self-Review

**Spec coverage:**
- Per-day page blocks, Agenda then Details, combined flow → Task 1 (`paginate_day`) + Task 3 (`mod.rs` loop). ✓
- Light day = 1 page, busy day spills, day never shares a page → `paginate_day` algorithm + Task 1 tests. ✓
- Running date header every page, `· cont.` marker, sub-heads only on a section's first page → `day_events.html` + `DayPagePlan` flags (Task 2) + Task 1 heading tests. ✓
- Pill `#agenda-{day}` lands on the day's first page; `monthly_view.html`/`daily_page.html` unchanged → `first_page` anchor in `day_events.html`, anchor name preserved (Task 2 test `day_events_first_page_anchor_and_links`). ✓
- Agenda event → `#evt-{idx}` detail anchor; `idx` per-month sequential (kept in `agenda_days`) → Task 2 test. ✓
- Header (date) → `#day-{day}` daily page → `day_events.html` + test. ✓
- Static-month page counts unchanged → preserved `if !days.is_empty()` guard; `month_page_count_static` untouched. ✓
- Remove `Agenda`/`Details`/`agenda.html`/`details.html`/old `paginate`/old CSS; new golden → Task 4. ✓
- Non-goals (no interleaving with writing pages, no prev/next nav) → event pages still appended after dailies; no nav added. ✓

**Placeholder scan:** No TBD/TODO; every code step shows full code. ✓

**Type consistency:** `DayPagePlan` fields (`agenda`, `details`, `show_agenda_heading`, `show_details_heading`, `continued`, `first_page`) match the `DayEvents` template struct fields and the `mod.rs` construction. `paginate_day` signature `(day, usable_pt, header_pt, subhead_pt, agenda_pt, detail_pt)` matches its call site in `mod.rs` and all four tests. `SUBHEAD_PT`/`HEADER_PT` referenced consistently. ✓
