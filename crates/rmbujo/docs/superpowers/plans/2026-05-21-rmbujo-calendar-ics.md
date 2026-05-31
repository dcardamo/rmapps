# rmbujo Phase 2b — Calendar + ICS Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers-extended-cc:subagent-driven-development (recommended) or superpowers-extended-cc:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the month notebook into a navigable calendar: static writing pages (monthly + tasks + per-day daily) up front, an ICS-driven agenda + details section appended at the end, wired with tappable internal links and a per-day event-count badge.

**Architecture:** Static leading section (event-independent, fixed page count) you write on; volatile agenda/details appended at the end (safe to grow under `rmapi put --content-only`, proven in the spikes). ICS feeds are fetched into a per-year cache and parsed (all-day + timed, RRULE + multi-day, converted to a configured timezone) into a per-day event map. A toolbar safe-area clears the reMarkable pen toolbar, and the dot pitch matches the built-in "Dots Small" template (4.756 mm) so user-inserted pages line up.

**Tech Stack:** Rust, fulgur (HTML/CSS→PDF, internal links via `<a href="#id">`), askama templates, `ureq`+`ical`+`rrule`+`chrono-tz`+`iana-time-zone`, Nix.

**Spec:** `docs/superpowers/specs/2026-05-21-rmbujo-calendar-ics-design.md`
**Validated prototype (HTML/navigation reference):** `examples/spike_month_prototype.rs`
**Device mechanics:** `docs/remarkable-pdf-mechanics.md`

---

## File structure

| File | Responsibility |
|------|----------------|
| `src/geometry.rs` (mod) | `DEFAULT_SPACING_MM` → 4.756; `TOOLBAR_SAFE_PT`; `monthly_row_pt()` fit-to-height |
| `src/render.rs` (mod) | CSS: top safe-area on `.page` (covers exempt); badge pill; agenda/detail/link styles; monthly `--row` var |
| `src/config.rs` (mod) | add `pages_per_day` (default 1), `timezone` (default detected); validate |
| `src/calendar.rs` (mod) | `pub fn days_in_month`; weekday helpers already present |
| `src/ics/{mod,fetch,parse}.rs` (new) | feeds+year+tz → `BTreeMap<NaiveDate, Vec<EventOccurrence>>` |
| `src/notebooks/month/{mod,agenda}.rs` (new, replaces `month.rs`) | assemble monthly+tasks+daily+agenda+details |
| `templates/{monthly_view,daily_page,agenda,details}.html` (new) | calendar page templates |
| `src/{generate,cli,wizard}.rs` (mod) | wire event map; `--refresh-feeds`; wizard prompts |
| `Cargo.toml`, `flake.nix` | ICS crates |
| `tests/*`, `tests/goldens/*` | parse/fetch/structure/link tests; regenerated goldens |
| `README.md` | cloud-sync rule, ICS+timezone, Dots-Small inserted pages |

---

## Task 1: Toolbar safe-area + Dots-Small spacing + regenerate goldens

**Goal:** All non-cover pages start below the toolbar; default dot pitch matches "Dots Small" (4.756 mm). Existing notebooks render correctly; goldens regenerated.

**Files:**
- Modify: `src/geometry.rs`, `src/render.rs`
- Regenerate: `tests/goldens/*.png`

**Acceptance Criteria:**
- [ ] `DEFAULT_SPACING_MM == 4.756`; `geometry::TOOLBAR_SAFE_PT == 36.0`
- [ ] `.page` reserves `TOOLBAR_SAFE_PT` at the top (content below the toolbar); `.cover` unaffected (it is `position:absolute; inset:0`)
- [ ] dot-grid backgrounds start at the reserve line, not the page top
- [ ] `make test` green after `make update-goldens`

**Verify:** `nix develop -c env RMBUJO_UPDATE_GOLDENS=1 cargo test --test visual` then `nix develop -c cargo test`

**Steps:**

- [ ] **Step 1: geometry constants.** In `src/geometry.rs` set `pub const DEFAULT_SPACING_MM: f32 = 4.756;` (update the doc comment: "matches reMarkable's built-in 'Dots Small' template (4.756 mm = 42.5 device-units), so user-inserted 'Dots Small' pages line up"). Add:

```rust
/// Vertical space (pt) reserved at the top of every non-cover page so content
/// clears the reMarkable pen toolbar. Measured on the Paper Pro Move.
pub const TOOLBAR_SAFE_PT: f32 = 36.0;
```

- [ ] **Step 2: CSS reserve.** In `src/render.rs` `build_css`, add `crate::geometry::TOOLBAR_SAFE_PT` as `top` and change `.page` padding from uniform `{m}pt` to top-reserve:

```rust
let top = crate::geometry::TOOLBAR_SAFE_PT;
// in the format! string, replace the `.page { ... padding: {m}pt; ... }` rule with:
".page {{ position: relative; width: {w}pt; height: {h}pt; padding: {top}pt {m}pt {m}pt {m}pt; overflow: hidden; background: #fff; break-after: page; }}\n\"
```

Shift the dot-grid backgrounds down by the reserve so dots begin below the toolbar: change every `background-position: {m}pt {m}pt` (the `.dotgrid` and `.dotpage` rules) to `background-position: {m}pt {top}pt`, and the `.month-list` `background-position: 0pt {half_sp}pt` to `0pt calc({top}pt + {half_sp}pt)`. Add `top = top` to the `format!` args. `.cover` is `position:absolute; inset:0` and is unaffected — covers stay full-bleed (verify in Step 4).

