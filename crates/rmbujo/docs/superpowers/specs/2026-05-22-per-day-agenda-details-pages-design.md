# Per-Day Agenda/Details Pages

**Date:** 2026-05-22
**Status:** Approved (design)

## Problem

Each month notebook currently ends with two month-wide sections: **all** days' agenda
rows flow across one or more pages, then **all** days' detail blocks flow across one or
more pages. The calendar pill on the monthly and daily pages links to `#agenda-{day}`,
which drops the reader onto a page shared by several days. On the device the reader then
has to scan the page to find the date they tapped. The link technically works; the
landing experience does not.

## Goal

Clicking a day's pill lands the reader on a page that contains **only that day**. Each
day with events gets its own page block: a compact **Agenda** list followed by expanded
**Details**. A light day fits on one page; a busy day spills onto additional pages
(Agenda first, then Details). Day blocks never share a page with another day.

Out of scope (non-goals):

- Interleaving event pages with the daily dot-grid writing pages. Event pages stay
  grouped at the end of the month notebook; pill navigation makes physical position
  irrelevant, and interleaving would disturb the `pages_per_day` page-count logic.
- Prev/next-day navigation controls on event pages.
- Changing which fields appear in Agenda vs. Details, or their capping behavior.

## UX

For each day that has events (in date order), emit a page block:

```
[ 5.05 Wed ]            ← running date header, links to #day-5
Agenda                  ← sub-heading, first agenda page only
 • 09:00  Standup
 • 12:00  Lunch w/ Sam — Cafe
Details                 ← sub-heading, first details page only
 09:00 Standup
  Where: Zoom
 12:00 Lunch w/ Sam
  Where: Cafe
```

Pagination within a day (combined flow):

- All Agenda lines come first, then all Details blocks.
- A light day fits on one page (Agenda list + Details below).
- A busy day spills onto more pages: Agenda fills page(s), then Details fills page(s).
  Agenda and Details may share a page when they fit; otherwise Details starts on a fresh
  page.
- The running date header repeats on every page of the day. Pages after the day's first
  carry a `· cont.` marker.
- The **Agenda** sub-heading shows only on the first page that carries agenda content;
  the **Details** sub-heading only on the first page that carries detail content.
  Continuation pages within a section show neither sub-heading — the running date header
  supplies the context.

## Navigation / links

| From               | Link            | Target                            | Change                       |
|--------------------|-----------------|-----------------------------------|------------------------------|
| Monthly pill       | `#agenda-{day}` | the day's first event page        | none — anchor name kept      |
| Daily-page badge   | `#agenda-{day}` | the day's first event page        | none — anchor name kept      |
| Day header (date)  | `#day-{day}`    | the daily dot-grid writing page   | folded into running header   |
| Agenda event line  | `#evt-{idx}`    | that event's Details block         | unchanged (same or later page) |

Keeping the `agenda-{day}` and `evt-{idx}` anchor names means `templates/monthly_view.html`
and `templates/daily_page.html` need **no** changes. The `agenda-{day}` anchor moves onto
the day's first event page; `evt-{idx}` stays on each event's detail block. `idx` is still
assigned per-month sequentially in `agenda_days`, so anchors stay unique across the month.

## Approach

One per-day pagination producing page descriptors, rendered by a single new template.

Rejected alternative: keep the two existing month-wide `paginate` passes and interleave
them per day (day-N agenda pages, then day-N detail pages, then day-N+1 …). This reuses
the existing templates with a smaller diff, but it can never place a day's Agenda and
Details on the same page — which contradicts the chosen combined-flow layout. So the page
model is replaced rather than reordered.

## Components

### `src/templates.rs`

- Remove the `Agenda` and `Details` template structs.
- Add a `DayEvents` page struct (one instance per emitted page):
  - `year: i32`, `month_num: u32`, `day: u32`, `day_pad: String`, `weekday: &'static str`
  - `agenda: &[AgendaEvent]` — agenda lines on this page (may be empty)
  - `details: &[AgendaEvent]` — detail blocks on this page (may be empty)
  - `show_agenda_heading: bool`, `show_details_heading: bool`
  - `continued: bool` — true on pages after the day's first (drives `· cont.`)
  - `first_page: bool` — true on the day's first page (emits `id="agenda-{day}"`)
- Keep `AgendaEvent` unchanged. Keep `AgendaDay` as the per-day input to pagination.

A page is always structured `[agenda lines] [details blocks]` (Agenda always precedes
Details within a day), so two slices plus heading booleans fully describe any page: an
agenda-only page, a details-only continuation page, or a shared page carrying the tail of
Agenda and the head of Details.

