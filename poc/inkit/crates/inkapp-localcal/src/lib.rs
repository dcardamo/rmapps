//! A writable, CalDAV-shaped *local* calendar connector — the stand-in that lets
//! `CalendarView`'s Editable branch run end to end without real CalDAV. Reads come
//! from an `RwLock` cache; `cancel(uid)` applies an optimistic overlay (visible
//! this same render) AND enqueues a durable cancel; `flush` persists the queued
//! cancels to the local store. Local writes don't fail over a network, so there is
//! no retry / permanent-failure machinery (unlike the Readwise connector).

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use serde::{Deserialize, Serialize};

use inkapp_core::calendar::EventRow;
use inkapp_core::connector::{Connector, ConnectorError};
use inkapp_core::single_flight::SingleFlight;

/// The durable local store: uids whose cancel has been flushed.
#[derive(Default, Serialize, Deserialize)]
struct Store {
    cancelled: HashSet<String>,
}

/// Optimistic, not-yet-flushed cancels recorded this session.
#[derive(Default)]
struct Overlay {
    pending: HashSet<String>,
}

pub struct LocalCal {
    /// Base events (a committed fixture; a live build would load from CalDAV).
    base: Vec<EventRow>,
    cache: Arc<RwLock<Vec<EventRow>>>,
    overlay: Mutex<Overlay>,
    store: Mutex<Store>,
    persist_path: Option<PathBuf>,
    refresh_flight: SingleFlight<Result<(), ConnectorError>>,
}

impl LocalCal {
    fn build(base: Vec<EventRow>, store: Store, persist_path: Option<PathBuf>) -> Self {
        let cache = apply(&base, &store.cancelled, &HashSet::new());
        Self {
            base,
            cache: Arc::new(RwLock::new(cache)),
            overlay: Mutex::new(Overlay::default()),
            store: Mutex::new(store),
            persist_path,
            refresh_flight: SingleFlight::new(),
        }
    }

    /// A tiny inline calendar for tests / the app.
    pub fn fake() -> Self {
        Self::build(sample_events(), Store::default(), None)
    }

    /// Load persisted cancels from `path` (if present); save on flush.
    pub fn persisted(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let store = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self::build(sample_events(), store, Some(path))
    }

    /// Build from typed config: persist cancels to `cfg.store_path`. Errors if the
    /// path is empty (a present-but-incomplete `[connector.localcal.*]` section),
    /// rather than silently accepting cancels that would never persist.
    pub fn from_config(cfg: &LocalCalConfig) -> Result<Self, inkapp_config::ConfigError> {
        if cfg.store_path.is_empty() {
            return Err(inkapp_config::ConfigError::Missing(
                "localcal.store_path".into(),
            ));
        }
        Ok(Self::persisted(cfg.store_path.clone()))
    }

    /// The current events (warm read under the read lock).
    pub fn events(&self) -> Vec<EventRow> {
        self.cache.read().unwrap().clone()
    }

    /// Record a cancel: optimistic (cache reflects it now) and enqueued for flush.
    ///
    /// The insert and the cache `recompute` are two steps, not one atomic update,
    /// so concurrent `cancel`s could momentarily race the cache projection. The
    /// framework drives this connector serially from `update`, so that window
    /// never opens in practice; a caller hitting it concurrently must serialize.
    pub fn cancel(&self, uid: &str) {
        self.overlay.lock().unwrap().pending.insert(uid.to_string());
        self.recompute();
    }

    /// Rebuild the cache from base + persisted store + pending overlay.
    fn recompute(&self) {
        let persisted = self.store.lock().unwrap().cancelled.clone();
        let pending = self.overlay.lock().unwrap().pending.clone();
        *self.cache.write().unwrap() = apply(&self.base, &persisted, &pending);
    }

    fn save(&self) {
        if let Some(path) = &self.persist_path {
            let store = self.store.lock().unwrap();
            if let Ok(json) = serde_json::to_string_pretty(&*store) {
                let _ = std::fs::write(path, json);
            }
        }
    }
}

/// Project base events given persisted + pending cancels.
fn apply(
    base: &[EventRow],
    persisted: &HashSet<String>,
    pending: &HashSet<String>,
) -> Vec<EventRow> {
    base.iter()
        .map(|e| {
            let mut e = e.clone();
            if persisted.contains(&e.uid) || pending.contains(&e.uid) {
                e.cancelled = true;
            }
            e
        })
        .collect()
}

fn sample_events() -> Vec<EventRow> {
    vec![
        EventRow {
            uid: "mine-1".into(),
            summary: "Write spec".into(),
            start: "20260525T110000Z".into(),
            end: "20260525T120000Z".into(),
            cancelled: false,
        },
        EventRow {
            uid: "mine-2".into(),
            summary: "Gym".into(),
            start: "20260525T180000Z".into(),
            end: "20260525T190000Z".into(),
            cancelled: false,
        },
    ]
}

/// The local calendar config section.
#[derive(Debug, Clone, serde::Deserialize, inkapp_config::Config)]
#[serde(default)]
#[config(kind = "localcal", namespace = "connector")]
pub struct LocalCalConfig {
    /// Path to the JSON store of cancelled-event uids.
    #[config(default = String::new())]
    pub store_path: String,
}

#[async_trait::async_trait]
impl Connector for LocalCal {
    fn name(&self) -> &str {
        "localcal"
    }

    async fn refresh(&self) -> Result<(), ConnectorError> {
        // "Fetch" = base + persisted store, with the pending overlay folded back
        // in so an un-flushed cancel survives. Single-flighted; no lock across await.
        let base = self.base.clone();
        let cache = Arc::clone(&self.cache);
        let persisted = self.store.lock().unwrap().cancelled.clone();
        let pending = self.overlay.lock().unwrap().pending.clone();
        self.refresh_flight
            .run(move || async move {
                *cache.write().unwrap() = apply(&base, &persisted, &pending);
                Ok(())
            })
            .await
    }

    async fn flush(&self) {
        // Move pending cancels into the persisted store and save. No retry: local
        // writes can't fail transiently, so there's no permanent-failure list.
        let pending = {
            let mut ov = self.overlay.lock().unwrap();
            std::mem::take(&mut ov.pending)
        };
        self.store.lock().unwrap().cancelled.extend(pending);
        self.save();
        self.recompute();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_config_uses_store_path() {
        use inkapp_config::{Config, Namespace, Registry};
        assert_eq!(LocalCalConfig::KIND, "localcal");
        assert!(Registry::find(Namespace::Connector, "localcal").is_some());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cal.json");
        let cfg = LocalCalConfig {
            store_path: path.to_string_lossy().into_owned(),
        };
        let cal = LocalCal::from_config(&cfg).expect("non-empty store_path");
        assert!(!cal.events().is_empty()); // sample events present
    }

    #[test]
    fn from_config_errors_on_empty_store_path() {
        let cfg = LocalCalConfig::default(); // store_path == ""
        assert!(matches!(
            LocalCal::from_config(&cfg),
            Err(inkapp_config::ConfigError::Missing(_))
        ));
    }
}
