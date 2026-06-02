//! Thin reqwest helpers: a single place that maps HTTP statuses to [`Error`] and that
//! transparently retries on `429 Too Many Requests` (honoring `Retry-After`).

use std::time::Duration;

use reqwest::StatusCode;

use std::sync::Arc;
use std::sync::OnceLock;
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};
use tokio::time::Instant;

use crate::error::{Error, Result};

/// Default cap on concurrent in-flight cloud requests (env: `RM_CLOUD_MAX_CONCURRENCY`).
const DEFAULT_MAX_CONCURRENCY: usize = 4;
/// Default minimum spacing between request starts in ms (env: `RM_CLOUD_MIN_INTERVAL_MS`).
const DEFAULT_MIN_INTERVAL_MS: u64 = 150;

/// A process-global request throttle: a concurrency cap plus a minimum interval between
/// request *starts*. The reMarkable cloud rate-limits aggressively and the account-wide
/// `ls` fan-out can otherwise burst hundreds of requests; the governor spreads them so a
/// cold cache cannot trip 429.
///
/// The two knobs are NOT independent: the spacing gate admits at most one request start
/// per `min_interval`, so sustained throughput is `1/min_interval` (~6.6 req/s at 150ms)
/// regardless of `max_concurrency`. The concurrency cap only bites when individual requests
/// outlast the interval (large blob PUTs). To go faster, lower `RM_CLOUD_MIN_INTERVAL_MS`;
/// raising `RM_CLOUD_MAX_CONCURRENCY` alone changes little.
#[derive(Clone)]
pub(crate) struct Governor {
    sem: Arc<Semaphore>,
    /// Earliest instant the next request may start.
    gate: Arc<Mutex<Instant>>,
    min_interval: Duration,
}

impl Governor {
    pub(crate) fn new(max_concurrency: usize, min_interval: Duration) -> Self {
        Self {
            sem: Arc::new(Semaphore::new(max_concurrency.max(1))),
            gate: Arc::new(Mutex::new(Instant::now())),
            min_interval,
        }
    }

    /// Build from env vars, falling back to the defaults above.
    fn from_env() -> Self {
        let max = std::env::var("RM_CLOUD_MAX_CONCURRENCY")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_MAX_CONCURRENCY);
        let interval_ms = std::env::var("RM_CLOUD_MIN_INTERVAL_MS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_MIN_INTERVAL_MS);
        Self::new(max, Duration::from_millis(interval_ms))
    }

    /// Acquire a slot: wait for a concurrency permit, then wait out the spacing gate.
    /// The returned permit must be held for the whole request (including retries).
    pub(crate) async fn acquire(&self) -> OwnedSemaphorePermit {
        let permit = self
            .sem
            .clone()
            .acquire_owned()
            .await
            .expect("governor semaphore is never closed");
        if self.min_interval > Duration::ZERO {
            let mut gate = self.gate.lock().await;
            let now = Instant::now();
            let next = (*gate).max(now);
            *gate = next + self.min_interval;
            drop(gate);
            tokio::time::sleep_until(next).await;
        }
        permit
    }
}

/// The process-global governor, initialized from env on first use.
fn governor() -> &'static Governor {
    static GOVERNOR: OnceLock<Governor> = OnceLock::new();
    GOVERNOR.get_or_init(Governor::from_env)
}

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
    // Hold a governor permit for the whole request (incl. 429 backoff) so a retrying
    // request keeps its concurrency slot rather than letting new requests pile on.
    let _permit = governor().acquire().await;
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

    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;

    #[tokio::test]
    async fn governor_caps_concurrency() {
        let gov = Governor::new(3, Duration::ZERO);
        let inflight = Arc::new(AtomicUsize::new(0));
        let max = Arc::new(AtomicUsize::new(0));
        let mut handles = Vec::new();
        for _ in 0..20 {
            let gov = gov.clone();
            let inflight = inflight.clone();
            let max = max.clone();
            handles.push(tokio::spawn(async move {
                let _permit = gov.acquire().await;
                let now = inflight.fetch_add(1, Ordering::SeqCst) + 1;
                max.fetch_max(now, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(10)).await;
                inflight.fetch_sub(1, Ordering::SeqCst);
            }));
        }
        for h in handles {
            h.await.unwrap();
        }
        assert!(max.load(Ordering::SeqCst) <= 3, "concurrency exceeded cap");
    }

    #[tokio::test]
    async fn governor_spaces_request_starts() {
        let interval = Duration::from_millis(20);
        let gov = Governor::new(8, interval);
        let start = Instant::now();
        let mut starts = Vec::new();
        for _ in 0..5 {
            let _permit = gov.acquire().await;
            starts.push(start.elapsed());
        }
        assert!(starts[4] >= interval * 4, "spacing not enforced: {:?}", starts);
    }
}
