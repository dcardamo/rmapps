use std::cell::Cell;
use std::path::PathBuf;

use rmbujo::config::IcsFeed;
use rmbujo::ics::fetch::{feed_bytes, Fetcher};

fn tmp_dir() -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static N: AtomicUsize = AtomicUsize::new(0);
    let mut p = std::env::temp_dir();
    p.push(format!(
        "rmbujo-fetch-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn feed() -> IcsFeed {
    IcsFeed {
        name: "Holidays".into(),
        url: "https://x/h.ics".into(),
        color: "brick".into(),
    }
}

struct OkFetcher {
    body: Vec<u8>,
    calls: Cell<usize>,
}
impl Fetcher for OkFetcher {
    fn get(&self, _url: &str) -> anyhow::Result<Vec<u8>> {
        self.calls.set(self.calls.get() + 1);
        Ok(self.body.clone())
    }
}

struct ErrFetcher;
impl Fetcher for ErrFetcher {
    fn get(&self, _url: &str) -> anyhow::Result<Vec<u8>> {
        anyhow::bail!("network down")
    }
}

#[test]
fn cold_cache_fetches_and_writes() {
    let dir = tmp_dir();
    let f = OkFetcher {
        body: b"BEGIN:VCALENDAR".to_vec(),
        calls: Cell::new(0),
    };
    let b = feed_bytes(&dir, &feed(), false, &f).unwrap();
    assert_eq!(b, b"BEGIN:VCALENDAR");
    assert_eq!(f.calls.get(), 1);
    assert!(dir.join(".ics-cache").join("holidays.ics").exists());
}

#[test]
fn warm_cache_no_fetch() {
    let dir = tmp_dir();
    let f = OkFetcher {
        body: b"X".to_vec(),
        calls: Cell::new(0),
    };
    feed_bytes(&dir, &feed(), false, &f).unwrap();
    let b = feed_bytes(&dir, &feed(), false, &f).unwrap();
    assert_eq!(b, b"X");
    assert_eq!(f.calls.get(), 1);
}

#[test]
fn refresh_refetches() {
    let dir = tmp_dir();
    let f = OkFetcher {
        body: b"X".to_vec(),
        calls: Cell::new(0),
    };
    feed_bytes(&dir, &feed(), false, &f).unwrap();
    feed_bytes(&dir, &feed(), true, &f).unwrap();
    assert_eq!(f.calls.get(), 2);
}

#[test]
fn fail_with_cache_returns_cache() {
    let dir = tmp_dir();
    let ok = OkFetcher {
        body: b"CACHED".to_vec(),
        calls: Cell::new(0),
    };
    feed_bytes(&dir, &feed(), false, &ok).unwrap();
    let b = feed_bytes(&dir, &feed(), true, &ErrFetcher).unwrap();
    assert_eq!(b, b"CACHED");
}

#[test]
fn fail_without_cache_errs() {
    let dir = tmp_dir();
    assert!(feed_bytes(&dir, &feed(), false, &ErrFetcher).is_err());
}
