//! Pull a real Readwise Reader account and prove the warm cache.
//! Requires a token in the secret store: cred `readwise-reader`
//! (Scope::ConnectorCred). Run: cargo run -p inkapp-readwise-reader --example pull

use std::sync::Arc;

use inkapp_config::SecretRef;
use inkapp_core::connector::Connector;
use inkapp_core::secrets::SecretStore;
use inkapp_readwise_reader::{ReaderConfig, Readwise};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let store = SecretStore::open_default()?;
    let cache_dir = std::env::temp_dir().join("inkapp-readwise-reader-pull");

    // Pass 1: live refresh.
    let cfg = ReaderConfig {
        token: SecretRef("readwise-reader".into()),
        ..Default::default()
    };
    let rw = Readwise::from_config(cfg, &store, &cache_dir).await?;
    rw.refresh().await?;
    println!(
        "LIVE  feed={} library={}",
        rw.feed().len(),
        rw.library().len()
    );
    for a in rw.feed().iter().take(5) {
        println!("  feed: {}", a.title);
    }
    for a in rw.library().iter().take(5) {
        println!("  lib : {}", a.title);
    }
    rw.close().await?; // flush durable cache

    // Pass 2: no network — hydrate from the durable cache only (no refresh()).
    let cache = Arc::new(inkapp_core::cache::Cache::open(&cache_dir, 16 << 20, 512 << 20).await?);
    let warm = Readwise::fake().with_cache_hydrated(cache).await;
    println!(
        "WARM  feed={} library={} (served offline from cache)",
        warm.feed().len(),
        warm.library().len()
    );
    Ok(())
}
