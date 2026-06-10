use lopdf::Document;
use rmbujo::device::get_device;
use rmbujo::geometry::default_grid;
use rmbujo::render::compile_pdf;
use rmbujo::render::doc::build_preamble;
use rmbujo::render::render_pdf;
use rmbujo::templates::{DayRow, DotGrid, MonthlyView};
use rmbujo::theme::load_theme;

fn tmp(name: &str) -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    p.push(format!("rmbujo-{name}-{n}.pdf"));
    p
}

fn fragments() -> Vec<String> {
    vec![DotGrid.render().unwrap(), DotGrid.render().unwrap()]
}

#[test]
fn page_count_and_size() {
    let dev = get_device("paper-pro-move").unwrap();
    let grid = default_grid(&dev);
    let theme = load_theme("library").unwrap();
    let out = tmp("size");
    render_pdf(&dev, &grid, &theme, &fragments(), &out).unwrap();

    let doc = Document::load(&out).unwrap();
    assert_eq!(doc.get_pages().len(), 2);
    let page_id = *doc.get_pages().get(&1).unwrap();
    let mb = doc
        .get_object(page_id)
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"MediaBox")
        .unwrap()
        .as_array()
        .unwrap();
    let w = mb[2].as_float().unwrap();
    let h = mb[3].as_float().unwrap();
    assert!((w - dev.width_pt()).abs() < 1.0, "width {w}");
    assert!((h - dev.height_pt()).abs() < 1.0, "height {h}");
}

#[test]
fn month_index_shares_device_dot_grid() {
    // The monthly page uses the SAME device-aligned dot grid as the daily/tasks
    // pages (so an inserted "Dots Small" page lines up on every page), and each day
    // label gets a paper knockout so it stays crisp over the dots. The visual golden
    // covers the rendered result; this guards the Typst that makes it work.
    let dev = get_device("paper-pro-move").unwrap();
    let grid = default_grid(&dev);
    let pre = build_preamble(&dev, &grid, &load_theme("library").unwrap());
    // A single device-aligned dot tiling, reused as the background of every dot page.
    assert!(
        pre.contains("#let dot-tile = tiling"),
        "preamble must define the dot tiling:\n{pre}"
    );
    assert!(
        pre.contains(&format!("#let sp = {}pt", grid.spacing_pt)),
        "dots must tile at the full device pitch:\n{pre}"
    );
    assert!(
        pre.contains("#let dot-page(body) = page(fill: white, background: dot-bg"),
        "dot pages must share the device dot background:\n{pre}"
    );
    // Day rows knock out a paper strip so the label stays crisp over the dots.
    let m = MonthlyView {
        month_name: "May",
        year: 2026,
        month_num: 5,
        spacing_pt: grid.spacing_pt,
        days: &[DayRow {
            day: 1,
            weekday: "Fri",
            week_start: false,
            event_count: 0,
        }],
    }
    .render()
    .unwrap();
    assert!(
        m.contains("box(fill: white, width: 44pt)"),
        "day labels need a white knockout to stay crisp over the dots:\n{m}"
    );
}

#[test]
fn preamble_icons_compile() {
    let dev = get_device("paper-pro-move").unwrap();
    let grid = default_grid(&dev);
    let theme = load_theme("library").unwrap();
    // A page exercising every new helper, plus the label sinks the tab links need.
    let body = "#dot-page[#plan-icon(primary, 12pt) #retro-icon(primary, 12pt) #wtab(8)]\n\
                #plain-page[#hide[#box[x]#label(\"wplan-8\")#box[x]#label(\"wretro-8\")]]\n";
    let src = format!("{}{}", build_preamble(&dev, &grid, &theme), body);
    let pdf = compile_pdf(&src, &[]).unwrap();
    assert!(
        pdf.len() > 1000,
        "expected a non-empty PDF, got {} bytes",
        pdf.len()
    );
}

#[test]
fn weekly_markup_has_labels_and_links() {
    use rmbujo::calendar::{build_month, segments};
    use rmbujo::templates::{WeeklyPlan, WeeklyRetro};

    let m = build_month(2026, 5, "mon").unwrap();
    let seg = segments(&m)
        .into_iter()
        .find(|s| s.first_day() == 4)
        .unwrap();

    let plan = WeeklyPlan {
        month_num: 5,
        segment: &seg,
    }
    .render()
    .unwrap();
    assert!(plan.contains("label(\"wplan-4\")"), "plan anchor");
    assert!(plan.contains("label(\"wretro-4\")"), "plan -> retro link");
    assert!(plan.contains("label(\"monthly\")"), "plan -> month link");
    assert!(
        plan.contains("label(\"day-4\")") && plan.contains("label(\"day-10\")"),
        "day links"
    );
    assert!(
        plan.contains("Intentions") && plan.contains("Tasks"),
        "section headings"
    );
    assert!(plan.contains("05.04 \u{2013} 05.10"), "date range header");

    let retro = WeeklyRetro {
        month_num: 5,
        segment: &seg,
    }
    .render()
    .unwrap();
    assert!(retro.contains("label(\"wretro-4\")"), "retro anchor");
    assert!(retro.contains("label(\"wplan-4\")"), "retro -> plan link");
    assert!(retro.contains("label(\"monthly\")"), "retro -> month link");
    assert!(retro.contains("Retro"), "retro heading");
}

#[test]
fn deterministic_bytes() {
    let dev = get_device("paper-pro-move").unwrap();
    let grid = default_grid(&dev);
    let theme = load_theme("library").unwrap();
    let a = tmp("a");
    let b = tmp("b");
    render_pdf(&dev, &grid, &theme, &fragments(), &a).unwrap();
    render_pdf(&dev, &grid, &theme, &fragments(), &b).unwrap();
    assert_eq!(std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap());
}
