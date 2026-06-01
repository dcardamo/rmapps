//! Gated, READ-ONLY live check of the notification websocket handshake (Task 7, Step 5).
//!
//! Subscribing to notifications makes no changes to the cloud — it only opens a receive-only
//! socket. The test is ignored by default; run it explicitly with a real device token:
//!
//! ```sh
//! RM_CLOUD_DEVICE_TOKEN="$(jq -r .device_token ~/.config/rmapps/auth.json)" \
//!   cargo test -p rmapps --test ws_live -- --ignored --nocapture
//! ```
//!
//! Optionally override the host: `RM_CLOUD_HOST=wss://<host>` (no path).
//!
//! Outcome is REPORTED, not asserted hard: a failed handshake means the shipped candidate
//! host is wrong/needs discovery — the daemon still works via the poll fallback — so we
//! print the error and fail with a clear message rather than masking it.

use rm_cloud::{Client, Config};

#[tokio::test]
#[ignore = "hits the live reMarkable notification websocket; needs RM_CLOUD_DEVICE_TOKEN"]
async fn ws_handshake_live() {
    let token = std::env::var("RM_CLOUD_DEVICE_TOKEN")
        .expect("set RM_CLOUD_DEVICE_TOKEN to run this live test");
    let client = Client::from_device_token(Config::from_env(), token);

    match client.notifications_subscribe().await {
        Ok(_stream) => {
            eprintln!("[ws_live] handshake OK — notification websocket connected");
            // Receive-only; drop immediately. No writes performed.
        }
        Err(e) => {
            panic!(
                "[ws_live] handshake FAILED: {e}\n\
                 The shipped notifications host is UNCONFIRMED and may require dynamic \
                 service-manager discovery. The daemon still functions via the poll fallback."
            );
        }
    }
}