- [ ] **Step 3: build + unit tests.** Run `nix develop -c cargo test --lib --test render --test layout` — these don't compare pixels; confirm they pass (page sizes/counts unchanged; only layout shifted).

- [ ] **Step 4: regenerate + inspect goldens.** Run `nix develop -c env RMBUJO_UPDATE_GOLDENS=1 cargo test --test visual`. Then rasterize one page and confirm visually the header sits ~36pt down and the cover is still full-bleed:

```bash
nix develop -c sh -c 'cargo run -- new </dev/null 2>/dev/null; true'   # not needed; instead inspect a golden:
nix develop -c sh -c 'ls tests/goldens'
```
Open `tests/goldens/month_index.png` (or equivalent) and confirm content starts below the reserve; `tests/goldens/cover.png` is unchanged full-bleed.

- [ ] **Step 5: full suite + commit.**

```bash
nix develop -c cargo test
nix develop -c cargo fmt && nix develop -c cargo clippy --all-targets -- -D warnings
git add src/geometry.rs src/render.rs tests/goldens
git commit -m "Toolbar safe-area on all pages; default dot pitch 4.756mm (Dots Small); regen goldens"
```

```json:metadata
{"files":["src/geometry.rs","src/render.rs","tests/goldens"],"verifyCommand":"nix develop -c cargo test","acceptanceCriteria":["spacing 4.756 + TOOLBAR_SAFE_PT 36","content below toolbar, cover exempt","dot bg shifted","goldens regenerated, suite green"]}
```

---

## Task 2: Config — `pages_per_day` + `timezone`

**Goal:** Config gains `pages_per_day` (default 1) and `timezone` (IANA, default detected system zone), both validated.

**Files:**
- Modify: `Cargo.toml` (add `chrono-tz`, `iana-time-zone`), `src/config.rs`, `src/wizard.rs` (Answers + assemble), `tests/config.rs`
- Modify: `flake.nix` only if the build needs it (pure-Rust crates; likely no change — confirm `nix develop -c cargo build` succeeds)

**Acceptance Criteria:**
- [ ] `Config.pages_per_day: u32` (default 1) and `Config.timezone: String` (default = detected zone, else `"UTC"`)
- [ ] `validate()` rejects `pages_per_day == 0` and a `timezone` that doesn't parse as `chrono_tz::Tz`
- [ ] round-trips through TOML; `Config::new` and wizard `assemble` set both

**Verify:** `nix develop -c cargo test --test config`

**Steps:**

- [ ] **Step 1: deps.** `Cargo.toml` `[dependencies]`: add `chrono-tz = "0.10"` and `iana-time-zone = "0.1"`. Run `nix develop -c cargo build` to confirm they compile under Nix (pure Rust; no flake change expected).

- [ ] **Step 2: failing tests** in `tests/config.rs`:

```rust
#[test]
fn pages_per_day_and_timezone_defaults_and_validate() {
    let c = Config::new(2026);
    assert_eq!(c.pages_per_day, 1);
    assert!(!c.timezone.is_empty());
    assert!(c.validate().is_ok());
    assert!(Config { pages_per_day: 0, ..Config::new(2026) }.validate().is_err());
    assert!(Config { timezone: "Not/AZone".into(), ..Config::new(2026) }.validate().is_err());
}
```

- [ ] **Step 3: config fields.** In `src/config.rs` `struct Config` add:

```rust
    #[serde(default = "default_pages_per_day")]
    pub pages_per_day: u32,
    #[serde(default = "default_timezone")]
    pub timezone: String,
```
and:
```rust
fn default_pages_per_day() -> u32 { 1 }
fn default_timezone() -> String {
    iana_time_zone::get_timezone().unwrap_or_else(|_| "UTC".into())
}
```
Set both in `Config::new` (use `default_pages_per_day()` / `default_timezone()`). In `validate()` add:
```rust
        if self.pages_per_day == 0 {
            anyhow::bail!("pages_per_day must be >= 1");
        }
        if self.timezone.parse::<chrono_tz::Tz>().is_err() {
            anyhow::bail!("unknown timezone: {:?}", self.timezone);
        }
```

- [ ] **Step 4: wizard plumbing (no prompt yet).** In `src/wizard.rs` add `pub pages_per_day: u32` and `pub timezone: String` to `Answers`, and set them in `assemble`'s `Config { .. }`. (Prompts added in Task 10.) Update any `Answers { .. }` literals in `tests/cli.rs` to include the two fields (`pages_per_day: 1, timezone: "America/Toronto".into()`).

- [ ] **Step 5: green + commit.**

```bash
nix develop -c cargo test && nix develop -c cargo fmt && nix develop -c cargo clippy --all-targets -- -D warnings
git add Cargo.toml Cargo.lock src/config.rs src/wizard.rs tests/config.rs tests/cli.rs
git commit -m "Config: pages_per_day (default 1) + timezone (default detected), validated"
```

```json:metadata
{"files":["Cargo.toml","src/config.rs","src/wizard.rs","tests/config.rs"],"verifyCommand":"nix develop -c cargo test --test config","acceptanceCriteria":["pages_per_day+timezone fields/defaults","validate rejects 0 and bad tz","round-trips"]}
```

---

## Task 3: Monthly fit-to-height row geometry

**Goal:** A pure helper computing the monthly view's per-row height so all days fit under the toolbar reserve, plus a public `days_in_month`.

**Files:**
- Modify: `src/calendar.rs` (expose `days_in_month`), `src/geometry.rs` (add `monthly_row_pt`)
- Test: `tests/geometry.rs`, `tests/calendar.rs`

