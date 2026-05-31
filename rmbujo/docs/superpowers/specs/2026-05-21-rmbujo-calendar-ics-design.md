# rmbujo Phase 2b — Calendar experience + ICS feeds — Design Spec

**Date:** 2026-05-21
**Status:** Approved for planning
**Author:** Dan (with Claude)
**Supersedes** the Phase-2 "ICS pills on the day-list" sketch in
`2026-05-20-rmbujo-design.md`. Validated on-device via the spikes
`2026-05-21-pages-links-spike.md` (page growth + tappable links + content-only
mechanics) and `examples/spike_month_prototype.rs` (the full month UX).

## Summary

The month notebook becomes a small navigable app. The pages you **write on** stay
static and calm; the **calendar** lives on dedicated pages at the end of the PDF and
is free to change. Internal PDF links wire it together, and a per-day **event-count
badge** jumps to the agenda. ICS feeds populate the calendar; they are fetched into a
cache so output stays reproducible and the cloud re-sync only changes when you ask.

The design rests on two device facts proven by the spike: **appending/replacing
trailing PDF pages under `rmapi put --content-only` preserves annotations on the
leading pages** (content-only never touches `.content`/`.rm`, only the PDF blob), and
**fulgur internal links (`<a href="#id">`) are tappable on the Move.**

## Goals

- A month notebook that "just works" and never puts changing content where you write.
- Fast in-document navigation: month ↔ day ↔ agenda ↔ event detail.
- ICS feeds (all-day **and** timed) rendered as an agenda + detail section that can
  grow/shrink across re-syncs without disturbing handwriting.
- Reproducible, offline-capable output (cached feeds); determinism preserved.
- Content clears the reMarkable toolbar on every page.

## Non-goals

- No reading back on-device ink (`.rm`) — annotation-aware layout and AI journaling
  insights are a separate future project. Collision is solved structurally instead.
- No events on the Future Log / Collection / Reference notebooks (calendar lives in
  the month notebooks).
- No multi-timezone display: every event renders in a single configured `timezone`
  (see ICS). (We *do* convert; we just don't show per-event zones.)

## Month notebook structure

Per-month PDF, in order. The **leading section is event-independent** (its bytes don't
depend on the calendar), so changing events only ever rewrites the trailing section.

| # | Section | Pages | Writable? | Depends on events? |
|---|---------|-------|-----------|--------------------|
| 1 | Monthly view (`id="monthly"`) | 1 | yes | no (badges show counts only) |
| 2 | Tasks | 1 | yes | no |
| 3 | Daily log | `days_in_month × pages_per_day` | yes | no (dated headers only) |
| 4 | Agenda | variable | no | yes |
| 5 | Details | variable | no | yes |

**1. Monthly view.** Header `"<Month> <Year>"`. One row per day: a **fixed-width date
link** (`<day#> <weekday>`) → that day's daily-log page, then (only on days with
events) a **count badge** → that date's agenda. Existing week-start divider rule
retained. The rest of each row is writing space. The badge is a navy stadium/pill,
about the dot-line height, wide enough that the digit never clips, with horizontal
padding so the tap target exceeds the visible pill.

**2. Tasks.** Unchanged bujo tasks page (header + dot grid).

**3. Daily log.** One headed page per day (configurable `pages_per_day`, default 1;
replaces the old fixed `daily_pages = 60`, which only existed because we couldn't add
templated pages on-device). Header: left = date `"<M>.<DD> <Dow>"` → monthly view;
right = count badge → that date's agenda (event days only). Dotted body for writing.

**4. Agenda (compact).** Title `"<Year> <Month> - Agenda"` where `"<Year> <Month>"` →
monthly view. Date-ordered; each **date header** → that day's daily-log page. Under
each date, events all-day-first then timed-by-time, each a one-liner: a small
**color swatch in the feed's color** (so multiple feeds are distinguishable), then
`**<label>**  <title>[ — <location>]` (label = `"All Day"` or `"HH:MM"`), the whole
line linking to that event's **detail** entry. Dates flow across pages; a date never
splits (`break-inside: avoid` → it starts a new page if it would wrap).

