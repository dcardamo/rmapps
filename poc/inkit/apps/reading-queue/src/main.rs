//! Assemble and run the reading-queue app from configuration. Subcommands:
//! `config` (config CLI), `sync` (one-shot sync_once), `run` (publish + serve
//! loop). With no subcommand, performs a one-shot publish (today's behaviour).

use std::time::Duration;

use clap::{Parser, Subcommand};
use inkapp::{app, cli, ConfigStore, DeviceConfig, SecretStore};
use inkapp_config::store::select_instance;
use reading_queue::{update, view, App, AppConfig, Connectors};

#[derive(Parser)]
#[command(name = "reading-queue")]
struct Cli {
    /// Config instance to run (default: $INKAPP_INSTANCE or "default").
    #[arg(long, global = true)]
    instance: Option<String>,
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Configuration management (instances, secrets, connectors).
    #[command(subcommand)]
    Config(cli::ConfigCmd),
    /// Publish the document set, then loop sync_once forever (Ctrl-C exits).
    Run {
        /// Override the configured `sync_interval_secs` for this run.
        #[arg(long)]
        interval: Option<u64>,
    },
    /// One-shot pull + fold + push.
    Sync,
}

#[tokio::main]
async fn main() {
    let args = Cli::parse();
    let cfg_path = ConfigStore::default_path().expect("config path");

    // `config` subcommand: run the config CLI and exit before any wiring.
    if let Some(Cmd::Config(cmd)) = args.cmd {
        let code = cli::run(cmd, cfg_path).expect("config command");
        std::process::exit(code);
    }

    let instance = select_instance(args.instance.as_deref());
    let store = ConfigStore::open(&cfg_path).expect("open config");
    let app_cfg: AppConfig = store.resolve(&instance).expect("resolve app config");
    let page: inkapp_core::geometry::PageConfig =
        store.resolve(&instance).expect("resolve page config");
    let device: DeviceConfig = store.resolve(&instance).expect("resolve device config");

    let mut secrets = SecretStore::open_default().expect("open secrets");
    let key = secrets.user_key().expect("user key");

    let cache_dir = std::env::var_os("XDG_CACHE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".cache")))
        .unwrap_or_else(std::env::temp_dir)
        .join("inkapp")
        .join(format!("reading-queue-{instance}"));

    let connectors = Connectors::from_config(&store, &app_cfg, &secrets, cache_dir)
        .await
        .expect("wire connectors from config");

    let mut application = app(App)
        .connector(connectors)
        .update(update)
        .view(view)
        .key(key)
        .page(page.into())
        .build();

    let transport = inkapp::resolve_transport(&device.backend, app_cfg.device_folder.clone())
        .expect("resolve device transport");

    match args.cmd {
        Some(Cmd::Config(_)) => unreachable!("handled above"),
        Some(Cmd::Sync) => {
            let cycle = inkapp::sync_once(&mut application, transport.as_ref())
                .await
                .expect("sync_once");
            println!(
                "reading-queue[{instance}]: synced {} msg(s), {} op(s)",
                cycle.decoded.len(),
                cycle.ops.len()
            );
        }
        Some(Cmd::Run { interval }) => {
            let secs = interval.unwrap_or(device.sync_interval_secs);
            println!(
                "reading-queue[{instance}]: serving every {secs}s on {} ({})",
                app_cfg.device_folder, device.backend
            );
            let shutdown = async {
                let _ = tokio::signal::ctrl_c().await;
            };
            inkapp::serve(
                &mut application,
                transport.as_ref(),
                Duration::from_secs(secs),
                shutdown,
            )
            .await
            .expect("serve loop");
        }
        None => {
            inkapp::publish(&mut application, transport.as_ref())
                .await
                .expect("publish to device");
            println!(
                "reading-queue[{instance}]: published to {} ({})",
                app_cfg.device_folder, device.backend
            );
        }
    }
}
