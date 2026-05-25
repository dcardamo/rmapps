//! Endpoint configuration, with env overrides for pointing tests at a fake.

/// Resolved base URLs for the three reMarkable cloud surfaces.
#[derive(Debug, Clone)]
pub struct Config {
    /// Auth host (token endpoints).
    pub auth: String,
    /// Sync host (root + blob endpoints).
    pub sync: String,
}

impl Config {
    /// Production defaults.
    pub fn production() -> Self {
        Self {
            auth: "https://webapp-prod.cloud.remarkable.engineering".into(),
            sync: "https://internal.cloud.remarkable.com".into(),
        }
    }

    /// Production defaults, overridden by `RM_CLOUD_HOST` (which sets all hosts to
    /// the same base — used to point the client at the fake cloud or a proxy).
    pub fn from_env() -> Self {
        match std::env::var("RM_CLOUD_HOST") {
            Ok(h) if !h.is_empty() => Self {
                auth: h.clone(),
                sync: h,
            },
            _ => Self::production(),
        }
    }

    /// All three host bases set to `base` (used by the fake cloud).
    pub fn single_host(base: impl Into<String>) -> Self {
        let base = base.into();
        Self {
            auth: base.clone(),
            sync: base,
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
    #[allow(dead_code)] // used by commit in Task 6
    pub(crate) fn root_put(&self) -> String {
        format!("{}/sync/v3/root", self.sync)
    }
    pub(crate) fn blob(&self, hash: &str) -> String {
        format!("{}/sync/v3/files/{}", self.sync, hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_env_override_sets_both_hosts() {
        // SAFETY: single-threaded test; restore after.
        std::env::set_var("RM_CLOUD_HOST", "http://127.0.0.1:9");
        let c = Config::from_env();
        assert_eq!(c.auth, "http://127.0.0.1:9");
        assert_eq!(c.sync, "http://127.0.0.1:9");
        std::env::remove_var("RM_CLOUD_HOST");
    }

    #[test]
    fn production_defaults() {
        let c = Config::production();
        assert!(c.sync.ends_with("remarkable.com"));
        assert_eq!(c.root_get(), format!("{}/sync/v4/root", c.sync));
    }
}