**Acceptance Criteria:**
- [ ] `pub fn calendar::days_in_month(year: i32, month: u32) -> u32` (28–31; Feb leap correct)
- [ ] `geometry::monthly_row_pt(device, reserve_pt, header_pt, bottom_margin_pt, num_days) -> f32` returns `(usable_height)/num_days`, never exceeding the configured dot pitch
- [ ] for the Move at 31 days the result is > 0 and `31 * row + header + reserve + margin <= page_height`

**Verify:** `nix develop -c cargo test --test geometry --test calendar`

**Steps:**

- [ ] **Step 1: expose days_in_month.** In `src/calendar.rs` change `fn days_in_month` to `pub fn days_in_month`. Test in `tests/calendar.rs`:
```rust
#[test]
fn days_in_month_counts() {
    assert_eq!(rmbujo::calendar::days_in_month(2026, 2), 28);
    assert_eq!(rmbujo::calendar::days_in_month(2024, 2), 29);
    assert_eq!(rmbujo::calendar::days_in_month(2026, 5), 31);
}
```

- [ ] **Step 2: failing geometry test** in `tests/geometry.rs`:
```rust
#[test]
fn monthly_row_fits_under_reserve() {
    let dev = rmbujo::device::get_device("paper-pro-move").unwrap();
    let grid = rmbujo::geometry::default_grid(&dev);
    let header = 22.0;
    let row = rmbujo::geometry::monthly_row_pt(
        &dev, rmbujo::geometry::TOOLBAR_SAFE_PT, header, grid.margin_pt, 31);
    assert!(row > 0.0 && row <= grid.spacing_pt + 0.001);
    assert!(31.0 * row + header + rmbujo::geometry::TOOLBAR_SAFE_PT + grid.margin_pt
            <= dev.height_pt() + 0.001);
}
```

- [ ] **Step 3: implement** in `src/geometry.rs`:
```rust
/// Row height (pt) for the monthly day-list so `num_days` rows fit between the
/// toolbar reserve and the bottom margin, never exceeding the dot pitch.
pub fn monthly_row_pt(
    device: &Device,
    reserve_pt: f32,
    header_pt: f32,
    bottom_margin_pt: f32,
    num_days: u32,
) -> f32 {
    let usable = device.height_pt() - reserve_pt - header_pt - bottom_margin_pt;
    let fit = usable / num_days as f32;
    fit.min(default_grid(device).spacing_pt)
}
```

- [ ] **Step 4: green + commit.**
```bash
nix develop -c cargo test --test geometry --test calendar
nix develop -c cargo fmt && nix develop -c cargo clippy --all-targets -- -D warnings
git add src/calendar.rs src/geometry.rs tests/geometry.rs tests/calendar.rs
git commit -m "Add days_in_month + monthly fit-to-height row geometry"
```

```json:metadata
{"files":["src/calendar.rs","src/geometry.rs","tests/geometry.rs","tests/calendar.rs"],"verifyCommand":"nix develop -c cargo test --test geometry --test calendar","acceptanceCriteria":["days_in_month pub+correct","monthly_row_pt fits + capped at pitch"]}
```

---

## Task 4: Monthly view template + builder (static; day→daily links)

**Goal:** New monthly view: a row per day with a fixed-width `day# weekday` link to that day's log page, sized via `monthly_row_pt`. No badges yet (badges arrive with events in Task 9).

**Files:**
- Create: `templates/monthly_view.html`, `src/notebooks/month/mod.rs` (start; replaces `src/notebooks/month.rs`)
- Modify: `src/templates.rs` (add `MonthlyView` struct + a `DayRow` view), `src/notebooks/mod.rs` (`pub mod month;` already points at `month.rs`; switch to the dir module)
- Test: `tests/templates_html.rs` (or `tests/month.rs` new)

**Acceptance Criteria:**
- [ ] monthly HTML has one row per day; each row links `href="#day-N"`; the page section has `id="monthly"`
- [ ] `.day` rows use the fit-to-height row size via a `--row` CSS var on the section
- [ ] renders without fulgur "unresolved internal anchor" warnings once daily pages exist (verified in Task 5)

**Verify:** `nix develop -c cargo test --test templates_html`

**Steps:**

- [ ] **Step 1: convert `month.rs` to a dir module.** `git mv src/notebooks/month.rs src/notebooks/month/mod.rs`. Keep `pub mod month;` in `src/notebooks/mod.rs`.

- [ ] **Step 2: template struct.** In `src/templates.rs` add:
```rust
#[derive(Clone, Debug)]
pub struct DayRow {
    pub day: u32,
    pub weekday: &'static str,
    pub week_start: bool,
    pub event_count: usize, // 0 in the static phase; populated from ICS later
}

#[derive(Template)]
#[template(path = "monthly_view.html")]
pub struct MonthlyView<'a> {
    pub month_name: &'a str,
    pub year: i32,
    pub month_num: u32,
    pub row_pt: f32,
    pub days: &'a [DayRow],
}
```

- [ ] **Step 3: template** `templates/monthly_view.html` — port the monthly row markup from `examples/spike_month_prototype.rs` (the `--- Monthly view ---` block: fixed-width date link `#day-{day}`, and the count badge only when `event_count > 0`). Use the `--row` var for row height:
```html
<section class="page month-index" id="monthly" style="--row:{{ row_pt }}pt;">
  <div class="h-month">{{ month_name }} {{ year }}</div>
  <div class="month-list">
    {% for d in days %}
    <div class="day{% if d.week_start %} weekstart{% endif %}">
      <a class="daylink" href="#day-{{ d.day }}"><span class="num">{{ d.day }}</span><span class="wd">{{ d.weekday }}</span></a>
      {% if d.event_count > 0 %}<a class="cbadge" href="#agenda-{{ d.day }}">{{ d.event_count }}</a>{% endif %}
    </div>
    {% endfor %}
  </div>
</section>
```

