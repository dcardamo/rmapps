//! Endpoint configuration, with env overrides for pointing tests at a fake.

/// Resolved base URLs for the reMarkable cloud surfaces (auth + sync + notifications).
#[derive(Debug, Clone)]
pub struct Config {
    /// Auth host (token endpoints).
    pub auth: String,
    /// Sync host (root + blob endpoints).
    pub sync: String,
    /// Notifications host (websocket push). See [`Config::notifications_ws`].
    ///
    /// CONFIRMED (live handshake, June 2026): modern reMarkable sync (1.5+) serves the
    /// notifications websocket on the SAME host as the sync API,
    /// `wss://internal.cloud.remarkable.com/notifications/ws/json/1`. The legacy appspot
    /// `service-manager` discovery (which returned per-request load-balanced
    /// `XXXX-notifications-production.cloud.remarkable.engineering` hosts) is retired and
    /// now 404s. Overridable via `RM_CLOUD_HOST` for fakes/proxies.
    pub notifications: String,
}

impl Config {
    /// Production defaults.
    pub fn production() -> Self {
        Self {
            auth: "https://webapp-prod.cloud.remarkable.engineering".into(),
            sync: "https://internal.cloud.remarkable.com".into(),
            // CONFIRMED via live 101 Switching Protocols handshake (see field doc): the
            // notifications websocket lives on the same host as the sync API.
            notifications: "wss://internal.cloud.remarkable.com".into(),
        }
    }

    /// Production defaults, overridden by `RM_CLOUD_HOST` (which sets all hosts to
    /// the same base — used to point the client at the fake cloud or a proxy).
    ///
    /// When overridden the notifications base reuses the same host but swaps the scheme to
    /// `ws://`/`wss://` so a fake ws server (out of scope here) can be addressed. A
    /// `ws`/`wss` override is passed through verbatim.
    pub fn from_env() -> Self {
        match std::env::var("RM_CLOUD_HOST") {
            Ok(h) if !h.is_empty() => {
                let notifications = ws_base_from_http(&h);
                Self {
                    auth: h.clone(),
                    sync: h,
                    notifications,
                }
            }
            _ => Self::production(),
        }
    }

    /// All host bases (auth + sync + notifications) set to `base` (used by the fake cloud).
    pub fn single_host(base: impl Into<String>) -> Self {
        let base = base.into();
        let notifications = ws_base_from_http(&base);
        Self {
            auth: base.clone(),
            sync: base,
            notifications,
        }
    }

    // URL builders — used by the transport layer in later tasks.
    pub(crate) fn device_new(&self) -> String {
        format!("{}/token/json/2/device/new", self.auth)
    }
    pub(crate) fn user_new(&self) -> String {
        format!("{}/token/json/2/user/new", self.auth)
    }
    pub(crate) fn root_get(&self) -> String {
        format!("{}/sync/v4/root", self.sync)
    }
    pub(crate) fn root_put(&self) -> String {
        format!("{}/sync/v3/root", self.sync)
    }
    pub(crate) fn blob(&self, hash: &str) -> String {
        format!("{}/sync/v3/files/{}", self.sync, hash)
    }

    /// Full notification websocket URL (notifications host + the well-known path).
    /// Path confirmed from rmfakecloud / rmapi: `/notifications/ws/json/1`.
    pub(crate) fn notifications_ws(&self) -> String {
        format!("{}/notifications/ws/json/1", self.notifications)
    }
}

/// Derive a websocket base (`ws://`/`wss://`) from an http(s) base. A `ws`/`wss` URL is
/// returned unchanged; `https://` → `wss://`, `http://` → `ws://`; anything else is left
/// as-is (the caller's `into_client_request()` will reject a truly invalid scheme).
fn ws_base_from_http(base: &str) -> String {
    if base.starts_with("ws://") || base.starts_with("wss://") {
        base.to_string()
    } else if let Some(rest) = base.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = base.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        base.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_env_override_sets_all_hosts() {
        // SAFETY: single-threaded test; restore after.
        std::env::set_var("RM_CLOUD_HOST", "http://127.0.0.1:9");
        let c = Config::from_env();
        assert_eq!(c.auth, "http://127.0.0.1:9");
        assert_eq!(c.sync, "http://127.0.0.1:9");
        // Notifications base derives a ws scheme from the http override.
        assert_eq!(c.notifications, "ws://127.0.0.1:9");
        assert_eq!(c.notifications_ws(), "ws://127.0.0.1:9/notifications/ws/json/1");
        std::env::remove_var("RM_CLOUD_HOST");
    }

    #[test]
    fn production_defaults() {
        let c = Config::production();
        assert!(c.sync.ends_with("remarkable.com"));
        assert_eq!(c.root_get(), format!("{}/sync/v4/root", c.sync));
        // Notifications default is a wss URL ending in the well-known path.
        assert!(c.notifications.starts_with("wss://"));
        assert!(c.notifications_ws().ends_with("/notifications/ws/json/1"));
    }

    #[test]
    fn single_host_sets_notifications_too() {
        let c = Config::single_host("https://example.test");
        assert_eq!(c.notifications, "wss://example.test");
        assert_eq!(
            c.notifications_ws(),
            "wss://example.test/notifications/ws/json/1"
        );
    }

    #[test]
    fn ws_base_passthrough_for_ws_scheme() {
        assert_eq!(ws_base_from_http("wss://h"), "wss://h");
        assert_eq!(ws_base_from_http("ws://h"), "ws://h");
        assert_eq!(ws_base_from_http("https://h"), "wss://h");
        assert_eq!(ws_base_from_http("http://h"), "ws://h");
    }
}
