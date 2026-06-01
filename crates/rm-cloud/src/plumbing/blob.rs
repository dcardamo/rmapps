//! Content-addressed blob transfer.

use base64::Engine;

use crate::error::{Error, Result};
use crate::transport::{check, send_retrying, status_error};

/// Logical filename header the cloud expects on blob transfers.
pub(crate) const RM_FILENAME: &str = "rm-filename";

/// GET a blob by hash. `name` is the logical filename header (e.g. `<id>.metadata`).
pub(crate) async fn get_blob(
    http: &reqwest::Client,
    url: &str,
    user_token: &str,
    name: &str,
) -> Result<Vec<u8>> {
    let resp = send_retrying(
        http.get(url)
            .bearer_auth(user_token)
            .header(RM_FILENAME, name),
    )
    .await?;
    let resp = check(resp)?;
    Ok(resp.bytes().await?.to_vec())
}

/// PUT a blob under `hash` (the caller computed it per the keying rules).
pub(crate) async fn put_blob(
    http: &reqwest::Client,
    url: &str,
    user_token: &str,
    name: &str,
    bytes: Vec<u8>,
) -> Result<()> {
    let content_type = if name == "root.docSchema" {
        "text/plain; charset=UTF-8"
    } else {
        "application/octet-stream"
    };
    // The v3 files endpoint requires an integrity checksum: CRC32C (Castagnoli) of the
    // body, big-endian, base64. Without it the server rejects the upload ("missing
    // checksum", HTTP 400).
    let crc = crc32c::crc32c(&bytes);
    let goog_hash = format!(
        "crc32c={}",
        base64::engine::general_purpose::STANDARD.encode(crc.to_be_bytes())
    );
    let resp = send_retrying(
        http.put(url)
            .bearer_auth(user_token)
            .header(RM_FILENAME, name)
            .header("content-type", content_type)
            .header("x-goog-hash", goog_hash)
            .body(bytes),
    )
    .await?;
    let status = resp.status();
    if status.is_success() {
        return Ok(());
    }
    // Surface the server's explanation (the v3 files endpoint returns a useful 400 body).
    let body = resp.text().await.unwrap_or_default();
    if body.is_empty() {
        Err(status_error(status))
    } else {
        Err(Error::Http(format!(
            "blob put {name} failed: {status}: {body}"
        )))
    }
}