- [ ] **Step 4: CSS for the new classes** in `src/render.rs` `build_css` (port pill sizing from the prototype v5 badge, taller box to avoid the clip):
```rust
".month-index .day {{ height: var(--row); }}\n\
.month-list {{ background-size: {sp}pt var(--row); }}\n\
.daylink {{ text-decoration:none; color:inherit; display:inline-flex; gap:6pt; align-items:center; width:44pt; }}\n\
.cbadge {{ display:inline-flex; align-items:center; justify-content:center; min-width:13pt; height:12pt; padding:0 4pt; border-radius:6pt; background:var(--navy); color:#fff; font-size:8pt; font-weight:bold; line-height:1; text-decoration:none; }}\n\"
```
(`height:12pt` gives the digit room — fixes the prototype clip.) The existing `.month-list` background-size line is overridden by this var-based one; keep a single definition.

- [ ] **Step 5: builder** in `src/notebooks/month/mod.rs` `build_month_pdf` — build `Vec<DayRow>` (event_count 0 for now), compute `row_pt = geometry::monthly_row_pt(&dev, TOOLBAR_SAFE_PT, header≈1.25*spacing+0.5*spacing, grid.margin_pt, days)`, render `MonthlyView`. (Full assembly in Task 5.)

- [ ] **Step 6: HTML test** in `tests/templates_html.rs`:
```rust
#[test]
fn monthly_view_rows_and_links() {
    use rmbujo::templates::{DayRow, MonthlyView};
    use askama::Template;
    let days: Vec<DayRow> = (1..=31).map(|day| DayRow { day, weekday: "Mon", week_start: false, event_count: 0 }).collect();
    let html = MonthlyView { month_name: "May", year: 2027, month_num: 5, row_pt: 12.0, days: &days }.render().unwrap();
    assert_eq!(html.matches("href=\"#day-").count(), 31);
    assert!(html.contains("id=\"monthly\""));
    assert!(!html.contains("cbadge")); // no badges when event_count == 0
}
```

- [ ] **Step 7: commit.**
```bash
nix develop -c cargo test && nix develop -c cargo fmt && nix develop -c cargo clippy --all-targets -- -D warnings
git add -A && git commit -m "Monthly view: per-day rows linking to daily pages (fit-to-height, badge-ready)"
```

```json:metadata
{"files":["templates/monthly_view.html","src/templates.rs","src/notebooks/month/mod.rs","src/render.rs","tests/templates_html.rs"],"verifyCommand":"nix develop -c cargo test --test templates_html","acceptanceCriteria":["row per day, #day-N links, id=monthly","--row sizing","no badge at count 0"]}
```

---

## Task 5: Daily pages + Tasks + month assembly (static)

**Goal:** Per-day daily-log pages (dated header linking to monthly), the Tasks page, assembled into the month PDF: monthly + tasks + `days × pages_per_day` daily pages. month↔day navigation works end-to-end.

**Files:**
- Create: `templates/daily_page.html`; Modify: `src/templates.rs` (`DailyPage` struct), `src/notebooks/month/mod.rs`
- Test: `tests/month.rs` (new) + reuse `tests/templates_html.rs`

**Acceptance Criteria:**
- [ ] month PDF page count == `2 + days_in_month × pages_per_day`
- [ ] daily page `id="day-N"`, header `M.DD Dow` links `href="#monthly"`
- [ ] full month PDF renders with **no unresolved-anchor warnings** (month↔day links resolve)

**Verify:** `nix develop -c cargo test --test month`

**Steps:**

- [ ] **Step 1: DailyPage template** `templates/daily_page.html` (port the per-day header from the prototype `--- Per-day log pages ---`, minus the badge for now):
```html
<section class="page dotpage" id="day-{{ day }}">
  <div class="dayhead">
    <a href="#monthly" class="h-month dayhead-date">{{ month_num }}.{{ day_pad }} {{ weekday }}</a>
    {% if event_count > 0 %}<a class="cbadge" href="#agenda-{{ day }}">{{ event_count }}</a>{% endif %}
  </div>
</section>
```
`src/templates.rs`:
```rust
#[derive(Template)]
#[template(path = "daily_page.html")]
pub struct DailyPage<'a> {
    pub day: u32,
    pub day_pad: String,   // "{:02}"
    pub month_num: u32,
    pub weekday: &'a str,
    pub event_count: usize,
}
```
CSS in `build_css`: `.dayhead {{ display:flex; justify-content:space-between; align-items:center; }} .dayhead-date {{ font-size:13pt; text-decoration:none; color:var(--navy); }}`.

- [ ] **Step 2: assemble** `src/notebooks/month/mod.rs` `build_month_pdf`: push `MonthlyView`, then `Tasks`, then for each day push `DailyPage` (repeated `pages_per_day` times; only the first per day carries the `id="day-N"` + header — extra pages are plain `DotGrid`). Compute `event_count = 0` for all (ICS in Task 9).

