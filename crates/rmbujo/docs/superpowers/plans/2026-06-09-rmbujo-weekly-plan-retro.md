# Weekly Plan & Retro Pages — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers-extended-cc:subagent-driven-development (recommended) or superpowers-extended-cc:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a weekly Plan page and a free-write Retro page for every week-segment of a month notebook, surfaced from the month index by a zero-height week-divider rule that draws out into a PLAN/RETRO icon tab.

**Architecture:** Each month is its own PDF; reMarkable links resolve only within one PDF, so weeks are sliced into **per-month segments** (a run of consecutive in-month days bounded by week starts). `calendar::segments()` produces the segments; new Typst page emitters (`WeeklyPlan`, `WeeklyRetro`) render the pages; `MonthlyView` places a divider + icon tab above each segment's first day; the month builder interleaves `Plan → segment day pages → Retro` and appends agenda pages last. Icons are drawn as Typst vector primitives in the shared preamble.

**Tech Stack:** Rust, Typst (in-process compile via `crate::render`), `chrono`, `lopdf` (page-count assertions), `pdftoppm` + `image` (golden visual diffs).

**Spec:** `crates/rmbujo/docs/superpowers/specs/2026-06-09-rmbujo-weekly-plan-retro-design.md`

All paths below are relative to `crates/rmbujo/`. Run all commands from `crates/rmbujo/`.

---

### Task 1: Week-segment calendar model

**Goal:** Add a `Segment` type and `calendar::segments(&Month)` that slices a month into per-week segments, with a padded `MM.DD – MM.DD` range helper.

**Files:**
- Modify: `src/calendar.rs` (append after `build_year`)
- Test: `tests/calendar.rs` (append)

**Acceptance Criteria:**
- [ ] `segments(&Month)` returns one `Segment` per week-segment; a new segment begins at day 1 and at every `week_start` day; all days are covered exactly once, in order.
- [ ] `Segment::first_day`/`last_day`/`id` and `Segment::date_range(month)` (`"MM.DD – MM.DD"`, zero-padded, en-dash) behave as specified, including single-day segments.
- [ ] Boundary day numbers match the existing `week_start` flags for both `sun` and `mon`.

**Verify:** `cargo test -p rmbujo --test calendar` → all pass

**Steps:**

- [ ] **Step 1: Write the failing tests** — append to `tests/calendar.rs`:

```rust
use rmbujo::calendar::segments;

#[test]
fn segments_may_2026_sunday() {
    // Sunday week-starts in May 2026 = 3,10,17,24,31; plus day-1 boundary.
    let m = build_month(2026, 5, "sun").unwrap();
    let segs = segments(&m);
    let firsts: Vec<u32> = segs.iter().map(|s| s.first_day()).collect();
    assert_eq!(firsts, vec![1, 3, 10, 17, 24, 31]);
    // Every day covered exactly once, in order.
    let flat: Vec<u32> = segs.iter().flat_map(|s| s.days.iter().map(|d| d.day)).collect();
    assert_eq!(flat, (1..=31).collect::<Vec<_>>());
    // First segment is the partial lead-in (days 1..=2).
    assert_eq!(segs[0].first_day(), 1);
    assert_eq!(segs[0].last_day(), 2);
    // Last segment is a single day (the 31st).
    assert_eq!(segs.last().unwrap().first_day(), 31);
    assert_eq!(segs.last().unwrap().last_day(), 31);
}

#[test]
fn segments_may_2026_monday() {
    // Monday week-starts in May 2026 = 4,11,18,25; plus day-1 boundary.
    let m = build_month(2026, 5, "mon").unwrap();
    let firsts: Vec<u32> = segments(&m).iter().map(|s| s.first_day()).collect();
    assert_eq!(firsts, vec![1, 4, 11, 18, 25]);
}

#[test]
fn segment_date_range_padded() {
    let m = build_month(2026, 5, "mon").unwrap();
    let segs = segments(&m);
    // Segment starting on the 4th runs 4..=10 (Mon..Sun).
    let s = segs.iter().find(|s| s.first_day() == 4).unwrap();
    assert_eq!(s.last_day(), 10);
    assert_eq!(s.date_range(5), "05.04 – 05.10");
    assert_eq!(s.id(), 4);
    // Single-day last segment renders a same-day range.
    let last = segs.last().unwrap();
    assert_eq!(last.date_range(5), format!("05.{:02} – 05.{:02}", last.first_day(), last.last_day()));
}
```

