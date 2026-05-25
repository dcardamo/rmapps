//! Manual on-device bars for the agenda app. Requires a paired reMarkable, an
//! authenticated `rmapi`, and a deploy config: set `INKAPP_DEPLOY_CONFIG` to a
//! `deploy.toml` with `backend = "remarkable"` and `folder = "/Agenda"`.
//!
//!   1. publish the agenda to the device:
//!      nix develop -c cargo test -p agenda --test device -- --ignored --nocapture publish_to_device
//!   2. (on the tablet: mark the cancel box on an event in the editable calendar, then SYNC)
//!   3. pull + fold + re-push:
//!      nix develop -c cargo test -p agenda --test device -- --ignored --nocapture sync_from_device
//!
//! State persists between the two runs via the gitignored local-calendar store
//! (`.localcal.json`).

use agenda::{update, view, App, Connectors, Msg};
use inkapp::{app, App as Framework, SecretStore};

/// Gitignored local-calendar store so manual cancels survive between the two runs.
const STORE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/.localcal.json");

/// Build the assembled app with the persisted (device-use) connectors.
fn build_app() -> Framework<App, Msg, Connectors> {
    let key = SecretStore::open_default()
        .and_then(|mut s| s.user_key())
        .expect("open secrets store / load user key");
    app(App)
        .connector(Connectors::persisted(STORE))
        .update(update)
        .view(view)
        .key(key)
        .build()
}

#[tokio::test]
#[ignore = "manual: requires a paired reMarkable + rmapi + INKAPP_DEPLOY_CONFIG"]
async fn publish_to_device() {
    let mut application = build_app();
    inkapp::publish(&mut application).await.expect("publish");
    eprintln!(
        "Published. On the tablet: open the agenda doc under /Agenda, mark the cancel box on an \
         event in the editable (lower) calendar, then SYNC the device. Then run `sync_from_device`."
    );
}

#[tokio::test]
#[ignore = "manual: requires a paired reMarkable + rmapi + INKAPP_DEPLOY_CONFIG; run after inking + syncing"]
async fn sync_from_device() {
    let mut application = build_app();
    inkapp::sync_once(&mut application).await.expect("sync");
    eprintln!("Synced. A cancelled event is reflected on the editable calendar on re-push.");
}
