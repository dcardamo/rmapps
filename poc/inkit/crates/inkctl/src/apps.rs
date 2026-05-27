//! Registry of in-tree harness fixtures publishable from `inkctl document publish`.

use inkapp_harness::session::PublishedApp;
use inkapp_harness::tests_common;

pub fn build(app_name: &str) -> Result<PublishedApp, String> {
    match app_name {
        "smoke" => Ok(tests_common::single_region_app("smoke")),
        "uri-link" => Ok(tests_common::app_with_uri_link(
            "uri-link",
            "r1",
            "https://example.org",
        )),
        other => Err(format!("unknown_app: {other}")),
    }
}
