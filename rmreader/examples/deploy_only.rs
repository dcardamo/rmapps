//! Dev-only: generate + deploy (full-replace) WITHOUT the annotation read-back
//! step, so re-deploying a fresh build clears stale on-device ink instead of
//! pushing it to Readwise. Usage: cargo run --example deploy_only -- <config.toml>
use rmreader::{deploy, generate, readwise};

fn main() -> anyhow::Result<()> {
    let path = std::env::args()
        .nth(1)
        .expect("usage: deploy_only <config.toml>");
    let cfg = rmreader::config::load(std::path::Path::new(&path))?;
    cfg.validate()?;
    let transport = readwise::http::UreqTransport;
    let fetcher = generate::UreqImageFetcher {
        timeout_secs: cfg.images.timeout_secs,
        concurrency: cfg.images.concurrency,
    };
    let deployer = deploy::get_deployer(&cfg)?;
    let targets = generate::generate(&cfg, &transport, &fetcher)?;
    for (pdf, folder) in &targets {
        eprintln!("[deploy_only] replacing {} in {folder}", pdf.display());
        deployer.replace(pdf, folder)?;
    }
    println!("Deployed {} PDF(s) (no read-back).", targets.len());
    Ok(())
}
