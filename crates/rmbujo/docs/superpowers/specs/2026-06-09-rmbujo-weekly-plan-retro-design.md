# rmbujo — Weekly Plan & Retro pages

**Status:** design approved 2026-06-09
**Scope:** add per-week Planning and Retrospective pages to each month notebook,
with a week-divider + link tab on the month index.

## Goal

Give each week of a month its own **Plan** page (intentions, tasks, per-day
notes) and a free-write **Retro** page. Surface them from the month index with a
coloured rule at each week boundary that draws out into a two-icon link tab. All
pages cross-link: month index → Plan/Retro, Plan ↔ Retro, Plan day-rows → day
pages, Plan/Retro → month index.

## Constraints that shape the design

- **One PDF per month** (`generate.rs` builds `YYYY.MM Name.pdf` per month).
  reMarkable resolves Typst `label`/`link` targets **only within a single PDF**,
  so every link must stay intra-PDF. This rules out whole-week pages for weeks
  that straddle a month boundary.
- **Month-index vertical budget is full.** On the Move, day 31 already centres at
  ≈451.6 pt on a 462.5 pt page (`geometry.rs:55-61`); 31 on-grid rows is "the
  only way" they fit one column. The week divider therefore must add **zero**
  vertical height.

## Week model — per-month segments

A **segment** is a maximal run of consecutive in-month days that belong to one
week, bounded by week starts:

- Segment boundaries = `{day 1} ∪ {days where DayRow.week_start == true}`.
  (`calendar.rs` already sets `week_start = (d != 1 && weekday == config week
  start)`, so day 1 is never itself flagged — the first segment always begins at
  day 1.)
- A segment runs from a boundary day up to (but not including) the next boundary
  day, or the end of the month.
- Consequences (all intentional):
  - The **first** segment of a month is partial when the month starts mid-week.
  - The **last** segment is partial when the month ends mid-week.
  - A week split across two months appears as a partial segment in **each**
    month's PDF, each covering only that month's days. E.g. with Monday starts, a
    Mon Jan 26 → Sun Feb 1 week yields a `01.26 – 01.31` segment in January and a
    `02.01 – 02.01` segment in February. This keeps every link intra-PDF.
- Each segment produces exactly **one Plan page and one Retro page** (≤ 6 of each
  per month).

**Week start** is the existing `config.week_start` (`"sun"` | `"mon"`). The code
default stays `"sun"`; Dan sets `week_start = "mon"` in his own config (outside
this repo). No new config keys.

A segment is identified for labelling by the **day number of its first day**
(unique within a month): `seg_id = first_day`.

## Month index changes

Add a week-divider above the first day row of **every** segment (including the
first segment, above day 1).