- [ ] **Step 3: structure test** `tests/month.rs`:
```rust
use rmbujo::config::Config;
use rmbujo::notebooks::month::build_month_pdf;
use lopdf::Document;

fn tmp() -> std::path::PathBuf { /* same nanos helper as other tests */ }

#[test]
fn month_page_count_is_static() {
    let cfg = Config { pages_per_day: 1, ..Config::new(2027) };
    let out = tmp();
    build_month_pdf(&cfg, 5, &out).unwrap();
    let doc = Document::load(&out).unwrap();
    assert_eq!(doc.get_pages().len(), 2 + 31); // monthly + tasks + 31 daily
}
```
(Write the `tmp()` nanos helper as in `tests/cli.rs`.)

- [ ] **Step 4: link sanity** — render and assert no unresolved anchors. Capture stderr:
```rust
#[test]
fn month_links_resolve() {
    let cfg = Config::new(2027);
    let out = tmp();
    build_month_pdf(&cfg, 5, &out).unwrap();
    let data = std::fs::read(&out).unwrap();
    assert!(data.windows(6).any(|w| w == b"/Annot")); // links present
}
```
(Manual stderr check during dev: run a one-off and confirm fulgur prints no "unresolved internal anchor".)

- [ ] **Step 5: commit.**
```bash
nix develop -c cargo test && nix develop -c cargo fmt && nix develop -c cargo clippy --all-targets -- -D warnings
git add -A && git commit -m "Per-day daily pages + month assembly; month<->day navigation"
```

```json:metadata
{"files":["templates/daily_page.html","src/templates.rs","src/notebooks/month/mod.rs","tests/month.rs"],"verifyCommand":"nix develop -c cargo test --test month","acceptanceCriteria":["page count 2+days*ppd","daily id+header->monthly","links resolve"]}
```

---

## Task 6: ICS parse (all-day + timed, RRULE, multi-day, timezone)

**Goal:** `src/ics/parse.rs` turns raw `.ics` bytes + a feed color + the config year + timezone into `Vec<EventOccurrence>` (expanded, year-clipped, tz-converted).

**Files:**
- Create: `src/ics/mod.rs` (declares `EventOccurrence` + `pub mod parse; pub mod fetch;`), `src/ics/parse.rs`, `tests/ics_parse.rs`, fixtures `tests/fixtures/*.ics`
- Modify: `Cargo.toml` (add `ical`, `rrule`), `src/lib.rs` (`pub mod ics;`)

**Acceptance Criteria:**
- [ ] `EventOccurrence { date: NaiveDate, time: Option<NaiveTime>, title, location, description, attendees: Vec<String>, color }`
- [ ] dated all-day, multi-day (DTEND exclusive), and yearly-RRULE events expand into per-day occurrences within the year; out-of-year excluded
- [ ] a `TZID`/UTC **timed** event is converted to the config tz (correct `HH:MM` and calendar day, incl. a midnight day-shift)

**Verify:** `nix develop -c cargo test --test ics_parse`

**Steps:**

- [ ] **Step 1: deps + module.** `Cargo.toml`: `ical = "0.11"`, `rrule = "0.13"`. `src/lib.rs`: add `pub mod ics;`. `src/ics/mod.rs`:
```rust
pub mod fetch;
pub mod parse;

use chrono::{NaiveDate, NaiveTime};

#[derive(Debug, Clone, PartialEq)]
pub struct EventOccurrence {
    pub date: NaiveDate,
    pub time: Option<NaiveTime>, // None = all-day; Some = timed (config tz)
    pub title: String,
    pub location: Option<String>,
    pub description: Option<String>,
    pub attendees: Vec<String>,
    pub color: String, // theme color name
}
```

- [ ] **Step 2: fixtures.** Create `tests/fixtures/holidays.ics` (a dated all-day VEVENT, a multi-day all-day VEVENT with exclusive DTEND, a yearly-RRULE birthday with DTSTART years earlier, and an out-of-year VEVENT) and `tests/fixtures/timed.ics` (a `TZID=America/Toronto` timed event and a UTC `...T030000Z` event that is the previous local day in Toronto). Use minimal valid iCalendar (VCALENDAR/VEVENT, DTSTART/DTEND/SUMMARY/RRULE/LOCATION/DESCRIPTION/ATTENDEE).

