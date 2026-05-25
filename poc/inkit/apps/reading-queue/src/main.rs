//! Assemble and run the reading-queue app from configuration. A `config`
//! subcommand exposes the config CLI; otherwise the selected instance is wired
//! from `config.toml` (+ `secrets.json`), its initial document set is rendered,
//! and the documents are deployed to the device resolved from `[device]`.

use clap::Parser;
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
    config: Option<cli::ConfigCmd>,
}

#[tokio::main]
async fn main() {
    let args = Cli::parse();
    let cfg_path = ConfigStore::default_path().expect("config path");

    // `config` subcommand: run the config CLI and exit before any wiring.
    if let Some(cmd) = args.config {
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

    // Persistent cache dir so warm-restart/offline reads survive across runs.
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

    // Resolve the device transport from `[device]` + the app's folder, then
    // deploy the rendered document set to the device.
    let transport = inkapp::resolve_transport(&device.backend, app_cfg.device_folder.clone())
        .expect("resolve device transport");
    inkapp::publish(&mut application, transport.as_ref())
        .await
        .expect("publish to device");
    println!(
        "reading-queue[{instance}]: published to {} ({})",
        app_cfg.device_folder, device.backend
    );
}