**5. Details (expanded).** Title `"<Year> <Month> - Details"` → monthly. Same
date-organized layout as the agenda, expanded: each date header → daily-log; each
event (`id="evt-<n>"`, the agenda's link target) shows `**<label>**  <title>`, then
**Where** (if location), **Notes** (if description), **Attendees** (if any) — empty
fields are omitted. Flows across pages like the agenda.

## Navigation (internal links)

All within one PDF (cross-PDF links don't work on-device). Proven tappable.

| From | Element | → To |
|------|---------|------|
| Monthly | day# / weekday | that day's daily-log page |
| Monthly | count badge | that date's agenda block |
| Daily | date header | monthly view |
| Daily | count badge | that date's agenda block |
| Agenda | title "<Year> <Month>" | monthly view |
| Agenda | date header | that day's daily-log page |
| Agenda | event line | that event's detail entry |
| Details | title | monthly view |
| Details | date header | that day's daily-log page |

## Toolbar safe-area + day-list fit

- **Top safe-area:** reserve **≈36pt (~12.7mm)** at the top of every page so content
  clears the reMarkable toolbar (measured on the Move). A tunable geometry constant.
  **Exception: cover pages** (full-bleed color behind the toolbar is fine). This
  applies to *all* page types including the existing Phase-1 ones, so **the golden
  images must be regenerated** (`make update-goldens`).
- **Badge detail:** the count pill needs ~2pt more vertical room / proper vertical
  centering so the digit doesn't clip at the top (observed in the prototype).
- **Day-list fit:** the toolbar reserve removes ~27pt of height, so 31 rows no longer
  fit at 4.5mm. Resolution: the **monthly view computes its row height to fit all
  days** in the available height (page − reserve − bottom margin − header), and sizes
  the month-list's own dot grid to that row height (dots stay aligned to rows via the
  existing half-pitch trick). Daily/grid pages keep the configured dot pitch (default
  = 4.756 mm, the "Dots Small" pitch; see below). The monthly list is thus
  self-consistent regardless of month length or reserve.

## Inserted pages — match a built-in template (no sideload)

To keep user-inserted pages visually consistent without sideloading a custom template
(which needs SSH and is wiped by firmware updates), rmbujo's dot grid is set to the
pitch of reMarkable's **built-in "Dots Small"** template. A user who needs more room
inserts a page and picks the built-in "Dots Small" — it has the same grid as our
generated pages. So rmbujo's **default dot spacing = 4.756 mm** — the measured "Dots
Small" pitch (42.5 reMarkable units × 0.31718 pt/unit = 13.48 pt; uniform x/y; dot
mark ~0.1 mm + hairline stroke). Measured from an exported template page
(`dots small.pdf`). The monthly view fits-to-height, so it adapts to this pitch;
`pages_per_day` still pre-allocates dotted pages so inserting is seldom needed.
(Phase/margins of an inserted page may differ slightly from ours — pages aren't
overlaid, so matching the pitch + small dot is what makes them look consistent.)

## ICS

- **Config:** existing `[[ics]]` feeds (`name`, `url`, `color` = theme color name);
  plus a top-level `timezone` (IANA name, e.g. `"America/Toronto"`), defaulting to the
  detected system timezone, validated against the tz database.
- **Fetch — cache-on-fetch snapshot.** Each feed's raw `.ics` is cached beside the
  toml at `<year>/.ics-cache/<feed-slug>.ics`. Generation always parses the cache, so
  output is reproducible and works offline. Re-fetch happens on `rmbujo new` and when
  the new `--refresh-feeds` flag is passed (to either `new` or `<config>`); a missing
  cache auto-fetches. Deploy uploads only `*.pdf`, so the cache never reaches the
  device. Events therefore change only on a deliberate refresh — every other
  regenerate is byte-identical and the cloud re-sync stays quiet.
- **Scope — all-day and timed.** All-day events (dated, recurring via `RRULE`, and
  multi-day via exclusive `DTEND`) and timed events. Recurrences and multi-day spans
  expand into per-day occurrences clipped to the config year.
- **Timezone.** A configured `timezone` is the single rendering zone. Timed events are
  parsed with their source zone (`TZID`, a `Z`/UTC suffix, or a floating local time)
  and **converted to the configured timezone** for both the displayed `HH:MM` and the
  **calendar day** they land on (a UTC event near midnight can fall on a different
  local day — so conversion happens before bucketing by date). RRULE expansion is
  zone-aware (expanded against the source zone, then converted). All-day events are
  **floating dates** — rendered as-is, never converted.
- **Event model:** `EventOccurrence { date: NaiveDate, time: Option<NaiveTime>,
  title: String, location: Option<String>, description: Option<String>,
  attendees: Vec<String>, color: String }`, where `date`/`time` are already in the
  configured timezone. Sorted deterministically: date, then all-day-before-timed,
  then time, then title.
- **Crates:** `ureq` (blocking HTTPS, no async runtime — fits the sync CLI), `ical`
  (iCalendar parser), `rrule` (zone-aware recurrence expansion), `chrono-tz` (IANA
  tz database) and `iana-time-zone` (detect the system zone for the default). A small
  parse spike during implementation validates them against a real Google holiday feed,
  a recurring birthday feed, and a `TZID`/UTC timed feed before committing to them.
- **Errors:** a re-fetch failure keeps the existing cached snapshot (never clobber a
  good cache) and warns; no cache + fetch fail warns and renders that feed empty;
  malformed events/feeds are skipped with a warning. A feed never aborts generation.
  `config.validate()` checks each feed `url` non-empty and `color` is a known theme
  color.

## Determinism, page-count invariant, refresh

- `render_pdf` stays pure; the only non-deterministic input (the network) is confined
  to fetch, which writes the cache. Given the cache, regeneration is byte-identical —
  the existing `deterministic_bytes` test still holds.
