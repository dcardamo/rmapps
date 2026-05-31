//! Local preview: render the app's document set to PDFs and optionally serve
//! them over HTTP for browser viewing (Tailscale-friendly: binds 0.0.0.0).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use inkapp_core::connector::ConnectorSet;
use inkapp_core::error::{Error, Result};
use inkapp_core::runtime::{App, DocSet};

pub use crate::cli::PreviewArgs;

#[derive(Debug, Clone)]
pub struct RenderedEntry {
    pub key: String,
    pub path: PathBuf,
    pub size_bytes: usize,
    pub page_count: usize,
}

/// Render the app's document set and write each to `<out>/<key>.pdf`.
pub async fn render_to_dir<M, Msg, Cx: ConnectorSet>(
    app: &mut App<M, Msg, Cx>,
    out: &Path,
) -> Result<Vec<RenderedEntry>> {
    std::fs::create_dir_all(out).map_err(|e| Error::Config(format!("preview mkdir: {e}")))?;
    let mut set = DocSet::default();
    let rendered = app.render(&mut set).await?;
    let mut entries = Vec::with_capacity(rendered.len());
    for rd in &rendered {
        let path = out.join(format!("{}.pdf", sanitize_key(&rd.key.0)));
        std::fs::write(&path, &rd.pdf).map_err(|e| Error::Config(format!("preview write: {e}")))?;
        entries.push(RenderedEntry {
            key: rd.key.0.clone(),
            path,
            size_bytes: rd.pdf.len(),
            page_count: rd.page_count,
        });
    }
    Ok(entries)
}

/// Replace path separators in a doc key so it can be a filename.
fn sanitize_key(k: &str) -> String {
    k.replace(['/', '\\'], "_")
}

/// Render to dir and also return an in-memory PDF map keyed by doc key (used by `run`).
pub(crate) async fn render_to_dir_and_map<M, Msg, Cx: ConnectorSet>(
    app: &mut App<M, Msg, Cx>,
    out: &Path,
) -> Result<(Vec<RenderedEntry>, HashMap<String, Vec<u8>>)> {
    std::fs::create_dir_all(out).map_err(|e| Error::Config(format!("preview mkdir: {e}")))?;
    let mut set = DocSet::default();
    let rendered = app.render(&mut set).await?;
    let mut entries = Vec::with_capacity(rendered.len());
    let mut pdfs: HashMap<String, Vec<u8>> = HashMap::new();
    for rd in rendered {
        let path = out.join(format!("{}.pdf", sanitize_key(&rd.key.0)));
        std::fs::write(&path, &rd.pdf).map_err(|e| Error::Config(format!("preview write: {e}")))?;
        entries.push(RenderedEntry {
            key: rd.key.0.clone(),
            path,
            size_bytes: rd.pdf.len(),
            page_count: rd.page_count,
        });
        pdfs.insert(rd.key.0, rd.pdf);
    }
    Ok((entries, pdfs))
}

use std::sync::Arc;