### `templates/day_events.html` (new — replaces `agenda.html` + `details.html`)

```html
<section class="page"{% if first_page %} id="agenda-{{ day }}"{% endif %}>
  <div class="h-month">
    <a href="#day-{{ day }}">{{ month_num }}.{{ day_pad }} {{ weekday }}</a>{% if continued %} · cont.{% endif %}
  </div>
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

### `src/notebooks/month/agenda.rs`

- Keep `agenda_days`, `agenda_event_pt`, `detail_event_pt`, `lines_for`, `cap`, `HEADER_PT`.
- Replace `paginate` with `paginate_day`:

  ```rust
  pub fn paginate_day(
      day: &AgendaDay,
      usable_pt: f32,
      content_w: f32,
  ) -> Vec<DayPagePlan>
  ```

  where `DayPagePlan { agenda: Vec<AgendaEvent>, details: Vec<AgendaEvent>,
  show_agenda_heading: bool, show_details_heading: bool, continued: bool, first_page: bool }`.

  Algorithm (greedy, single pass over the day's events twice — agenda items then detail
  items, in event order):

  - Each page starts with `HEADER_PT` (the running date header).
  - Add a sub-heading cost (`SUBHEAD_PT`) once, on the first page each section's content
    appears.
  - For each item compute its height (`agenda_event_pt` / `detail_event_pt` with
    `content_w`). When `current_height + needed > usable_pt` **and** the current page
    already has content, flush the page and start a fresh one (`continued = true`).
  - Orphan-heading guard: a sub-heading and its section's first item are placed together,
    so a heading never ends up alone at the bottom of a page (handled naturally by
    counting `heading + first item` as the "needed" height for that first item).
  - Oversized lone item guard: if a page holds only the date header and the next item
    still does not fit, place it anyway (field capping bounds item height), matching the
    current code's behaviour.
  - `first_page` is true only for the first descriptor; `continued` is true for the rest.

### `src/notebooks/month/mod.rs`

Replace the two `paginate` loops with a per-day loop:

```rust
let days = agenda::agenda_days(&m, events, config.year, month);
for day in &days {
    for plan in agenda::paginate_day(day, usable, content_w) {
        fragments.push(DayEvents { /* from plan + day + month context */ }.render()?);
    }
}
```

Leading pages (monthly view, tasks, daily writing pages) are untouched. Event pages are
still appended only when the month has events, so a static month keeps its exact page
count.

### `src/render.rs` (CSS)

- Add `.h-evt` — the Agenda/Details sub-heading: primary color, smaller than `.h-month`
  (~11pt), Fraunces, modest top/bottom margin.
- Remove the now-unused `.agenda-day`, `.detail-day`, and `.agenda-date` rules. Keep
  `.agenda-line`, `.detail-evt`, `.detail-title`, `.detail-meta`, `.swatch`, `.h-month`.

## Testing

- **`tests/templates_html.rs`** — rewrite `agenda_links_and_swatches` and
  `details_omit_empty_fields` against `DayEvents`. Assert: `id="agenda-{day}"` present on
  a `first_page` render and absent otherwise; agenda lines link to `#evt-{idx}`; detail
  blocks carry `id="evt-{idx}"`; swatches render; empty Where/Notes/Who omitted;
  `show_agenda_heading` / `show_details_heading` gate the sub-headings; `continued` emits
  `· cont.`.
- **`tests/month.rs`** — update `events_only_add_trailing_pages` (a small one-event day
  now produces a single combined page, not separate agenda + details pages); keep
  `busy_month_paginates_agenda_and_details` (assert the month still paginates beyond one
  page); rewrite `busy_day_splits_across_pages_with_repeated_header` for `paginate_day`,
  covering: agenda-only overflow across pages, an agenda→details split onto a fresh page,
  the orphan-heading guard, and the oversized-lone-item guard.
- **`tests/visual.rs` + goldens** — replace the `agenda.png` and `details.png` fragments
  with a single `day_events.png` golden built from sample data that exercises both a small
  day (Agenda + Details on one page) and a split day. Regenerate via `make update-goldens`.
- Run `make test` and `make clippy` before completion.

## Determinism notes

Page counts for the static parts of the notebook (cover, monthly view, tasks, daily
writing pages) are unchanged. Total event-page count may rise versus today because each
day starts a fresh page rather than packing multiple days per page — an accepted trade
for landing the reader on the right day.
