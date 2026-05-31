//! Render a small sample collection via the Typst path and write it to
//! /tmp/typst_preview.pdf, for visual parity tuning against the fulgur goldens.
use rmreader::device::get_device;
use rmreader::render::{compile_pdf, typst_doc};
use rmreader::theme::load_theme;

fn main() -> anyhow::Result<()> {
    let device = get_device("paper-pro-move")?;
    let theme = load_theme("reader")?;

    let rows = vec![
        typst_doc::Row {
            num: "01".into(),
            title: "Italy bans Kanye West, Travis Scott concerts over security concerns".into(),
            author: "CBC | Top Stories News".into(),
            reading_time: "2 mins".into(),
            anchor: "art-1".into(),
        },
        typst_doc::Row {
            num: "02".into(),
            title:
                "Rescuers free 4 men trapped in flooded cave in Laos, search for 2 still missing"
                    .into(),
            author: "CBC | Top Stories News".into(),
            reading_time: "3 mins".into(),
            anchor: "art-2".into(),
        },
        typst_doc::Row {
            num: "03".into(),
            title: "Cheap Yellow Display with Boosted PSRAM Turned Snazzy Emulator Station".into(),
            author: "Tyler August".into(),
            reading_time: "2 mins".into(),
            anchor: "art-3".into(),
        },
    ];

    let body = "Kanye West appears at the 67th annual Grammy Awards in Los Angeles on Feb. 2, 2025. (Jordan Strauss/The Associated Press)\n\n\
        Italy has banned two concerts involving American rappers Kanye West and Travis Scott that were due to take place in July in the northern city of Reggio Emilia, authorities said on Saturday.\n\n\
        The local prefect, Salvatore Angieri, ordered the cancellation because of concerns over public order and security, including the potential for protests.\n\n\
        West, also known as Ye, has faced a wave of cancellations across Europe for this summer following years of antisemitic remarks, including statements praising Adolf Hitler and the release of content using Nazi imagery.";

    let articles = vec![
        typst_doc::Article {
            anchor: "art-1".into(),
            title: "Italy bans Kanye West, Travis Scott concerts over security concerns".into(),
            byline: "CBC | Top Stories News · CBC · 2 mins".into(),
            body: body.into(),
        },
        typst_doc::Article {
            anchor: "art-2".into(),
            title: "Rescuers free 4 men trapped in flooded cave in Laos".into(),
            byline: "CBC | Top Stories News · 3 mins".into(),
            body: "Rescue teams in Laos freed four men on Saturday.".into(),
        },
        typst_doc::Article {
            anchor: "art-3".into(),
            title: "Cheap Yellow Display Turned Snazzy Emulator Station".into(),
            byline: "Tyler August · 2 mins".into(),
            body: "A neat hardware hack using a cheap display.".into(),
        },
    ];

    let src = typst_doc::build(&device, &theme, "Feed", &rows, &articles);
    std::fs::write("/tmp/typst_preview.typ", &src)?;
    let _ = compile_pdf; // keep import for the simple path

    let rendered =
        rmreader::render::render_collection(&device, &theme, "Feed", &rows, &articles, &[])?;
    std::fs::write("/tmp/typst_preview.pdf", &rendered.pdf)?;
    println!(
        "wrote /tmp/typst_preview.pdf ({} bytes)",
        rendered.pdf.len()
    );
    println!("page_ranges:");
    let mut prs: Vec<_> = rendered.page_ranges.iter().collect();
    prs.sort_by_key(|(k, _)| (*k).clone());
    for (id, pr) in prs {
        println!("  {id}: {}..={}", pr.first, pr.last);
    }
    println!("label_rects:");
    for lr in &rendered.label_rects {
        println!(
            "  {}: ({:.1},{:.1})-({:.1},{:.1})",
            lr.kind, lr.rect.x0, lr.rect.y0, lr.rect.x1, lr.rect.y1
        );
    }
    Ok(())
}
