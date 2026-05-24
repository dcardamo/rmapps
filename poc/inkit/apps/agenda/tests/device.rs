//! Manual on-device bars for the agenda app. Requires a paired reMarkable and an
//! authenticated `rmapi`. Mirrors reading-queue's device bar (the framework owns
//! the loop body; this exercises the app's `serve` transport against a device).
//!
//!   1. publish the agenda to the device:
//!      nix develop -c cargo test -p agenda --test device -- --ignored --nocapture publish_to_device
//!   2. (on the tablet: open the agenda doc, mark the cancel box on an event in
//!      the editable calendar, then SYNC)
//!   3. pull + fold + re-push:
//!      nix develop -c cargo test -p agenda --test device -- --ignored --nocapture sync_from_device
//!
//! State persists between the two runs via the gitignored local-calendar store
//! (`.localcal.json`); `sync_from_device` rebuilds the in-memory `DocSet` with a
//! deterministic `render()` before pulling.

use agenda::serve::{publish, sync_once};
use agenda::{update, view, App, Connectors, Msg};
use inkapp::{app, App as Framework, DocSet, Remarkable, SecretStore};

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
#[ignore = "manual: requires a paired reMarkable + rmapi"]
async fn publish_to_device() {
    let mut application = build_app();
    let mut set = DocSet::default();
    publish(&mut application, &mut set).await;
    eprintln!(
        "Published. On the tablet: open the agenda doc under /Agenda, mark the cancel box on an \
         event in the editable (lower) calendar, then SYNC the device. Then run `sync_from_device`."
    );
}

#[tokio::test]
#[ignore = "manual: requires a paired reMarkable + rmapi; run after inking + syncing the device"]
async fn sync_from_device() {
    let device = Remarkable::new();
    let mut application = build_app();
    let mut set = DocSet::default();
    // Rebuild the DocSet from current (persisted) state — the deterministic view
    // reproduces exactly what was published, so pulled ink attributes correctly.
    application.render(&mut set).await.expect("render");
    sync_once(&mut application, &device, &mut set).await;
    eprintln!("Synced. A cancelled event is reflected on the editable calendar on re-push.");
}
