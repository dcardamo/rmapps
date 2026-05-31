use clap::Subcommand;
use inkapp_harness::session::{DeviceId, Session};
use serde_json::json;

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
    /// Create a new virtual device on a session
    New {
        #[arg(long)]
        name: Option<String>,
    },
    /// List virtual devices
    List,
    /// Show the device filesystem tree
    Tree { id: String },
    /// Sync the device with the framework
    Sync { id: String },
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
        Cmd::New { name } => {
            let dev = match session.device_new(name.as_deref()) {
                Ok(d) => d,
                Err(e) => output::print_err("io_error", e),
            };
            if let Err(e) = session.flush() {
                output::print_err("io_error", e);
            }
            output::print_ok(json!({ "device_id": dev.as_str() }))
        }
        Cmd::List => {
            let devices = match session.device_list() {
                Ok(d) => d,
                Err(e) => output::print_err("io_error", e),
            };
            output::print_ok(json!({ "devices": devices }))
        }
        Cmd::Tree { id } => {
            let tree = match inkapp_harness::observe::device_tree(&session, &id, "/").await {
                Ok(t) => t,
                Err(e) => output::print_err("io_error", e),
            };
            output::print_ok(tree)
        }
        Cmd::Sync { id } => {
            let res = match session.device_sync(&DeviceId::new(id)).await {
                Ok(r) => r,
                Err(e) => output::print_err("io_error", e),
            };
            if let Err(e) = session.flush() {
                output::print_err("io_error", e);
            }
            output::print_ok(res)
        }
    }
}
