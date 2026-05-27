use clap::Subcommand;
use inkapp_harness::session::{DeviceId, Session};
use serde_json::json;
use std::path::PathBuf;

use crate::apps;
use crate::output;
use crate::util;

#[derive(clap::Args)]
pub struct Args {
    #[arg(long, global = true)]
    session: Option<String>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Publish a document to a device
    Publish { device: String, app_name: String },
    /// Open a published document (records current.json)
    Open { doc_id: String },
    /// Describe a published document
    Describe { doc_id: String },
    /// Extract the PDF for a published document
    Pdf {
        doc_id: String,
        #[arg(long)]
        out: PathBuf,
    },
    /// Extract a .rmdoc bundle (currently PDF-only fallback)
    Rmdoc {
        doc_id: String,
        #[arg(long)]
        out: PathBuf,
    },
}

pub async fn run(args: Args) -> ! {
    let session_id = match util::resolve_session_id(args.session) {
        Ok(s) => s,
        Err(e) => output::print_err("missing_session", e),
    };
    let dir = util::session_dir(&session_id);
    let mut session = match Session::open(&dir).await {
        Ok(s) => s,
        Err(e) => output::print_err("io_error", e),
    };

    match args.cmd {
        Cmd::Publish { device, app_name } => {
            let app = match apps::build(&app_name) {
                Ok(a) => a,
                Err(e) => output::print_err("unknown_app", e),
            };
            let summary = match session.document_publish(&DeviceId::new(device), app).await {
                Ok(s) => s,
                Err(e) => output::print_err("io_error", e),
            };
            if let Err(e) = session.flush() {
                output::print_err("io_error", e);
            }
            output::print_ok(json!({
                "doc_id": summary.id,
                "version": summary.version,
                "pages": summary.pages,
                "app_name": summary.app_name,
            }))
        }
        Cmd::Open { doc_id } => {
            let path = session.state_dir().join("current.json");
            if let Err(e) = std::fs::write(
                &path,
                serde_json::to_vec_pretty(&json!({ "doc_id": doc_id })).unwrap(),
            ) {
                output::print_err("io_error", e);
            }
            output::print_ok(json!({ "doc_id": doc_id }))
        }
        Cmd::Describe { doc_id } => {
            let desc = match inkapp_harness::observe::document_describe(&session, &doc_id) {
                Ok(d) => d,
                Err(e) => output::print_err("io_error", e),
            };
            output::print_ok(desc)
        }
        Cmd::Pdf { doc_id, out } => {
            let src = session
                .state_dir()
                .join("docs")
                .join(&doc_id)
                .join("pdf.pdf");
            let bytes = match std::fs::read(&src) {
                Ok(b) => b,
                Err(e) => output::print_err("io_error", e),
            };
            if let Err(e) = std::fs::write(&out, &bytes) {
                output::print_err("io_error", e);
            }
            output::print_ok(json!({
                "path": out.display().to_string(),
                "bytes": bytes.len(),
            }))
        }
        Cmd::Rmdoc { doc_id, out } => {
            let src = session
                .state_dir()
                .join("docs")
                .join(&doc_id)
                .join("pdf.pdf");
            let bytes = match std::fs::read(&src) {
                Ok(b) => b,
                Err(e) => output::print_err("io_error", e),
            };
            if let Err(e) = std::fs::write(&out, &bytes) {
                output::print_err("io_error", e);
            }
            output::print_ok(json!({
                "path": out.display().to_string(),
                "bytes": bytes.len(),
                "note": "rmdoc bundling deferred — pdf-only fallback",
            }))
        }
    }
}
