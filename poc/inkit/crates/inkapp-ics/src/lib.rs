//! Read-only ICS calendar feed, as an inkapp `Connector` plugin (the doc's
//! "read-only feed" archetype: pull, cache, done). `refresh` parses the .ics
//! source into the `RwLock` cache (single-flighted via core's `SingleFlight`);
//! `flush` is a no-op; the app-facing sync `events()` reads the warm cache.

use std::io::BufReader;
use std::sync::{Arc, RwLock};

use ical::IcalParser;
use inkapp_core::calendar::EventRow;
use inkapp_core::connector::{Connector, ConnectorError};
use inkapp_core::single_flight::SingleFlight;

/// A read-only calendar feed. Reads come from the warm cache; `refresh` re-parses
/// `source`. A live build would fetch `source` over HTTP inside `refresh`
/// (outside the lock); here it's a committed string.
pub struct IcsConnector {
    source: String,
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
            source,
            cache: Arc::new(RwLock::new(events)),
            refresh_flight: SingleFlight::new(),
        }
    }

    /// The committed sample feed.
    pub fn from_fixture() -> Self {
        Self::from_ics(include_str!("../fixtures/feed.ics"))
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
        let source = self.source.clone();
        let cache = Arc::clone(&self.cache);
        self.refresh_flight
            .run(move || async move {
                let events = parse_ics(&source);
                // Brief write lock, no await held across it (the doc's rule).
                *cache.write().unwrap() = events;
                Ok(())
            })
            .await
    }

    async fn flush(&self) {} // read-only feed: nothing to push
}
