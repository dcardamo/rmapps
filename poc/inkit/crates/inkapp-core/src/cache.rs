//! Durable bounded keyed cache backed by foyer (hybrid memory + disk) with sha256 integrity.

use std::path::PathBuf;

use foyer::{
    BlockEngineConfig, DeviceBuilder, FsDeviceBuilder, HybridCacheBuilder, PsyncIoEngineConfig,
};
use serde::{de::DeserializeOwned, Serialize};

use crate::error::{Error, Result};

/// Content-addressed integrity digest (hex SHA-256 of the stored bytes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Integrity(pub String);

/// Hybrid (memory + disk) keyed cache.  Keys are `String`; values are `Vec<u8>`.
pub struct Cache {
    inner: foyer::HybridCache<String, Vec<u8>>,
}

impl Cache {
    /// Open (or create) a cache at `dir`.
    ///
    /// * `mem_bytes`  — memory tier capacity in bytes
    /// * `disk_bytes` — disk tier capacity in bytes
    pub async fn open(
        dir: impl Into<PathBuf>,
        mem_bytes: usize,
        disk_bytes: usize,
    ) -> Result<Self> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir)
            .map_err(|e| Error::Cache(format!("create dir: {e}")))?;

        let device = FsDeviceBuilder::new(&dir)
            .with_capacity(disk_bytes)
            .build()
            .map_err(|e| Error::Cache(format!("device: {e}")))?;

        // Use 1 MiB blocks so small test caches (8 MiB) have usable block counts.
        // The block size must divide evenly into disk_bytes; 1 MiB is fine down to a few MiB.
        const BLOCK_SIZE: usize = 1 << 20; // 1 MiB

        let inner = HybridCacheBuilder::new()
            .memory(mem_bytes)
            .storage()
            .with_io_engine_config(PsyncIoEngineConfig::new())
            .with_engine_config(BlockEngineConfig::new(device).with_block_size(BLOCK_SIZE))
            .build()
            .await
            .map_err(|e| Error::Cache(format!("build: {e}")))?;

        Ok(Self { inner })
    }

    /// Flush and close the underlying storage engine.
    pub async fn close(&self) -> Result<()> {
        self.inner
            .close()
            .await
            .map_err(|e| Error::Cache(format!("close: {e}")))
    }

    /// Retrieve raw bytes, or `None` on a cache miss.
    pub async fn get_bytes(&self, key: &str) -> Result<Option<Vec<u8>>> {
        self.inner
            .get(key)
            .await
            .map(|opt| opt.map(|e| e.value().clone()))
            .map_err(|e| Error::Cache(format!("get: {e}")))
    }

    /// Insert raw bytes and return their integrity digest.
    pub async fn put_bytes(&self, key: &str, bytes: &[u8]) -> Result<Integrity> {
        let integrity = Self::integrity(bytes);
        self.inner.insert(key.to_string(), bytes.to_vec());
        Ok(integrity)
    }

    /// Retrieve a deserialised JSON value, or `None` on a cache miss.
    pub async fn get_json<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        match self.get_bytes(key).await? {
            Some(bytes) => {
                let value = serde_json::from_slice(&bytes)
                    .map_err(|e| Error::Cache(format!("json deserialise: {e}")))?;
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }

    /// Serialise `value` to JSON bytes, insert, and return the integrity digest.
    pub async fn put_json<T: Serialize>(&self, key: &str, value: &T) -> Result<Integrity> {
        let bytes =
            serde_json::to_vec(value).map_err(|e| Error::Cache(format!("json serialise: {e}")))?;
        self.put_bytes(key, &bytes).await
    }

    /// Derive a stable cache key from ordered parts by joining them with the
    /// ASCII Unit Separator (`0x1F`) and hex-encoding the SHA-256 digest.
    pub fn derived_key(parts: &[&str]) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        for (i, part) in parts.iter().enumerate() {
            if i > 0 {
                hasher.update([0x1f]);
            }
            hasher.update(part.as_bytes());
        }
        hex::encode(hasher.finalize())
    }

    // ── private helpers ──────────────────────────────────────────────────────

    fn integrity(bytes: &[u8]) -> Integrity {
        use sha2::{Digest, Sha256};
        Integrity(hex::encode(Sha256::digest(bytes)))
    }
}

// ── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    async fn open(dir: &std::path::Path) -> Cache {
        Cache::open(dir, 1 << 20, 8 << 20).await.unwrap()
    }

    #[tokio::test]
    async fn bytes_round_trip_and_miss() {
        let dir = tempfile::tempdir().unwrap();
        let c = open(dir.path()).await;
        assert!(c.get_bytes("k").await.unwrap().is_none());
        c.put_bytes("k", b"hello").await.unwrap();
        assert_eq!(c.get_bytes("k").await.unwrap().unwrap(), b"hello");
    }

    #[tokio::test]
    async fn json_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let c = open(dir.path()).await;
        let v = vec!["a".to_string(), "b".to_string()];
        c.put_json("j", &v).await.unwrap();
        let got: Vec<String> = c.get_json("j").await.unwrap().unwrap();
        assert_eq!(got, v);
    }

    #[tokio::test]
    async fn integrity_is_content_addressed() {
        let dir = tempfile::tempdir().unwrap();
        let c = open(dir.path()).await;
        let a = c.put_bytes("a", b"same").await.unwrap();
        let b = c.put_bytes("b", b"same").await.unwrap();
        let d = c.put_bytes("c", b"different").await.unwrap();
        assert_eq!(a, b);
        assert_ne!(a, d);
    }

    #[test]
    fn derived_key_stable_and_distinct() {
        assert_eq!(
            Cache::derived_key(&["i", "rm", "2x"]),
            Cache::derived_key(&["i", "rm", "2x"])
        );
        assert_ne!(
            Cache::derived_key(&["i", "rm", "2x"]),
            Cache::derived_key(&["i", "rm", "1x"])
        );
    }

    #[tokio::test]
    async fn survives_restart() {
        let dir = tempfile::tempdir().unwrap();
        {
            let c = open(dir.path()).await;
            c.put_bytes("persist", b"v").await.unwrap();
            c.close().await.unwrap();
        }
        let c2 = open(dir.path()).await;
        assert_eq!(c2.get_bytes("persist").await.unwrap().unwrap(), b"v");
    }
}
