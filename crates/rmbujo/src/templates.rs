//! Per-page Typst emitters. Each struct holds one page's data and `render()`s a
//! Typst fragment that calls the preamble helpers (see `crate::render::doc`).
//! Each fragment is already wrapped in a `*-page(...)` helper, so the renderer
//! just concatenates the preamble and the fragments.

use crate::render::doc::{esc_markup, esc_str};

/// Build a `month.day weekday` date label, e.g. `5.02 Sat`.
fn date_label(month_num: u32, day_pad: &str, weekday: &str) -> String {
    format!(
        "{month_num}.{} {}",
        esc_markup(day_pad),
        esc_markup(weekday)
    )
}

/// The journal cover: full-bleed indigo gradient, year + title bottom-left. A
/// blank cover (collection template) shows a write-in rule instead of a title.
pub struct Cover<'a> {
    pub year: i32,
    pub title: &'a str,
    pub blank_title: bool,
}

impl Cover<'_> {
    pub fn render(&self) -> anyhow::Result<String> {
        let title = if self.blank_title {
            "#box(width: 70%, height: 22pt, \
             stroke: (bottom: 1pt + nav.transparentize(40%)))[]"
                .to_string()
        } else {
            format!(
                "#text(font: \"Fraunces 72pt\", size: 24pt, weight: 600, fill: nav)[{}]",
                esc_markup(self.title)
            )
        };
        // The trailing weak:false spacer holds the title ~one margin off the
        // bottom edge (matching the old CSS `.cover` padding); without it Typst's
        // text metrics let the title settle almost flush with the page bottom.
        Ok(format!(
            "#cover-page[\n\
             #v(1fr)\n\
             #text(font: \"Lora\", size: 9pt, fill: nav, tracking: 2.25pt)[{year}]\n\
             #v(5pt)\n\
             {title}\n\
             #v(8pt, weak: false)\n\
             ]\n",
            year = self.year,
            title = title,
        ))
    }
}

/// A blank dot-grid page (collection pages, extra per-day pages).
pub struct DotGrid;

impl DotGrid {
    pub fn render(&self) -> anyhow::Result<String> {
        Ok("#dot-page[]\n".to_string())
    }
}

/// The monthly Tasks page: dot grid with a heading.
pub struct Tasks;

impl Tasks {
    pub fn render(&self) -> anyhow::Result<String> {
        Ok("#dot-page[\n\
            #text(font: \"Fraunces 72pt\", size: head-fs, weight: 600, fill: primary)[Tasks]\n\
            ]\n"
        .to_string())
    }
}

/// The Future Log: up to three month blocks per page, each with its own dot grid
/// and a hairline divider beneath.
pub struct FutureLog<'a> {
    pub months: &'a [&'a str],
}

impl FutureLog<'_> {
    pub fn render(&self) -> anyhow::Result<String> {
        let mut blocks = String::new();
        for name in self.months {
            blocks.push_str(&format!(
                // Divider is ink (near-black), matching the deployed fulgur look:
                // fulgur didn't resolve `var(--rule)` in the border shorthand and
                // fell back to currentColor (the block's inherited body ink).
                "#block(width: 100%, height: (page-h - toolbar-pt - margin-pt) / 3, \
                 inset: (top: 4pt), stroke: (bottom: 0.6pt + ink), \
                 spacing: 0pt, fill: dot-tile)[\n\
                 #text(font: \"Fraunces 72pt\", size: 12pt, weight: 600, fill: primary)[{name}]\n\
                 ]\n",
                name = esc_markup(name),
            ));
        }
        Ok(format!("#plain-page[\n{blocks}]\n"))
    }
}

#[derive(Clone, Debug)]
pub struct DayRow {
    pub day: u32,
    pub weekday: &'static str,
    pub week_start: bool,
    pub event_count: usize,
}

/// The month index: a Fraunces masthead over a numbered, weekday-labelled day
/// list, each row a tap target into the day page (with an event-count badge).
pub struct MonthlyView<'a> {
    pub month_name: &'a str,
    pub year: i32,
    pub month_num: u32,
    /// Dot pitch in pt; each day is placed on a real dot-row centre at this pitch.
    pub spacing_pt: f32,
    pub days: &'a [DayRow],
}

