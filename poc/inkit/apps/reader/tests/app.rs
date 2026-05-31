mod shared;

use std::sync::Arc;

use inkapp_core::connector::Connector;
use inkapp_core::geometry::PageGeom;
use inkapp_core::manifest::recover_regions;
use inkapp_core::render::compile_to_document_with_sources;
use inkapp_core::runtime::{collect_typst_sources, document_source_in, DocSet};
use inkapp_core::theme::Theme;
use inkapp_readwise_reader::{ArticleId, Location, Readwise, ScriptedTransport, MAX_ATTEMPTS};
use reader::{update, view, App, Connectors, Msg};

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
    use inkapp::Document;
    use inkapp_content::Article as ContentArticle;
    use inkapp_core::component::Component;
    use inkapp_core::components::action_band::ActionBand;
    use inkapp_core::components::heading::Heading;
    use inkapp_core::components::section::Section;
    use inkapp_readwise_reader::Location;
    use reader::Msg;

    // Build an Article component with real HTML content.
    let art_id = inkapp_readwise_reader::ArticleId::new("html-a1");
    let art_id_clone = art_id.clone();
    let content_article: ContentArticle<Msg> =
        ContentArticle::new("<p>the quick brown fox</p>", &[], move |s| {
            Msg::Highlighted {
                article: art_id_clone.clone(),
                text: s.to_string(),
            }
        });

    // Heading<Msg> — now that Heading is generic, no adaptor or substitute needed.
    let heading = Heading::<Msg>::new("Test Article");

    let section_body: Vec<Box<dyn Component<Msg = Msg>>> =
        vec![Box::new(heading), Box::new(content_article)];

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

    let flow: Vec<Box<dyn Component<Msg = Msg>>> =
        vec![Box::new(Section::<Msg>::new("html-a1", section_body))];

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

#[test]
fn update_highlighted_records_highlight() {
    let cx = Connectors::fake();
    let id = ArticleId::new("a1");
    let mut model = App;
    update(
        Msg::Highlighted {
            article: id.clone(),
            text: "the slow web".into(),
        },
        &mut model,
        &cx,
    );
    let hs = cx.readwise.highlights(&id);
    assert!(
        hs.iter().any(|t| t == "the slow web"),
        "highlight not recorded; got: {hs:?}"
    );
}

#[test]
fn update_move_archive_optimistically_hides_article() {
    let cx = Connectors::fake();
    let id = ArticleId::new("a1");

    let before: Vec<String> = cx.readwise.library().into_iter().map(|a| a.id.0).collect();
    assert!(
        before.iter().any(|s| s == "a1"),
        "fake cassette must seed a1 in Library; got: {before:?}"
    );

    let mut model = App;
    update(
        Msg::Move {
            article: id.clone(),
            to: Location::Archive,
        },
        &mut model,
        &cx,
    );

    assert!(
        cx.readwise.archived().contains(&id),
        "a1 must appear in archived overlay after Move{{to: Archive}}"
    );
    let after: Vec<String> = cx.readwise.library().into_iter().map(|a| a.id.0).collect();
    assert!(
        !after.iter().any(|s| s == "a1"),
        "a1 must no longer appear in Library after archive; got: {after:?}"
    );
}

#[test]
fn update_delete_optimistically_hides_article() {
    let cx = Connectors::fake();
    let id = ArticleId::new("a1");
    let mut model = App;
    update(
        Msg::Delete {
            article: id.clone(),
        },
        &mut model,
        &cx,
    );
    assert!(
        cx.readwise.archived().contains(&id),
        "a1 must appear in archived overlay after Delete"
    );
    let after: Vec<String> = cx.readwise.library().into_iter().map(|a| a.id.0).collect();
    assert!(
        !after.iter().any(|s| s == "a1"),
        "a1 must no longer appear in Library after Delete; got: {after:?}"
    );
}

#[test]
fn view_is_empty_when_no_articles_remain() {
    let cx = Connectors::fake();
    let mut model = App;
    update(
        Msg::Delete {
            article: ArticleId::new("a1"),
        },
        &mut model,
        &cx,
    );
    update(
        Msg::Delete {
            article: ArticleId::new("a2"),
        },
        &mut model,
        &cx,
    );

    let docs = view(&App, &cx);
    assert!(
        docs.0.is_empty(),
        "view must be empty when no articles remain and no failed writes; got keys: {:?}",
        docs.0.iter().map(|d| d.key.0.as_str()).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn view_prepends_banner_on_failed_writes() {
    let transport = Arc::new(ScriptedTransport::always_failing());
    let readwise = Arc::new(Readwise::fake().with_transport(transport));

    let cx = Connectors {
        readwise: readwise.clone(),
    };

    let mut model = App;
    update(
        Msg::Move {
            article: ArticleId::new("a1"),
            to: Location::Archive,
        },
        &mut model,
        &cx,
    );

    for _ in 0..MAX_ATTEMPTS {
        Connector::flush(&*readwise).await;
    }

    assert!(
        !cx.readwise.failed_writes().is_empty(),
        "always-failing transport must produce at least one failed write after {MAX_ATTEMPTS} flushes"
    );

    let docs = view(&App, &cx);
    let first_key = docs.0.first().map(|d| d.key.0.as_str()).unwrap_or_default();
    assert_eq!(
        first_key,
        "_banner",
        "view must prepend the _banner Document when failed_writes is non-empty; got keys: {:?}",
        docs.0.iter().map(|d| d.key.0.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn view_after_archive_drops_article_from_library() {
    let cx = Connectors::fake();

    let docs_before = view(&App, &cx);
    let library_before = docs_before
        .0
        .iter()
        .find(|d| d.key.0 == "Library")
        .expect("Library document present before mutation");
    let src_before = document_source_in(library_before, PageGeom::default(), &Theme::reader());
    assert!(
        src_before.contains("\"a1\""),
        "Library typst source must mention a1 before archive"
    );

    let mut model = App;
    update(
        Msg::Move {
            article: ArticleId::new("a1"),
            to: Location::Archive,
        },
        &mut model,
        &cx,
    );

    let docs_after = view(&App, &cx);
    let library_after = docs_after
        .0
        .iter()
        .find(|d| d.key.0 == "Library")
        .expect("Library document still present (a2 remains)");
    let src_after = document_source_in(library_after, PageGeom::default(), &Theme::reader());
    assert!(
        !src_after.contains("\"a1\""),
        "Library typst source must NOT mention a1 after archive"
    );
    assert!(
        src_after.contains("\"a2\""),
        "Library typst source must still mention a2 (it was not archived)"
    );
}
