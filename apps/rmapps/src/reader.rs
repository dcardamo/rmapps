//! `rmapps reader` — pull Readwise Reader collections, render reader PDFs, run
//! on-device annotation read-back, and deploy to the cloud via the native client.
//!
//! Ported from `rmreader`'s old regenerate CLI path. Read-back is best-effort
//! (logged on error, never fatal). Reader PDFs are write-only (no ink to keep),
//! so they deploy with destructive `replace`.

use std::path::PathBuf;

use anyhow::{Context, Result};

use rmreader::deploy::BundleFetch;
use rmreader::generate::UreqImageFetcher;
use rmreader::readback;
use rmreader::readwise::http::UreqTransport;

use crate::cloud::{self, Cloud};
use crate::config::Config;

/// Adapts the native cloud client to rmreader's `BundleFetch` seam: read-back
/// downloads a deployed bundle to inspect on-device annotations.
struct CloudFetch<'a> {
    cloud: &'a Cloud,
}

impl BundleFetch for CloudFetch<'_> {
    fn fetch(&self, folder: &str, name: &str) -> Result<Option<PathBuf>> {
        self.cloud.fetch_bundle(folder, name)
    }
}

pub fn run(cfg: &Config) -> Result<()> {
    let reader = cfg
        .reader
        .as_ref()
        .context("no [reader] section in rmapps config")?;
    reader.validate()?;

    // Honor the per-app cache dir for the article cache unless the user pinned
    // one explicitly in their reader config.
    let mut reader = reader.clone();
    if reader.cache.dir.is_none() {
        reader.cache.dir = Some(crate::config::cache_dir("reader")?.to_string_lossy().into_owned());
    }
    if reader.output_dir == "." {
        reader.output_dir = crate::config::cache_dir("reader")?.to_string_lossy().into_owned();
    }

    let transport = UreqTransport;
    let fetcher = UreqImageFetcher {
        timeout_secs: reader.images.timeout_secs,
        concurrency: reader.images.concurrency,
    };

    // backend == "none" means "generate only, skip upload" (and skip read-back,
    // since there is nothing deployed to read back from).
    let upload = reader.deploy.backend.as_str() != "none";

    if upload {
        let cl = Cloud::from_stored()?;
        let bf = CloudFetch { cloud: &cl };
        // Read-back on-device annotations BEFORE regenerating. Best-effort.
        if let Err(e) = readback::sync_collection(
            &bf,
            &transport,
            &reader.readwise.token,
            &reader.deploy.library_folder,
            "Library",
        ) {
            eprintln!("[rmapps] Library read-back failed (continuing): {e:#}");
        }
        if reader.feed.enabled {
            if let Err(e) = readback::sync_collection(
                &bf,
                &transport,
                &reader.readwise.token,
                &reader.deploy.feed_folder,
                "Feed",
            ) {
                eprintln!("[rmapps] Feed read-back failed (continuing): {e:#}");
            }
        }

        let targets = rmreader::generate::generate(&reader, &transport, &fetcher)?;
        for (pdf, folder) in &targets {
            cl.replace(folder, &cloud::doc_name(pdf)?, std::fs::read(pdf)?)?;
        }
        println!("Deployed {} reader PDF(s)", targets.len());
    } else {
        let targets = rmreader::generate::generate(&reader, &transport, &fetcher)?;
        println!("Generated {} reader PDF(s) (upload skipped)", targets.len());
    }
    Ok(())
}