impl MonthlyView<'_> {
    pub fn render(&self) -> anyhow::Result<String> {
        let sp = self.spacing_pt;
        // Badge sized to fit inside one dot cell (with a little air on each side)
        // so adjacent days' badges never touch — the overlap bug was a 13pt badge
        // crammed into a sub-pitch row.
        let badge_h = sp - 2.5;
        let mut rows = String::new();
        for (i, d) in self.days.iter().enumerate() {
            // Sundays/week starts get the indigo number; other days the tomato.
            let numcol = if d.week_start { "primary" } else { "accent" };
            let badge = if d.event_count > 0 {
                format!(
                    " #h(6pt) #cbadge({}, label(\"agenda-{}\"), h: {badge_h}pt)",
                    d.event_count, d.day
                )
            } else {
                String::new()
            };
            // Place an sp-tall row whose centre lands on this day's dot-row centre;
            // `align(horizon)` then sits the text on the dot line. `top` is the
            // row's upper edge in absolute page Y (month-page has no top margin).
            let top = crate::geometry::monthly_row_center(sp, i) - sp / 2.0;
            rows.push_str(&format!(
                "#place(top + left, dy: {top}pt)[#block(width: 100%, height: {sp}pt)[\
                 #align(horizon)[\
                 #box(fill: white, width: 44pt)[#link(label(\"day-{day}\"))[\
                 #box(width: 16pt)[#align(right)[#text(font: \"Hanken Grotesk\", size: num-fs, \
                 weight: 700, fill: {numcol})[{day}]]] #h(6pt) \
                 #text(font: \"Hanken Grotesk\", size: wd-fs, fill: muted)[{wd}]]]{badge}]]]\n",
                day = d.day,
                numcol = numcol,
                wd = esc_markup(d.weekday),
                badge = badge,
            ));
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
        }
        // Masthead tucked into the top toolbar-reserve band (above the first day
        // row); its paper backdrop keeps it legible over the dots. Fully visible in
        // reading/navigation mode — only hidden behind the pen toolbar while writing.
        Ok(format!(
            "#month-page[\n\
             #place(top + left, dy: 4pt)[#box(fill: white, inset: (x: 4pt))[\
             #text(font: \"Fraunces 72pt\", size: head-fs, weight: 600, fill: primary)[{title} {year}]] \
             #label(\"monthly\")]\n\
             {rows}]\n",
            title = esc_markup(self.month_name),
            year = self.year,
            rows = rows,
        ))
    }
}

/// The static reference notebook: a key of bullet-journal symbols and a short
/// how-to. Two plain pages.
pub struct Reference;

impl Reference {
    pub fn render(&self) -> anyhow::Result<String> {
        // (symbol, label) — the bullet-journal key. Symbols resolve via the
        // typst-assets fallback when Lora lacks the glyph.
        let key: [(&str, &str); 8] = [
            ("•", "Task"),
            ("×", "Task complete"),
            (">", "Migrated"),
            ("<", "Scheduled"),
            ("○", "Event"),
            ("—", "Note"),
            ("★", "Priority"),
            ("=", "Feeling / mood"),
        ];
        let mut legend = String::new();
        for (sym, lbl) in key {
            legend.push_str(&format!(
                "text(weight: 600, fill: primary)[{sym}], [{lbl}],\n",
                sym = esc_markup(sym),
                lbl = esc_markup(lbl),
            ));
        }
        let key_page = format!(
            "#plain-page[\n\
             #block(below: 8pt)[#text(font: \"Fraunces 72pt\", size: 14pt, weight: 600, \
             fill: primary)[Key]]\n\
             #grid(columns: (16pt, auto), column-gutter: 0pt, row-gutter: 0.7em,\n{legend})\n\
             ]\n",
        );
        let using_page = "#plain-page[\n\
             #block(below: 8pt)[#text(font: \"Fraunces 72pt\", size: 14pt, weight: 600, \
             fill: primary)[Using this journal]]\n\
             #set par(leading: 0.55em, spacing: 0.9em)\n\
             #strong[Start a month:] set up the day list and the Tasks page, then migrate open \
             items forward from last month and the Future Log.\n\n\
             #strong[End a month:] review each day and the Tasks page. Complete (×), migrate (>) \
             unfinished tasks to next month, or schedule (<) them into the Future Log. Drop what \
             no longer matters.\n\
             ]\n"
        .to_string();
        Ok(format!("{key_page}{using_page}"))
    }
}

/// A single day page: a dot grid with a date header (link back to the month
/// index) and an optional event-count badge linking to the day's event list.
pub struct DailyPage<'a> {
    pub day: u32,
    pub day_pad: String,
    pub month_num: u32,
    pub weekday: &'a str,
    pub event_count: usize,
}

