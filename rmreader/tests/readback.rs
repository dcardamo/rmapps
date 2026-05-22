//! Integration tests for the read-back orchestrator (`readback::sync_collection`).
use std::cell::RefCell;
use std::path::{Path, PathBuf};

use rmreader::deploy::Deployer;
use rmreader::readback::sync_collection;
use rmreader::readwise::{HttpMethod, HttpResponse, HttpTransport};

// ---------------------------------------------------------------------------
// FakeDeployer — configurable fetch result; deploy/refresh/replace are no-ops.
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct FakeDeployer {
    /// What `fetch` returns.
    fetch_result: Option<PathBuf>,
}

impl FakeDeployer {
    fn none() -> Self {
        Self { fetch_result: None }
    }
    fn with_path(p: PathBuf) -> Self {
        Self {
            fetch_result: Some(p),
        }
    }
}

impl Deployer for FakeDeployer {
    fn deploy(&self, _targets: &[(PathBuf, String)]) -> anyhow::Result<()> {
        Ok(())
    }
    fn refresh(&self, _targets: &[(PathBuf, String)]) -> anyhow::Result<()> {
        Ok(())
    }
    fn fetch(&self, _folder: &str, _name: &str) -> anyhow::Result<Option<PathBuf>> {
        Ok(self.fetch_result.clone())
    }
    fn replace(&self, _pdf: &Path, _folder: &str) -> anyhow::Result<()> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// RecordingTransport — records all calls; returns HTTP 200 with an empty body.
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
struct RecordingTransport {
    calls: RefCell<Vec<(HttpMethod, String, Option<String>)>>,
}

impl RecordingTransport {
    fn calls(&self) -> Vec<(HttpMethod, String, Option<String>)> {
        self.calls.borrow().clone()
    }
}

impl HttpTransport for RecordingTransport {
    fn request(
        &self,
        method: HttpMethod,
        url: &str,
        _token: &str,
        body: Option<&str>,
    ) -> anyhow::Result<HttpResponse> {
        self.calls
            .borrow_mut()
            .push((method, url.to_string(), body.map(|b| b.to_string())));
        Ok(HttpResponse {
            status: 200,
            retry_after: None,
            body: "{}".to_string(),
        })
    }
}

// ---------------------------------------------------------------------------
// Test 1: no document on device -> clean no-op, zero transport calls.
// ---------------------------------------------------------------------------

#[test]
fn first_run_no_doc_is_noop() {
    let deployer = FakeDeployer::none();
    let transport = RecordingTransport::default();

    let plan = sync_collection(&deployer, &transport, "tok", "/Reader", "Library")
        .expect("sync_collection");

    assert_eq!(plan, rmreader::readback::Plan::default());
    assert!(
        transport.calls().is_empty(),
        "expected zero HTTP calls, got: {:?}",
        transport.calls()
    );
}

// ---------------------------------------------------------------------------
// Test 2: real fixture -> content highlight containing "fox".
//
// The fixture `stamped-labels.rmdoc` embeds one doc (collection "Spike",
// no label_rects) with source text "The quick brown fox …".  The single
// highlighter stroke on page 0 should produce one content highlight whose
// `text` includes the word "fox", verified via a POST to the highlights
// endpoint.
// ---------------------------------------------------------------------------

#[test]
fn pipeline_from_real_fixture_reconstructs_content_highlight() {
    let fixture = Path::new("../rmfiles/tests/fixtures/stamped-labels.rmdoc");
    if !fixture.exists() {
        // Running outside the monorepo layout — skip gracefully.
        eprintln!("SKIP: fixture not found at {}", fixture.display());
        return;
    }

    let deployer = FakeDeployer::with_path(fixture.to_path_buf());
    let transport = RecordingTransport::default();

    let plan =
        sync_collection(&deployer, &transport, "tok", "/Spike", "Spike").expect("sync_collection");

    let calls = transport.calls();

    // At least one POST to the highlights endpoint.
    let hl_post = calls
        .iter()
        .find(|(method, url, _body)| *method == HttpMethod::Post && url.contains("highlights"));

    assert!(
        hl_post.is_some(),
        "expected a POST to the highlights endpoint, got calls: {:?}",
        calls
    );

    let body = hl_post.unwrap().2.as_deref().unwrap_or("");

    // Emit useful debug info before asserting.
    eprintln!("[test] transport calls: {:?}", calls);
    eprintln!("[test] highlights POST body: {body}");
    eprintln!("[test] plan.highlights: {:?}", plan.highlights);
    eprintln!("[test] plan.warnings: {:?}", plan.warnings);

    assert!(
        body.contains("fox"),
        "highlights POST body does not contain \"fox\": {body}"
    );
}
