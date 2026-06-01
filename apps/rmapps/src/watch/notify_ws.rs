//! Real push source: subscribes to the reMarkable notification websocket.
//!
//! ## Discovery notes (Task 7 spike — June 2026)
//!
//! The reMarkable notification protocol is undocumented. Findings, with sources and a
//! confidence verdict:
//!
//! - **Path: `/notifications/ws/json/1`** — CONFIRMED across multiple independent sources.
//!   rmfakecloud serves exactly this path for its notifications websocket, and rmapi/the
//!   reMarkable desktop client connect to it. The `json/1` suffix is the message-format +
//!   API version.
//!   - rmfakecloud (ddvk): <https://github.com/ddvk/rmfakecloud>
//!   - remsync protocol notes: <https://github.com/kinnison/remsync/blob/master/protocol.md>
//!   - akeil.de cloud API writeup: <https://akeil.de/posts/remarkable-cloud-api/>
//!
//! - **Auth: `Authorization: Bearer <user-token>`** — CONFIRMED. Same short-lived user JWT
//!   used for the sync API. The websocket upgrade request carries it as a header (set in
//!   `rm_cloud::Client::notifications_subscribe`).
//!
//! - **Host: `wss://internal.cloud.remarkable.com` — CONFIRMED** via a live handshake on
//!   2026-06-01: a websocket upgrade to `/notifications/ws/json/1` on the sync host, with the
//!   user-token bearer header, returned `HTTP/1.1 101 Switching Protocols`. Modern reMarkable
//!   sync (1.5+) serves notifications on the SAME host as the sync API. The OLD discovery
//!   mechanism described by the historical sources — an unauthenticated GET to
//!   `service-manager-production-dot-remarkable-production.appspot.com/service/json/1/notifications`
//!   returning a per-request `XXXX-notifications-production.cloud.remarkable.engineering`
//!   host — is RETIRED (now 404s). The host is overridable via `RM_CLOUD_HOST`. The gated
//!   live check is `apps/rmapps/tests/ws_live.rs` (run with `--ignored` and a device token).
//!
//! - **Message envelope** — irrelevant for us. ANY frame = "account may have changed" = one
//!   `Wakeup`. We never parse the payload, so envelope drift cannot break us.
//!
//! The poll source remains the backstop: if the host is wrong or the socket dies, the
//! resident loop's `recv_timeout` still drives reconciles on the safety-net cadence.

use crate::watch::notify::{NotificationSource, Wakeup};
use async_trait::async_trait;
use futures_util::StreamExt;
use rm_cloud::Client;
use std::time::Duration;

/// A [`NotificationSource`] backed by the reMarkable notification websocket. Owns its
/// connection, reconnecting with exponential backoff; `next_wakeup` never surfaces an error.
pub struct WsSource {
    client: Client,
    stream: Option<rm_cloud::NotifyStream>,
    backoff: Duration,
}

impl WsSource {
    /// `Client` is cheap to clone (Arc'd creds + a `reqwest::Client` handle), so we take it
    /// by value rather than wrapping in another `Arc`.
    pub fn new(client: Client) -> Self {
        Self {
            client,
            stream: None,
            backoff: Duration::from_secs(1),
        }
    }

    /// Block until a websocket connection is established, retrying with exponential backoff
    /// (capped at 60s). Resets the backoff on success.
    async fn ensure_connected(&mut self) {
        while self.stream.is_none() {
            match self.client.notifications_subscribe().await {
                Ok(s) => {
                    self.stream = Some(s);
                    self.backoff = Duration::from_secs(1);
                }
                Err(e) => {
                    eprintln!(
                        "[rmapps] watch: ws connect failed: {e}; retrying in {:?}",
                        self.backoff
                    );
                    tokio::time::sleep(self.backoff).await;
                    self.backoff = (self.backoff * 2).min(Duration::from_secs(60));
                }
            }
        }
    }
}

#[async_trait]
impl NotificationSource for WsSource {
    async fn next_wakeup(&mut self) -> Wakeup {
        loop {
            self.ensure_connected().await;
            // `unwrap` is safe: ensure_connected guarantees `stream` is Some.
            match self.stream.as_mut().unwrap().next().await {
                // Any frame means the account may have changed → one wakeup.
                Some(Ok(_msg)) => return Wakeup,
                Some(Err(e)) => {
                    eprintln!("[rmapps] watch: ws error: {e}; reconnecting");
                    self.stream = None;
                }
                // Server closed the stream; reconnect.
                None => {
                    self.stream = None;
                }
            }
        }
    }
}
