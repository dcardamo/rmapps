use clap::Subcommand;
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
    /// Start recording a command trace
    Start,
    /// Stop recording
    Stop,
    /// Add an assertion to the trace
    Assert {
        target: String,
        expected_json: String,
    },
    /// Replay a recorded trace
    Replay { trace_path: PathBuf },
    /// Emit a Rust #[test] from a trace
    EmitTest {
        #[arg(long)]
        from: PathBuf,
        #[arg(long)]
        name: String,
        #[arg(long)]
        out: PathBuf,
    },
}

pub async fn run(args: Args) -> ! {
    match args.cmd {
        Cmd::Replay { .. } => output::print_err(
            "not_implemented",
            "record replay requires re-dispatching CLI subcommands — deferred",
        ),
        Cmd::EmitTest { from, name, out } => emit_test(from, name, out),
        Cmd::Start => {
            with_session(args.session, |s| {
                s.record_start().map_err(|e| ("io_error", e.to_string()))?;
                Ok(json!({ "recording": true }))
            })
            .await
        }
        Cmd::Stop => {
            with_session(args.session, |s| {
                s.record_stop().map_err(|e| ("io_error", e.to_string()))?;
                Ok(json!({ "recording": false }))
            })
            .await
        }
        Cmd::Assert {
            target,
            expected_json,
        } => {
            with_session(args.session, move |s| {
                let v: serde_json::Value = serde_json::from_str(&expected_json)
                    .map_err(|e| ("bad_args", format!("expected_json: {e}")))?;
                s.record_assert(&target, v)
                    .map_err(|e| ("io_error", e.to_string()))?;
                Ok(json!({ "asserted": true }))
            })
            .await
        }
    }
}

async fn with_session<F>(session: Option<String>, f: F) -> !
where
    F: FnOnce(&mut Session) -> Result<serde_json::Value, (&'static str, String)>,
{
    let session_id = match util::resolve_session_id(session) {
        Ok(s) => s,
        Err(e) => output::print_err("missing_session", e),
    };
    let dir = util::session_dir(&session_id);
    let mut s = match Session::open(&dir).await {
        Ok(s) => s,
        Err(e) => output::print_err("io_error", e),
    };
    let result = f(&mut s);
    let _ = s.flush();
    match result {
        Ok(v) => output::print_ok(v),
        Err((kind, msg)) => output::print_err(kind, msg),
    }
}

fn emit_test(from: PathBuf, name: String, out: PathBuf) -> ! {
    let entries = match inkapp_harness::trace::read_trace(&from) {
        Ok(e) => e,
        Err(e) => output::print_err("io_error", e),
    };
    let source = inkapp_harness::emit::to_rust(&entries, &name);
    if let Err(e) = std::fs::write(&out, source.as_bytes()) {
        output::print_err("io_error", e);
    }
    output::print_ok(json!({
        "out": out.display().to_string(),
        "name": name,
    }))
}
