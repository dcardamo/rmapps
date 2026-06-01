//! Thin reqwest helpers: a single place that maps HTTP statuses to [`Error`] and that
//! transparently retries on `429 Too Many Requests` (honoring `Retry-After`).

use std::time::Duration;

use reqwest::StatusCode;

use crate::error::{Error, Result};

/// Max automatic retries on HTTP 429 before giving up and surfacing [`Error::RateLimited`].
const MAX_RATE_LIMIT_RETRIES: u32 = 5;
/// Base delay for exponential backoff when the server gives no `Retry-After` header.
const BACKOFF_BASE: Duration = Duration::from_millis(500);
/// Cap on a single backoff/Retry-After sleep, so a hostile or huge value can't wedge a
/// long-running daemon for minutes.
const BACKOFF_CAP: Duration = Duration::from_secs(60);

/// Map a non-success status to the corresponding error.
pub(crate) fn status_error(status: StatusCode) -> Error {
    match status {
        StatusCode::UNAUTHORIZED => Error::Unauthorized,
        StatusCode::CONFLICT => Error::Conflict,
        StatusCode::PRECONDITION_FAILED => Error::WrongGeneration,
        StatusCode::NOT_FOUND => Error::NotFound,
        StatusCode::TOO_MANY_REQUESTS => Error::RateLimited,
        other => Error::Http(format!("request failed: {other}")),
    }
}

/// Return `Ok(resp)` for 2xx, else the mapped error.
pub(crate) fn check(resp: reqwest::Response) -> Result<reqwest::Response> {
    let status = resp.status();
    if status.is_success() {
        Ok(resp)
    } else {
        Err(status_error(status))
    }
}

/// Parse a `Retry-After` header as a delay. Supports the delta-seconds form
/// (`Retry-After: 30`); the HTTP-date form (which the reMarkable cloud does not emit) is
/// treated as absent so the caller falls back to exponential backoff. The result is
/// clamped to [`BACKOFF_CAP`].
fn retry_after_delay(resp: &reqwest::Response) -> Option<Duration> {
    let raw = resp.headers().get(reqwest::header::RETRY_AFTER)?;
    let secs: u64 = raw.to_str().ok()?.trim().parse().ok()?;
    Some(Duration::from_secs(secs).min(BACKOFF_CAP))
}

/// Exponential backoff for the `attempt`-th retry (0-based), clamped to [`BACKOFF_CAP`].
fn backoff_delay(attempt: u32) -> Duration {
    BACKOFF_BASE
        .saturating_mul(2u32.saturating_pow(attempt))
        .min(BACKOFF_CAP)
}

/// Send `builder`, transparently retrying on HTTP 429. The wait between attempts honors
/// the server's `Retry-After` header when present, else uses exponential backoff. After
/// [`MAX_RATE_LIMIT_RETRIES`] exhausted retries the final 429 response is returned as-is;
/// the caller's status check ([`check`] / a manual match) maps it to [`Error::RateLimited`].
///
/// The builder's body must be cloneable — every request in this crate carries an
/// in-memory `Vec<u8>` body or no body, so [`reqwest::RequestBuilder::try_clone`] always
/// succeeds.
pub(crate) async fn send_retrying(builder: reqwest::RequestBuilder) -> Result<reqwest::Response> {
    let mut attempt = 0u32;
    loop {
        let req = builder
            .try_clone()
            .expect("retryable request must have a cloneable (in-memory) body");
        let resp = req.send().await?;
        if resp.status() == StatusCode::TOO_MANY_REQUESTS && attempt < MAX_RATE_LIMIT_RETRIES {
            let delay = retry_after_delay(&resp).unwrap_or_else(|| backoff_delay(attempt));
            tokio::time::sleep(delay).await;
            attempt += 1;
            continue;
        }
        return Ok(resp);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_mapping() {
        assert!(matches!(
            status_error(StatusCode::UNAUTHORIZED),
            Error::Unauthorized
        ));
        assert!(matches!(
            status_error(StatusCode::CONFLICT),
            Error::Conflict
        ));
        assert!(matches!(
            status_error(StatusCode::PRECONDITION_FAILED),
            Error::WrongGeneration
        ));
        assert!(matches!(
            status_error(StatusCode::NOT_FOUND),
            Error::NotFound
        ));
        assert!(matches!(
            status_error(StatusCode::TOO_MANY_REQUESTS),
            Error::RateLimited
        ));
        assert!(matches!(
            status_error(StatusCode::INTERNAL_SERVER_ERROR),
            Error::Http(_)
        ));
    }

    #[test]
    fn backoff_grows_and_caps() {
        assert_eq!(backoff_delay(0), BACKOFF_BASE);
        assert_eq!(backoff_delay(1), BACKOFF_BASE * 2);
        assert_eq!(backoff_delay(2), BACKOFF_BASE * 4);
        // Huge attempt saturates to the cap rather than overflowing.
        assert_eq!(backoff_delay(1000), BACKOFF_CAP);
    }
}
