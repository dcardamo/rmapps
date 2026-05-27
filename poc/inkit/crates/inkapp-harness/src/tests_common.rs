//! Shared helpers for harness tests. Builds small Typst docs through the
//! framework's real render+manifest pipeline so tests exercise the same code
//! path apps go through.

use inkapp_core::components::action_band::ActionBand;
use inkapp_core::components::gesture::GestureAction;
use inkapp_core::components::heading::Heading;
use inkapp_core::components::section::Section;
use inkapp_core::document::Document;
use inkapp_core::flow;
use inkapp_core::geometry::PageGeom;
use inkapp_core::manifest::recover_regions;
use inkapp_core::render::{compile_to_document_with_sources, document_to_pdf};
use inkapp_core::runtime::compile_document_in;
use inkapp_core::theme::Theme;
use inkapp_core::REGION_PRELUDE;

use crate::session::PublishedApp;

/// Build a `PublishedApp` from a Typst document body. The body is wrapped in a
/// 200×200pt page with a 10pt margin and the region prelude is preloaded.
fn build_publishable_app(name: &str, body: &str) -> PublishedApp {
    let src = format!(
        r#"#import "/inkapp/region.typ": region
#set page(width: 200pt, height: 200pt, margin: 10pt)
{body}
"#
    );
    let sources = vec![(REGION_PRELUDE.0.to_string(), REGION_PRELUDE.1.to_string())];
    let doc = compile_to_document_with_sources(&src, &sources).expect("compile typst");
    let pdf_bytes = document_to_pdf(&doc).expect("render pdf");
    let manifest = recover_regions(&doc)
        .expect("recover manifest")
        .with_version(1);
    PublishedApp {
        app_name: name.to_string(),
        pdf_bytes,
        manifest,
        source_typ: Some(src),
    }
}

/// Build a one-page Typst doc with a single region named `r1` and return it as a
/// `PublishedApp` ready for `Session::document_publish`.
pub fn single_region_app(name: &str) -> PublishedApp {
    build_publishable_app(name, r#"#region("r1")[hello]"#)
}

/// Like `single_region_app` but the region body is a Typst `#link(uri)[…]` so the
/// rendered PDF carries a `/Link` annotation whose rect sits inside the region.
pub fn app_with_uri_link(name: &str, region_name: &str, uri: &str) -> PublishedApp {
    let body = format!(r#"#region("{region_name}")[#link("{uri}")[{region_name} content]]"#);
    build_publishable_app(name, &body)
}

/// Multi-component, multi-page fixture for Layer-4 follow-up lens parity:
/// two `Section`s (each with a `Heading` + a `GestureAction`) under an
/// `ActionBand` page-header. Compiles through the framework's real Document
/// pipeline so the regions match what app-authoring code produces.
pub fn multi_component_app(name: &str) -> PublishedApp {
    let band: ActionBand<()> = ActionBand::new([
        (
            "Inbox".to_string(),
            Box::new(|_id: &str| ()) as Box<dyn Fn(&str) + Send + Sync>,
        ),
        ("Archive".to_string(), Box::new(|_id: &str| ())),
    ]);

    let s1: Section<()> = Section::new(
        "art-1",
        vec![
            Box::new(Heading::<()>::new("First article")),
            Box::new(GestureAction::with_msg("title-1", "tap me", ())),
        ],
    );
    let s2: Section<()> = Section::new(
        "art-2",
        vec![
            Box::new(Heading::<()>::new("Second article")),
            Box::new(GestureAction::with_msg("title-2", "tap me too", ())),
        ],
    );

    let doc: Document<()> = Document::keyed("multi", flow![s1, s2]).page_header(band);
    let geom = PageGeom {
        w: 240.0,
        h: 140.0,
        margin: 6.0,
    };
    let compiled = compile_document_in(&doc, geom, &Theme::reader()).expect("compile multi doc");
    let pdf_bytes = document_to_pdf(&compiled).expect("render pdf");
    let manifest = recover_regions(&compiled)
        .expect("recover manifest")
        .with_version(1);
    PublishedApp {
        app_name: name.to_string(),
        pdf_bytes,
        manifest,
        source_typ: None,
    }
}
