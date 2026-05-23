//! Manual on-device round-trip. Requires a paired reMarkable and `rmapi`.
//!
//! Run:
//!   nix develop -c cargo test -p reading-queue --test device -- --ignored on_device_round_trip
//!
//! It publishes the queue, waits for you to ink+sync on the tablet, then syncs
//! once (pull -> step -> apply). Honors rmapi v4/token/mkdir notes
//! (remarkable-pdf-mechanics.md §10).

use inkapp::{app, DocSet, Remarkable};
use reading_queue::serve::{publish, sync_once};
use reading_queue::{update, view, App, Connectors};

#[test]
#[ignore = "manual: requires a paired reMarkable + rmapi"]
fn on_device_round_trip() {
    let device = Remarkable::new();
    let mut application = app(App)
        .connector(Connectors::persisted(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/.overlay.json"
        )))
        .update(update)
        .view(view)
        .build();
    let mut set = DocSet::default();

    publish(&mut application, &mut set);
    eprintln!("Ink on the device (highlight an article, check an Archive box), then SYNC.");
    eprintln!("Press Enter here when the device has synced…");
    let mut _line = String::new();
    std::io::stdin().read_line(&mut _line).unwrap();

    sync_once(&mut application, &device, &mut set);
    eprintln!("Re-published. Archived articles are gone; highlights are baked into the bodies.");
}
