//! Content-addressed blob transfer.

use crate::error::Result;
use crate::transport::check;

/// Logical filename header the cloud expects on blob transfers.
pub(crate) const RM_FILENAME: &str = "rm-filename";

/// GET a blob by hash. `name` is the logical filename header (e.g. `<id>.metadata`).
pub(crate) async fn get_blob(
    http: &reqwest::Client,
    url: &str,
    user_token: &str,
    name: &str,
) -> Result<Vec<u8>> {
    let resp = http
        .get(url)
        .bearer_auth(user_token)
        .header(RM_FILENAME, name)
        .send()
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
    let mut req = http
        .put(url)
        .bearer_auth(user_token)
        .header(RM_FILENAME, name);
    if name == "root.docSchema" {
        req = req.header("content-type", "text/plain; charset=UTF-8");
    }
    let resp = req.body(bytes).send().await?;
    check(resp)?;
    Ok(())
}