- **Zero-height rule.** The day rows keep their exact `monthly_row_center(sp, i)`
  positions. The divider is drawn as a `place`d horizontal rule **on the dot-row
  boundary** between the previous day's cell and the segment's first day cell
  (i.e. at the cell's top edge, `monthly_row_center(sp, i) - sp/2`). It occupies
  no row, so nothing below it shifts. For the first segment the boundary is above
  day 1 (just under the masthead band).
- **Style.** Indigo (`primary`) rule, ~2.0–2.5 pt thick, rounded ends, running
  from the left margin to the start of the tab.
- **Tab.** At the right end, a bordered two-cell tab that "draws out" from the
  rule: left cell filled indigo with the **PLAN** target icon (white stroke),
  right cell white with the **RETRO** ↺ icon (indigo stroke). A small white
  backdrop keeps it legible over the dots. The tab straddles the rule vertically
  over the empty right side of the adjacent cells (day number + weekday sit at the
  left, so no collision).
- **Links.** PLAN cell → `label("wplan-{seg_id}")`, RETRO cell →
  `label("wretro-{seg_id}")`.

## Weekly Plan page

A `dot-page` (white background, dot grid) carrying `label("wplan-{seg_id}")`.

Layout, top to bottom:

1. **Header** (minimal, no full-width rule):
   - Left: the segment date range `MM.DD – MM.DD` (first..last in-month day),
     as a **body-size** (≈ `num-fs`/11 pt) bold indigo label with its **own
     underline** — not a Fraunces masthead.
   - Right: a muted underlined `Month ↩` link → `label("monthly")`, then a small
     bordered button with the **RETRO** ↺ icon → `label("wretro-{seg_id}")`.
2. **Intentions** — heading at **body text size**, indigo + underline only (no
   oversized type), followed by a few lines of open dot-grid write space.
3. **Tasks** — same heading treatment; write space for ~3–5 short items.
4. **Per-day rows**, one for each in-month day of the segment:
   - Label `MM.DD Wd` (date in indigo, weekday in tomato/`accent`), underlined,
     linking to `label("day-{day}")`.
   - Two writing lines of space beneath each (rely on the dot grid; the mockup's
     dotted lines are indicative only).

Headings (Intentions, Tasks, the date) share one convention: **same size as body
text, distinguished by underline + colour**, never enlarged. A full 7-day segment
with two lines per day fits the Move page; partial segments are roomier.

## Weekly Retro page

A `dot-page` carrying `label("wretro-{seg_id}")`. Free-write — **no prescribed
sections**.

- **Header** (same minimal convention): left, `Retro · MM.DD – MM.DD` as a
  body-size underlined indigo label; right, `Month ↩` link → `label("monthly")`
  and a small bordered button with the **PLAN** target icon →
  `label("wplan-{seg_id}")`.
- Remainder of the page is open dot grid for writing.

## Icons

Both icons are drawn as **Typst vector primitives** (not font glyphs) for crisp
e-ink rendering, as small reusable preamble helpers parameterised by stroke
colour and size:

- **PLAN — target/bullseye:** two concentric `circle`s (stroke) + a filled centre
  dot.
- **RETRO — ↺ counter-clockwise:** a ~300° arc (open circle) with a short
  arrowhead at the leading end (two strokes), drawn via `path`/`curve`.

They appear: in the month-index tab (PLAN white-on-indigo, RETRO indigo-on-white),
on the Plan header (RETRO, indigo), and on the Retro header (PLAN, indigo).

## Page order within the month PDF (interleaved)

```
Month index  →  Tasks  →
  for each segment in order:
      Plan page
      that segment's day pages (DailyPage + extra dot pages per day)
      Retro page
  →  agenda / event pages (unchanged, appended at the end)
```

Linking is label-based, so order affects only swipe-through reading flow. Today's
flow (Month → Tasks → all days → all agenda) becomes Month → Tasks → per-week
{Plan → days → Retro} → agenda.

## Linking scheme (summary)

| From | Target label |
|--------------------------------|----------------------|
| Month tab — PLAN icon | `wplan-{seg_id}` |
| Month tab — RETRO icon | `wretro-{seg_id}` |
| Plan page (anchor) | `wplan-{seg_id}` |
| Plan header — Month ↩ | `monthly` |
| Plan header — RETRO icon | `wretro-{seg_id}` |
| Plan day-row `MM.DD Wd` | `day-{day}` |
| Retro page (anchor) | `wretro-{seg_id}` |
| Retro header — Month ↩ | `monthly` |
| Retro header — PLAN icon | `wplan-{seg_id}` |

## Components / code touch points

- **`calendar.rs`** — add a segmenting helper over `Month.days` that returns the
  ordered list of segments (each: `first_day`, `last_day`, the in-month `&[Day]`,
  and the `MM.DD – MM.DD` range). Drives both the month-index dividers and the
  Plan pages. Unit-tested independently of rendering.
- **`render/doc.rs` (preamble)** — add the two icon helpers (`plan-icon`,
  `retro-icon`) and a `week-divider`/tab helper, alongside the existing
  `cbadge`/`swatch` helpers.
- **`templates.rs`** — new `WeeklyPlan` and `WeeklyRetro` page emitters; extend
  `MonthlyView` to place a divider+tab above each segment's first day.
- **`notebooks/month/mod.rs`** — compute segments once; interleave Plan / day /
  Retro fragments per segment; keep agenda pages appended at the end.
- **`geometry.rs`** — a small helper for the divider's boundary Y
  (`monthly_row_center(sp, i) - sp/2`) if it clarifies the placement; no change to
  the row budget.

## Testing

- **Calendar segmentation** (`tests/calendar.rs`): segments for representative
  months — month starting on the week-start day, mid-week start, mid-week end,
  and a split-week month for both `sun` and `mon` starts. Assert boundaries,
  per-segment day sets, and `MM.DD – MM.DD` ranges.
- **Generation** (`tests/generate.rs` / `month.rs`): a generated month emits the
  expected count of Plan/Retro pages (= segment count) and the expected
  `wplan-*`/`wretro-*` labels and links; day-row links resolve to `day-{n}`;
  interleaved order is as specified.
- **Visual** (`tests/visual.rs` / `examples/screenshots.rs`): render the Move
  month index + a Plan + a Retro page and eyeball the divider/tab, the quiet
  headings, and that all 31 day rows still fit on-grid.
- Follow existing rmbujo test patterns; no manual testing.

## Out of scope / non-goals

- No cross-month (cross-PDF) whole-week pages, and no merge to a single combined
  PDF — explicitly rejected to preserve intra-PDF linking and the per-month model.
- No new config keys; `week_start` already exists and is reused.
- No change to the Future Log, Collection, Reference, or agenda/event pages beyond
  their position in the page order.
- Deploying Dan's `week_start = "mon"` is a config change in his dotfiles, not
  part of this repo's work.