impl DailyPage<'_> {
    pub fn render(&self) -> anyhow::Result<String> {
        let badge = if self.event_count > 0 {
            format!(
                "#cbadge({}, label(\"agenda-{}\"))",
                self.event_count, self.day
            )
        } else {
            String::new()
        };
        Ok(format!(
            "#dot-page[\n\
             #box(width: 100%)[\
             #link(label(\"monthly\"))[#box(fill: white, \
             inset: (left: 3pt, right: 3pt, top: 1pt, bottom: 2pt), \
             stroke: (bottom: 0.75pt + primary))[\
             #text(font: \"Fraunces 72pt\", size: 13pt, weight: 600, fill: primary)[{date}]]] \
             #label(\"day-{day}\")\
             #h(1fr){badge}]\n\
             ]\n",
            date = date_label(self.month_num, &self.day_pad, self.weekday),
            day = self.day,
            badge = badge,
        ))
    }
}

#[derive(Clone, Debug)]
pub struct AgendaEvent {
    /// "All Day" or "HH:MM" (start).
    pub label: String,
    /// End time "HH:MM" for timed events with a DTEND; rendered as a start–end range.
    pub end_label: Option<String>,
    pub title: String,
    pub location: Option<String>,
    pub description: Option<String>,
    pub attendees: Vec<String>,
    /// Theme color name (looked up in the preamble's `theme-col` dict).
    pub color: String,
    pub is_all_day: bool,
}

#[derive(Clone, Debug)]
pub struct AgendaDay {
    pub day: u32,
    pub weekday: &'static str,
    pub events: Vec<AgendaEvent>,
}

/// One page of a single day's events: each event shown in full (time, title,
/// Where/Notes/Who). `events` holds only the events on THIS page; the
/// continuation/first-page flags come from `notebooks::month::agenda::DayPagePlan`.
pub struct DayEvents<'a> {
    pub month_num: u32,
    pub day: u32,
    pub day_pad: String,
    pub weekday: &'a str,
    pub events: &'a [AgendaEvent],
    pub continued: bool,
    pub first_page: bool,
}

impl DayEvents<'_> {
    pub fn render(&self) -> anyhow::Result<String> {
        let mut events = String::new();
        for e in self.events {
            // Time label: "All Day" / "HH:MM" / "HH:MM–HH:MM".
            let time = match &e.end_label {
                Some(end) => format!("{}–{}", esc_markup(&e.label), esc_markup(end)),
                None => esc_markup(&e.label),
            };
            let mut meta = String::new();
            let mut push_meta = |prefix: &str, val: &str| {
                meta.push_str(&format!(
                    "#linebreak()#text(font: \"Hanken Grotesk\", size: 9pt, fill: muted)[{}: {}]",
                    prefix,
                    esc_markup(val),
                ));
            };
            if let Some(loc) = &e.location {
                push_meta("Where", loc);
            }
            if let Some(desc) = &e.description {
                push_meta("Notes", desc);
            }
            if !e.attendees.is_empty() {
                push_meta("Who", &e.attendees.join(", "));
            }
            events.push_str(&format!(
                "#block(inset: (left: 8pt), above: 3pt, below: 6pt)[\
                 #text(size: 10pt, fill: ink)[#swatch(\"{color}\")#h(4pt)#strong[{time}]#h(0.6em){title}]\
                 {meta}]\n",
                color = esc_str(&e.color),
                time = time,
                title = esc_markup(&e.title),
                meta = meta,
            ));
        }
        // Header: date link back to the day page (+ " · cont." on continuations).
        // The first page of a day carries the agenda-{day} link target.
        let cont = if self.continued {
            " #text(font: \"Fraunces 72pt\", size: 16pt, fill: primary)[· cont.]".to_string()
        } else {
            String::new()
        };
        let anchor = if self.first_page {
            format!(" #label(\"agenda-{}\")", self.day)
        } else {
            String::new()
        };
        Ok(format!(
            "#plain-page[\n\
             #block(below: 6pt)[\
             #link(label(\"day-{day}\"))[#text(font: \"Fraunces 72pt\", size: 16pt, weight: 600, \
             fill: primary)[{date}]]{cont}{anchor}]\n\
             {events}]\n",
            day = self.day,
            date = date_label(self.month_num, &self.day_pad, self.weekday),
            cont = cont,
            anchor = anchor,
            events = events,
        ))
    }
}
