//! ICS feed fetching with local cache and refresh semantics.
//!
//! `feed_bytes` is the main entry point: it returns the raw ICS bytes for a
//! feed, reading from a local disk cache when possible and only hitting the
//! network when needed (cold cache or explicit `refresh`).  The `Fetcher`
//! trait lets tests inject a fake HTTP backend without opening real sockets.

use std::io::Read;
use std::path::Path;

use crate::config::IcsFeed;

// ---------------------------------------------------------------------------
// Fetcher trait + real impl
// ---------------------------------------------------------------------------

pub trait Fetcher {
    fn get(&self, url: &str) -> anyhow::Result<Vec<u8>>;
}

pub struct UreqFetcher;

impl Fetcher for UreqFetcher {
    fn get(&self, url: &str) -> anyhow::Result<Vec<u8>> {
        let mut buf = Vec::new();
        ureq::get(url).call()?.into_reader().read_to_end(&mut buf)?;
        Ok(buf)
    }
}

// ---------------------------------------------------------------------------
// Cache helpers
// ---------------------------------------------------------------------------

/// Map a feed name to a safe filesystem slug: lowercase, non-alphanumeric → `-`.
fn slug(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Return the raw ICS bytes for `feed`.
///
/// * If `refresh` is `false` **and** the cache file exists, return cached
///   bytes without touching the network.
/// * Otherwise attempt a fetch via `fetcher`.  On success, write the result
///   to `<out_dir>/.ics-cache/<slug>.ics` and return it.
/// * If the fetch fails but a stale cache file exists, emit a warning to
///   stderr and return the cached bytes so the caller can still render.
/// * If the fetch fails and there is no cache, propagate the error.
pub fn feed_bytes(
    out_dir: &Path,
    feed: &IcsFeed,
    refresh: bool,
    fetcher: &dyn Fetcher,
) -> anyhow::Result<Vec<u8>> {
    let cache_path = out_dir
        .join(".ics-cache")
        .join(format!("{}.ics", slug(&feed.name)));

    // Serve from cache when it exists and a refresh was not requested.
    if !refresh && cache_path.exists() {
        return Ok(std::fs::read(&cache_path)?);
    }

    match fetcher.get(&feed.url) {
        Ok(bytes) => {
            // Persist to cache (create the directory if needed).
            std::fs::create_dir_all(cache_path.parent().unwrap())?;
            std::fs::write(&cache_path, &bytes)?;
            Ok(bytes)
        }
        Err(e) => {
            if cache_path.exists() {
                eprintln!(
                    "rmbujo: feed {:?} fetch failed ({e}); using cached copy",
                    feed.name
                );
                Ok(std::fs::read(&cache_path)?)
            } else {
                Err(e)
            }
        }
    }
}
