use inkapp_core::component::Component;
use inkapp_core::geometry::{PdfPoint, PdfRect};
use inkapp_core::ink::{RegionInk, Stroke};
use inkapp_core::manifest::{Manifest, Region};
use inkapp_readwise_reader::ArticleId;
use reading_queue::{update, view, App, ArticleBody, Checkbox, Connectors, Msg};

#[test]
fn archiving_pushes_to_readwise() {
    let cx = Connectors::fake();
    let mut m = App;
    update(
        Msg::Archived {
            article: ArticleId::new("a1"),
        },
        &mut m,
        &cx,
    );
    assert_eq!(cx.readwise.archived(), vec![ArticleId::new("a1")]);
}

#[test]
fn ink_on_the_box_decodes_to_archive() {
    let c = Checkbox::with_msg(
        "done",
        Msg::Archived {
            article: ArticleId::new("a1"),
        },
    );
    let manifest = Manifest {
        version: 1,
        regions: vec![Region {
            name: "done".into(),
            page: 0,
            rect: PdfRect {
                x0: 0.0,
                y0: 0.0,
                x1: 20.0,
                y1: 20.0,
            },
        }],
        ..Default::default()
    };
    let ink = vec![RegionInk {
        region: "done".into(),
        strokes: vec![Stroke {
            points: vec![PdfPoint { x: 10.0, y: 10.0 }],
            highlighter: false,
        }],
    }];
    assert_eq!(
        c.decode(&ink, &manifest),
        vec![Msg::Archived {
            article: ArticleId::new("a1")
        }]
    );
}

#[test]
fn view_is_one_document_per_article() {
    let cx = Connectors::fake();
    let docs = view(&App, &cx);
    assert_eq!(docs.0.len(), cx.readwise.queue().len());
    assert!(
        docs.0.iter().all(|d| d.flow.len() == 2),
        "body + archive checkbox"
    );
}

#[test]
fn article_body_decodes_highlight_to_msg() {
    use inkapp_readwise_reader::Article;

    let article = Article {
        id: ArticleId::new("a1"),
        title: "T".into(),
        body: "lazy dog".into(),
        highlights: vec![],
    };
    let body = ArticleBody::new(&article);

    // tok-0 == "lazy" (first whitespace token of the body).
    let manifest = Manifest {
        version: 1,
        regions: vec![Region {
            name: "tok-0".into(),
            page: 0,
            rect: PdfRect {
                x0: 0.0,
                y0: 0.0,
                x1: 30.0,
                y1: 12.0,
            },
        }],
        ..Default::default()
    };
    let ink = vec![RegionInk {
        region: "tok-0".into(),
        strokes: vec![Stroke {
            points: vec![PdfPoint { x: 2.0, y: 4.0 }, PdfPoint { x: 28.0, y: 8.0 }],
            highlighter: true,
        }],
    }];

    assert_eq!(
        body.decode(&ink, &manifest),
        vec![Msg::Highlighted {
            article: ArticleId::new("a1"),
            text: "lazy".to_string()
        }]
    );
}