- **Page-count invariant:** the leading section's page count is event-independent and
  fixed for a given config (`1 + 1 + days × pages_per_day`). The agenda + details
  pages are appended at the end and may grow/shrink with events. We never
  insert/reorder pages in the middle of the generated PDF, and never change the
  meaning of a leading page index. This is exactly what makes `put --content-only`
  safe (it only swaps the PDF blob; `.content`/`.rm` are preserved).
- **Sync rule (documented in README):** sync the device → run rmbujo (`--refresh-feeds`
  to update events) → sync the device. rmapi fetches the latest cloud state before
  modifying; the discipline is to let the device's edits reach the cloud first.

## Module / file layout

```
src/ics/
  mod.rs     # orchestrate feeds + year -> BTreeMap<NaiveDate, Vec<EventOccurrence>>;
             # pub EventOccurrence
  fetch.rs   # Fetcher trait (testable seam) + HTTP impl; cache read/write;
             # --refresh-feeds logic
  parse.rs   # raw .ics bytes -> events; RRULE + multi-day expansion; year filter
src/notebooks/month/
  mod.rs     # assemble the month PDF (monthly + tasks + daily + agenda + details)
  agenda.rs  # agenda + details fragment builders (date grouping, badges, links)
templates/   # askama: monthly_view, daily_page, agenda, details (+ existing)
```

- `generate.rs`: build the year's event map once (via `ics`), pass each month its
  slice. Pure (no I/O beyond the cache read in `ics::fetch`).
- `render.rs`: CSS gains the toolbar safe-area (all non-cover pages), the count-pill
  badge, agenda/detail/link styles, and the monthly fit-to-height row sizing.
- `config.rs`: add `pages_per_day` (default 1) and `timezone` (IANA, default detected
  system zone; validated against the tz database); validate ics feeds. `cli.rs`: add
  `--refresh-feeds`. `wizard.rs`: prompt for `pages_per_day` and `timezone` (prefilled
  with the detected zone); **does not** prompt for ICS feeds (add `[[ics]]` by editing
  the toml — documented in README).
- The badge, link helpers, and date-grouping logic from the prototype
  (`examples/spike_month_prototype.rs`) are the reference for the real builders.

## Testing

- **ics/parse** (offline, fixtures): a dated holiday; a multi-day all-day span
  (DTEND exclusivity); a yearly `RRULE` birthday expanding into the target year; a
  timed event (time + location); an out-of-year event excluded; deterministic order.
  **Timezone:** a `TZID` timed event converts to the configured zone's `HH:MM`; a
  UTC event near midnight lands on the correct **local day** (day-shift); an all-day
  event stays on its date regardless of zone.
- **ics/fetch:** cache read/write and `--refresh-feeds` via a fake `Fetcher` (no
  network); fetch-failure keeps the cached snapshot.
- **templates/HTML:** monthly view has the right badge counts + day/agenda links;
  daily pages have dated headers + links; agenda is date-ordered with all-day/timed
  lines + per-event anchors; details omit empty Where/Notes/Attendees. **Leading
  section page count is event-independent** (assert it doesn't change with events).
- **PDF/links:** internal links resolve (no unresolved anchors; annotation/destination
  counts match expectations, as in the spike).
- **layout/visual:** new goldens for monthly-with-badges, daily-with-header, agenda,
  details; regenerate all existing goldens for the toolbar reserve. Determinism holds.

## Open items folded into the plan

- Apply the toolbar reserve to the real CSS and **regenerate goldens** (behavior
  change to all pages).
- Fix the badge vertical clip (taller box / centering).
- Parse spike to confirm `ureq` + `ical` + `rrule` against real feeds before wiring.
- ~~Measure "Dots Small" pitch~~ **done: 4.756 mm** (from `dots small.pdf`); set as
  the default dot spacing. (Lines Small = 5.82 mm, recorded for the future lined
  option.)
- `notebooks/month.rs` will grow; split into `month/{mod,agenda}.rs` as above.

## Future (out of scope)

- A **lined** grid as a config option (matching reMarkable's built-in "Lines Small",
  measured at 5.82 mm row spacing), alongside the dot grid.
- Reading device ink back via rmapi (`.rm` → rendered images → vision model) for
  journaling summaries/insights, and optionally annotation-aware layout. A separate,
  read-only project; this design deliberately needs none of it.

## Rationale

- **Calendar on dedicated trailing pages** rather than inline on writing pages: the
  generator can't see your ink, so the only robust way to avoid clobbering it is
  spatial separation. The spike proved trailing pages can grow/shrink safely.
- **Count badge over a calendar icon:** a tiny icon was untappable and uninformative;
  a count pill is a bigger target and shows how busy a day is.
- **Cache-on-fetch:** keeps the project's determinism/offline guarantees while still
  reading live URLs; the user controls when events (and thus the cloud doc) change.
