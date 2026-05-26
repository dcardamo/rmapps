//! Assemble and run the agenda app from configuration. Supports
//! `config`, `preview`, and `doctor` subcommands as framework facades.

use agenda::{update, view, App, AppConfig, Connectors};
use clap::{Parser, Subcommand};
use inkapp::{app, cli, ConfigStore, DeviceConfig, SecretStore};
use inkapp_config::store::select_instance;

#[derive(Parser)]
#[command(name = "agenda")]
struct Cli {
    /// Config instance to run (default: $INKAPP_INSTANCE or "default").
    #[arg(long, global = true)]
    instance: Option<String>,
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Manage configuration.
    #[command(subcommand)]
    Config(cli::ConfigCmd),
    /// Render the document set locally for browser preview.
    Preview(inkapp::cli::PreviewArgs),
    /// Run preflight checks (secrets, config, connectors, render).
    Doctor,
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
        Some(Cmd::Preview(args)) => {
            let mut application = build_app(&cfg_path, &instance);
            let code = inkapp::preview::run(&mut application, args)
                .await
                .expect("preview run");
            std::process::exit(code);
        }
        None => {
            // Default behavior preserved: publish to device.
            let store = ConfigStore::open(&cfg_path).expect("open config");
            let device: DeviceConfig = store.resolve(&instance).expect("resolve device config");
            let app_cfg: AppConfig = store.resolve(&instance).expect("resolve app config");
            let mut application = build_app(&cfg_path, &instance);
            let transport =
                inkapp::resolve_transport(&device.backend, app_cfg.device_folder.clone())
                    .expect("resolve device transport");
            inkapp::publish(&mut application, transport.as_ref())
                .await
                .expect("publish to device");
            println!(
                "agenda[{instance}]: published to {} ({})",
                app_cfg.device_folder, device.backend
            );
        }
    }
}

fn build_app(
    cfg_path: &std::path::Path,
    instance: &str,
) -> inkapp_core::runtime::App<App, agenda::Msg, Connectors> {
    let store = ConfigStore::open(cfg_path).expect("open config");
    let app_cfg: AppConfig = store.resolve(instance).expect("resolve app config");
    let page: inkapp_core::geometry::PageConfig =
        store.resolve(instance).expect("resolve page config");
    let mut secrets = SecretStore::open_default().expect("open secrets");
    let key = secrets.user_key().expect("user key");

    // Both calendar connectors are wired synchronously from config (no token needed).
    let connectors =
        Connectors::from_config(&store, &app_cfg).expect("wire connectors from config");

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
    let device_auth_name = device.backend.clone(); // "remarkable"

    // ICS and localcal connectors carry no credentials — skip secret() checks for both.
    // Build them if config allows; otherwise skip connector_refresh gracefully.
    let connectors_opt: Option<Connectors> =
        Connectors::from_config(&store, &app_cfg).ok();

    let secrets = SecretStore::open(secrets_path).expect("open secrets");
    let instance_owned = instance.to_string();

    let mut checklist = inkapp::doctor::Checklist::new(secrets_path)
        .user_key()
        // No connector credentials for ICS or localcal — skipped intentionally.
        .secret(Scope::DeviceAuth, device_auth_name.clone())
        .config_resolves::<AppConfig>(&store, instance, "app.agenda")
        .config_resolves::<inkapp_core::geometry::PageConfig>(&store, instance, "page")
        .config_resolves::<DeviceConfig>(&store, instance, "device");

    // Add a connector_refresh for each connector if wiring succeeded.
    if let Some(ref cx) = connectors_opt {
        checklist = checklist.connector_refresh("ics feed", cx.feed.clone());
        checklist = checklist.connector_refresh("localcal", cx.cal.clone());
    }

    // Render probe: guard against missing user_key so the SecretCheck failure
    // surfaces cleanly rather than a panic.
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
            let connectors = Connectors::from_config(&store, &app_cfg)
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
