use clap::Subcommand;
use serde_json::json;

use crate::output;
use crate::util;

#[derive(clap::Args)]
pub struct Args {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Create a new session
    New {
        #[arg(long)]
        name: Option<String>,
    },
    /// List sessions
    List,
    /// Destroy a session
    Destroy { id: String },
    /// Print INKCTL_SESSION export line for eval
    Env { id: String },
    /// Drive the app loop one cycle on the session's device
    Step {
        #[arg(long)]
        device: String,
        #[arg(long)]
        session: Option<String>,
    },
}

pub async fn run(args: Args) -> ! {
    match args.cmd {
        Cmd::New { name } => new(name).await,
        Cmd::List => list(),
        Cmd::Destroy { id } => destroy(id),
        Cmd::Env { id } => env_cmd(id),
        Cmd::Step { device, session } => step(device, session).await,
    }
}

async fn new(name: Option<String>) -> ! {
    let id = uuid::Uuid::new_v4().to_string();
    let dir = util::session_dir(&id);
    let session = match inkapp_harness::session::Session::new_fake(&dir).await {
        Ok(s) => s,
        Err(e) => output::print_err("io_error", e),
    };
    if let Err(e) = session.flush() {
        output::print_err("io_error", e);
    }
    output::print_ok(json!({
        "session_id": id,
        "backend": session.backend(),
        "path": dir.display().to_string(),
        "name": name,
    }))
}

fn list() -> ! {
    let home = util::home_dir();
    let mut sessions: Vec<serde_json::Value> = Vec::new();
    if home.exists() {
        let entries = match std::fs::read_dir(&home) {
            Ok(e) => e,
            Err(e) => output::print_err("io_error", e),
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let session_json = path.join("session.json");
            if !session_json.exists() {
                continue;
            }
            let bytes = match std::fs::read(&session_json) {
                Ok(b) => b,
                Err(_) => continue,
            };
            let val: serde_json::Value = match serde_json::from_slice(&bytes) {
                Ok(v) => v,
                Err(_) => continue,
            };
            sessions.push(json!({
                "id": entry.file_name().to_string_lossy(),
                "backend": val.get("backend"),
                "created_at": val.get("created_at"),
                "path": path.display().to_string(),
            }));
        }
    }
    output::print_ok(json!({ "sessions": sessions }))
}

fn destroy(id: String) -> ! {
    let dir = util::session_dir(&id);
    match inkapp_harness::session::Session::destroy(&dir) {
        Ok(()) => output::print_ok(json!({ "destroyed": id })),
        Err(e) => output::print_err("io_error", e),
    }
}

fn env_cmd(id: String) -> ! {
    // Plain shell-eval format; no JSON envelope.
    println!("INKCTL_SESSION={}", id);
    std::process::exit(0);
}

async fn step(_device: String, _session: Option<String>) -> ! {
    output::print_err(
        "not_implemented",
        "session step requires App registry — wired by user-side code",
    )
}
