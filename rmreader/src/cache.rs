//! On-disk per-document content cache: sanitized HTML + normalized image blobs,
//! keyed by a content hash. A hit skips both the network image fetch and the
//! image decode/transcode. Touch-on-hit plus an mtime-based expiry sweep reclaim
//! entries for documents that have rolled out of the library/feed.
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::config::CacheConfig;

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Bump to invalidate every cache entry when the on-disk format or the
/// processing pipeline changes in a way that affects rendered output.
pub const CACHE_FORMAT_VERSION: u32 = 1;

#[inline]
fn mix(h: &mut u64, bytes: &[u8]) {
    for &b in bytes {
        *h ^= b as u64;
        *h = h.wrapping_mul(FNV_PRIME);
    }
}

/// Content-addressed cache key (FNV-1a, 64-bit, hex). Any change to the document
/// id, its HTML, the byte cap, the images flag, or the format version yields a
/// different key — i.e. a "new or changed document".
pub fn key(doc_id: &str, html_content: &str, max_bytes: usize, images_enabled: bool) -> String {
    let mut h = FNV_OFFSET;
    mix(&mut h, &CACHE_FORMAT_VERSION.to_le_bytes());
    mix(&mut h, doc_id.as_bytes());
    mix(&mut h, &[0]); // field separator
    mix(&mut h, &(max_bytes as u64).to_le_bytes());
    mix(&mut h, &[images_enabled as u8]);
    mix(&mut h, html_content.as_bytes());
    format!("{h:016x}")
}

#[derive(Serialize, Deserialize)]
struct Meta {
    version: u32,
    assets: Vec<String>,
}

/// A cached processed document: render-ready HTML + (asset_key, bytes) blobs.
pub struct Cached {
    pub html: String,
    pub assets: Vec<(String, Vec<u8>)>,
}

pub struct Cache {
    dir: PathBuf,
    enabled: bool,
    expiry: Duration,
}

fn default_cache_dir() -> PathBuf {
    if let Some(x) = std::env::var_os("XDG_CACHE_HOME") {
        if !x.is_empty() {
            return PathBuf::from(x).join("rmreader");
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".cache").join("rmreader");
    }
    std::env::temp_dir().join("rmreader-cache")
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn unique_suffix() -> String {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{}-{}-{}", std::process::id(), nanos, n)
}

fn touch(meta_path: &Path) {
    if let Ok(f) = fs::OpenOptions::new().write(true).open(meta_path) {
        let _ = f.set_modified(SystemTime::now());
    }
}

impl Cache {
    pub fn new(dir: PathBuf, enabled: bool, expiry_days: u64) -> Self {
        Self {
            dir,
            enabled,
            expiry: Duration::from_secs(expiry_days.saturating_mul(86_400)),
        }
    }

    pub fn from_config(c: &CacheConfig) -> Self {
        let dir = c
            .dir
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(default_cache_dir);
        Self::new(dir, c.enabled, c.expiry_days)
    }

    /// Read a cached entry, touching its mtime so the sweep treats it as fresh.
    /// Returns `None` on miss, when disabled, on version mismatch, or any IO/parse error.
    pub fn get(&self, key: &str) -> Option<Cached> {
        if !self.enabled {
            return None;
        }
        let entry = self.dir.join(key);
        let meta: Meta = serde_json::from_slice(&fs::read(entry.join("meta.json")).ok()?).ok()?;
        if meta.version != CACHE_FORMAT_VERSION {
            return None;
        }
        let html = fs::read_to_string(entry.join("html")).ok()?;
        let mut assets = Vec::with_capacity(meta.assets.len());
        for k in &meta.assets {
            assets.push((k.clone(), fs::read(entry.join(k)).ok()?));
        }
        touch(&entry.join("meta.json"));
        Some(Cached { html, assets })
    }

    /// Write an entry atomically (temp dir + rename). Best-effort: never fails the
    /// build. A concurrent writer of the same key wins; we just drop our temp.
    pub fn put(&self, key: &str, html: &str, assets: &[(String, Vec<u8>)]) {
        if !self.enabled {
            return;
        }
        let _ = self.try_put(key, html, assets);
    }

    fn try_put(&self, key: &str, html: &str, assets: &[(String, Vec<u8>)]) -> std::io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let tmp = self.dir.join(format!(".tmp-{}", unique_suffix()));
        fs::create_dir_all(&tmp)?;
        let meta = Meta {
            version: CACHE_FORMAT_VERSION,
            assets: assets.iter().map(|(k, _)| k.clone()).collect(),
        };
        fs::write(tmp.join("meta.json"), serde_json::to_vec(&meta)?)?;
        fs::write(tmp.join("html"), html)?;
        for (k, bytes) in assets {
            fs::write(tmp.join(k), bytes)?;
        }
        let dest = self.dir.join(key);
        if dest.exists() || fs::rename(&tmp, &dest).is_err() {
            let _ = fs::remove_dir_all(&tmp);
        }
        Ok(())
    }

    /// Remove entries not used within `expiry` (mtime of `meta.json`), plus any
    /// leftover temp dirs and partial entries (missing/unreadable `meta.json`).
    pub fn sweep(&self) {
        if !self.enabled {
            return;
        }
        let Ok(rd) = fs::read_dir(&self.dir) else {
            return;
        };
        let now = SystemTime::now();
        for ent in rd.flatten() {
            let p = ent.path();
            if !p.is_dir() {
                continue;
            }
            if ent.file_name().to_string_lossy().starts_with(".tmp-") {
                let _ = fs::remove_dir_all(&p);
                continue;
            }
            let fresh = fs::metadata(p.join("meta.json"))
                .and_then(|m| m.modified())
                .ok()
                .and_then(|mtime| now.duration_since(mtime).ok())
                .map(|age| age <= self.expiry)
                .unwrap_or(false);
            if !fresh {
                let _ = fs::remove_dir_all(&p);
            }
        }
    }
}
