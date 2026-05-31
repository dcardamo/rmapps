use clap::{Parser, Subcommand};

mod apps;
mod cmd;
mod output;
mod util;

#[derive(Parser)]
#[command(
    name = "inkctl",
    version,
    about = "Agent-drivable CLI for the inkapp test harness"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Top,
}

#[derive(Subcommand)]
enum Top {
    /// Session lifecycle (new, list, destroy, env, step)
    Session(cmd::session::Args),
    /// Virtual devices on a session
    Device(cmd::device::Args),
    /// Published documents
    Document(cmd::document::Args),
    /// Page-level observation (describe, snapshot, inspect, links)
    Page(cmd::page::Args),
    /// Ink synthesis (tap, swipe, fixture, draw, list) and link follow
    Ink(cmd::ink::Args),
    /// Command-trace recording and Rust-test emission
    Record(cmd::record::Args),
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    match cli.cmd {
        Top::Session(a) => cmd::session::run(a).await,
        Top::Device(a) => cmd::device::run(a).await,
        Top::Document(a) => cmd::document::run(a).await,
        Top::Page(a) => cmd::page::run(a).await,
        Top::Ink(a) => cmd::ink::run(a).await,
        Top::Record(a) => cmd::record::run(a).await,
    }
}
