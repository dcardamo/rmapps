use clap::Subcommand;
use inkapp_harness::inspector::{InspectOpts, ShowFlags};
use inkapp_harness::session::Session;
use serde_json::json;
use std::path::PathBuf;

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
    /// Describe a page (regions, manifest)
    Describe { doc_id: String, page: usize },
    /// Render a page snapshot to PNG
    Snapshot {
        doc_id: String,
        page: usize,
        #[arg(long)]
        out: PathBuf,
    },
    /// Inspect a page with layer + link overlays
    Inspect {
        doc_id: String,
        page: usize,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        layers: Option<String>,
        #[arg(long)]
        show: Option<String>,
    },
    /// List links on a page
    Links { doc_id: String, page: usize },
}

pub async fn run(args: Args) -> ! {
    let session_id = match util::resolve_session_id(args.session) {
        Ok(s) => s,
        Err(e) => output::print_err("missing_session", e),
    };
    let dir = util::session_dir(&session_id);
    let session = match Session::open(&dir).await {
        Ok(s) => s,
        Err(e) => output::print_err("io_error", e),
    };

    match args.cmd {
        Cmd::Describe { doc_id, page } => {
            let desc = match inkapp_harness::observe::page_describe(&session, &doc_id, page) {
                Ok(d) => d,
                Err(e) => output::print_err("io_error", e),
            };
            output::print_ok(desc)
        }
        Cmd::Snapshot { doc_id, page, out } => {
            let png = match inkapp_harness::observe::page_snapshot(&session, &doc_id, page) {
                Ok(p) => p,
                Err(e) => output::print_err("io_error", e),
            };
            if let Err(e) = std::fs::write(&out, &png) {
                output::print_err("io_error", e);
            }
            output::print_ok(json!({
                "path": out.display().to_string(),
                "bytes": png.len(),
            }))
        }
        Cmd::Inspect {
            doc_id,
            page,
            out,
            layers,
            show,
        } => {
            let opts = build_inspect_opts(layers, show);
            let png = match inkapp_harness::observe::page_inspect(&session, &doc_id, page, &opts) {
                Ok(p) => p,
                Err(e) => output::print_err("io_error", e),
            };
            if let Err(e) = std::fs::write(&out, &png) {
                output::print_err("io_error", e);
            }
            output::print_ok(json!({
                "path": out.display().to_string(),
                "bytes": png.len(),
            }))
        }
        Cmd::Links { doc_id, page } => {
            let desc = match inkapp_harness::observe::page_describe(&session, &doc_id, page) {
                Ok(d) => d,
                Err(e) => output::print_err("io_error", e),
            };
            output::print_ok(json!({ "links": desc.links }))
        }
    }
}

fn build_inspect_opts(layers: Option<String>, show: Option<String>) -> InspectOpts {
    let layers = layers.map(|s| {
        s.split(',')
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
            .collect::<Vec<_>>()
    });
    let show = match show {
        None => ShowFlags::default(),
        Some(s) => {
            let items: Vec<&str> = s.split(',').map(|x| x.trim()).collect();
            ShowFlags {
                regions: items.contains(&"regions"),
                links: items.contains(&"links"),
                synth_strokes: items.contains(&"strokes"),
                attributed_strokes: items.contains(&"attributed"),
            }
        }
    };
    InspectOpts { layers, show }
}
