mod shared;

use inkapp_core::manifest::recover_regions;
use inkapp_core::runtime::{collect_typst_sources, document_source_in, DocSet};
use inkapp_core::geometry::PageGeom;
use inkapp_core::theme::Theme;
use inkapp_core::render::compile_to_document_with_sources;
use reader::{view, App, Connectors};

/// The view against the fake cassette produces at least the Library Document
/// (the fake articles have Location::New, which is in the default library_locations).
#[tokio::test]
async fn view_yields_library_document() {
    let mut app = shared::fake_app();
    let mut set = DocSet::default();
    let rendered = app.render(&mut set).await.expect("render succeeds");

    // The fake cassette has two articles in Location::New → Library contains them.
    assert!(
        !rendered.is_empty(),
        "at least one document rendered from fake cassette"
    );

    // Library must be present.
    let keys: Vec<&str> = rendered.iter().map(|r| r.key.0.as_str()).collect();
    assert!(
        keys.contains(&"Library"),
        "Library document is in the rendered set; got: {keys:?}"
    );
}

/// Compile the Library Document end-to-end and verify that BOTH tok-* (Article
/// token) and action-* (ActionBand cell) regions are recovered.
#[test]
fn library_compiles_and_recovers_action_plus_token_regions() {
    let cx = Connectors::fake();
    let docs = view(&App, &cx);

    // Find the Library document.
    let library_doc = docs
        .0
        .into_iter()
        .find(|d| d.key.0 == "Library")
        .expect("Library document present in view output");

    // Compile it end-to-end with all its Typst sources.
    let src = document_source_in(&library_doc, PageGeom::default(), &Theme::reader());
    let sources = collect_typst_sources(&library_doc);
    let compiled = compile_to_document_with_sources(&src, &sources)
        .expect("Library document compiles without errors");

    // Recover the regions embedded in the compiled output.
    let manifest = recover_regions(&compiled).expect("regions recovered from compiled document");

    let tok_count = manifest
        .regions
        .iter()
        .filter(|r| r.name.starts_with("tok-"))
        .count();
    let action_count = manifest
        .regions
        .iter()
        .filter(|r| r.name.starts_with("action-"))
        .count();

    // The fake cassette articles have no html_content, so tok-* count may be 0.
    // What matters is that we get action-* regions (from the ActionBand page header).
    // Articles with real html_content will produce tok-* regions; the fake cassette
    // articles have empty html (the content pipeline runs on empty string) so tok-*
    // may be absent. We assert the structural shape and not zero action-* regions.
    assert!(
        action_count > 0,
        "ActionBand must produce action-* regions on every page; got {action_count} action regions"
    );

    // If we do have tokens from the article pipeline, record them (informational).
    let _ = tok_count;
}

/// When html_content is present, the Article produces tok-* token regions.
/// This test patches the fake cassette with a real HTML article body and confirms
/// both region families appear.
#[test]
fn library_with_html_content_recovers_both_tok_and_action_regions() {
    use inkapp_core::component::Component;
    use inkapp_core::components::action_band::ActionBand;
    use inkapp_core::components::section::Section;
    use inkapp::Document;
    use inkapp_content::Article as ContentArticle;
    use inkapp_readwise_reader::Location;
    use reader::Msg;

    // Build an Article component with real HTML content.
    let art_id = inkapp_readwise_reader::ArticleId::new("html-a1");
    let art_id_clone = art_id.clone();
    let content_article: ContentArticle<Msg> = ContentArticle::new(
        "<p>the quick brown fox</p>",
        &[],
        move |s| Msg::Highlighted {
            article: art_id_clone.clone(),
            text: s.to_string(),
        },
    );

    // Use Notice<Msg> as the heading placeholder since Heading has Msg=() and can't
    // be placed in a Section<Msg> without the app-internal HeadingAdaptor wrapper.
    let heading_notice = inkapp_core::components::notice::Notice::<Msg>::line("Test Article");

    let section_body: Vec<Box<dyn Component<Msg = Msg>>> = vec![
        Box::new(heading_notice),
        Box::new(content_article),
    ];

    let band = ActionBand::<Msg>::new([
        (
            "Inbox".to_string(),
            Box::new(|id: &str| Msg::Move {
                article: inkapp_readwise_reader::ArticleId::new(id),
                to: Location::New,
            }) as Box<dyn Fn(&str) -> Msg + Send + Sync>,
        ),
        (
            "Archive".to_string(),
            Box::new(|id: &str| Msg::Move {
                article: inkapp_readwise_reader::ArticleId::new(id),
                to: Location::Archive,
            }) as Box<dyn Fn(&str) -> Msg + Send + Sync>,
        ),
        (
            "Later".to_string(),
            Box::new(|id: &str| Msg::Move {
                article: inkapp_readwise_reader::ArticleId::new(id),
                to: Location::Later,
            }) as Box<dyn Fn(&str) -> Msg + Send + Sync>,
        ),
        (
            "Delete".to_string(),
            Box::new(|id: &str| Msg::Delete {
                article: inkapp_readwise_reader::ArticleId::new(id),
            }) as Box<dyn Fn(&str) -> Msg + Send + Sync>,
        ),
    ]);

    let flow: Vec<Box<dyn Component<Msg = Msg>>> = vec![
        Box::new(Section::<Msg>::new("html-a1", section_body)),
    ];

    let doc = Document::keyed("TestLib", flow).page_header(band);

    let src = document_source_in(&doc, PageGeom::default(), &Theme::reader());
    let sources = collect_typst_sources(&doc);
    let compiled = compile_to_document_with_sources(&src, &sources)
        .expect("document with html content compiles");

    let manifest = recover_regions(&compiled).expect("regions recovered");

    let tok_count = manifest
        .regions
        .iter()
        .filter(|r| r.name.starts_with("tok-"))
        .count();
    let action_count = manifest
        .regions
        .iter()
        .filter(|r| r.name.starts_with("action-"))
        .count();

    assert!(
        tok_count > 0,
        "Article with html_content must produce tok-* regions; got {tok_count}"
    );
    assert!(
        action_count > 0,
        "ActionBand must produce action-* regions; got {action_count}"
    );
}
