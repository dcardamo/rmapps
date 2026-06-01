//! Content-addressed on-disk blob cache. Blobs are immutable-by-hash, so the hash is a
//! perfect cache key and a stored entry is verified by re-hashing on read.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::plumbing::index::sha256_hex;

static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// A content-addressed blob store under a single root directory.
#[derive(Debug, Clone)]
pub struct BlobCache {
    root: PathBuf,
}

impl BlobCache {
    /// Create a cache rooted at `root` (created on first write).
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Path of the entry for `hash`: `<root>/<first-2-hex>/<hash>`.
    fn path_for(&self, hash: &str) -> PathBuf {
        let shard = if hash.len() >= 2 { &hash[0..2] } else { "00" };
        self.root.join(shard).join(hash)
    }

    /// Read the blob for `hash`. `None` on miss or on a corrupt entry (sha256 ≠ hash),
    /// removing the corrupt entry.
    pub fn get(&self, hash: &str) -> Option<Vec<u8>> {
        let path = self.path_for(hash);
        let bytes = fs::read(&path).ok()?;
        if sha256_hex(&bytes) == hash {
            Some(bytes)
        } else {
            let _ = fs::remove_file(&path);
            None
        }
    }

    /// Read the blob for `hash` WITHOUT re-hash verification. For blobs whose key is not
    /// `sha256(bytes)` (e.g. the per-doc index blob, keyed by the Merkle doc hash). The key
    /// is still a stable immutable identifier; we simply can't self-verify the bytes.
    pub fn get_unverified(&self, hash: &str) -> Option<Vec<u8>> {
        std::fs::read(self.path_for(hash)).ok()
    }

    /// Write `bytes` under `hash` atomically (temp file + rename within the shard dir).
    pub fn put(&self, hash: &str, bytes: &[u8]) -> std::io::Result<()> {
        let path = self.path_for(hash);
        let dir = path.parent().expect("entry path always has a parent");
        fs::create_dir_all(dir)?;
        let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let tmp = dir.join(format!(".{hash}.{}.{}.tmp", std::process::id(), seq));
        {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(bytes)?;
            f.sync_all()?;
        }
        fs::rename(&tmp, &path)?;
        Ok(())
    }

    /// Total bytes stored (for `cache info`/`gc`).
    pub fn total_size(&self) -> u64 {
        self.entries().iter().map(|(_, len, _)| *len).sum()
    }

    /// All entries as `(path, len_bytes, modified)` for gc/info. Skips temp files.
    pub fn entries(&self) -> Vec<(PathBuf, u64, std::time::SystemTime)> {
        let mut out = Vec::new();
        let Ok(shards) = fs::read_dir(&self.root) else { return out };
        for shard in shards.flatten() {
            let Ok(files) = fs::read_dir(shard.path()) else { continue };
            for f in files.flatten() {
                let name = f.file_name();
                if name.to_string_lossy().ends_with(".tmp") {
                    continue;
                }
                if let Ok(md) = f.metadata() {
                    let mtime = md.modified().unwrap_or(std::time::UNIX_EPOCH);
                    out.push((f.path(), md.len(), mtime));
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cache() -> (tempfile::TempDir, BlobCache) {
        let dir = tempfile::tempdir().unwrap();
        let c = BlobCache::new(dir.path());
        (dir, c)
    }

    #[test]
    fn put_then_get_round_trips() {
        let (_d, c) = cache();
        let bytes = b"hello world".to_vec();
        let hash = sha256_hex(&bytes);
        c.put(&hash, &bytes).unwrap();
        assert_eq!(c.get(&hash), Some(bytes));
    }

    #[test]
    fn miss_returns_none() {
        let (_d, c) = cache();
        assert_eq!(c.get(&sha256_hex(b"absent")), None);
    }

    #[test]
    fn corrupt_entry_is_rejected_and_removed() {
        let (_d, c) = cache();
        let bytes = b"correct".to_vec();
        let hash = sha256_hex(&bytes);
        c.put(&hash, &bytes).unwrap();
        let path = c.path_for(&hash);
        std::fs::write(&path, b"tampered").unwrap();
        assert_eq!(c.get(&hash), None, "corrupt entry must miss");
        assert!(!path.exists(), "corrupt entry must be removed");
    }

    #[test]
    fn total_size_sums_entries() {
        let (_d, c) = cache();
        for s in [b"a".as_slice(), b"bb", b"ccc"] {
            let h = sha256_hex(s);
            c.put(&h, s).unwrap();
        }
        assert_eq!(c.total_size(), 6);
    }
}