- [ ] **Step 3: failing tests** `tests/ics_parse.rs`:
```rust
use chrono::{NaiveDate, NaiveTime};
use rmbujo::ics::parse::parse_feed;

fn parse(file: &str, tz: &str) -> Vec<rmbujo::ics::EventOccurrence> {
    let bytes = std::fs::read(format!("tests/fixtures/{file}")).unwrap();
    parse_feed(&bytes, "brick", 2027, &tz.parse().unwrap()).unwrap()
}

#[test]
fn all_day_dated_multiday_rrule() {
    let evs = parse("holidays.ics", "America/Toronto");
    let on = |y, m, d| evs.iter().filter(|e| e.date == NaiveDate::from_ymd_opt(y, m, d).unwrap()).count();
    assert!(evs.iter().all(|e| e.time.is_none()));
    assert_eq!(on(2027, 5, 24) + on(2027, 5, 25) + on(2027, 5, 26), 3); // 3-day span, DTEND exclusive
    assert!(evs.iter().any(|e| e.title.contains("birthday"))); // RRULE expanded into 2027
    assert!(evs.iter().all(|e| e.date.year() == 2027)); // year-clipped
}

#[test]
fn timed_events_convert_to_config_tz() {
    let evs = parse("timed.ics", "America/Toronto");
    assert!(evs.iter().any(|e| e.time == Some(NaiveTime::from_hms_opt(14, 0, 0).unwrap())));
    // the UTC 03:00Z event lands on the previous local day, not its UTC date
    assert!(evs.iter().any(|e| e.time.is_some() && e.date.day() != /* its UTC day */ 0));
}
```
(Adjust the day-shift assertion to your fixture's exact dates.)

- [ ] **Step 4: implement** `src/ics/parse.rs` `pub fn parse_feed(bytes: &[u8], color: &str, year: i32, tz: &chrono_tz::Tz) -> anyhow::Result<Vec<EventOccurrence>>`: parse VEVENTs with `ical`; for each, read DTSTART (date vs datetime + TZID/UTC/floating), DTEND, RRULE, SUMMARY/LOCATION/DESCRIPTION/ATTENDEE. All-day: iterate dates `DTSTART..DTEND` (exclusive). Timed: build a tz-aware `DateTime`, convert to `tz`, take local date+time. RRULE: build an `rrule::RRuleSet` from DTSTART + RRULE, generate occurrences within the year, convert each to `tz`. Clip every occurrence to `year`. Sort: date, all-day-before-timed, time, title. (See spec §ICS; validate the exact `ical`/`rrule` APIs against the fixtures — the parse spike.)

- [ ] **Step 5: green + commit.**
```bash
nix develop -c cargo test --test ics_parse
nix develop -c cargo fmt && nix develop -c cargo clippy --all-targets -- -D warnings
git add -A && git commit -m "ICS parse: all-day/timed, RRULE, multi-day, timezone conversion"
```

```json:metadata
{"files":["src/ics/mod.rs","src/ics/parse.rs","tests/ics_parse.rs","tests/fixtures"],"verifyCommand":"nix develop -c cargo test --test ics_parse","acceptanceCriteria":["EventOccurrence","all-day/multiday/RRULE/year-clip","timed tz convert + day-shift"]}
```

---

## Task 7: ICS fetch + cache (`--refresh-feeds`)

**Goal:** `src/ics/fetch.rs`: a `Fetcher` trait (HTTP impl via `ureq`) plus cache read/write beside the toml, with refresh semantics. Hermetically tested via a fake fetcher.

**Files:**
- Create: `src/ics/fetch.rs`, `tests/ics_fetch.rs`; Modify: `Cargo.toml` (`ureq`)

**Acceptance Criteria:**
- [ ] `Fetcher` trait (`fn get(&self, url: &str) -> anyhow::Result<Vec<u8>>`); `UreqFetcher` impl
- [ ] `feed_bytes(out_dir, feed, refresh, fetcher) -> Vec<u8>`: reads `<out_dir>/.ics-cache/<slug>.ics` unless `refresh` (or cache missing), else fetches + writes cache
- [ ] on fetch failure with an existing cache: returns the cached bytes (warns); with no cache: returns `Err`
- [ ] cache slug is filesystem-safe (derived from feed name)

**Verify:** `nix develop -c cargo test --test ics_fetch`

**Steps:**

- [ ] **Step 1: dep.** `Cargo.toml`: `ureq = { version = "2", features = ["tls"] }` (rustls; pure-Rust TLS). Confirm `nix develop -c cargo build`.

- [ ] **Step 2: failing tests** `tests/ics_fetch.rs` with a fake fetcher (records calls, returns canned bytes or errors), covering: cold cache fetches + writes; warm cache (no refresh) does not fetch; `refresh=true` re-fetches; fetch error + existing cache → cached bytes; fetch error + no cache → Err. Use a temp dir.

- [ ] **Step 3: implement** `src/ics/fetch.rs`:
```rust
use std::path::Path;
use crate::config::IcsFeed;

pub trait Fetcher {
    fn get(&self, url: &str) -> anyhow::Result<Vec<u8>>;
}

pub struct UreqFetcher;
impl Fetcher for UreqFetcher {
    fn get(&self, url: &str) -> anyhow::Result<Vec<u8>> {
        let mut buf = Vec::new();
        ureq::get(url).call()?.into_reader().read_to_end(&mut buf)?;
        Ok(buf)
    }
}

fn slug(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect()
}

pub fn feed_bytes(out_dir: &Path, feed: &IcsFeed, refresh: bool, f: &dyn Fetcher) -> anyhow::Result<Vec<u8>> {
    let cache = out_dir.join(".ics-cache").join(format!("{}.ics", slug(&feed.name)));
    if !refresh && cache.exists() {
        return Ok(std::fs::read(&cache)?);
    }
    match f.get(&feed.url) {
        Ok(bytes) => {
            std::fs::create_dir_all(cache.parent().unwrap())?;
            std::fs::write(&cache, &bytes)?;
            Ok(bytes)
        }
        Err(e) => {
            if cache.exists() {
                eprintln!("rmbujo: feed {:?} fetch failed ({e}); using cached copy", feed.name);
                Ok(std::fs::read(&cache)?)
            } else {
                Err(e)
            }
        }
    }
}
```
(Add `use std::io::Read;` for `read_to_end`.)

- [ ] **Step 4: green + commit.**
```bash
nix develop -c cargo test --test ics_fetch
nix develop -c cargo fmt && nix develop -c cargo clippy --all-targets -- -D warnings
git add -A && git commit -m "ICS fetch + cache with refresh semantics (Fetcher seam)"
```

```json:metadata
{"files":["src/ics/fetch.rs","tests/ics_fetch.rs","Cargo.toml"],"verifyCommand":"nix develop -c cargo test --test ics_fetch","acceptanceCriteria":["Fetcher trait+ureq","cache read/write/refresh","fail keeps cache / errs without"]}
```

---

## Task 8: ICS orchestration (year event map)

**Goal:** `src/ics/mod.rs` ties feeds + year + tz + cache into `BTreeMap<NaiveDate, Vec<EventOccurrence>>`, deterministically ordered.

**Files:**
- Modify: `src/ics/mod.rs`; Test: `tests/ics_mod.rs`

**Acceptance Criteria:**
- [ ] `build_event_map(config, out_dir, refresh, fetcher) -> anyhow::Result<BTreeMap<NaiveDate, Vec<EventOccurrence>>>`
- [ ] merges all feeds; per-day list sorted (all-day-first, then time, then title); a feed that fails with no cache is skipped with a warning (doesn't abort)

**Verify:** `nix develop -c cargo test --test ics_mod`

**Steps:**

- [ ] **Step 1: failing test** `tests/ics_mod.rs`: two feeds via a fake fetcher → assert the map buckets by date and orders all-day before timed; a failing feed (no cache) is skipped, others still present.

- [ ] **Step 2: implement** `build_event_map` in `src/ics/mod.rs`: for each `config.ics` feed, `fetch::feed_bytes` → `parse::parse_feed(bytes, &feed.color, config.year, &tz)`; collect into the map; sort each day's vec. Parse `config.timezone` once. Skip (warn) a feed whose `feed_bytes` errors.

- [ ] **Step 3: green + commit.**
```bash
nix develop -c cargo test --test ics_mod
nix develop -c cargo fmt && nix develop -c cargo clippy --all-targets -- -D warnings
git add -A && git commit -m "ICS orchestration: per-year event map from cached feeds"
```

```json:metadata
{"files":["src/ics/mod.rs","tests/ics_mod.rs"],"verifyCommand":"nix develop -c cargo test --test ics_mod","acceptanceCriteria":["build_event_map merges+sorts","failing feed skipped not fatal"]}
```

---

## Task 9: Agenda + details rendering; badges; wire events through

**Goal:** Render the agenda + details pages from the event map, populate monthly/daily badges, and thread the map from `generate.rs` into the month builder.

**Files:**
- Create: `templates/agenda.html`, `templates/details.html`, `src/notebooks/month/agenda.rs`
- Modify: `src/templates.rs`, `src/notebooks/month/mod.rs`, `src/generate.rs`
- Test: `tests/month.rs` (extend), `tests/templates_html.rs`

**Acceptance Criteria:**
- [ ] agenda: date-ordered; each date header → `#day-N`; each event line has a feed-color swatch + `All Day`/`HH:MM` label + title (+ ` — location`) linking to `#evt-K`; title `Year Month` → `#monthly`
- [ ] details: date-organized; each event `id="evt-K"` shows label+title, Where/Notes/Attendees only when present
- [ ] monthly + daily badges show the per-day count and link to `#agenda-N`; days with 0 events show no badge
- [ ] a month with events renders with no unresolved anchors; leading-section page count unchanged by events

**Verify:** `nix develop -c cargo test --test month --test templates_html`

**Steps:**

- [ ] **Step 1: template structs + templates.** Add `Agenda`/`Details` structs to `src/templates.rs` over a shared `AgendaDay { day, weekday, events: Vec<AgendaEvent> }` and `AgendaEvent { idx, label, title, location, description, attendees, color, is_all_day }`. Create `templates/agenda.html` and `templates/details.html` porting markup from `examples/spike_month_prototype.rs` (`--- Agenda ---` and `--- Details ---`), adding the feed-color swatch (`<span class="swatch" style="background:var(--{{ color }})">` — map color name to theme var) before each agenda line. CSS in `build_css`: `.swatch {{ display:inline-block; width:7pt; height:7pt; border-radius:2pt; margin-right:4pt; vertical-align:-0.5pt; }}` plus agenda/detail text styles from the prototype.

- [ ] **Step 2: builders.** `src/notebooks/month/agenda.rs`: from the month's `&[ (NaiveDate, &[EventOccurrence]) ]` slice build `Vec<AgendaDay>` (stable `idx` per event for `#evt-K`), render `Agenda` and `Details`. In `month/mod.rs`, accept the month's events, set `DayRow.event_count`/`DailyPage.event_count`, and append the agenda + details sections after the daily pages.

- [ ] **Step 3: wire generate.** `src/generate.rs`: call `ics::build_event_map(config, out_dir, refresh, &UreqFetcher)` once; pass each month its date-slice to `build_month_pdf`. Thread a `refresh: bool` param through `generate_year` (CLI sets it in Task 10; default false).

- [ ] **Step 4: tests.** Extend `tests/templates_html.rs`: agenda has N `#evt-` links + date `#day-` links + swatches; details omit empty Where/Notes. Extend `tests/month.rs`: with a small injected event map, monthly badge count is correct and the **leading page count is unchanged** vs the no-events case (only trailing pages added).

- [ ] **Step 5: green + commit.**
```bash
nix develop -c cargo test && nix develop -c cargo fmt && nix develop -c cargo clippy --all-targets -- -D warnings
git add -A && git commit -m "Agenda + details from ICS; per-day badges; wire event map through generate"
```

```json:metadata
{"files":["templates/agenda.html","templates/details.html","src/notebooks/month/agenda.rs","src/notebooks/month/mod.rs","src/generate.rs","src/templates.rs"],"verifyCommand":"nix develop -c cargo test --test month --test templates_html","acceptanceCriteria":["agenda date-ordered+swatch+evt links","details omit-empty","badges count+link","leading page count unchanged by events"]}
```

---

## Task 10: CLI `--refresh-feeds`, wizard prompts, README

**Goal:** `--refresh-feeds` flag re-fetches; wizard prompts for `pages_per_day` + `timezone`; README documents ICS, timezone, the sync rule, and Dots-Small inserted pages.

**Files:**
- Modify: `src/cli.rs`, `src/wizard.rs`, `README.md`, `tests/cli.rs`

**Acceptance Criteria:**
- [ ] `rmbujo new` and `rmbujo <config>` accept `--refresh-feeds`; it sets `refresh=true` into `generate_year`
- [ ] wizard prompts for daily pages-per-day (default 1) and timezone (prefilled with detected zone)
- [ ] README has an ICS section (feeds, timezone, `--refresh-feeds`), the device sync rule, and the "insert pages with built-in Dots Small" note

**Verify:** `nix develop -c cargo test --test cli`

**Steps:**

- [ ] **Step 1: CLI flag.** In `src/cli.rs` add a `--refresh-feeds` bool to both the `new` and config-path arms (clap), pass it to `generate::generate_year(.., refresh)`. `new` defaults refresh to true (first upload always fetches); config-path uses the flag.

- [ ] **Step 2: wizard prompts.** In `src/wizard.rs` `run_wizard`, add `Input` prompts: "Daily pages per day" (default 1) and "Timezone" (default `default_timezone()`), thread into `Answers`.

- [ ] **Step 3: tests.** `tests/cli.rs`: `wizard_assemble` covers `pages_per_day`/`timezone`; a `run(["rmbujo", "<cfg>", "--refresh-feeds"])` parses without error (backend none → no network).

- [ ] **Step 4: README.** Add an "ICS calendar feeds" section: `[[ics]]` `name`/`url`/`color`, top-level `timezone`, `--refresh-feeds`, the cache location; the device **sync rule** (sync → run rmbujo → sync); and "to add pages on-device, insert a page and pick the built-in **Dots Small** template (matches rmbujo's grid)."

- [ ] **Step 5: green + commit.**
```bash
nix develop -c cargo test && nix develop -c cargo fmt && nix develop -c cargo clippy --all-targets -- -D warnings
git add -A && git commit -m "CLI --refresh-feeds; wizard pages_per_day+timezone; README ICS/timezone/Dots Small"
```

```json:metadata
{"files":["src/cli.rs","src/wizard.rs","README.md","tests/cli.rs"],"verifyCommand":"nix develop -c cargo test --test cli","acceptanceCriteria":["--refresh-feeds wired","wizard prompts","README ICS+timezone+sync+Dots Small"]}
```

---

## Task 11: Visual goldens for calendar pages + final green

**Goal:** Golden coverage for the new page types and a fully green gate.

**Files:**
- Modify: `tests/visual.rs`; Add: `tests/goldens/{monthly_view,daily_page,agenda,details}.png`

**Acceptance Criteria:**
- [ ] visual goldens exist for monthly-with-badges, daily, agenda, details and pass within tolerance
- [ ] `make test && make clippy && make fmt-check` all pass

**Verify:** `make test && make clippy && make fmt-check`

**Steps:**

- [ ] **Step 1: add fixtures to `tests/visual.rs`** for the four new page types (a monthly view with a couple of event badges, a daily page, an agenda page, a details page) using a small hardcoded event set, following the existing `fragment_pages()` pattern.

- [ ] **Step 2: generate goldens.** `nix develop -c env RMBUJO_UPDATE_GOLDENS=1 cargo test --test visual`. Eyeball each new golden (badges legible, content below toolbar, swatches/links visible).

- [ ] **Step 3: final gate + commit.**
```bash
make test && make clippy && make fmt-check
git add -A && git commit -m "Visual goldens for monthly/daily/agenda/details; final green"
```

```json:metadata
{"files":["tests/visual.rs","tests/goldens"],"verifyCommand":"make test && make clippy && make fmt-check","acceptanceCriteria":["goldens for 4 new pages","full gate green"]}
```

---

## Self-Review notes

- **Spec coverage:** structure (T4–T5,T9); navigation/links (T4,T5,T9); count badge (T4,T9); toolbar reserve + cover-exempt (T1); day-list fit (T3,T4); Dots-Small spacing (T1); ICS cache-on-fetch + `--refresh-feeds` (T7,T10); all-day+timed+RRULE+multiday (T6); **timezone** (T2,T6); feed color swatch (T9); errors (T6,T7,T8); determinism/page-count invariant (T5,T9 assert leading count unchanged); config/CLI/wizard (T2,T10); module layout (T6–T9); tests incl. goldens (every task + T11); README + sync rule + Dots-Small note (T10).
- **Type consistency:** `EventOccurrence`/`AgendaEvent`/`DayRow`/`DailyPage`/`MonthlyView`, `parse_feed`, `feed_bytes`, `Fetcher`, `build_event_map`, `monthly_row_pt`, `days_in_month`, `TOOLBAR_SAFE_PT`, `pages_per_day`/`timezone` used consistently across tasks.
- **Prototype reference:** the HTML/navigation markup is ported from the committed, validated `examples/spike_month_prototype.rs` (badge → taller pill per the noted clip fix); not re-pasted here to stay DRY.
- **Crate-API caveat:** exact `ical`/`rrule`/`chrono-tz` call shapes are validated against the fixtures in Task 6 (the parse spike); if an API differs, adjust within that task before proceeding.
- **Sequencing:** badges/agenda links only appear once events exist (Task 9), so Tasks 4–5 ship a clean static month with resolved month↔day links and no dangling anchors.
