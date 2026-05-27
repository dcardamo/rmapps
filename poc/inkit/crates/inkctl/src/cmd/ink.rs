use clap::Subcommand;
use inkapp_core::geometry::PdfPoint;
use inkapp_harness::observe::ObserveGroup;
use inkapp_harness::session::{DeviceId, Session};
use serde_json::json;

use crate::output;
use crate::util;

#[derive(clap::Args)]
pub struct Args {
    #[arg(long, global = true)]
    session: Option<String>,
    #[arg(long, global = true)]
    device: Option<String>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Synthesize a tap on a region
    Tap {
        doc_id: String,
        page: usize,
        region: String,
    },
    /// Synthesize a swipe
    Swipe {
        doc_id: String,
        page: usize,
        region: String,
    },
    /// Apply an ink fixture
    Fixture {
        doc_id: String,
        page: usize,
        region: String,
        fixture: String,
    },
    /// Draw raw strokes
    Draw {
        doc_id: String,
        page: usize,
        #[arg(long)]
        path: String,
        #[arg(long)]
        highlighter: bool,
    },
    /// List ink on a page
    List {
        doc_id: String,
        page: usize,
        #[arg(long, conflicts_with = "by_region")]
        by_layer: bool,
        #[arg(long)]
        by_region: bool,
    },
    /// Follow a link in a region
    Follow {
        doc_id: String,
        page: usize,
        region: String,
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

    // Commands that mutate need a device id; observation does not.
    let need_device = !matches!(args.cmd, Cmd::List { .. });
    let device_id = if need_device {
        match args.device {
            Some(d) => Some(DeviceId::new(d)),
            None => output::print_err("bad_args", "--device required for this subcommand"),
        }
    } else {
        None
    };

    match args.cmd {
        Cmd::Tap {
            doc_id,
            page,
            region,
        } => {
            if let Err(e) = session.ink_tap(device_id.as_ref().unwrap(), &doc_id, page, &region) {
                output::print_err("io_error", e);
            }
            let _ = session.flush();
            output::print_ok(json!({ "ok": true }))
        }
        Cmd::Swipe {
            doc_id,
            page,
            region,
        } => {
            if let Err(e) = session.ink_swipe(device_id.as_ref().unwrap(), &doc_id, page, &region) {
                output::print_err("io_error", e);
            }
            let _ = session.flush();
            output::print_ok(json!({ "ok": true }))
        }
        Cmd::Fixture {
            doc_id,
            page,
            region,
            fixture,
        } => {
            if let Err(e) = session.ink_fixture(
                device_id.as_ref().unwrap(),
                &doc_id,
                page,
                &region,
                &fixture,
            ) {
                output::print_err("io_error", e);
            }
            let _ = session.flush();
            output::print_ok(json!({ "ok": true }))
        }
        Cmd::Draw {
            doc_id,
            page,
            path,
            highlighter,
        } => {
            let points = match parse_path(&path) {
                Ok(p) => p,
                Err(e) => output::print_err("bad_args", e),
            };
            if let Err(e) = session.ink_draw(
                device_id.as_ref().unwrap(),
                &doc_id,
                page,
                &points,
                highlighter,
            ) {
                output::print_err("io_error", e);
            }
            let _ = session.flush();
            output::print_ok(json!({ "points": points.len(), "highlighter": highlighter }))
        }
        Cmd::List {
            doc_id,
            page,
            by_layer,
            by_region,
        } => {
            let group = if by_layer {
                ObserveGroup::ByLayer
            } else if by_region {
                ObserveGroup::ByRegion
            } else {
                ObserveGroup::Flat
            };
            let list = match inkapp_harness::observe::ink_list(&session, &doc_id, page, group) {
                Ok(l) => l,
                Err(e) => output::print_err("io_error", e),
            };
            output::print_ok(list)
        }
        Cmd::Follow {
            doc_id,
            page,
            region,
        } => {
            let res = match session.link_follow(device_id.as_ref().unwrap(), &doc_id, page, &region)
            {
                Ok(r) => r,
                Err(e) => output::print_err("io_error", e),
            };
            let _ = session.flush();
            output::print_ok(res)
        }
    }
}

fn parse_path(s: &str) -> Result<Vec<PdfPoint>, String> {
    let mut out = Vec::new();
    for tok in s.split_whitespace() {
        let mut parts = tok.split(',');
        let xs = parts.next().ok_or_else(|| format!("bad token: {tok}"))?;
        let ys = parts.next().ok_or_else(|| format!("bad token: {tok}"))?;
        let x: f64 = xs.parse().map_err(|e| format!("bad x in {tok}: {e}"))?;
        let y: f64 = ys.parse().map_err(|e| format!("bad y in {tok}: {e}"))?;
        out.push(PdfPoint { x, y });
    }
    Ok(out)
}