use axum::{
    extract::{Path as AxumPath, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::get,
    Router,
};

#[derive(Clone)]
struct PdfState {
    pdfs: Arc<HashMap<String, Vec<u8>>>,
}

/// Build a router that lists and serves an in-memory PDF set.
/// Pure: takes the map by value, returns a configured Router.
pub fn make_router(pdfs: HashMap<String, Vec<u8>>) -> Router {
    let state = PdfState {
        pdfs: Arc::new(pdfs),
    };
    Router::new()
        .route("/", get(index))
        .route("/{filename}", get(serve_pdf))
        .with_state(state)
}

async fn index(State(s): State<PdfState>) -> Html<String> {
    let mut keys: Vec<&String> = s.pdfs.keys().collect();
    keys.sort();
    let mut html = String::from(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>inkapp preview</title>\
         <style>body{font-family:sans-serif;max-width:60em;margin:2em auto;padding:0 1em}\
         li{margin:.4em 0}.meta{color:#888;font-size:.9em}</style></head><body>\
         <h1>inkapp preview</h1><ul>",
    );
    for k in keys {
        let bytes = s.pdfs[k].len();
        html.push_str(&format!(
            "<li><a href=\"/{k}.pdf\">{k}</a> <span class=\"meta\">({bytes} bytes)</span></li>"
        ));
    }
    html.push_str("</ul></body></html>");
    Html(html)
}

async fn serve_pdf(State(s): State<PdfState>, AxumPath(filename): AxumPath<String>) -> Response {
    // filename is "<key>.pdf"; strip the suffix to get the lookup key.
    let key = filename.strip_suffix(".pdf").unwrap_or(&filename);
    match s.pdfs.get(key) {
        Some(bytes) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/pdf")],
            bytes.clone(),
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

/// Render to `args.out`; if `args.serve`, bind `0.0.0.0:port` and serve the same
/// PDFs over HTTP, printing a Tailscale-reachable URL using the local hostname.
pub async fn run<M, Msg, Cx: ConnectorSet>(
    app: &mut App<M, Msg, Cx>,
    args: PreviewArgs,
) -> Result<i32> {
    let (entries, pdfs) = render_to_dir_and_map(app, &args.out).await?;
    println!(
        "preview: wrote {} PDF(s) to {}",
        entries.len(),
        args.out.display()
    );
    for e in &entries {
        println!(
            "  {}  ({} pages, {} bytes)  -> {}",
            e.key,
            e.page_count,
            e.size_bytes,
            e.path.display()
        );
    }
    if !args.serve {
        return Ok(0);
    }
    let host = gethostname::gethostname().to_string_lossy().into_owned();
    let addr: std::net::SocketAddr = ([0, 0, 0, 0], args.port).into();
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| Error::Config(format!("preview bind {addr}: {e}")))?;
    println!(
        "preview: serving at http://{host}:{port}",
        host = host,
        port = args.port
    );
    let router = make_router(pdfs);
    axum::serve(listener, router)
        .await
        .map_err(|e| Error::Config(format!("preview serve: {e}")))?;
    Ok(0)
}

#[cfg(test)]
mod tests {
    mod fixture {
        use inkapp_core::crypto::Key;
        use inkapp_core::runtime::{app, App};
        use inkapp_readwise_reader::Readwise;
        use reading_queue::{update, view, App as RqApp, Connectors};
        use std::sync::Arc;

        pub fn cassette_app() -> App<RqApp, reading_queue::Msg, Connectors> {
            let connectors = Connectors::from_arc(Arc::new(Readwise::from_cassette()));
            app(RqApp)
                .connector(connectors)
                .update(update)
                .view(view)
                .key(Key::from_bytes([7u8; 32]))
                .build()
        }
    }

    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    #[tokio::test]
    async fn render_to_dir_writes_nonempty_pdfs_starting_with_magic() {
        let tmp = tempfile::tempdir().unwrap();
        let mut application = fixture::cassette_app();
        let entries = render_to_dir(&mut application, tmp.path()).await.unwrap();
        assert!(!entries.is_empty(), "cassette must yield at least one doc");
        for e in &entries {
            assert!(e.path.exists(), "{} must exist on disk", e.path.display());
            let bytes = std::fs::read(&e.path).unwrap();
            assert!(!bytes.is_empty(), "{} must be non-empty", e.key);
            assert!(bytes.starts_with(b"%PDF"), "{} must start with %PDF", e.key);
            assert_eq!(bytes.len(), e.size_bytes, "size_bytes matches file");
            assert!(e.page_count >= 1, "{} must have at least one page", e.key);
        }
    }

    fn fixture_pdfs() -> std::collections::HashMap<String, Vec<u8>> {
        let mut m = std::collections::HashMap::new();
        m.insert("alpha".to_string(), b"%PDF-1.4\n...alpha...".to_vec());
        m.insert("beta".to_string(), b"%PDF-1.4\n...beta...".to_vec());
        m
    }

    #[tokio::test]
    async fn router_lists_keys_at_root() {
        let router = make_router(fixture_pdfs());
        let resp = router
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(
            body.contains(r#"href="/alpha.pdf""#),
            "index lists alpha: {body}"
        );
        assert!(
            body.contains(r#"href="/beta.pdf""#),
            "index lists beta: {body}"
        );
    }

    #[tokio::test]
    async fn router_serves_known_pdf_with_correct_content_type_and_bytes() {
        let pdfs = fixture_pdfs();
        let expected = pdfs.get("alpha").cloned().unwrap();
        let router = make_router(pdfs);
        let resp = router
            .oneshot(
                Request::builder()
                    .uri("/alpha.pdf")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "application/pdf",
        );
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(body.as_ref(), expected.as_slice());
    }

    #[tokio::test]
    async fn router_returns_404_for_unknown_key() {
        let router = make_router(fixture_pdfs());
        let resp = router
            .oneshot(
                Request::builder()
                    .uri("/missing.pdf")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn run_without_serve_writes_pdfs_and_returns_zero() {
        let tmp = tempfile::tempdir().unwrap();
        let mut application = fixture::cassette_app();
        let args = PreviewArgs {
            out: tmp.path().to_path_buf(),
            serve: false,
            port: 4747,
        };
        let code = run(&mut application, args).await.unwrap();
        assert_eq!(code, 0);
        // At least one PDF exists.
        let count = std::fs::read_dir(tmp.path())
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .path()
                    .extension()
                    .is_some_and(|x| x == "pdf")
            })
            .count();
        assert!(
            count >= 1,
            "expected at least one .pdf in {}",
            tmp.path().display()
        );
    }
}
