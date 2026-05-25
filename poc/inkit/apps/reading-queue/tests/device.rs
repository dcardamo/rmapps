//! Manual on-device bars. Requires a paired reMarkable, an authenticated `rmapi`,
//! and a deploy config: set `INKAPP_DEPLOY_CONFIG` to a `deploy.toml` with
//! `backend = "remarkable"` and `folder = "/ReadingQueue"`. Two steps, run as
//! separate processes so inking happens out-of-band:
//!
//!   1. publish the queue to the device:
//!      nix develop -c cargo test -p reading-queue --test device -- --ignored --nocapture publish_to_device
//!   2. (on the tablet: open the docs, highlight a word, tick an Archive box, then SYNC)
//!   3. pull + fold + re-push:
//!      nix develop -c cargo test -p reading-queue --test device -- --ignored --nocapture sync_from_device
//!
//! State persists between the two runs via the gitignored overlay file
//! (`.overlay.json`). Honors rmapi v4/token/mkdir notes (remarkable-pdf-mechanics.md §10).

use inkapp::{app, App as Framework, SecretStore};
use reading_queue::{update, view, App, Connectors, Msg};

/// Gitignored overlay path so manual archives/highlights survive between the two runs.
const OVERLAY: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/.overlay.json");

/// Build the assembled app with the persisted (device-use) connector.
fn build_app() -> Framework<App, Msg, Connectors> {
    let key = SecretStore::open_default()
        .and_then(|mut s| s.user_key())
        .expect("open secrets store / load user key");
    app(App)
        .connector(Connectors::persisted(OVERLAY))
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
        "Published. On the tablet: open the docs under /ReadingQueue, highlight a word in one \
         article and tick the Archive box in another, then SYNC the device. Then run \
         `sync_from_device`."
    );
}

#[tokio::test]
#[ignore = "manual: requires a paired reMarkable + rmapi + INKAPP_DEPLOY_CONFIG; run after inking + syncing"]
async fn sync_from_device() {
    let mut application = build_app();
    inkapp::sync_once(&mut application).await.expect("sync");
    eprintln!(
        "Synced. Archived articles are deleted; highlights are baked into the bodies on re-push."
    );
}
