//! `rmapps cache` — inspect and prune the content-addressed blob cache.

use anyhow::Result;
use clap::{Args, Subcommand};
use rm_cloud::BlobCache;

use crate::cloud::default_cache_dir;

#[derive(Args)]
pub struct CacheArgs {
    #[command(subcommand)]
    cmd: CacheCmd,
}

#[derive(Subcommand)]
enum CacheCmd {
    /// Print entry count and total size.
    Info,
    /// Evict oldest entries until the store is under --max-size bytes.
    Gc {
        /// Maximum total size in bytes (default 3 GiB).
        #[arg(long, default_value_t = 3u64 * 1024 * 1024 * 1024)]
        max_size: u64,
    },
    /// Remove the entire cache.
    Clear,
}

/// Evict oldest-by-mtime entries until total size <= max_size. Returns (freed_bytes, removed_count).
fn gc(cache: &BlobCache, max_size: u64) -> (u64, usize) {
    let mut entries = cache.entries();
    let mut total: u64 = entries.iter().map(|(_, len, _)| *len).sum();
    entries.sort_by_key(|(_, _, mtime)| *mtime); // oldest first
    let (mut freed, mut removed) = (0u64, 0usize);
    for (path, len, _) in entries {
        if total <= max_size {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total -= len;
            freed += len;
            removed += 1;
        }
    }
    (freed, removed)
}

pub fn run(args: CacheArgs) -> Result<()> {
    let dir = default_cache_dir();
    let cache = BlobCache::new(&dir);
    match args.cmd {
        CacheCmd::Info => {
            let entries = cache.entries();
            let total: u64 = entries.iter().map(|(_, len, _)| *len).sum();
            println!(
                "cache: {}\n  entries: {}\n  size: {:.1} MiB",
                dir.display(),
                entries.len(),
                total as f64 / (1024.0 * 1024.0)
            );
        }
        CacheCmd::Gc { max_size } => {
            let (freed, removed) = gc(&cache, max_size);
            println!(
                "gc: freed {:.1} MiB across {} entries",
                freed as f64 / (1024.0 * 1024.0),
                removed
            );
        }
        CacheCmd::Clear => {
            if dir.exists() {
                std::fs::remove_dir_all(&dir)?;
            }
            println!("cache cleared: {}", dir.display());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::gc;
    use rm_cloud::{sha256_hex, BlobCache};

    #[test]
    fn gc_evicts_down_to_cap() {
        let dir = tempfile::tempdir().unwrap();
        let cache = BlobCache::new(dir.path());
        for i in 0u8..3 {
            let bytes = vec![i; 100];
            cache.put(&sha256_hex(&bytes), &bytes).unwrap();
        }
        assert_eq!(cache.total_size(), 300);
        let (freed, removed) = gc(&cache, 150);
        assert!(cache.total_size() <= 150, "must evict down to cap");
        assert!(freed >= 150 && removed >= 1, "freed {freed} removed {removed}");
    }

    /// `gc` operates on the blob root only; the sync index lives as a SIBLING of
    /// `blobs/`, so eviction must never touch it.
    #[test]
    fn gc_ignores_sync_index_sibling() {
        let dir = tempfile::tempdir().unwrap();
        let blobs = dir.path().join("blobs");
        let cache = BlobCache::new(&blobs);
        let h = sha256_hex(b"x");
        cache.put(&h, b"x").unwrap();
        let idx = dir.path().join("sync-index.json");
        std::fs::write(&idx, b"{}").unwrap();
        super::gc(&cache, 0); // evict everything
        assert!(idx.exists(), "sync index must survive gc");
    }
}