- [ ] **Step 2: Run tests to verify they fail** — Run: `cargo test -p rmbujo --test calendar segments` → Expected: FAIL (`cannot find function segments`).

- [ ] **Step 3: Implement** — append to `src/calendar.rs`:

```rust
/// One per-week segment of a month: a run of consecutive in-month days bounded
/// by week starts. A new segment begins at day 1 and at every `week_start` day,
/// so a week split across a month boundary appears only as the in-month run.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub days: Vec<Day>,
}

impl Segment {
    pub fn first_day(&self) -> u32 {
        self.days.first().expect("segment is never empty").day
    }
    pub fn last_day(&self) -> u32 {
        self.days.last().expect("segment is never empty").day
    }
    /// Stable label id for this segment = its first day number (unique in a month).
    pub fn id(&self) -> u32 {
        self.first_day()
    }
    /// `"MM.DD – MM.DD"` (zero-padded, en-dash), the segment's in-month range.
    pub fn date_range(&self, month: u32) -> String {
        format!(
            "{:02}.{:02} – {:02}.{:02}",
            month,
            self.first_day(),
            month,
            self.last_day()
        )
    }
}

/// Slice a month into per-week segments. A new segment starts at day 1 and at
/// each `week_start` day; every day lands in exactly one segment, in order.
pub fn segments(m: &Month) -> Vec<Segment> {
    let mut out: Vec<Segment> = Vec::new();
    for d in &m.days {
        if out.is_empty() || d.week_start {
            out.push(Segment { days: vec![d.clone()] });
        } else {
            out.last_mut().unwrap().days.push(d.clone());
        }
    }
    out
}
```

