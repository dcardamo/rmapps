//! Non-destructive read-back inspector: print the plan rmreader WOULD apply for a
//! downloaded `.rmdoc`, without calling Readwise. Use to verify detection before a
//! real sync.
//!
//! Usage:
//!   cargo run --example readback_inspect -- <bundle.rmdoc>
//!   cargo run --example readback_inspect -- <bundle.rmdoc> --execute <config.toml>
use rmreader::readback;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let path = args
        .get(1)
        .expect("usage: readback_inspect <bundle.rmdoc> [--execute <config.toml>]");

    // Check for --execute <config.toml>
    let execute_config: Option<&str> = args
        .windows(2)
        .find(|w| w[0] == "--execute")
        .map(|w| w[1].as_str());

    let plan = readback::detect(std::path::Path::new(path))?;
    println!("actions ({}):", plan.actions.len());
    for (id, kind) in &plan.actions {
        println!("  {kind:?}  doc={id}");
    }
    println!("content highlights ({}):", plan.highlights.len());
    for h in &plan.highlights {
        println!("  source_url={}  text={:?}", h.source_url, h.text);
    }
    println!("warnings ({}):", plan.warnings.len());
    for w in &plan.warnings {
        println!("  {w}");
    }

    if let Some(cfg_path) = execute_config {
        let cfg = rmreader::config::load(std::path::Path::new(cfg_path))?;
        let transport = rmreader::readwise::http::UreqTransport;
        readback::execute(&transport, &cfg.readwise.token, &plan);
        println!("executed.");
    }

    Ok(())
}
