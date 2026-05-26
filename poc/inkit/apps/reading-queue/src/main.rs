//! Assemble and run the reading-queue app from configuration. Subcommands:
//! `config`, `preview`, `doctor`, `sync` (one-shot sync_once), `run` (publish +
//! serve loop). With no subcommand, performs a one-shot publish.

use std::time::Duration;

use clap::{Parser, Subcommand};
use inkapp::{app, cli, ConfigStore, DeviceConfig, SecretStore};
use inkapp_config::store::select_instance;
use reading_queue::{update, view, App, AppConfig, Connectors};
use std::sync::Arc;

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
    /// Render the document set locally for browser preview.
    Preview(inkapp::cli::PreviewArgs),
    /// Run preflight checks (secrets, config, connectors, render).
    Doctor,
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
    let secrets_path = SecretStore::default_path().expect("secrets path");
    let instance = select_instance(args.instance.as_deref());

    match args.cmd {
        Some(Cmd::Config(c)) => {
            let code = cli::run(c, cfg_path).expect("config command");
            std::process::exit(code);
        }
        Some(Cmd::Doctor) => {
            let code = run_doctor(&cfg_path, &secrets_path, &instance).await;
            std::process::exit(code);
        }
        Some(Cmd::Preview(p_args)) => {
            let mut application = build_app(&cfg_path, &instance).await;
            let code = inkapp::preview::run(&mut application, p_args)
                .await
                .expect("preview run");
            std::process::exit(code);
        }
        Some(Cmd::Sync) => {
            let store = ConfigStore::open(&cfg_path).expect("open config");
            let device: DeviceConfig = store.resolve(&instance).expect("resolve device config");
            let app_cfg: AppConfig = store.resolve(&instance).expect("resolve app config");
            let mut application = build_app(&cfg_path, &instance).await;
            let transport =
                inkapp::resolve_transport(&device.backend, app_cfg.device_folder.clone())
                    .expect("resolve device transport");
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
            let store = ConfigStore::open(&cfg_path).expect("open config");
            let device: DeviceConfig = store.resolve(&instance).expect("resolve device config");
            let app_cfg: AppConfig = store.resolve(&instance).expect("resolve app config");
            let secs = interval.unwrap_or(device.sync_interval_secs);
            let mut application = build_app(&cfg_path, &instance).await;
            let transport =
                inkapp::resolve_transport(&device.backend, app_cfg.device_folder.clone())
                    .expect("resolve device transport");
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
            // Default behavior preserved: publish to device.
            let store = ConfigStore::open(&cfg_path).expect("open config");
            let device: DeviceConfig = store.resolve(&instance).expect("resolve device config");
            let app_cfg: AppConfig = store.resolve(&instance).expect("resolve app config");
            let mut application = build_app(&cfg_path, &instance).await;
            let transport =
                inkapp::resolve_transport(&device.backend, app_cfg.device_folder.clone())
                    .expect("resolve device transport");
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

async fn build_app(
    cfg_path: &std::path::Path,
    instance: &str,
) -> inkapp_core::runtime::App<App, reading_queue::Msg, Connectors> {
    let store = ConfigStore::open(cfg_path).expect("open config");
    let app_cfg: AppConfig = store.resolve(instance).expect("resolve app config");
    let page: inkapp_core::geometry::PageConfig =
        store.resolve(instance).expect("resolve page config");
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

    app(App)
        .connector(connectors)
        .update(update)
        .view(view)
        .key(key)
        .page(page.into())
        .build()
}

async fn run_doctor(
    cfg_path: &std::path::Path,
    secrets_path: &std::path::Path,
    instance: &str,
) -> i32 {
    use inkapp::Scope;
    let store = match ConfigStore::open(cfg_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("doctor: cannot open config at {}: {e}", cfg_path.display());
            return 1;
        }
    };
    let app_cfg: AppConfig = store.resolve(instance).unwrap_or_default();
    let device: DeviceConfig = store.resolve(instance).unwrap_or_default();
    let connector_token_name = app_cfg.readwise.kind.clone(); // "readwise"
    let device_auth_name = device.backend.clone(); // "remarkable"

    // Build the connector from config IF possible; otherwise skip the
    // connector_refresh check (graceful fallback when secrets missing).
    let cache_dir = std::env::temp_dir().join("inkapp-doctor").join(instance);
    let secrets = SecretStore::open(secrets_path).expect("open secrets");
    let readwise_arc: Option<Arc<dyn inkapp::Connector>> =
        match Connectors::from_config(&store, &app_cfg, &secrets, cache_dir.clone()).await {
            Ok(cx) => Some(cx.readwise.clone()),
            Err(_) => None,
        };

    let instance_owned = instance.to_string();
    let mut checklist = inkapp::doctor::Checklist::new(secrets_path)
        .user_key()
        .secret(Scope::ConnectorCred, connector_token_name.clone())
        .secret(Scope::DeviceAuth, device_auth_name.clone())
        .config_resolves::<AppConfig>(&store, instance, "app.reading-queue")
        .config_resolves::<inkapp_core::geometry::PageConfig>(&store, instance, "page")
        .config_resolves::<DeviceConfig>(&store, instance, "device");

    if let Some(rw) = readwise_arc {
        checklist = checklist.connector_refresh(&connector_token_name, rw);
    }

    // Render probe builds the app the normal way. Guard against a missing
    // user_key so the SecretCheck failure surfaces cleanly rather than a panic.
    if secrets
        .get(Scope::UserKey, "default")
        .ok()
        .flatten()
        .is_some()
    {
        let cfg_path = cfg_path.to_path_buf();
        let instance = instance_owned.clone();
        checklist = checklist.render_probe(move || async move {
            let store = ConfigStore::open(&cfg_path)
                .map_err(|e| inkapp_core::error::Error::Config(e.to_string()))?;
            let app_cfg: AppConfig = store
                .resolve(&instance)
                .map_err(|e| inkapp_core::error::Error::Config(e.to_string()))?;
            let page: inkapp_core::geometry::PageConfig = store
                .resolve(&instance)
                .map_err(|e| inkapp_core::error::Error::Config(e.to_string()))?;
            let mut secrets = SecretStore::open_default()?;
            let key = secrets.user_key()?;
            let cache_dir = std::env::temp_dir()
                .join("inkapp-doctor-probe")
                .join(&instance);
            let connectors = Connectors::from_config(&store, &app_cfg, &secrets, cache_dir)
                .await
                .map_err(|e| inkapp_core::error::Error::Config(e.to_string()))?;
            Ok(app(App)
                .connector(connectors)
                .update(update)
                .view(view)
                .key(key)
                .page(page.into())
                .build())
        });
    }

    checklist.run().await
}
