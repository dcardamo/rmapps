//! reMarkable device pairing + credential resolution.
//!
//! Pairing calls `rm_cloud::register_device` with an 8-char one-time code from
//! <https://my.remarkable.com/device/desktop/connect> and persists the returned
//! long-lived device token into [`SecretStore`] under
//! [`Scope::DeviceAuth`] / [`REMARKABLE_DEVICE_AUTH_NAME`].
//!
//! Credential resolution prefers the stored device token, falling back to the
//! `RM_CLOUD_DEVICE_TOKEN` env var. The short-lived user token always comes
//! from `RM_CLOUD_USER_TOKEN` env (it is refreshed lazily from the device
//! token anyway).

use inkapp_core::error::Error as CoreError;
use inkapp_core::secrets::{Scope, SecretStore};
use rm_cloud::{register_device, Config, Credentials, Error as CloudError};

/// Name under which the reMarkable device token is stored in [`SecretStore`].
pub const REMARKABLE_DEVICE_AUTH_NAME: &str = "remarkable";

/// Pair this machine with a reMarkable using an 8-char one-time code.
///
/// `config` is taken as a parameter so tests can point at the in-process fake
/// cloud via `Config::single_host(...)`. Production callers pass
/// `Config::from_env()`.
pub async fn pair(
    secrets: &mut SecretStore,
    config: &Config,
    code: &str,
) -> Result<(), CloudError> {
    let http = reqwest::Client::new();
    let token = register_device(&http, config, code).await?;
    secrets
        .set(
            Scope::DeviceAuth,
            REMARKABLE_DEVICE_AUTH_NAME,
            token.as_bytes(),
        )
        .map_err(|e| CloudError::Http(format!("secrets: {e}")))?;
    Ok(())
}

/// Resolve credentials with store-takes-precedence-over-env semantics.
/// Public wrapper around [`resolve_with`] using `std::env::var`.
pub fn resolve_credentials(secrets: &SecretStore) -> Result<Credentials, CloudError> {
    resolve_with(secrets, |k| std::env::var(k).ok())
}

/// Inner, env-injectable resolver — race-free in parallel tests.
///
/// Rules:
/// - `device_token`: store value at `Scope::DeviceAuth / REMARKABLE_DEVICE_AUTH_NAME`
///   if present, else `get_env("RM_CLOUD_DEVICE_TOKEN")`.
/// - `user_token`: `get_env("RM_CLOUD_USER_TOKEN")` only.
/// - Empty strings are treated as absent.
/// - Returns `MissingCredential` when neither a device token nor a user token is found.
pub fn resolve_with(
    secrets: &SecretStore,
    get_env: impl Fn(&str) -> Option<String>,
) -> Result<Credentials, CloudError> {
    fn non_empty(s: Option<String>) -> Option<String> {
        s.filter(|v| !v.is_empty())
    }

    let stored = secrets
        .get(Scope::DeviceAuth, REMARKABLE_DEVICE_AUTH_NAME)
        .map_err(|e: CoreError| CloudError::Http(format!("secrets: {e}")))?
        .and_then(|b| String::from_utf8(b).ok());

    let device_token = non_empty(stored).or_else(|| non_empty(get_env("RM_CLOUD_DEVICE_TOKEN")));
    let user_token = non_empty(get_env("RM_CLOUD_USER_TOKEN"));

    if device_token.is_none() && user_token.is_none() {
        return Err(CloudError::MissingCredential(
            "no paired device and no RM_CLOUD_* env tokens",
        ));
    }
    Ok(Credentials {
        device_token,
        user_token,
    })
}
