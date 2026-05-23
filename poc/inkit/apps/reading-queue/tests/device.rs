//! Manual on-device bars. Requires a paired reMarkable and an authenticated
//! `rmapi`. Two steps, run as separate processes so inking happens out-of-band:
//!
//!   1. publish the queue to the device:
//!      nix develop -c cargo test -p reading-queue --test device -- --ignored --nocapture publish_to_device
//!   2. (on the tablet: open the docs, highlight a word, tick an Archive box, then SYNC)
//!   3. pull + fold + re-push:
//!      nix develop -c cargo test -p reading-queue --test device -- --ignored --nocapture sync_from_device
//!
//! State persists between the two runs via the gitignored overlay file
//! (`.overlay.json`); `sync_from_device` rebuilds the in-memory `DocSet` with a
//! deterministic `render()` before pulling. Honors rmapi v4/token/mkdir notes
//! (remarkable-pdf-mechanics.md §10).

use inkapp::{app, App as Framework, DocSet, Remarkable};
use reading_queue::serve::{publish, sync_once};
use reading_queue::{update, view, App, Connectors, Msg};

/// Gitignored overlay path so manual archives/highlights survive between the two
/// runs (and across days of on-device use).
const OVERLAY: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/.overlay.json");

/// Build the assembled app with the persisted (device-use) connector.
fn build_app() -> Framework<App, Msg, Connectors> {
    app(App)
        .connector(Connectors::persisted(OVERLAY))
        .update(update)
        .view(view)
        .build()
}

#[test]
#[ignore = "manual: requires a paired reMarkable + rmapi"]
fn publish_to_device() {
    let mut application = build_app();
    let mut set = DocSet::default();
    publish(&mut application, &mut set);
    eprintln!(
        "Published. On the tablet: open the docs under /ReadingQueue, highlight a word in one \
         article and tick the Archive box in another, then SYNC the device. Then run \
         `sync_from_device`."
    );
}

#[test]
#[ignore = "manual: requires a paired reMarkable + rmapi; run after inking + syncing the device"]
fn sync_from_device() {
    let device = Remarkable::new();
    let mut application = build_app();
    let mut set = DocSet::default();
    // Rebuild the DocSet from current (persisted) state — the deterministic view
    // reproduces exactly what was published, so pulled ink attributes correctly.
    application.render(&mut set).expect("render");
    sync_once(&mut application, &device, &mut set);
    eprintln!(
        "Synced. Archived articles are deleted; highlights are baked into the bodies on re-push."
    );
}
