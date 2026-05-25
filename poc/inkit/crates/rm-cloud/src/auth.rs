//! Credentials seam + device pairing + user-token refresh.
//!
//! Tokens come from env vars for now (`RM_CLOUD_DEVICE_TOKEN`, `RM_CLOUD_USER_TOKEN`);
//! a forthcoming config system will construct [`Credentials`] another way. The struct is
//! the thin replaceable seam.

use serde::Serialize;
use uuid::Uuid;

use crate::config::Config;
use crate::error::{Error, Result};

/// A device + user token pair. The device token is long-lived; the user token is
/// short-lived and refreshed from the device token.
#[derive(Debug, Clone, Default)]
pub struct Credentials {
    /// Long-lived device token (from pairing).
    pub device_token: Option<String>,
    /// Short-lived user token (refreshed as needed).
    pub user_token: Option<String>,
}

impl Credentials {
    /// Read tokens from the environment.
    pub fn from_env() -> Self {
        Self {
            device_token: std::env::var("RM_CLOUD_DEVICE_TOKEN")
                .ok()
                .filter(|s| !s.is_empty()),
            user_token: std::env::var("RM_CLOUD_USER_TOKEN")
                .ok()
                .filter(|s| !s.is_empty()),
        }
    }

    /// Construct from an explicit device token.
    pub fn from_device_token(token: impl Into<String>) -> Self {
        Self {
            device_token: Some(token.into()),
            user_token: None,
        }
    }
}

#[derive(Serialize)]
struct DeviceReq<'a> {
    code: &'a str,
    #[serde(rename = "deviceDesc")]
    device_desc: &'a str,
    #[serde(rename = "deviceID")]
    device_id: String,
}

/// Pair a new device with a one-time 8-char code from
/// <https://my.remarkable.com/device/desktop/connect>; returns the device token.
pub async fn register_device(
    http: &reqwest::Client,
    config: &Config,
    code: &str,
) -> Result<String> {
    let req = DeviceReq {
        code,
        device_desc: "desktop-linux",
        device_id: Uuid::new_v4().to_string(),
    };
    let resp = http.post(config.device_new()).json(&req).send().await?;
    if !resp.status().is_success() {
        return Err(Error::Http(format!(
            "device pairing failed: {}",
            resp.status()
        )));
    }
    Ok(resp.text().await?)
}

/// Exchange a device token for a fresh user token.
pub async fn refresh_user_token(
    http: &reqwest::Client,
    config: &Config,
    device_token: &str,
) -> Result<String> {
    if device_token.is_empty() {
        return Err(Error::MissingCredential("device_token"));
    }
    // The user-token endpoint takes no body, but the cloud rejects a POST with no
    // `Content-Length` (HTTP 411). reqwest omits the header for an empty body, so set it
    // explicitly to 0.
    let resp = http
        .post(config.user_new())
        .bearer_auth(device_token)
        .header(reqwest::header::CONTENT_LENGTH, "0")
        .body(Vec::<u8>::new())
        .send()
        .await?;
    match resp.status() {
        s if s.is_success() => Ok(resp.text().await?),
        reqwest::StatusCode::UNAUTHORIZED => Err(Error::Unauthorized),
        s => Err(Error::Http(format!("user token refresh failed: {s}"))),
    }
}
