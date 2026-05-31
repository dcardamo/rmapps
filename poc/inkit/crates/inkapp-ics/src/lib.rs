//! Read-only ICS calendar feed, as an inkapp `Connector` plugin (the doc's
//! "read-only feed" archetype: pull, cache, done). `refresh` fetches/parses the
//! .ics source into the `RwLock` cache (single-flighted via core's `SingleFlight`);
//! `flush` is a no-op; the app-facing sync `events()` reads the warm cache.

use std::io::BufReader;
use std::sync::{Arc, RwLock};

use ical::IcalParser;
use inkapp_core::calendar::EventRow;
use inkapp_core::connector::{Connector, ConnectorError};
use inkapp_core::single_flight::SingleFlight;

/// The ICS feed config section.
#[derive(Debug, Clone, serde::Deserialize, inkapp_config::Config)]
#[serde(default)]
#[config(kind = "ics", namespace = "connector")]
pub struct IcsConfig {
    /// URL of the .ics feed to fetch.
    #[config(default = String::new())]
    pub url: String,
}

/// Where the connector's .ics text comes from.
enum Source {
    /// Committed/inline text (tests, the sample fixture).
    Inline(String),
    /// A URL fetched over HTTP on refresh.
    Url(String),
}

/// A read-only calendar feed. Reads come from the warm cache; `refresh` fetches
/// (or re-parses) the source and repopulates the cache.
pub struct IcsConnector {
    source: Source,
    cache: Arc<RwLock<Vec<EventRow>>>,
    refresh_flight: SingleFlight<Result<(), ConnectorError>>,
}

impl IcsConnector {
    /// Build from raw .ics text; the cache is pre-populated so `events()` works
    /// before the first explicit `refresh`.
    pub fn from_ics(source: impl Into<String>) -> Self {
        let source = source.into();
        let events = parse_ics(&source);
        Self {
            source: Source::Inline(source),
            cache: Arc::new(RwLock::new(events)),
            refresh_flight: SingleFlight::new(),
        }
    }

    /// The committed sample feed.
    pub fn from_fixture() -> Self {
        Self::from_ics(include_str!("../fixtures/feed.ics"))
    }

    /// Build from typed config: fetch `cfg.url` over HTTP on refresh. The cache
    /// starts empty and is filled on the first successful `refresh`.
    pub fn from_config(cfg: &IcsConfig) -> Self {
        Self {
            source: Source::Url(cfg.url.clone()),
            cache: Arc::new(RwLock::new(Vec::new())),
            refresh_flight: SingleFlight::new(),
        }
    }

    /// The cached events (warm read under the read lock).
    pub fn events(&self) -> Vec<EventRow> {
        self.cache.read().unwrap().clone()
    }
}

/// Parse VEVENTs into `EventRow`s, taking the fields we render. Events without a
/// UID are skipped (no stable identity); other fields default to empty.
fn parse_ics(source: &str) -> Vec<EventRow> {
    let mut out = Vec::new();
    let parser = IcalParser::new(BufReader::new(source.as_bytes()));
    // `flatten` drops any calendar that failed to parse (Result -> IntoIterator).
    for cal in parser.flatten() {
        for event in cal.events {
            let mut uid = None;
            let (mut summary, mut start, mut end) = (String::new(), String::new(), String::new());
            for prop in event.properties {
                let val = prop.value.unwrap_or_default();
                match prop.name.as_str() {
                    // A blank UID is no identity at all — treat it as absent so
                    // the event is skipped below (a malformed feed can emit `UID:`).
                    "UID" => uid = (!val.is_empty()).then_some(val),
                    "SUMMARY" => summary = val,
                    "DTSTART" => start = val,
                    "DTEND" => end = val,
                    _ => {}
                }
            }
            if let Some(uid) = uid {
                out.push(EventRow {
                    uid,
                    summary,
                    start,
                    end,
                    cancelled: false,
                });
            }
        }
    }
    out
}

#[async_trait::async_trait]
impl Connector for IcsConnector {
    fn name(&self) -> &str {
        "ics"
    }

    async fn refresh(&self) -> Result<(), ConnectorError> {
        // Resolve the .ics text. For a URL source the network fetch happens here,
        // OUTSIDE the single-flight closure, so the closure stays 'static and
        // holds no lock across an await (matching the readwise connector pattern).
        // Concurrent refreshes each fetch independently — acceptable for now.
        let text = match &self.source {
            Source::Inline(s) => s.clone(),
            Source::Url(url) => {
                if url.is_empty() {
                    return Err(ConnectorError::Transport("ics url is empty".into()));
                }
                reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(30))
                    .build()
                    .map_err(|e| ConnectorError::Transport(e.to_string()))?
                    .get(url)
                    .send()
                    .await
                    .map_err(|e| ConnectorError::Transport(e.to_string()))?
                    .error_for_status()
                    .map_err(|e| ConnectorError::Transport(e.to_string()))?
                    .text()
                    .await
                    .map_err(|e| ConnectorError::Transport(e.to_string()))?
            }
        };
        let cache = Arc::clone(&self.cache);
        self.refresh_flight
            .run(move || async move {
                let events = parse_ics(&text);
                // Brief write lock, no await held across it (the doc's rule).
                *cache.write().unwrap() = events;
                Ok(())
            })
            .await
    }

    async fn flush(&self) {} // read-only feed: nothing to push
}

#[cfg(test)]
mod tests {
    use super::*;
    use inkapp_core::connector::Connector;

    #[tokio::test]
    async fn inline_source_refreshes_from_committed_text() {
        let c = IcsConnector::from_fixture();
        c.refresh().await.unwrap();
        assert!(!c.events().is_empty());
    }

    #[tokio::test]
    async fn from_config_with_empty_url_errors_and_keeps_empty_cache() {
        let c = IcsConnector::from_config(&IcsConfig::default());
        assert!(c.refresh().await.is_err());
        assert!(c.events().is_empty());
    }

    #[test]
    fn ics_config_kind_registered() {
        use inkapp_config::{Config, Namespace, Registry};
        assert_eq!(IcsConfig::KIND, "ics");
        assert!(Registry::find(Namespace::Connector, "ics").is_some());
    }
}