- [ ] **Step 4: Run tests to verify they pass** — Run: `cargo test -p rmbujo --test calendar` → Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/calendar.rs tests/calendar.rs
git commit -m "feat(rmbujo): week-segment model for monthly week slicing"
```

---

### Task 2: Typst preamble — PLAN/RETRO icons + week tab helper

**Goal:** Add reusable Typst helpers to the shared preamble: `plan-icon` (target/bullseye), `retro-icon` (counter-clockwise arrow), and `wtab` (the month-index two-icon link tab).

**Files:**
- Modify: `src/render/doc.rs` (inside `build_preamble`, alongside `cbadge`/`swatch`)
- Test: `tests/render.rs` (append a compile smoke test)

**Acceptance Criteria:**
- [ ] `plan-icon(col, sz)`, `retro-icon(col, sz)`, and `wtab(seg)` are defined in the preamble and compile.
- [ ] `wtab(seg)` renders a bordered two-cell tab: left cell filled `primary` with a white `plan-icon` linking `label("wplan-" + str(seg))`; right cell white with an indigo `retro-icon` linking `label("wretro-" + str(seg))`.
- [ ] A page using all three helpers compiles to a non-empty PDF.

**Verify:** `cargo test -p rmbujo --test render preamble_icons` → PASS

**Steps:**

- [ ] **Step 1: Write the failing test** — append to `tests/render.rs`:

```rust
#[test]
fn preamble_icons_compile() {
    use rmbujo::device::get_device;
    use rmbujo::geometry::default_grid;
    use rmbujo::render::doc::build_preamble;
    use rmbujo::render::compile_pdf;
    use rmbujo::theme::load_theme;

    let dev = get_device("paper-pro-move").unwrap();
    let grid = default_grid(&dev);
    let theme = load_theme("library").unwrap();
    // A page exercising every new helper, plus the label sinks the tab links need.
    let body = "#dot-page[#plan-icon(primary, 12pt) #retro-icon(primary, 12pt) #wtab(8)]\n\
                #plain-page[#hide[#box[x]#label(\"wplan-8\")#box[x]#label(\"wretro-8\")]]\n";
    let src = format!("{}{}", build_preamble(&dev, &grid, &theme), body);
    let pdf = compile_pdf(&src, &[]).unwrap();
    assert!(pdf.len() > 1000, "expected a non-empty PDF, got {} bytes", pdf.len());
}
```

> Note: `compile_pdf` and `render::doc` are already `pub` (see `src/render/mod.rs`). If `build_preamble` is not re-exported, reference it as `rmbujo::render::doc::build_preamble` (it is `pub` in `doc.rs`).

- [ ] **Step 2: Run test to verify it fails** — Run: `cargo test -p rmbujo --test render preamble_icons` → Expected: FAIL (typst compile error: unknown `plan-icon`).

- [ ] **Step 3: Implement** — in `src/render/doc.rs`, inside the big `format!(r#"…"#)` preamble string, add these `#let`s next to the existing `cbadge`/`swatch` definitions (they may use `primary`, `white`, `calc`, `place`, `circle`, `line` — all available):

```typst
// PLAN icon — a target/bullseye: two concentric rings + a filled centre dot,
// all in `col`. `sz` is the box edge.
#let plan-icon(col, sz) = box(width: sz, height: sz, {{
  let r = sz / 2
  let th = sz * 0.12
  place(center + horizon, circle(radius: r * 0.92, stroke: (paint: col, thickness: th)))
  place(center + horizon, circle(radius: r * 0.46, stroke: (paint: col, thickness: th)))
  place(center + horizon, circle(radius: r * 0.15, fill: col, stroke: none))
}})

// RETRO icon — a counter-clockwise open-circle arrow. The arc is drawn as a
// fan of short round-capped segments (robust across renderers, no arc
// primitive), with a two-stroke arrowhead at the leading (top) end.
#let retro-icon(col, sz) = box(width: sz, height: sz, {{
  let cx = sz / 2
  let cy = sz / 2
  let r = sz * 0.34
  let th = sz * 0.12
  let a0 = 60deg   // leading end (arrowhead here)
  let a1 = 330deg  // trailing end; gap sits on the lower-right
  let n = 28
  let pt(a) = (cx + r * calc.cos(a), cy - r * calc.sin(a))
  for i in range(n) {{
    let t0 = a0 + (a1 - a0) * (i / n)
    let t1 = a0 + (a1 - a0) * ((i + 1) / n)
    place(top + left, line(start: pt(t0), end: pt(t1),
      stroke: (paint: col, thickness: th, cap: "round")))
  }}
  // Arrowhead at a0, drawn as two short legs off the leading point.
  let h = sz * 0.22
  let tip = pt(a0)
  place(top + left, line(start: tip, end: (tip.at(0) - h, tip.at(1) - h * 0.2),
    stroke: (paint: col, thickness: th, cap: "round")))
  place(top + left, line(start: tip, end: (tip.at(0) + h * 0.2, tip.at(1) - h),
    stroke: (paint: col, thickness: th, cap: "round")))
}})

// Month-index week tab — PLAN | RETRO, each linking its weekly page. `seg` is
// the segment id (its first day number).
#let wtab(seg) = box(stroke: 2pt + primary, radius: 4pt, clip: true)[
  #grid(columns: 2, rows: auto, gutter: 0pt,
    box(fill: primary, inset: (x: 5pt, y: 2.5pt))[
      #link(label("wplan-" + str(seg)))[#plan-icon(white, 11pt)]],
    box(fill: white, inset: (x: 5pt, y: 2.5pt))[
      #link(label("wretro-" + str(seg)))[#retro-icon(primary, 11pt)]])
]
```

> The preamble is a Rust `format!(r#"…"#)` raw string, so every literal `{` and `}` inside this Typst code MUST be doubled (`{{` / `}}`) exactly as written above. Do not double the `{xxx}pt` interpolations that already exist elsewhere in the preamble.

- [ ] **Step 4: Run test to verify it passes** — Run: `cargo test -p rmbujo --test render preamble_icons` → Expected: PASS. If it fails on doubled-brace typos, fix the `{{`/`}}` and re-run.

- [ ] **Step 5: Commit**

```bash
git add src/render/doc.rs tests/render.rs
git commit -m "feat(rmbujo): Typst PLAN/RETRO icons + week tab preamble helpers"
```

---

### Task 3: Month-index week dividers + tabs

**Goal:** `MonthlyView` places a zero-height indigo rule with a `wtab` above the first day of every segment (day 1 and every `week_start` day), without shifting any day row.

**Files:**
- Modify: `src/templates.rs` (`MonthlyView::render`, around lines 120-169)
- Modify: `tests/visual.rs` (`label_sink`, ~line 151-158)
- Test: `tests/visual.rs` golden `monthly_view` (regenerated)

**Acceptance Criteria:**
- [ ] For each boundary day at row index `i` (i.e. `i == 0` or `d.week_start`), a rule + `wtab(d.day)` is `place`d at `monthly_row_center(sp, i) - sp/2` pt.
- [ ] Dividers are out-of-flow (`#place`), so all day rows keep their existing centres and all 31 days remain on the page.
- [ ] `label_sink` defines `wplan-N`/`wretro-N` for `N` in 1..=31 so the isolated fragment compiles.
- [ ] `visual_regression` passes against a regenerated `monthly_view` golden showing the rules and tabs.

**Verify:** `cargo test -p rmbujo --test visual visual_regression` → PASS

**Steps:**

- [ ] **Step 1: Update `label_sink`** in `tests/visual.rs` so the isolated `MonthlyView`/weekly fragments resolve their new link targets. Replace the loop body:

```rust
fn label_sink() -> String {
    let mut anchors = String::from("#box[x]#label(\"monthly\")");
    for n in 1..=31 {
        anchors.push_str(&format!("#box[x]#label(\"day-{n}\")"));
        anchors.push_str(&format!("#box[x]#label(\"agenda-{n}\")"));
        anchors.push_str(&format!("#box[x]#label(\"wplan-{n}\")"));
        anchors.push_str(&format!("#box[x]#label(\"wretro-{n}\")"));
    }
    format!("#plain-page[#hide[{anchors}]]\n")
}
```

- [ ] **Step 2: Implement the divider** in `MonthlyView::render` (`src/templates.rs`). Inside the existing `for (i, d) in self.days.iter().enumerate()` loop, after the row is pushed, emit a divider when the day is a segment boundary. Add this just before the closing `}` of the loop body:

```rust
            // A segment boundary (day 1, or any week-start day) gets a zero-height
            // rule + PLAN/RETRO tab on the dot-row boundary above this day's cell.
            // It is `place`d (out of flow), so no day row shifts.
            if i == 0 || d.week_start {
                let yb = crate::geometry::monthly_row_center(sp, i) - sp / 2.0;
                rows.push_str(&format!(
                    "#place(top + left, dy: {yb}pt)[#box(width: 100%)[\
                     #place(left + horizon, line(length: 100%, \
                     stroke: (paint: primary, thickness: 2.4pt, cap: \"round\"))) \
                     #place(right + horizon, wtab({seg}))]]\n",
                    yb = yb,
                    seg = d.day,
                ));
            }
```

> `sp`, `rows`, `i`, and `d` are already in scope in that loop (see the existing row-placement code). `wtab`, `primary` come from the preamble (Task 2).

- [ ] **Step 3: Run the visual test to confirm it now renders (and update the golden)** —

```bash
RMBUJO_UPDATE_GOLDENS=1 cargo test -p rmbujo --test visual visual_regression
```

Then inspect `tests/goldens/monthly_view.png`: confirm an indigo rule with a PLAN|RETRO tab sits just above day 1 and above each week-start day, the tab icons read as a target (left) and a counter-clockwise arrow (right), and all 31 day rows are still present and on-grid. If the retro arrow reads clockwise, swap `a0`/`a1` in `retro-icon` (Task 2) and regenerate.

- [ ] **Step 4: Verify against the committed golden** — Run: `cargo test -p rmbujo --test visual visual_regression` → Expected: PASS (diff < 0.01).

- [ ] **Step 5: Commit**

```bash
git add src/templates.rs tests/visual.rs tests/goldens/monthly_view.png
git commit -m "feat(rmbujo): week-divider rule + PLAN/RETRO tab on month index"
```

---

### Task 4: WeeklyPlan and WeeklyRetro page emitters

**Goal:** Add `WeeklyPlan` and `WeeklyRetro` Typst page emitters with the agreed minimal-header layout and cross-links, plus golden coverage.

**Files:**
- Modify: `src/templates.rs` (new structs + `impl render`, after `DailyPage`)
- Modify: `tests/visual.rs` (`fragment_pages` — build + register the two new fragments)
- Test: `tests/visual.rs` golden `weekly_plan`, `weekly_retro`; markup assertions in `tests/render.rs`

**Acceptance Criteria:**
- [ ] `WeeklyPlan` renders: a body-size underlined indigo date label (`MM.DD – MM.DD`); a muted underlined `Month` link → `label("monthly")`; a bordered button with `retro-icon` → `label("wretro-{seg}")`; an `Intentions` then a `Tasks` body-size underlined heading each over open dot space; one row per segment day `MM.DD Wd` (date indigo, weekday accent) linking `label("day-{day}")`; and carries `label("wplan-{seg}")`. No full-width header rule.
- [ ] `WeeklyRetro` renders: `Retro · MM.DD – MM.DD` underlined indigo label; `Month` link → `label("monthly")`; a button with `plan-icon` → `label("wplan-{seg}")`; carries `label("wretro-{seg}")`; rest is free dot grid. No full-width header rule.
- [ ] Both are `dot-page`s (white background, dot grid).
- [ ] `visual_regression` passes for `weekly_plan` and `weekly_retro`; markup assertions confirm the labels/links.

**Verify:** `cargo test -p rmbujo --test visual visual_regression && cargo test -p rmbujo --test render weekly_markup` → PASS

**Steps:**

- [ ] **Step 1: Write the markup assertions** — append to `tests/render.rs`:

```rust
#[test]
fn weekly_markup_has_labels_and_links() {
    use rmbujo::calendar::{build_month, segments};
    use rmbujo::templates::{WeeklyPlan, WeeklyRetro};

    let m = build_month(2026, 5, "mon").unwrap();
    let seg = segments(&m).into_iter().find(|s| s.first_day() == 4).unwrap();

    let plan = WeeklyPlan { month_num: 5, segment: &seg }.render().unwrap();
    assert!(plan.contains("label(\"wplan-4\")"), "plan anchor");
    assert!(plan.contains("label(\"wretro-4\")"), "plan -> retro link");
    assert!(plan.contains("label(\"monthly\")"), "plan -> month link");
    assert!(plan.contains("label(\"day-4\")") && plan.contains("label(\"day-10\")"), "day links");
    assert!(plan.contains("Intentions") && plan.contains("Tasks"), "section headings");
    assert!(plan.contains("05.04 – 05.10"), "date range header");

    let retro = WeeklyRetro { month_num: 5, segment: &seg }.render().unwrap();
    assert!(retro.contains("label(\"wretro-4\")"), "retro anchor");
    assert!(retro.contains("label(\"wplan-4\")"), "retro -> plan link");
    assert!(retro.contains("label(\"monthly\")"), "retro -> month link");
    assert!(retro.contains("Retro"), "retro heading");
}
```

- [ ] **Step 2: Run to verify it fails** — Run: `cargo test -p rmbujo --test render weekly_markup` → Expected: FAIL (`WeeklyPlan` not found).

- [ ] **Step 3: Implement the emitters** — append to `src/templates.rs` (after `DailyPage`'s `impl`). Reuse the file's existing `esc_markup`/`date_label` helpers and the `crate::calendar::Segment` type:

```rust
use crate::calendar::Segment;

/// A weekly Plan page: minimal underlined date header (Month link + retro icon
/// button), Intentions and Tasks blocks, then one linked row per segment day.
pub struct WeeklyPlan<'a> {
    pub month_num: u32,
    pub segment: &'a Segment,
}

impl WeeklyPlan<'_> {
    pub fn render(&self) -> anyhow::Result<String> {
        let seg = self.segment.id();
        let range = self.segment.date_range(self.month_num);
        // Body-size underlined indigo heading used for the date + section labels.
        // Header row: date (left) … Month link + retro-icon button (right).
        let header = format!(
            "#box(width: 100%)[\
             #text(font: \"Hanken Grotesk\", size: 11pt, weight: 700, fill: primary)[\
             #underline(offset: 2.5pt)[{range}]] \
             #h(1fr) \
             #link(label(\"monthly\"))[#text(font: \"Hanken Grotesk\", size: 9pt, fill: muted)[\
             #underline[Month]]] #h(6pt) \
             #box(stroke: 1.5pt + primary, radius: 5pt, inset: (x: 4pt, y: 2pt))[\
             #link(label(\"wretro-{seg}\"))[#retro-icon(primary, 11pt)]] \
             #label(\"wplan-{seg}\")]\n",
        );
        // Section heading = body size, underline + colour only (no large type).
        let heading = |t: &str| {
            format!(
                "#text(font: \"Hanken Grotesk\", size: 11pt, weight: 700, fill: primary)[\
                 #underline(offset: 2pt)[{t}]]",
                t = t
            )
        };
        let mut days = String::new();
        for d in &self.segment.days {
            // "MM.DD Wd": date in indigo, weekday in accent; whole label links day page.
            days.push_str(&format!(
                "#block(above: 6pt, below: 0pt)[\
                 #link(label(\"day-{day}\"))[\
                 #text(font: \"Hanken Grotesk\", size: 11pt, weight: 700, fill: primary)[\
                 #underline(offset: 2pt)[{mm}.{dd} ]]\
                 #text(font: \"Hanken Grotesk\", size: 11pt, weight: 700, fill: accent)[\
                 #underline(offset: 2pt)[{wd}]]]]\n",
                day = d.day,
                mm = format!("{:02}", self.month_num),
                dd = format!("{:02}", d.day),
                wd = esc_markup(d.weekday),
            ));
        }
        Ok(format!(
            "#dot-page[\n{header}\
             #block(above: 8pt, below: 2pt)[{intentions}]\n\
             #v(34pt)\n\
             #block(below: 2pt)[{tasks}]\n\
             #v(46pt)\n\
             {days}]\n",
            header = header,
            intentions = heading("Intentions"),
            tasks = heading("Tasks"),
            days = days,
        ))
    }
}

/// A weekly Retro page: minimal underlined `Retro · range` header (Month link +
/// plan icon button), then free dot-grid write space.
pub struct WeeklyRetro<'a> {
    pub month_num: u32,
    pub segment: &'a Segment,
}

impl WeeklyRetro<'_> {
    pub fn render(&self) -> anyhow::Result<String> {
        let seg = self.segment.id();
        let range = self.segment.date_range(self.month_num);
        Ok(format!(
            "#dot-page[\n\
             #box(width: 100%)[\
             #text(font: \"Hanken Grotesk\", size: 11pt, weight: 700, fill: primary)[\
             #underline(offset: 2.5pt)[Retro · {range}]] \
             #h(1fr) \
             #link(label(\"monthly\"))[#text(font: \"Hanken Grotesk\", size: 9pt, fill: muted)[\
             #underline[Month]]] #h(6pt) \
             #box(stroke: 1.5pt + primary, radius: 5pt, inset: (x: 4pt, y: 2pt))[\
             #link(label(\"wplan-{seg}\"))[#plan-icon(primary, 11pt)]] \
             #label(\"wretro-{seg}\")]\n\
             ]\n",
        ))
    }
}
```

> Add `use crate::calendar::Segment;` to the top-of-file `use` group rather than mid-file if the linter prefers; functionally either works. `accent`/`muted`/`primary` are preamble bindings.

- [ ] **Step 4: Run markup assertions** — Run: `cargo test -p rmbujo --test render weekly_markup` → Expected: PASS. Fix any label-string mismatches until green.

- [ ] **Step 5: Register golden fragments** — in `tests/visual.rs` `fragment_pages()`, build the two fragments and add them to the returned vec. Add near the other fragment construction (after `day_events`):

```rust
    // weekly plan + retro for the Monday-start segment beginning May 4 2026.
    let m_mon = build_month(2026, 5, "mon").unwrap();
    let seg = rmbujo::calendar::segments(&m_mon)
        .into_iter()
        .find(|s| s.first_day() == 4)
        .unwrap();
    let weekly_plan = rmbujo::templates::WeeklyPlan { month_num: 5, segment: &seg }
        .render()
        .unwrap();
    let weekly_retro = rmbujo::templates::WeeklyRetro { month_num: 5, segment: &seg }
        .render()
        .unwrap();
```

…and add to the returned `vec![…]`:

```rust
        ("weekly_plan", weekly_plan),
        ("weekly_retro", weekly_retro),
```

- [ ] **Step 6: Generate + inspect goldens** —

```bash
RMBUJO_UPDATE_GOLDENS=1 cargo test -p rmbujo --test visual visual_regression
```

Inspect `tests/goldens/weekly_plan.png` and `weekly_retro.png`: headings are body-size (underline + indigo, not large Fraunces), no full-width header rule, all seven day rows present with two lines of space each, the retro/plan icon buttons render. Adjust the `#v(..pt)` spacers in `WeeklyPlan` if Sunday is crowded, and regenerate.

- [ ] **Step 7: Verify against committed goldens** — Run: `cargo test -p rmbujo --test visual visual_regression` → Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add src/templates.rs tests/visual.rs tests/render.rs tests/goldens/weekly_plan.png tests/goldens/weekly_retro.png
git commit -m "feat(rmbujo): weekly Plan and Retro page emitters"
```

---

### Task 5: Interleave Plan/Retro into the month notebook

**Goal:** The month builder emits, after the month index + Tasks, `Plan → that segment's day pages → Retro` for each segment, with agenda pages still appended last; existing page-count tests updated to the new totals.

**Files:**
- Modify: `src/notebooks/month/mod.rs` (`build_month_pdf`, lines 13-103)
- Test: `tests/month.rs` (update `month_page_count_static`, `month_pages_per_day_multiplies_daily`, `events_only_add_trailing_pages`, `busy_month_paginates_per_day_pages`)

**Acceptance Criteria:**
- [ ] Fragment order is: `MonthlyView`, `Tasks`, then for each segment `WeeklyPlan` → (each segment day's `DailyPage` + `pages_per_day-1` `DotGrid`) → `WeeklyRetro`, then all agenda pages.
- [ ] Total page count (no events) = `2 + days * pages_per_day + 2 * segment_count`.
- [ ] Agenda pages still append only when events exist; their order/labels are unchanged.
- [ ] Updated `tests/month.rs` assertions derive expected counts from `calendar::segments` (not hard-coded), and pass.

**Verify:** `cargo test -p rmbujo --test month` → PASS

**Steps:**

- [ ] **Step 1: Update the builder** — in `src/notebooks/month/mod.rs`, replace the day-page loop (current lines 43-68, the `let mut fragments = vec![ MonthlyView…, Tasks ]` block and the `for d in &m.days { … }` loop) with a segment-interleaved build. Keep the agenda block (lines 78-100) unchanged.

```rust
    let mut fragments = vec![
        MonthlyView {
            month_name: m.name,
            year: config.year,
            month_num: month,
            spacing_pt: grid.spacing_pt,
            days: &day_rows,
        }
        .render()?,
        Tasks.render()?,
    ];

    // Per-week segments, interleaved: Plan → that segment's day pages → Retro.
    let segs = crate::calendar::segments(&m);
    for seg in &segs {
        fragments.push(
            crate::templates::WeeklyPlan {
                month_num: month,
                segment: seg,
            }
            .render()?,
        );
        for d in &seg.days {
            fragments.push(
                DailyPage {
                    day: d.day,
                    day_pad: format!("{:02}", d.day),
                    month_num: month,
                    weekday: d.weekday,
                    event_count: count_for(d.day),
                }
                .render()?,
            );
            for _ in 1..config.pages_per_day {
                fragments.push(DotGrid.render()?);
            }
        }
        fragments.push(
            crate::templates::WeeklyRetro {
                month_num: month,
                segment: seg,
            }
            .render()?,
        );
    }
```

> The agenda/event-pages block below this (the `let days = agenda::agenda_days(…)` section) stays exactly as-is. `count_for`, `m`, `day_rows`, `grid`, `dev`, `config` are all already in scope.

- [ ] **Step 2: Update the page-count tests** — in `tests/month.rs`, replace the four affected assertions to derive the weekly-page count from `segments`. Add a helper and update each test:

```rust
fn weekly_pages(year: i32, month: u32, week_start: &str) -> usize {
    let m = rmbujo::calendar::build_month(year, month, week_start).unwrap();
    rmbujo::calendar::segments(&m).len() * 2
}
```

Update `month_page_count_static` (May 2027, default `sun`, pages_per_day 1):

```rust
    let weekly = weekly_pages(2027, 5, "sun");
    assert_eq!(doc.get_pages().len(), 2 + 31 + weekly);
```

Update `month_pages_per_day_multiplies_daily` (Feb 2027, pages_per_day 2):

```rust
    let weekly = weekly_pages(2027, 2, "sun");
    assert_eq!(doc.get_pages().len(), 2 + 28 * 2 + weekly);
```

Update `events_only_add_trailing_pages` — `base` now includes weekly pages, and one event still adds exactly one trailing page:

```rust
    let weekly = weekly_pages(2027, 5, "sun");
    let base = lopdf::Document::load(&out_a).unwrap().get_pages().len();
    assert_eq!(base, 2 + 31 + weekly);
    // … (event insertion unchanged) …
    assert_eq!(withev, base + 1, "a small one-event day is a single combined agenda+details page");
```

Update `busy_month_paginates_per_day_pages` — compute the static base and assert events page beyond one extra page:

```rust
    let weekly = weekly_pages(2027, 1, "sun");
    let base = 2 + 31 + weekly;
    assert!(pages > base + 1,
        "busy month should paginate per-day event pages beyond one page (got {pages}, base {base})");
```

- [ ] **Step 3: Run the month tests** — Run: `cargo test -p rmbujo --test month` → Expected: PASS. (The `Config::new(2027)` default `week_start` is `"sun"`, matching the helper.)

- [ ] **Step 4: Run the whole crate suite** — Run: `cargo test -p rmbujo` → Expected: all green (calendar, render, month, visual, generate, notebooks, layout, etc.). If `tests/generate.rs` or `tests/notebooks.rs` assert exact month page counts, update them the same way (derive via `weekly_pages`).

- [ ] **Step 5: Commit**

```bash
git add src/notebooks/month/mod.rs tests/month.rs
git commit -m "feat(rmbujo): interleave weekly Plan/Retro pages into month notebook"
```

---

## Self-Review

**Spec coverage:**
- Per-month segments + split-week behaviour → Task 1 (`segments`) + Task 5 (assembly). ✓
- Monday start via existing config → no code change; `segments` reads `week_start` flags (Task 1). ✓
- Zero-height month-index divider on dot-row boundary → Task 3 (`monthly_row_center(sp,i) - sp/2`, `#place`). ✓
- ◎|↺ tab with intra-PDF links → Task 2 (`wtab`, icons) + Task 3 (placement). ✓
- Plan page (minimal underlined headings, Intentions, Tasks, linked day rows, anchors) → Task 4. ✓
- Retro page (free write, header links) → Task 4. ✓
- Interleaved page order, agenda last → Task 5. ✓
- Typst-vector icons (not glyphs) → Task 2. ✓
- Full linking table (`monthly`, `day-{n}`, `wplan-{seg}`, `wretro-{seg}`) → Tasks 2/3/4 produce, Task 4 markup test asserts. ✓
- Testing: calendar unit (T1), preamble compile (T2), markup (T4), goldens (T3/T4), page counts (T5). ✓

**Placeholder scan:** No TBD/placeholder steps; every code step shows complete code. ✓

**Type consistency:** `Segment` (field `days`, methods `first_day`/`last_day`/`id`/`date_range`) defined in T1 and used identically in T4/T5. `WeeklyPlan`/`WeeklyRetro { month_num, segment: &Segment }` defined in T4 and constructed with the same fields in T4 tests and T5. Preamble helpers `plan-icon`/`retro-icon`/`wtab` defined in T2, called in T2 test, T3, T4. Label scheme `wplan-{seg}`/`wretro-{seg}` consistent across T2/T3/T4. ✓

**Out of scope (unchanged):** Future Log, Collection, Reference, agenda emitters; no new config keys; deploying `week_start="mon"` is a dotfiles change outside this repo.
